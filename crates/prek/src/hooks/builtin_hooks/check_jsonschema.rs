use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use jsonschema::Validator;
use serde_json::Value;

use crate::hook::Hook;
use crate::hooks::HookOutput;
use crate::hooks::pre_commit_hooks::{parse_hook_args, run_blocking_file_checks};

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
#[command(disable_version_flag = true)]
#[command(disable_help_flag = true)]
pub(crate) struct Args {
    /// Path to a JSON Schema file (JSON or YAML), relative to the project root.
    #[arg(long, value_name = "PATH")]
    schemafile: PathBuf,
    #[arg(value_name = "FILENAMES")]
    filenames: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
enum Format {
    Json,
    Yaml,
    Toml,
}

impl Format {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }

    fn parse(self, content: &str) -> Result<Value, String> {
        match self {
            Self::Json => serde_json::from_str(content).map_err(|e| e.to_string()),
            Self::Yaml => serde_saphyr::from_str(content).map_err(|e| e.to_string()),
            Self::Toml => toml::from_str(content).map_err(|e| e.to_string()),
        }
    }
}

/// Runs the `check-jsonschema` hook.
pub(crate) async fn run(hook: &Hook, filenames: &[&Path]) -> Result<HookOutput> {
    let args: Args = parse_hook_args(hook)?;
    let file_base = hook.project().relative_path();
    let validator = Arc::new(compile_schema(&file_base.join(&args.schemafile))?);

    run_blocking_file_checks(
        file_base,
        &args.filenames,
        filenames,
        move |file_path, display_path| check_file(&validator, file_path, display_path),
    )
    .await
}

/// Compiles the schema once so every checked file reuses it.
fn compile_schema(path: &Path) -> Result<Validator> {
    let content = fs_err::read_to_string(path)?;
    let format = Format::from_path(path).unwrap_or(Format::Json);
    let schema = format
        .parse(&content)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("Failed to parse schema file `{}`", path.display()))?;
    jsonschema::validator_for(&schema)
        .with_context(|| format!("Invalid JSON Schema in `{}`", path.display()))
}

fn check_file(validator: &Validator, file_path: &Path, display_path: &Path) -> Result<HookOutput> {
    let Some(format) = Format::from_path(display_path) else {
        let message = format!(
            "{}: Unsupported file type (expected .json, .yaml, .yml or .toml)\n",
            display_path.display()
        );
        return Ok(HookOutput::unchanged(1, message.into_bytes()));
    };

    let content = fs_err::read(file_path)?;
    let Ok(content) = simdutf8::compat::from_utf8(&content) else {
        let message = format!(
            "{}: Failed to decode (invalid UTF-8)\n",
            display_path.display()
        );
        return Ok(HookOutput::unchanged(1, message.into_bytes()));
    };

    let instance = match format.parse(content) {
        Ok(instance) => instance,
        Err(e) => {
            let message = format!("{}: Failed to decode ({e})\n", display_path.display());
            return Ok(HookOutput::unchanged(1, message.into_bytes()));
        }
    };

    let mut output = String::new();
    for error in validator.iter_errors(&instance) {
        let pointer = error.instance_path().to_string();
        let pointer = if pointer.is_empty() { "/" } else { &pointer };
        writeln!(output, "{}: {pointer}: {error}", display_path.display())?;
    }
    if output.is_empty() {
        Ok(HookOutput::unchanged(0, Vec::new()))
    } else {
        Ok(HookOutput::unchanged(1, output.into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator() -> Validator {
        jsonschema::validator_for(&serde_json::json!({
            "type": "object",
            "required": ["name"],
            "properties": { "name": { "type": "string" }, "port": { "type": "integer" } }
        }))
        .unwrap()
    }

    fn check(name: &str, content: &str) -> HookOutput {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        fs_err::write(&path, content).unwrap();
        check_file(&validator(), &path, Path::new(name)).unwrap()
    }

    #[test]
    fn valid_files() {
        assert_eq!(check("a.json", r#"{"name": "x"}"#).exit_status, 0);
        assert_eq!(check("a.yaml", "name: x\nport: 1\n").exit_status, 0);
        assert_eq!(check("a.toml", "name = \"x\"\n").exit_status, 0);
    }

    #[test]
    fn reports_all_errors_with_pointers() {
        let out = check("a.yml", "port: nope\n");
        assert_eq!(out.exit_status, 1);
        let text = String::from_utf8(out.output).unwrap();
        assert!(text.contains("a.yml: /port:"), "{text}");
        assert!(text.contains("\"name\" is a required property"), "{text}");
    }

    #[test]
    fn reports_parse_errors_and_unsupported_types() {
        assert_eq!(check("a.json", "{").exit_status, 1);
        assert_eq!(check("a.txt", "x").exit_status, 1);
    }
}
