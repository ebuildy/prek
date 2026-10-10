use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use jsonschema::Validator;

use crate::hook::Hook;
use crate::hooks::HookOutput;
use crate::hooks::pre_commit_hooks::{parse_hook_args, run_blocking_file_checks};

use self::filetype::FileType;

mod filetype;

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
#[command(disable_version_flag = true)]
#[command(disable_help_flag = true)]
pub(crate) struct Args {
    /// Path to a JSON Schema file, relative to the project root.
    #[arg(long, value_name = "PATH")]
    schemafile: PathBuf,
    /// File type used when the extension is not recognized.
    #[arg(long, value_enum, default_value = "json")]
    default_filetype: FileType,
    /// File type used for every instance file, whatever its extension.
    #[arg(long, value_enum)]
    force_filetype: Option<FileType>,
    #[arg(value_name = "FILENAMES")]
    filenames: Vec<PathBuf>,
}

/// Runs the `check-jsonschema` hook.
pub(crate) async fn run(hook: &Hook, filenames: &[&Path]) -> Result<HookOutput> {
    let args: Args = parse_hook_args(hook)?;
    let file_base = hook.project().relative_path();
    let validator = Arc::new(compile_schema(&file_base.join(&args.schemafile))?);
    let default_filetype = args.default_filetype;
    let force_filetype = args.force_filetype;

    run_blocking_file_checks(
        file_base,
        &args.filenames,
        filenames,
        move |file_path, display_path| {
            let file_type = match force_filetype {
                Some(file_type) => file_type,
                None => FileType::detect(display_path, default_filetype),
            };
            check_file(&validator, file_type, file_path, display_path)
        },
    )
    .await
}

/// Compiles the schema once so every checked file reuses it.
fn compile_schema(path: &Path) -> Result<Validator> {
    let content = fs_err::read_to_string(path)?;
    let schema = FileType::detect(path, FileType::Json)
        .parse(&content)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("Failed to parse schema file `{}`", path.display()))?;
    jsonschema::validator_for(&schema)
        .with_context(|| format!("Invalid JSON Schema in `{}`", path.display()))
}

fn check_file(
    validator: &Validator,
    file_type: FileType,
    file_path: &Path,
    display_path: &Path,
) -> Result<HookOutput> {
    let content = fs_err::read(file_path)?;
    let Ok(content) = simdutf8::compat::from_utf8(&content) else {
        let message = format!(
            "{}: Failed to decode (invalid UTF-8)\n",
            display_path.display()
        );
        return Ok(HookOutput::unchanged(1, message.into_bytes()));
    };

    let instance = match file_type.parse(content) {
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
        let file_type = FileType::detect(Path::new(name), FileType::Json);
        check_file(&validator(), file_type, &path, Path::new(name)).unwrap()
    }

    #[test]
    fn valid_files() {
        assert_eq!(check("a.json", r#"{"name": "x"}"#).exit_status, 0);
        assert_eq!(check("a.yaml", "name: x\nport: 1\n").exit_status, 0);
        assert_eq!(check("a.toml", "name = \"x\"\n").exit_status, 0);
        assert_eq!(check("a.json5", "{name: 'x'}").exit_status, 0);
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
    fn reports_parse_errors() {
        assert_eq!(check("a.json", "{").exit_status, 1);
        // Unknown extensions are parsed as the default type (JSON), like upstream.
        assert_eq!(check(".renovaterc", r#"{"name": "x"}"#).exit_status, 0);
        assert_eq!(check("a.txt", "name: x").exit_status, 1);
    }

    #[test]
    fn yaml_1_1_words_are_strings() {
        assert_eq!(check("a.yaml", "name: yes\n").exit_status, 0);
    }

    #[test]
    fn filetype_flags() {
        let args = Args::try_parse_from([
            "check-jsonschema",
            "--schemafile",
            "s.json",
            "--default-filetype",
            "yaml",
            "--force-filetype",
            "json5",
        ])
        .unwrap();
        assert_eq!(args.default_filetype, FileType::Yaml);
        assert_eq!(args.force_filetype, Some(FileType::Json5));
    }
}
