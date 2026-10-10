//! A Rust port of [check-jsonschema](https://github.com/python-jsonschema/check-jsonschema).
//!
//! It accepts the same options as the upstream CLI so the upstream hook entries
//! (`check-jsonschema --builtin-schema vendor.github-workflows ...`) run unchanged.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use clap::{ArgAction, Parser};
use jsonschema::Validator;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::hook::Hook;
use crate::hooks::HookOutput;
use crate::hooks::pre_commit_hooks::{hook_filenames, parse_hook_args};
use crate::http::REQWEST_CLIENT;
use crate::store::{CacheBucket, Store};

use self::download::Downloader;
use self::filetype::FileType;
use self::report::{FileResult, Issue, Outcome, OutputFormat};
use self::retriever::SchemaRetriever;
use self::transforms::DataTransform;
use self::validator::{MetaValidators, Settings};

mod catalog;
mod download;
mod filetype;
mod formats;
mod report;
mod retriever;
mod source;
mod transforms;
mod validator;

#[cfg(test)]
mod tests;

/// prek runs these hooks from the upstream repository with this implementation (fast path).
/// Their manifest entries are `check-jsonschema` plus arguments this port accepts.
const UPSTREAM_REPO: &str = "https://github.com/python-jsonschema/check-jsonschema";
const UPSTREAM_HOOK_IDS: &[&str] = &[
    "check-jsonschema",
    "check-metaschema",
    "check-azure-pipelines",
    "check-bamboo-spec",
    "check-bitbucket-pipelines",
    "check-buildkite",
    "check-changie",
    "check-circle-ci",
    "check-citation-file-format",
    "check-cloudbuild",
    "check-codecov",
    "check-compose-spec",
    "check-dependabot",
    "check-drone-ci",
    "check-github-actions",
    "check-github-discussion",
    "check-github-issue-config",
    "check-github-issue-forms",
    "check-github-workflows",
    "check-gitlab-ci",
    "check-meltano",
    "check-mergify",
    "check-readthedocs",
    "check-renovate",
    "check-snapcraft",
    "check-taskfile",
    "check-travis",
    "check-woodpecker-ci",
    "check-github-workflows-require-timeout",
];

/// Whether `hook_id` from `repo_url` is an upstream check-jsonschema hook.
pub(crate) fn is_upstream_hook(repo_url: &str, hook_id: &str) -> bool {
    repo_url.trim_end_matches(".git") == UPSTREAM_REPO && UPSTREAM_HOOK_IDS.contains(&hook_id)
}

/// Regex dialect for `pattern`, `patternProperties` and the `regex` format.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum RegexVariant {
    /// ECMAScript syntax.
    #[default]
    Default,
    /// ECMAScript syntax. prek does not emulate non-unicode mode, so this is `default`.
    Nonunicode,
    /// Python `re` syntax.
    Python,
}

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
#[command(disable_version_flag = true)]
#[command(disable_help_flag = true)]
pub(crate) struct Args {
    /// Path or HTTP(S) URI of the JSON Schema. Relative paths are relative to the project root.
    #[arg(long, value_name = "PATH|URI")]
    schemafile: Option<String>,
    /// Override the base URI (`$id`) of the schema.
    #[arg(long)]
    base_uri: Option<String>,
    /// Name of a schema bundled with check-jsonschema, such as `vendor.github-workflows`.
    #[arg(long, value_name = "BUILTIN_SCHEMA_NAME")]
    builtin_schema: Option<String>,
    /// Validate each file as a schema, against the metaschema named by its `$schema`.
    #[arg(long)]
    check_metaschema: bool,
    /// Always download remote schemas, and do not write the cache.
    #[arg(long)]
    no_cache: bool,
    #[arg(long, hide = true)]
    cache_filename: Option<String>,
    /// Formats to stop checking, comma separated. `*` disables every format check.
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = clap::builder::PossibleValuesParser::new(formats::DISABLE_FORMATS_CHOICES),
    )]
    disable_formats: Vec<String>,
    #[arg(long, value_enum, ignore_case = true, hide = true)]
    format_regex: Option<RegexVariant>,
    /// Regex dialect for `pattern` and the `regex` format.
    #[arg(long, value_enum, ignore_case = true)]
    regex_variant: Option<RegexVariant>,
    /// File type used when the extension is not recognized.
    #[arg(long, value_enum, default_value = "json")]
    default_filetype: FileType,
    /// File type used for every instance file, whatever its extension.
    #[arg(long, value_enum)]
    force_filetype: Option<FileType>,
    /// Accepted for compatibility. Has no effect.
    #[arg(long, hide = true, value_parser = ["full", "short"])]
    traceback_mode: Option<String>,
    /// Transform applied to each file before validation.
    #[arg(long, value_enum)]
    data_transform: Option<DataTransform>,
    /// Fill in `default` values from the schema before validating.
    #[arg(long)]
    fill_defaults: bool,
    #[arg(long, hide = true)]
    validator_class: Option<String>,
    /// Output format.
    #[arg(
        short = 'o',
        long,
        value_enum,
        ignore_case = true,
        default_value = "text"
    )]
    output_format: OutputFormat,
    /// Accepted for compatibility. prek controls colors.
    #[arg(long, hide = true, value_parser = ["auto", "always", "never"])]
    color: Option<String>,
    /// Show every error under `anyOf` and `oneOf`.
    #[arg(short = 'v', long, action = ArgAction::Count)]
    verbose: u8,
    /// Print nothing; only the exit code reports the result.
    #[arg(short = 'q', long, action = ArgAction::Count)]
    quiet: u8,
    #[arg(value_name = "FILENAMES")]
    filenames: Vec<PathBuf>,
}

