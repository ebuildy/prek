//! `$ref` retrieval, matching upstream `schema_loader/resolver.py`: local files and
//! http(s) URLs, parsed as JSON, YAML, TOML or JSON5 by extension (default JSON).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jsonschema::{Retrieve, Uri};
use serde_json::Value;

use super::download::Downloader;
use super::filetype::FileType;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub(super) struct SchemaRetriever {
    downloader: Arc<Downloader>,
    /// Resources already loaded in this run, keyed by URI without fragment. They are shared
    /// so registering a large schema does not copy it.
    loaded: Mutex<HashMap<String, Arc<Value>>>,
}

impl SchemaRetriever {
    pub(super) fn new(downloader: Arc<Downloader>) -> Self {
        Self {
            downloader,
            loaded: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a document under `uri`, so a `$ref` to it is served without a request.
    /// Upstream registers the main schema under its retrieval URI this way.
    pub(super) fn preload(&self, uri: &str, value: Arc<Value>) {
        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.insert(uri.to_string(), value);
        }
    }

    fn load(&self, uri: &str) -> Result<Value, BoxError> {
        let scheme = uri.split_once(':').map_or("", |(scheme, _)| scheme);
        match scheme {
            "http" | "https" => {
                let file_type = FileType::detect_url(uri, FileType::Json);
                let body = self
                    .downloader
                    .get(uri, |body| file_type.parse_bytes(body).is_ok())?;
                Ok(file_type.parse_bytes(&body)?)
            }
            "file" => {
                let path = url::Url::parse(uri)?
                    .to_file_path()
                    .map_err(|()| format!("`{uri}` is not a local file path"))?;
                let content = fs_err::read(&path)?;
                Ok(FileType::detect(&path, FileType::Json).parse_bytes(&content)?)
            }
            _ => Err(format!(
                "cannot retrieve `{uri}`: only http, https and file URIs are supported"
            )
            .into()),
        }
    }
}

impl Retrieve for SchemaRetriever {
    fn retrieve(&self, uri: &Uri<String>) -> Result<Value, BoxError> {
        let uri = uri.as_str();
        let uri = uri.split_once('#').map_or(uri, |(base, _)| base);
        if let Some(value) = self
            .loaded
            .lock()
            .ok()
            .and_then(|loaded| loaded.get(uri).cloned())
        {
            return Ok(Value::clone(&value));
        }
        let value = self.load(uri)?;
        self.preload(uri, Arc::new(value.clone()));
        Ok(value)
    }
}

/// Lets one retriever be shared by the validator options and the caller.
pub(super) struct SharedRetriever(pub(super) Arc<SchemaRetriever>);

impl Retrieve for SharedRetriever {
    fn retrieve(&self, uri: &Uri<String>) -> Result<Value, BoxError> {
        self.0.retrieve(uri)
    }
}
