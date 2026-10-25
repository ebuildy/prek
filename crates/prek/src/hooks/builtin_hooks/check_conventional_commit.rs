use std::io::Write;
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use git_conventional::Commit;
use itertools::Itertools;
use regex::Regex;

use crate::hook::Hook;
use crate::hooks::builtin_hooks::commit_messages::commit_messages;
use crate::hooks::{HookOutput, parse_hook_args};

const DEFAULT_TYPES: &[&str] = &[
    "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
];

/// Prefixes Git uses for commits that `git rebase --autosquash` folds into earlier commits.
const AUTOSQUASH_PREFIXES: &[&str] = &["amend! ", "fixup! ", "squash! "];

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
#[command(disable_version_flag = true)]
#[command(disable_help_flag = true)]
pub(crate) struct Args {
    /// Require a scope, as in `feat(api): description`.
    #[arg(long)]
    force_scope: bool,
    /// Comma-separated list of allowed scopes.
    #[arg(long, value_name = "SCOPE,...", value_delimiter = ',')]
    scopes: Vec<String>,
    /// Also check `fixup!`, `squash!`, `amend!`, and merge commits.
    #[arg(long)]
    strict: bool,
    /// Allowed commit types. `feat` and `fix` are always allowed. Defaults to build, chore, ci,
    /// docs, feat, fix, perf, refactor, revert, style, and test.
    #[arg(value_name = "TYPE")]
    types: Vec<String>,
}

struct Rules {
    types: Vec<String>,
    force_scope: bool,
    scopes: Option<Regex>,
    scope_names: Vec<String>,
}

impl Rules {
    fn new(args: Args) -> Result<Self> {
        let mut types = if args.types.is_empty() {
            DEFAULT_TYPES.iter().map(ToString::to_string).collect()
        } else {
            args.types
        };
        for required in ["feat", "fix"] {
            if !types.iter().any(|ty| ty.eq_ignore_ascii_case(required)) {
                types.push(required.to_string());
            }
        }
        types.sort();

        // A scope may list several allowed scopes, such as `api,client` or `api/client`.
        // Matching with a regex rather than splitting keeps scopes like `pre-commit` intact
        // even though `-` is also accepted as a separator.
        let scopes = if args.scopes.is_empty() {
            None
        } else {
            let names = args
                .scopes
                .iter()
                .map(|scope| regex::escape(scope))
                .join("|");
            Some(Regex::new(&format!(
                r"^(?i:{names})(?:\s*[,/.:#-]\s*(?i:{names}))*$"
            ))?)
        };
        Ok(Self {
            types,
            force_scope: args.force_scope,
            scopes,
            scope_names: args.scopes,
        })
    }

    fn check(&self, source: &str, message: &str, output: &mut Vec<u8>) -> Result<()> {
        let commit = match Commit::parse(message.trim_ascii_end()) {
            Ok(commit) => commit,
            Err(err) => {
                writeln!(output, "{source}: {err}")?;
                return Ok(());
            }
        };
        let ty = commit.type_();
        if !self
            .types
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(ty.as_str()))
        {
            writeln!(
                output,
                "{source}: type `{ty}` is not allowed, expected one of: {}",
                self.types.join(", ")
            )?;
        }
        match commit.scope() {
            Some(scope) => {
                if let Some(allowed) = &self.scopes
                    && !allowed.is_match(scope.as_str().trim())
                {
                    writeln!(
                        output,
                        "{source}: scope `{scope}` is not allowed, expected one of: {}",
                        self.scope_names.join(", ")
                    )?;
                }
            }
            None => {
                if self.force_scope {
                    writeln!(
                        output,
                        "{source}: missing scope, expected `type(scope): description`"
                    )?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) async fn run(hook: &Hook, filenames: &[&Path]) -> Result<HookOutput> {
    let args: Args = parse_hook_args(hook)?;
    let strict = args.strict;
    let rules = Rules::new(args)?;

    let mut output = Vec::new();
    for commit in commit_messages(hook, filenames, !strict).await? {
        if !strict
            && AUTOSQUASH_PREFIXES
                .iter()
                .any(|prefix| commit.message.starts_with(prefix))
        {
            continue;
        }
        rules.check(&commit.source, &commit.message, &mut output)?;
    }
    Ok(HookOutput::unchanged(i32::from(!output.is_empty()), output))
}