enum SchemaMode<'a> {
    File(&'a str),
    Builtin(&'a str),
    Metaschema,
}

impl Args {
    fn schema_mode(&self) -> Result<SchemaMode<'_>> {
        if self.validator_class.is_some() {
            bail!("--validator-class is not supported by prek, it loads a Python class");
        }
        let mode = match (
            self.schemafile.as_deref(),
            self.builtin_schema.as_deref(),
            self.check_metaschema,
        ) {
            (Some(path), None, false) => SchemaMode::File(path),
            (None, Some(name), false) => SchemaMode::Builtin(name),
            (None, None, true) => SchemaMode::Metaschema,
            (None, None, false) => {
                bail!(
                    "Either --schemafile, --builtin-schema, or --check-metaschema must be provided"
                )
            }
            _ => bail!(
                "--schemafile, --builtin-schema, and --check-metaschema are mutually exclusive"
            ),
        };
        if matches!(mode, SchemaMode::Metaschema) && self.base_uri.is_some() {
            bail!("'--base-uri' was used with '--metaschema'. This combination is not supported.");
        }
        Ok(mode)
    }

    fn settings(&self) -> Settings {
        Settings {
            disable_formats: self.disable_formats.clone(),
            regex_variant: self.regex_variant.or(self.format_regex).unwrap_or_default(),
            base_uri: self.base_uri.clone(),
        }
    }

    fn verbosity(&self) -> i32 {
        1 + i32::from(self.verbose) - i32::from(self.quiet)
    }
}

/// Where the hook runs: paths are relative to `base`, downloads use `client` and `cache_dir`.
pub(super) struct Context {
    pub(super) base: PathBuf,
    pub(super) cache_dir: PathBuf,
    pub(super) client: reqwest::Client,
}

/// Runs the `check-jsonschema` hook.
pub(crate) async fn run(store: &Store, hook: &Hook, filenames: &[&Path]) -> Result<HookOutput> {
    let args: Args = parse_hook_args(hook)?;
    let context = Context {
        base: hook.project().relative_path().to_path_buf(),
        cache_dir: store.cache_path(CacheBucket::CheckJsonschema),
        client: REQWEST_CLIENT.clone(),
    };
    check(args, context, filenames).await
}

pub(super) async fn check(args: Args, context: Context, selected: &[&Path]) -> Result<HookOutput> {
    let files: Vec<PathBuf> = hook_filenames(&args.filenames, selected)
        .map(Path::to_path_buf)
        .collect();
    let runtime = Handle::current();
    // Schema downloads block on the runtime, so all the work runs on the blocking pool.
    tokio::task::spawn_blocking(move || check_blocking(&args, &context, runtime, &files)).await?
}

