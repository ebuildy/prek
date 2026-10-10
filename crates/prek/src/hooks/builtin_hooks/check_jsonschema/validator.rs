//! Builds validators like upstream `schema_loader/main.py`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jsonschema::Validator;
use jsonschema::error::ValidationErrorKind;
use serde_json::{Value, json};

use super::RegexVariant;
use super::formats;
use super::retriever::{SchemaRetriever, SharedRetriever};
use super::source::LoadedSchema;

pub(super) struct Settings {
    pub(super) disable_formats: Vec<String>,
    pub(super) regex_variant: RegexVariant,
    pub(super) base_uri: Option<String>,
}

impl Settings {
    fn options(&self) -> jsonschema::ValidationOptions<'static> {
        formats::configure(
            jsonschema::options(),
            &self.disable_formats,
            self.regex_variant,
        )
    }
}

/// Compiles the schema. Upstream sets `$id` to `--base-uri`, and also registers the schema
/// under its retrieval URI so a `$ref` back to that URI needs no request.
pub(super) fn build(
    loaded: LoadedSchema,
    settings: &Settings,
    retriever: &Arc<SchemaRetriever>,
) -> Result<Validator, String> {
    let LoadedSchema {
        mut schema,
        retrieval_uri,
    } = loaded;
    if let (Some(base_uri), Value::Object(map)) = (&settings.base_uri, &mut schema) {
        map.insert("$id".to_string(), Value::String(base_uri.clone()));
    }
    let has_id = schema.get("$id").is_some() || schema.get("id").is_some_and(Value::is_string);
    let mut options = settings
        .options()
        .with_retriever(SharedRetriever(retriever.clone()));
    if let Some(uri) = retrieval_uri {
        retriever.preload(&uri, schema.clone());
        if !has_id {
            options = options.with_base_uri(uri);
        }
    }
    options.build(&schema).map_err(|err| match err.kind() {
        ValidationErrorKind::Referencing(_) => {
            format!("Failure resolving $ref within schema\n  {err}")
        }
        _ => format!("Error: schemafile was not valid\n  {err}"),
    })
}

/// Validators for `--check-metaschema`, one per metaschema seen in this run.
pub(super) struct MetaValidators {
    settings: Settings,
    cache: Mutex<HashMap<&'static str, Arc<Validator>>>,
}

impl MetaValidators {
    pub(super) fn new(settings: Settings) -> Self {
        Self {
            settings,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Upstream picks the metaschema from the document's `$schema` and uses the latest
    /// draft when it is missing or unknown. Returns `None` for Draft 3, which the
    /// `jsonschema` crate does not support, so those documents are not checked.
    pub(super) fn for_document(&self, document: &Value) -> Result<Option<Arc<Validator>>, String> {
        let Some(uri) = metaschema_uri(document) else {
            return Ok(None);
        };
        if let Some(validator) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(uri).cloned())
        {
            return Ok(Some(validator));
        }
        let validator = Arc::new(
            self.settings
                .options()
                .build(&json!({ "$schema": uri, "$ref": uri }))
                .map_err(|err| {
                    format!("Error: Unexpected Error building schema validator\n  {err}")
                })?,
        );
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(uri, validator.clone());
        }
        Ok(Some(validator))
    }
}

fn metaschema_uri(document: &Value) -> Option<&'static str> {
    let declared = document
        .get("$schema")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end_matches('#');
    let uri = match declared {
        "http://json-schema.org/draft-03/schema" => return None,
        "http://json-schema.org/draft-04/schema" => "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema" => "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema" => "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema" => {
            "https://json-schema.org/draft/2019-09/schema"
        }
        _ => "https://json-schema.org/draft/2020-12/schema",
    };
    Some(uri)
}

/// `--fill-defaults`: upstream fills `default` values of `properties` before validating.
/// This follows `properties`, `items` and the `allOf`/`anyOf`/`oneOf` branches; `$ref`s are
/// not followed.
pub(super) fn fill_defaults(schema: &Value, instance: &mut Value) {
    let Value::Object(schema) = schema else {
        return;
    };
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(branches)) = schema.get(key) {
            for branch in branches {
                fill_defaults(branch, instance);
            }
        }
    }
    match instance {
        Value::Object(object) => {
            if let Some(Value::Object(properties)) = schema.get("properties") {
                for (name, subschema) in properties {
                    if !object.contains_key(name)
                        && let Some(default) = subschema.get("default")
                    {
                        object.insert(name.clone(), default.clone());
                    }
                    if let Some(value) = object.get_mut(name) {
                        fill_defaults(subschema, value);
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema @ Value::Object(_)) = schema.get("items") {
                for item in items {
                    fill_defaults(item_schema, item);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_defaults_nested() {
        let schema = json!({
            "properties": {
                "title": {"default": "Untitled"},
                "nested": {"properties": {"x": {"default": 1}}},
                "list": {"items": {"properties": {"y": {"default": 2}}}}
            }
        });
        let mut instance = json!({"nested": {}, "list": [{}]});
        fill_defaults(&schema, &mut instance);
        assert_eq!(
            instance,
            json!({"title": "Untitled", "nested": {"x": 1}, "list": [{"y": 2}]})
        );
    }

    #[test]
    fn metaschema_selection() {
        assert_eq!(
            metaschema_uri(&json!({"$schema": "http://json-schema.org/draft-07/schema#"})),
            Some("http://json-schema.org/draft-07/schema#")
        );
        assert_eq!(
            metaschema_uri(&json!({})),
            Some("https://json-schema.org/draft/2020-12/schema")
        );
        assert_eq!(
            metaschema_uri(&json!({"$schema": "http://json-schema.org/draft-03/schema#"})),
            None
        );
    }
}
