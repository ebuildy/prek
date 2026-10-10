//! Output, matching upstream `reporter.py` for `-o json` and verbosity. Text output keeps
//! prek's one-line-per-error style.

use std::fmt::Write as _;

use jsonschema::ValidationError;
use jsonschema::error::ValidationErrorKind;
use jsonschema::paths::{Location, LocationSegment};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum OutputFormat {
    Text,
    Json,
}

pub(super) struct FileResult {
    pub(super) path: String,
    pub(super) outcome: Outcome,
}

pub(super) enum Outcome {
    Valid,
    ParseError(String),
    /// The schema a document declared could not be loaded or compiled.
    SchemaError(String),
    Invalid(Vec<Issue>),
}

#[derive(Clone, PartialEq)]
pub(super) struct ErrorLocation {
    pointer: String,
    json_path: String,
    message: String,
    weak: bool,
    depth: usize,
}

pub(super) struct Issue {
    error: ErrorLocation,
    /// Instance path segments, used to find `line`.
    pub(super) path: Vec<String>,
    /// Line of the failing value in the source file, when it can be found.
    pub(super) line: Option<usize>,
    /// Errors under `anyOf`/`oneOf`, flattened depth first like upstream.
    sub_errors: Vec<ErrorLocation>,
}

impl Issue {
    pub(super) fn new(error: &ValidationError<'_>) -> Self {
        let mut sub_errors = Vec::new();
        collect_sub_errors(error, &mut sub_errors);
        let path = error
            .instance_path()
            .iter()
            .map(|segment| match segment {
                LocationSegment::Index(index) => index.to_string(),
                LocationSegment::Property(name) => name.into_owned(),
            })
            .collect();
        Self {
            error: ErrorLocation::new(error),
            path,
            line: None,
            sub_errors,
        }
    }

    /// The most relevant sub-error: a non-`anyOf`/`oneOf` error closest to the failing value.
    fn best_match(&self) -> Option<&ErrorLocation> {
        let direct_depth = self.error.depth;
        self.sub_errors
            .iter()
            .filter(|error| !error.weak)
            .min_by_key(|error| error.depth.saturating_sub(direct_depth))
            .or(self.sub_errors.first())
    }

    /// Upstream `find_best_deep_match`: the deepest non-`anyOf`/`oneOf` error.
    fn best_deep_match(&self) -> Option<&ErrorLocation> {
        self.sub_errors
            .iter()
            .rev()
            .max_by_key(|error| (!error.weak, error.depth))
    }
}

impl ErrorLocation {
    fn new(error: &ValidationError<'_>) -> Self {
        let location = error.instance_path();
        let pointer = location.as_str();
        Self {
            pointer: if pointer.is_empty() {
                "/".to_string()
            } else {
                pointer.to_string()
            },
            json_path: json_path(location),
            message: error.to_string(),
            weak: is_weak(error.kind()),
            depth: location.iter().count(),
        }
    }
}

fn is_weak(kind: &ValidationErrorKind) -> bool {
    matches!(
        kind,
        ValidationErrorKind::AnyOf { .. }
            | ValidationErrorKind::OneOfNotValid { .. }
            | ValidationErrorKind::OneOfMultipleValid { .. }
    )
}

fn collect_sub_errors(error: &ValidationError<'_>, into: &mut Vec<ErrorLocation>) {
    let (ValidationErrorKind::AnyOf { context } | ValidationErrorKind::OneOfNotValid { context }) =
        error.kind()
    else {
        return;
    };
    for branch in context {
        for sub_error in branch {
            into.push(ErrorLocation::new(sub_error));
            collect_sub_errors(sub_error, into);
        }
    }
}

/// `$.a[0].b`, like `ValidationError.json_path` upstream.
fn json_path(location: &Location) -> String {
    let mut path = String::from("$");
    for segment in location {
        match segment {
            LocationSegment::Index(index) => {
                let _ = write!(path, "[{index}]");
            }
            LocationSegment::Property(name) => {
                path.push('.');
                path.push_str(&name);
            }
        }
    }
    path
}