enum Schema {
    Fixed {
        validator: Validator,
        /// The schema, kept for `--fill-defaults`.
        defaults: Option<Value>,
    },
    Meta(MetaValidators),
}

fn check_blocking(
    args: &Args,
    context: &Context,
    runtime: Handle,
    files: &[PathBuf],
) -> Result<HookOutput> {
    let mode = args.schema_mode()?;
    let settings = args.settings();
    let cache_dir = if args.no_cache {
        None
    } else {
        Some(context.cache_dir.clone())
    };
    let downloader = Arc::new(Downloader::new(context.client.clone(), cache_dir, runtime));

    let loaded = match mode {
        SchemaMode::Metaschema => None,
        SchemaMode::File(schemafile) => Some(source::load_schemafile(
            schemafile,
            &context.base,
            &downloader,
        )),
        SchemaMode::Builtin(name) => Some(source::load_builtin(name)),
    };
    let schema = match loaded {
        None => Schema::Meta(MetaValidators::new(settings)),
        Some(Err(message)) => return Ok(failure(&message)),
        Some(Ok(loaded)) => {
            let defaults = if args.fill_defaults {
                Some(loaded.schema.clone())
            } else {
                None
            };
            let retriever = Arc::new(SchemaRetriever::new(downloader));
            match validator::build(loaded, &settings, &retriever) {
                Ok(validator) => Schema::Fixed {
                    validator,
                    defaults,
                },
                Err(message) => return Ok(failure(&message)),
            }
        }
    };

    let results = files
        .par_iter()
        .map(|file| check_file(&schema, args, &context.base.join(file), file))
        .collect::<Result<Vec<_>, String>>();
    let results = match results {
        Ok(results) => results,
        Err(message) => return Ok(failure(&message)),
    };
    let (exit_status, output) = report::render(&results, args.output_format, args.verbosity());
    Ok(HookOutput::unchanged(exit_status, output))
}

/// Errors that stop the whole check (bad schema, failed download) fail this hook only.
fn failure(message: &str) -> HookOutput {
    HookOutput::unchanged(1, format!("{message}\n").into_bytes())
}

fn check_file(
    schema: &Schema,
    args: &Args,
    file_path: &Path,
    display_path: &Path,
) -> Result<FileResult, String> {
    let path = display_path.display().to_string();
    let content = fs_err::read(file_path).map_err(|err| err.to_string())?;
    let file_type = match args.force_filetype {
        Some(file_type) => file_type,
        None => FileType::detect(display_path, args.default_filetype),
    };
    let mut instance = match load_instance(&content, file_type, args.data_transform) {
        Ok(instance) => instance,
        Err(message) => {
            return Ok(FileResult {
                path,
                outcome: Outcome::ParseError(message),
            });
        }
    };
    let meta_validator;
    let validator = match schema {
        Schema::Fixed {
            validator,
            defaults,
        } => {
            if let Some(defaults) = defaults {
                validator::fill_defaults(defaults, &mut instance);
            }
            validator
        }
        Schema::Meta(validators) => match validators.for_document(&instance)? {
            Some(validator) => {
                meta_validator = validator;
                &*meta_validator
            }
            None => {
                return Ok(FileResult {
                    path,
                    outcome: Outcome::Valid,
                });
            }
        },
    };
    let issues: Vec<Issue> = validator
        .iter_errors(&instance)
        .map(|error| Issue::new(&error))
        .collect();
    let outcome = if issues.is_empty() {
        Outcome::Valid
    } else {
        Outcome::Invalid(issues)
    };
    Ok(FileResult { path, outcome })
}

fn load_instance(
    content: &[u8],
    file_type: FileType,
    transform: Option<DataTransform>,
) -> Result<Value, String> {
    let Ok(content) = simdutf8::compat::from_utf8(content) else {
        return Err("invalid UTF-8".to_string());
    };
    let Some(transform) = transform else {
        return file_type.parse(content);
    };
    let content = if file_type == FileType::Yaml {
        transform.preprocess(content)?
    } else {
        content.into()
    };
    transform.apply(file_type.parse(&content)?)
}
