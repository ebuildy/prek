//! `--schema-from-instances`: each document names its own schema, with a top-level `$schema`
//! key or a `# yaml-language-server: $schema=...` comment, as editors do (upstream issues
//! #310, #340 and #644). Relative locations resolve against the instance file's directory.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use jsonschema::Validator;
use serde_json::Value;

use super::download::Downloader;
use super::retriever::SchemaRetriever;
use super::source;
use super::validator::{self, Settings};

pub(super) struct DeclaredSchemas {
    settings: Settings,
    downloader: Arc<Downloader>,
    /// Compiled schemas by directory and location, so each one is loaded once per run.
    cache: Mutex<HashMap<(String, String), Arc<Validator>>>,
}

impl DeclaredSchemas {
    pub(super) fn new(settings: Settings, downloader: Arc<Downloader>) -> Self {
        Self {
            settings,
            downloader,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn get(&self, location: &str, base_dir: &Path) -> Result<Arc<Validator>, String> {
        let key = (base_dir.display().to_string(), location.to_string());
        if let Some(validator) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            return Ok(validator);
        }
        let loaded = source::load_schemafile(location, base_dir, &self.downloader)?;
        let retriever = Arc::new(SchemaRetriever::new(self.downloader.clone()));
        let (validator, _) = validator::build(loaded, &self.settings, &retriever)?;
        let validator = Arc::new(validator);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, validator.clone());
        }
        Ok(validator)
    }
}

/// The schema a document declares: its `$schema` key, else the file's modeline.
pub(super) fn declared_schema<'a>(
    document: &'a Value,
    modeline: Option<&'a str>,
) -> Option<&'a str> {
    document.get("$schema").and_then(Value::as_str).or(modeline)
}

/// Finds `# yaml-language-server: $schema=<location>` in a YAML file.
pub(super) fn yaml_modeline(content: &str) -> Option<&str> {
    content.lines().find_map(|line| {
        let comment = line.trim_start().strip_prefix('#')?;
        let directive = comment.trim_start().strip_prefix("yaml-language-server:")?;
        let value = directive.trim_start().strip_prefix("$schema=")?;
        value.split_whitespace().next()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn modeline() {
        assert_eq!(
            yaml_modeline("# yaml-language-server: $schema=https://x/s.json\na: 1\n"),
            Some("https://x/s.json")
        );
        assert_eq!(
            yaml_modeline("a: 1\n  #yaml-language-server:   $schema=./s.json  extra\n"),
            Some("./s.json")
        );
        assert_eq!(yaml_modeline("# schema: x\na: 1\n"), None);
    }

    #[test]
    fn schema_key_wins_over_modeline() {
        let document = json!({"$schema": "a.json"});
        assert_eq!(declared_schema(&document, Some("b.json")), Some("a.json"));
        assert_eq!(declared_schema(&json!({}), Some("b.json")), Some("b.json"));
        assert_eq!(declared_schema(&json!([]), None), None);
    }
}
