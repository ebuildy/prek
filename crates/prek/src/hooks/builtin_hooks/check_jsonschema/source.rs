//! Loads the schema chosen by `--schemafile` or `--builtin-schema`, matching upstream
//! `schema_loader/readers.py` and `utils.py`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::catalog;
use super::download::Downloader;
use super::filetype::FileType;

/// Schemes upstream recognizes as URLs at all. Anything else, such as `C:\schema.json`, is a
/// local path.
const KNOWN_URL_SCHEMES: &[&str] = &[
    "", "ftp", "gopher", "http", "file", "https", "shttp", "rsync", "svn", "svn+ssh", "sftp",
    "nfs", "git", "git+ssh", "ws", "wss",
];

pub(super) struct LoadedSchema {
    pub(super) schema: Value,
    /// Where the schema came from. Relative `$ref`s resolve against it when the schema has
    /// no `$id`.
    pub(super) retrieval_uri: Option<String>,
}

/// Reads `--schemafile`. Relative paths are relative to `base`.
pub(super) fn load_schemafile(
    schemafile: &str,
    base: &Path,
    downloader: &Downloader,
) -> Result<LoadedSchema, String> {
    if schemafile == "-" {
        return Err(
            "Error: reading the schema from stdin (`--schemafile -`) is not supported by prek"
                .to_string(),
        );
    }
    let scheme = url_scheme(schemafile);
    match scheme.as_deref() {
        Some("http" | "https") => {
            let file_type = FileType::detect_url(schemafile, FileType::Json);
            let body = downloader
                .get(schemafile, |body| file_type.parse_bytes(body).is_ok())
                .map_err(|err| {
                    format!("Error: Unexpected Error building schema validator\n  {err}")
                })?;
            let schema = parse_schema(file_type, &body, schemafile)?;
            Ok(LoadedSchema {
                schema,
                retrieval_uri: Some(schemafile.to_string()),
            })
        }
        None | Some("file" | "") => {
            let path = local_path(schemafile, base)?;
            let content = fs_err::read(&path)
                .map_err(|err| format!("Error: schemafile could not be parsed as JSON\n  {err}"))?;
            let schema = parse_schema(
                FileType::detect(&path, FileType::Json),
                &content,
                &path.display().to_string(),
            )?;
            let retrieval_uri = url::Url::from_file_path(&path).ok().map(String::from);
            Ok(LoadedSchema {
                schema,
                retrieval_uri,
            })
        }
        Some(_) => Err(format!(
            "Error: check-jsonschema only supports http, https, and local files. detected parsed URL had an unrecognized scheme: {schemafile}"
        )),
    }
}

/// Reads a `--builtin-schema`. Builtin schemas have no retrieval URI, like upstream.
pub(super) fn load_builtin(name: &str) -> Result<LoadedSchema, String> {
    let Some(text) = catalog::builtin_schema(name) else {
        return Err(format!(
            "Error: no builtin schema named `{name}`. Choose from: {}",
            catalog::builtin_schema_names().join(", ")
        ));
    };
    let schema = parse_schema(FileType::Json, text.as_bytes(), name)?;
    Ok(LoadedSchema {
        schema,
        retrieval_uri: None,
    })
}

/// Upstream requires the schema document to be an object.
fn parse_schema(file_type: FileType, content: &[u8], location: &str) -> Result<Value, String> {
    match file_type.parse_bytes(content) {
        Ok(schema @ Value::Object(_)) => Ok(schema),
        Ok(_) => Err(format!(
            "Error: schemafile could not be parsed as JSON\n  {location}: the schema is not an object"
        )),
        Err(err) => Err(format!(
            "Error: schemafile could not be parsed as JSON\n  {location}: {err}"
        )),
    }
}

/// Returns the lowercase scheme when `value` looks like a URL to upstream.
fn url_scheme(value: &str) -> Option<String> {
    let (scheme, _) = value.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    if KNOWN_URL_SCHEMES.contains(&scheme.as_str()) {
        Some(scheme)
    } else {
        None
    }
}

/// Converts a path or `file://` URI to an absolute path. `~` is expanded for plain paths.
fn local_path(value: &str, base: &Path) -> Result<PathBuf, String> {
    let path = if value.starts_with("file://") {
        url::Url::parse(value)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or_else(|| format!("Error: `{value}` is not a valid file URI"))?
    } else if let Some(rest) = value.strip_prefix("~/").or(value.strip_prefix("~\\")) {
        match etcetera::home_dir() {
            Ok(home) => home.join(rest),
            Err(_) => PathBuf::from(value),
        }
    } else {
        PathBuf::from(value)
    };
    let path = if path.is_absolute() {
        path
    } else {
        std::path::absolute(base.join(path)).map_err(|err| format!("Error: {err}"))?
    };
    Ok(canonicalize_or_keep(path))
}

/// Resolves symlinks like upstream's `Path.resolve()`, keeping the path when it does not
/// exist so the read error names it.
fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    match fs_err::canonicalize(&path) {
        Ok(canonical) => canonical,
        Err(_) => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_detection() {
        assert_eq!(url_scheme("https://x/y.json").as_deref(), Some("https"));
        assert_eq!(url_scheme("file:///tmp/s.json").as_deref(), Some("file"));
        assert_eq!(url_scheme("FTP://x").as_deref(), Some("ftp"));
        assert_eq!(url_scheme(r"C:\schemas\s.json"), None);
        assert_eq!(url_scheme("schemas/s.json"), None);
    }

    #[test]
    fn relative_paths_use_base() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(dir.path().join("s.json"), "{}").unwrap();
        let path = local_path("s.json", dir.path()).unwrap();
        assert_eq!(
            path,
            fs_err::canonicalize(dir.path().join("s.json")).unwrap()
        );
    }
}