/// Renders the results. Verbosity is `1 + count(-v) - count(-q)`, like upstream.
pub(super) fn render(
    results: &[FileResult],
    format: OutputFormat,
    verbosity: i32,
) -> (i32, Vec<u8>) {
    let failed = results
        .iter()
        .any(|result| !matches!(result.outcome, Outcome::Valid));
    let output = match format {
        OutputFormat::Text => render_text(results, verbosity),
        OutputFormat::Json => render_json(results, failed, verbosity),
    };
    (i32::from(failed), output.into_bytes())
}

fn render_text(results: &[FileResult], verbosity: i32) -> String {
    let mut out = String::new();
    if verbosity < 1 {
        return out;
    }
    for result in results {
        let path = &result.path;
        match &result.outcome {
            Outcome::Valid => {}
            Outcome::ParseError(message) => {
                let _ = writeln!(out, "{path}: Failed to parse ({message})");
            }
            Outcome::SchemaError(message) => {
                let _ = writeln!(out, "{path}: {message}");
            }
            Outcome::Invalid(issues) => {
                for issue in issues {
                    let error = &issue.error;
                    let location = match issue.line {
                        Some(line) => format!("{path}:{line}"),
                        None => path.clone(),
                    };
                    let _ = writeln!(out, "{location}: {}: {}", error.pointer, error.message);
                    render_sub_errors(&mut out, issue, verbosity);
                }
            }
        }
    }
    out
}

fn render_sub_errors(out: &mut String, issue: &Issue, verbosity: i32) {
    if issue.sub_errors.is_empty() {
        return;
    }
    if verbosity > 1 {
        for error in &issue.sub_errors {
            let _ = writeln!(out, "    {}: {}", error.pointer, error.message);
        }
        return;
    }
    let best = issue.best_match();
    let deep = issue.best_deep_match();
    if let Some(best) = best {
        let _ = writeln!(out, "  Best match: {}: {}", best.pointer, best.message);
    }
    let mut shown = usize::from(best.is_some());
    if let Some(deep) = deep
        && Some(deep) != best
    {
        let _ = writeln!(out, "  Best deep match: {}: {}", deep.pointer, deep.message);
        shown += 1;
    }
    let others = issue.sub_errors.len().saturating_sub(shown);
    if others > 0 {
        let _ = writeln!(
            out,
            "  {others} other errors were produced. Use '--verbose' to see all errors."
        );
    }
}

fn render_json(results: &[FileResult], failed: bool, verbosity: i32) -> String {
    let successes: Vec<&str> = results
        .iter()
        .filter(|result| matches!(result.outcome, Outcome::Valid))
        .map(|result| result.path.as_str())
        .collect();
    let mut report = serde_json::Map::new();
    if failed {
        report.insert("status".into(), json!("fail"));
        if verbosity > 1 {
            report.insert("successes".into(), json!(successes));
        }
        if verbosity > 0 {
            let mut errors = Vec::new();
            let mut parse_errors = Vec::new();
            for result in results {
                match &result.outcome {
                    Outcome::Valid => {}
                    Outcome::ParseError(message) => parse_errors.push(json!({
                        "filename": result.path,
                        "message": format!("Failed to parse {}: {message}", result.path),
                    })),
                    Outcome::SchemaError(message) => parse_errors.push(json!({
                        "filename": result.path,
                        "message": message,
                    })),
                    Outcome::Invalid(issues) => {
                        for issue in issues {
                            errors.push(json_issue(&result.path, issue, verbosity));
                        }
                    }
                }
            }
            report.insert("errors".into(), Value::Array(errors));
            report.insert("parse_errors".into(), Value::Array(parse_errors));
        }
    } else {
        report.insert("status".into(), json!("ok"));
        if verbosity > 0 {
            report.insert("errors".into(), json!([]));
        }
        if verbosity > 1 {
            report.insert("checked_paths".into(), json!(successes));
        }
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(report)).unwrap_or_default();
    text.push('\n');
    text
}

