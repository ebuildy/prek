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
    /// Picks the format from the extension, using the same case-sensitive map as upstream
    /// `identify_filetype.py`. Files without a known extension use `default`.
    pub(super) fn detect(path: &Path, default: Self) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
            .unwrap_or(default)
    }

    /// Upstream picks a downloaded schema's type from the text after the last `.` of the URL,
    /// so `https://json.schemastore.org/github-workflow` falls back to `default`.
    pub(super) fn detect_url(url: &str, default: Self) -> Self {
        url.rsplit_once('.')
            .and_then(|(_, extension)| Self::from_extension(extension))
            .unwrap_or(default)
    }

    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "json" | "jsonld" | "geojson" => Some(Self::Json),
            "yaml" | "yml" | "ymlld" | "eyaml" | "cff" => Some(Self::Yaml),
            "json5" => Some(Self::Json5),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }

    pub(super) fn parse_bytes(self, content: &[u8]) -> Result<Value, String> {
        match simdutf8::compat::from_utf8(content) {
            Ok(content) => self.parse(content),
            Err(_) => Err("invalid UTF-8".to_string()),
        }
    }

    /// Parses every document of a file. Only YAML can hold several (`---` separated); an
    /// empty YAML file is one `null` document, like upstream.
    pub(super) fn parse_documents(self, content: &str) -> Result<Vec<Value>, String> {
        if self != Self::Yaml {
            return Ok(vec![self.parse(content)?]);
        }
        let content = strip_bom(content);
        let documents: Vec<Value> =
            serde_saphyr::from_multiple_with_options(content, yaml_options())
                .map_err(|err| err.to_string())?;
        if documents.is_empty() {
            return Ok(vec![Value::Null]);
        }
        Ok(documents
            .into_iter()
            .map(|mut document| {
                yaml_numbers(&mut document);
                document
            })
            .collect())
    }

    pub(super) fn parse(self, content: &str) -> Result<Value, String> {
        let content = match self {
            Self::Json | Self::Json5 | Self::Yaml => strip_bom(content),
            Self::Toml => content,
        };
        match self {
            Self::Json => serde_json::from_str(content).map_err(|err| err.to_string()),
            Self::Json5 => json5::from_str(content).map_err(|err| err.to_string()),
            Self::Yaml => serde_saphyr::from_str_with_options(content, yaml_options())
                .map(|mut document| {
                    yaml_numbers(&mut document);
                    document
                })
                .map_err(|err| err.to_string()),
            Self::Toml => {
                let table: toml::Table = toml::from_str(content).map_err(|err| err.to_string())?;
                toml_to_json(toml::Value::Table(table))
            }
        }
    }
}

/// Python's JSON and YAML loaders skip a UTF-8 byte order mark.
fn strip_bom(content: &str) -> &str {
    content.strip_prefix('\u{feff}').unwrap_or(content)
}

/// Matches the ruamel.yaml safe loader used upstream. Only `true` and `false` are booleans
/// (YAML 1.2), so `on:` in a GitHub workflow stays a string key. Unknown tags such as
/// `!reference` or `!Ref` are errors instead of being dropped silently.
fn yaml_options() -> serde_saphyr::Options {
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    options.reject_unsupported_tags = true;
    // Non-finite floats arrive as the strings `.inf`, `-.inf` and `.nan`, converted below.
    options.reject_non_finite_typeless_float = false;
    options
}

/// Fixes up scalars that ruamel.yaml reads as floats but serde-saphyr leaves as strings:
/// `.inf`/`.nan` (JSON cannot hold them, see [`non_finite_number`]) and floats with `_`
/// digit separators such as `224_617.445_991_228`.
fn yaml_numbers(value: &mut Value) {
    match value {
        Value::String(text) => {
            let number = match text.as_str() {
                ".inf" => Some(non_finite_number(f64::INFINITY)),
                "-.inf" => Some(non_finite_number(f64::NEG_INFINITY)),
                ".nan" => Some(non_finite_number(f64::NAN)),
                _ => underscored_float(text).map(Value::from),
            };
            if let Some(number) = number {
                *value = number;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(yaml_numbers),
        Value::Object(map) => map.values_mut().for_each(yaml_numbers),
        _ => {}
    }
}

fn underscored_float(text: &str) -> Option<f64> {
    let is_float_text = text.contains('_')
        && text.contains(['.', 'e', 'E'])
        && text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '_' | '.' | '+' | '-' | 'e' | 'E'));
    if !is_float_text {
        return None;
    }
    text.replace('_', "")
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// Infinity becomes the largest finite number of the same sign and NaN becomes 0, so
/// `type: number` passes as it does upstream. Range keywords may differ for these values.
fn non_finite_number(value: f64) -> Value {
    let stand_in = if value.is_nan() {
        0.0
    } else if value.is_sign_positive() {
        f64::MAX
    } else {
        f64::MIN
    };
    Value::from(stand_in)
}

/// Converts TOML to JSON. Upstream turns datetimes into ISO strings and adds a `Z` when a
/// datetime or time has no offset, because the `date-time` and `time` formats require one.
fn toml_to_json(value: toml::Value) -> Result<Value, String> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => match serde_json::Number::from_f64(f) {
            Some(number) => Value::Number(number),
            None => non_finite_number(f),
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
        // Upstream matches extensions case-sensitively.
        assert_eq!(detect("a.YML"), FileType::Json);
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

    #[test]
    fn detects_url_types() {
        let detect = |url: &str| FileType::detect_url(url, FileType::Json);
        assert_eq!(detect("https://example.org/main.yaml"), FileType::Yaml);
        assert_eq!(
            detect("https://json.schemastore.org/github-workflow"),
            FileType::Json
        );
        assert_eq!(detect("https://example.org/a.toml"), FileType::Toml);
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
    fn non_finite_floats_are_numbers() {
        assert_eq!(
            parse(FileType::Yaml, "a: .inf\nb: -.inf\nc: .NaN\nd: '.inf'\n"),
            serde_json::json!({"a": f64::MAX, "b": f64::MIN, "c": 0.0, "d": f64::MAX})
        );
        assert_eq!(
            parse(
                FileType::Yaml,
                "a: 224_617.445_991_228\nb: 1_000\nc: a_b.c\n"
            ),
            serde_json::json!({"a": 224_617.445_991_228, "b": 1000, "c": "a_b.c"})
        );
        assert_eq!(
            parse(FileType::Toml, "a = inf\nb = nan\n"),
            serde_json::json!({"a": f64::MAX, "b": 0.0})
        );
    }

    #[test]
    fn yaml_documents() {
        let parse = |content: &str| FileType::Yaml.parse_documents(content).unwrap();
        assert_eq!(parse(""), vec![Value::Null]);
        assert_eq!(parse("a: 1\n"), vec![serde_json::json!({"a": 1})]);
        assert_eq!(parse("---\n- a\n"), vec![serde_json::json!(["a"])]);
        assert_eq!(
            parse("a: 1\n---\nb: .inf\n"),
            vec![
                serde_json::json!({"a": 1}),
                serde_json::json!({"b": f64::MAX})
            ]
        );
        assert_eq!(
            FileType::Json.parse_documents("[1]").unwrap(),
            vec![serde_json::json!([1])]
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
    fn byte_order_mark_is_skipped() {
        assert_eq!(parse(FileType::Json, "\u{feff}{}"), serde_json::json!({}));
        assert_eq!(
            parse(FileType::Yaml, "\u{feff}a: 1"),
            serde_json::json!({"a": 1})
        );
    }

    #[test]
    fn json_errors_are_reported() {
        assert!(FileType::Json.parse("{").is_err());
    }
}
