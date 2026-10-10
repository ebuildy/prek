use std::path::Path;

use serde_json::Value;

/// File formats accepted by `check-jsonschema`, with the same names as the upstream CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum FileType {
    Json,
    Yaml,
    Toml,
    Json5,
}

impl FileType {
    /// Picks the format from the extension, using the same map as upstream
    /// `identify_filetype.py`. Files without a known extension use `default`.
    pub(super) fn detect(path: &Path, default: Self) -> Self {
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            return default;
        };
        match extension.to_ascii_lowercase().as_str() {
            "json" | "jsonld" | "geojson" => Self::Json,
            "yaml" | "yml" | "ymlld" | "eyaml" | "cff" => Self::Yaml,
            "json5" => Self::Json5,
            "toml" => Self::Toml,
            _ => default,
        }
    }

    pub(super) fn parse(self, content: &str) -> Result<Value, String> {
        match self {
            Self::Json => serde_json::from_str(content).map_err(|err| err.to_string()),
            Self::Json5 => json5::from_str(content).map_err(|err| err.to_string()),
            Self::Yaml => serde_saphyr::from_str_with_options(content, yaml_options())
                .map_err(|err| err.to_string()),
            Self::Toml => {
                let table: toml::Table = toml::from_str(content).map_err(|err| err.to_string())?;
                toml_to_json(toml::Value::Table(table))
            }
        }
    }
}

/// Matches the ruamel.yaml safe loader used upstream. Only `true` and `false` are booleans
/// (YAML 1.2), so `on:` in a GitHub workflow stays a string key. Unknown tags such as
/// `!reference` or `!Ref` are errors instead of being dropped silently.
fn yaml_options() -> serde_saphyr::Options {
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    options.reject_unsupported_tags = true;
    options
}

/// Converts TOML to JSON. Upstream turns datetimes into ISO strings and adds a `Z` when a
/// datetime or time has no offset, because the `date-time` and `time` formats require one.
fn toml_to_json(value: toml::Value) -> Result<Value, String> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => match serde_json::Number::from_f64(f) {
            Some(number) => Value::Number(number),
            None => return Err(format!("non-finite float `{f}` cannot be validated")),
        },
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(datetime) => {
            let mut text = datetime.to_string();
            if datetime.time.is_some() && datetime.offset.is_none() {
                text.push('Z');
            }
            Value::String(text)
        }
        toml::Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(toml_to_json)
                .collect::<Result<_, _>>()?,
        ),
        toml::Value::Table(table) => Value::Object(
            table
                .into_iter()
                .map(|(key, value)| Ok((key, toml_to_json(value)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_upstream_extensions() {
        let detect = |name: &str| FileType::detect(Path::new(name), FileType::Json);
        assert_eq!(detect("a.json"), FileType::Json);
        assert_eq!(detect("a.jsonld"), FileType::Json);
        assert_eq!(detect("a.geojson"), FileType::Json);
        assert_eq!(detect("a.yaml"), FileType::Yaml);
        assert_eq!(detect("a.YML"), FileType::Yaml);
        assert_eq!(detect("a.ymlld"), FileType::Yaml);
        assert_eq!(detect("a.eyaml"), FileType::Yaml);
        assert_eq!(detect("CITATION.cff"), FileType::Yaml);
        assert_eq!(detect("a.json5"), FileType::Json5);
        assert_eq!(detect("a.toml"), FileType::Toml);
    }

    #[test]
    fn unknown_extension_uses_default() {
        assert_eq!(
            FileType::detect(Path::new(".renovaterc"), FileType::Json),
            FileType::Json
        );
        assert_eq!(
            FileType::detect(Path::new("ci"), FileType::Yaml),
            FileType::Yaml
        );
        assert_eq!(
            FileType::detect(Path::new("a.txt"), FileType::Toml),
            FileType::Toml
        );
    }

    fn parse(file_type: FileType, content: &str) -> Value {
        file_type.parse(content).unwrap()
    }

    #[test]
    fn yaml_uses_yaml_1_2_booleans() {
        // ruamel.yaml's safe loader keeps YAML 1.1 words as strings. GitHub workflows rely on `on:`.
        assert_eq!(
            parse(FileType::Yaml, "a: yes\nb: on\non: push\nc: true\n"),
            serde_json::json!({"a": "yes", "b": "on", "on": "push", "c": true})
        );
    }

    #[test]
    fn yaml_rejects_unknown_tags() {
        assert!(FileType::Yaml.parse("x: !reference [a, b]\n").is_err());
        assert!(FileType::Yaml.parse("x: !Ref foo\n").is_err());
        assert_eq!(
            parse(FileType::Yaml, "x: !!str 1\n"),
            serde_json::json!({"x": "1"})
        );
    }

    #[test]
    fn yaml_keeps_timestamps_as_strings_and_empty_is_null() {
        assert_eq!(
            parse(FileType::Yaml, "t: 2001-12-14\n"),
            serde_json::json!({"t": "2001-12-14"})
        );
        assert_eq!(parse(FileType::Yaml, ""), Value::Null);
    }

    #[test]
    fn toml_datetimes_become_strings() {
        let value = parse(
            FileType::Toml,
            "a = 1979-05-27T07:32:00Z\nb = 1979-05-27T07:32:00\nc = 1979-05-27\nd = 07:32:00\ne = 1979-05-27T00:32:00-07:00\n",
        );
        assert_eq!(
            value,
            serde_json::json!({
                "a": "1979-05-27T07:32:00Z",
                "b": "1979-05-27T07:32:00Z",
                "c": "1979-05-27",
                "d": "07:32:00Z",
                "e": "1979-05-27T00:32:00-07:00",
            })
        );
    }

    #[test]
    fn toml_nested_values() {
        assert_eq!(
            parse(FileType::Toml, "x = [1, 2.5, true]\n[t]\nk = \"v\"\n"),
            serde_json::json!({"x": [1, 2.5, true], "t": {"k": "v"}})
        );
    }

    #[test]
    fn json5_is_supported() {
        assert_eq!(
            parse(FileType::Json5, "{a: 1, // c\n b: 'x',}"),
            serde_json::json!({"a": 1, "b": "x"})
        );
    }

    #[test]
    fn json_errors_are_reported() {
        assert!(FileType::Json.parse("{").is_err());
    }
}
