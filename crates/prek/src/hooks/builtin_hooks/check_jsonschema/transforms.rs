//! `--data-transform`, ported from upstream `transforms/`.

use std::borrow::Cow;

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum DataTransform {
    AzurePipelines,
    GitlabCi,
}

impl DataTransform {
    /// Runs on the raw YAML text before parsing.
    ///
    /// Upstream registers GitLab's `!reference [job, key]` tag with the YAML loader and loads
    /// it as the list of its items. prek's YAML parser rejects unknown tags, so the tag is
    /// blanked out here, which leaves exactly that list. Like upstream, a `!reference` whose
    /// value is not a sequence is an error.
    pub(super) fn preprocess(self, content: &str) -> Result<Cow<'_, str>, String> {
        match self {
            Self::AzurePipelines => Ok(Cow::Borrowed(content)),
            Self::GitlabCi => strip_gitlab_references(content),
        }
    }

    /// Runs on the parsed document.
    pub(super) fn apply(self, value: Value) -> Result<Value, String> {
        match self {
            Self::GitlabCi => Ok(value),
            Self::AzurePipelines => match value {
                Value::Object(map) => Ok(Value::Object(azure_object(map))),
                Value::Array(_) => Err(
                    "azure-pipelines transform: this transform requires that the data be an object, got list"
                        .to_string(),
                ),
                other => Ok(other),
            },
        }
    }
}

const GITLAB_TAG: &str = "!reference";

fn strip_gitlab_references(content: &str) -> Result<Cow<'_, str>, String> {
    if !content.contains(GITLAB_TAG) {
        return Ok(Cow::Borrowed(content));
    }
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(index) = rest.find(GITLAB_TAG) {
        let (before, after_start) = rest.split_at(index);
        let after = &after_start[GITLAB_TAG.len()..];
        let starts_token = before
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || matches!(c, '[' | '{' | ',' | ':' | '-'));
        let ends_token = after.chars().next().is_none_or(char::is_whitespace);
        output.push_str(before);
        if starts_token && ends_token {
            let value = after.trim_start_matches([' ', '\t']);
            let is_sequence = value.starts_with('[')
                || value.starts_with('\n')
                || value.starts_with("\r\n")
                || value.is_empty();
            if !is_sequence {
                let line = value.lines().next().unwrap_or_default();
                return Err(format!(
                    "check-jsonschema rejects this gitlab !reference tag: non-list-value `{line}`"
                ));
            }
            output.push_str(&" ".repeat(GITLAB_TAG.len()));
        } else {
            output.push_str(GITLAB_TAG);
        }
        rest = after;
    }
    output.push_str(rest);
    Ok(Cow::Owned(output))
}

fn is_expression(key: &str) -> bool {
    key.starts_with("${{") && key.ends_with("}}")
}

fn azure_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(azure_object(map)),
        Value::Array(items) => Value::Array(azure_list(items)),
        other => other,
    }
}

/// A list item that is a single `${{ expr }}: value` mapping is replaced by its value, and a
/// list value is spliced into the parent list.
fn azure_list(items: Vec<Value>) -> Vec<Value> {
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let expression_value = match &item {
            Value::Object(map) if map.len() == 1 => map
                .iter()
                .next()
                .filter(|(key, _)| is_expression(key))
                .map(|(_, value)| value.clone()),
            _ => None,
        };
        match expression_value {
            Some(value) => match azure_value(value) {
                Value::Array(values) => result.extend(values),
                value => result.push(value),
            },
            None => result.push(azure_value(item)),
        }
    }
    result
}

/// An `${{ expr }}` key whose value is an object has its entries lifted into the parent.
/// Any other expression key is dropped, like the Azure Pipelines language server does.
fn azure_object(map: Map<String, Value>) -> Map<String, Value> {
    let mut result = Map::new();
    for (key, value) in map {
        let value = azure_value(value);
        if is_expression(&key) {
            if let Value::Object(lifted) = value {
                result.extend(lifted);
            }
        } else {
            result.insert(key, value);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hooks::builtin_hooks::check_jsonschema::filetype::FileType;

    fn gitlab(content: &str) -> Result<Value, String> {
        let content = DataTransform::GitlabCi.preprocess(content)?;
        FileType::Yaml.parse(&content)
    }

    // Ported from upstream tests/unit/test_gitlab_data_transform.py.
    #[test]
    fn gitlab_reference_becomes_list() {
        assert_eq!(
            gitlab("a: b\nc: !reference [.setup, script]\n").unwrap(),
            json!({"a": "b", "c": [".setup", "script"]})
        );
        assert_eq!(
            gitlab("test:\n  script:\n    - !reference [.setup, script]\n    - echo hi\n").unwrap(),
            json!({"test": {"script": [[".setup", "script"], "echo hi"]}})
        );
    }

    #[test]
    fn gitlab_reference_must_be_a_list() {
        let err =
            gitlab("test:\n  script:\n    - !reference .setup\n    - echo running\n").unwrap_err();
        assert!(err.contains("non-list-value"), "{err}");
    }

    #[test]
    fn gitlab_other_tags_still_rejected() {
        assert!(gitlab("x: !Ref foo\n").is_err());
        assert!(FileType::Yaml.parse("c: !reference [a]\n").is_err());
    }

    #[test]
    fn azure_unnests_expressions() {
        let data = json!({
            "jobs": [
                {"${{ each val in parameter.vals }}": [{"job": "foo", "steps": [{"bash": "echo ${{ val }}"}]}]},
                {"job": "bar"}
            ],
            "parent": {"${{ if true }}": {"k": "v"}, "${{ x }}": "${{ y }}"}
        });
        assert_eq!(
            DataTransform::AzurePipelines.apply(data).unwrap(),
            json!({
                "jobs": [{"job": "foo", "steps": [{"bash": "echo ${{ val }}"}]}, {"job": "bar"}],
                "parent": {"k": "v"}
            })
        );
        assert!(DataTransform::AzurePipelines.apply(json!([1])).is_err());
    }
}