fn json_issue(filename: &str, issue: &Issue, verbosity: i32) -> Value {
    let error = &issue.error;
    let mut item = serde_json::Map::new();
    item.insert("filename".into(), json!(filename));
    item.insert("path".into(), json!(error.json_path));
    item.insert("message".into(), json!(error.message));
    if let Some(line) = issue.line {
        item.insert("line".into(), json!(line));
    }
    item.insert("has_sub_errors".into(), json!(!issue.sub_errors.is_empty()));
    if let (Some(best), Some(deep)) = (issue.best_match(), issue.best_deep_match()) {
        item.insert(
            "best_match".into(),
            json!({"path": best.json_path, "message": best.message}),
        );
        item.insert(
            "best_deep_match".into(),
            json!({"path": deep.json_path, "message": deep.message}),
        );
        item.insert("num_sub_errors".into(), json!(issue.sub_errors.len()));
        if verbosity > 1 {
            let sub_errors: Vec<Value> = issue
                .sub_errors
                .iter()
                .map(|sub| json!({"path": sub.json_path, "message": sub.message}))
                .collect();
            item.insert("sub_errors".into(), Value::Array(sub_errors));
        }
    }
    Value::Object(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(schema: &Value, instance: &Value) -> Vec<Issue> {
        let validator = jsonschema::validator_for(schema).unwrap();
        validator
            .iter_errors(instance)
            .map(|e| Issue::new(&e))
            .collect()
    }

    fn results(issues: Vec<Issue>) -> Vec<FileResult> {
        vec![
            FileResult {
                path: "good.json".into(),
                outcome: Outcome::Valid,
            },
            FileResult {
                path: "bad.json".into(),
                outcome: Outcome::Invalid(issues),
            },
            FileResult {
                path: "broken.json".into(),
                outcome: Outcome::ParseError("oops".into()),
            },
        ]
    }

    #[test]
    fn json_paths() {
        let found = issues(
            &json!({"properties": {"a": {"items": {"type": "string"}}}}),
            &json!({"a": ["x", 1]}),
        );
        assert_eq!(found[0].error.json_path, "$.a[1]");
        assert_eq!(found[0].error.pointer, "/a/1");
    }

    // Ported from upstream tests/unit/test_reporters.py: JSON success shapes by verbosity.
    #[test]
    fn json_success_shape() {
        let ok = vec![FileResult {
            path: "a.json".into(),
            outcome: Outcome::Valid,
        }];
        let parse = |verbosity| -> Value {
            serde_json::from_slice(&render(&ok, OutputFormat::Json, verbosity).1).unwrap()
        };
        assert_eq!(parse(0), json!({"status": "ok"}));
        assert_eq!(parse(1), json!({"status": "ok", "errors": []}));
        assert_eq!(
            parse(2),
            json!({"status": "ok", "errors": [], "checked_paths": ["a.json"]})
        );
    }

    #[test]
    fn json_failure_shape_with_sub_errors() {
        let found = issues(
            &json!({"anyOf": [{"type": "string"}, {"properties": {"a": {"type": "integer"}}, "required": ["b"]}]}),
            &json!({"a": "x"}),
        );
        let (code, out) = render(&results(found), OutputFormat::Json, 2);
        assert_eq!(code, 1);
        let report: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["status"], "fail");
        assert_eq!(report["successes"], json!(["good.json"]));
        assert_eq!(report["parse_errors"][0]["filename"], "broken.json");
        let error = &report["errors"][0];
        assert_eq!(error["filename"], "bad.json");
        assert_eq!(error["path"], "$");
        assert_eq!(error["has_sub_errors"], true);
        assert_eq!(error["num_sub_errors"], 3);
        assert_eq!(error["sub_errors"].as_array().unwrap().len(), 3);
        assert_eq!(error["best_deep_match"]["path"], "$.a");
    }

    #[test]
    fn text_verbosity() {
        let schema = json!({"anyOf": [{"type": "string"}, {"type": "integer"}]});
        let text = |verbosity| {
            let found = issues(&schema, &json!(null));
            String::from_utf8(render(&results(found), OutputFormat::Text, verbosity).1).unwrap()
        };
        assert_eq!(text(0), "");
        let normal = text(1);
        assert!(normal.contains("bad.json: /: "), "{normal}");
        assert!(normal.contains("Best match"), "{normal}");
        assert!(
            normal.contains("broken.json: Failed to parse (oops)"),
            "{normal}"
        );
        let verbose = text(2);
        assert!(!verbose.contains("Best match"), "{verbose}");
        assert_eq!(verbose.matches("\n    /: ").count(), 2, "{verbose}");
    }
}
