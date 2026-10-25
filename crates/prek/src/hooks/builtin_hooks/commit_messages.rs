use std::path::Path;

use anyhow::{Context, Result};
use itertools::Itertools;
use prek_consts::env_vars::{EnvVars, EnvVarsRead};

use crate::git;
use crate::hook::Hook;

pub(super) struct CommitMessage {
    /// Where the message came from: the commit message file, or an abbreviated commit hash.
    pub(super) source: String,
    pub(super) message: String,
}

/// Collects the commit messages to check: the pending message at `commit-msg`, or the
/// messages of the commits being pushed at `pre-push`.
pub(super) async fn commit_messages(
    hook: &Hook,
    filenames: &[&Path],
    skip_merges: bool,
) -> Result<Vec<CommitMessage>> {
    let mut messages = Vec::new();
    match EnvVars.var(EnvVars::PREK_HOOK_STAGE).as_deref() {
        Ok("commit-msg") => {
            if filenames.is_empty() {
                anyhow::bail!("Expected a commit message filename");
            }
            // Skip commit-msg checks triggered by `git merge`.
            if skip_merges && git::git_dir()?.join("MERGE_HEAD").try_exists()? {
                return Ok(messages);
            }
            for filename in filenames {
                let message = fs_err::tokio::read_to_string(hook.work_dir().join(filename)).await?;
                let message = message
                    .lines()
                    // Ignore content below Git's scissors marker, including verbose diffs.
                    .take_while(|line| {
                        *line != "# ------------------------ >8 ------------------------"
                    })
                    .filter(|line| !line.starts_with('#'))
                    .join("\n");
                messages.push(CommitMessage {
                    source: filename.display().to_string(),
                    message,
                });
            }
        }
        Ok("pre-push") => {
            let to = EnvVars
                .var(EnvVars::PRE_COMMIT_TO_REF)
                .context("The pre-push stage requires a commit range")?;
            let range = match EnvVars.var(EnvVars::PRE_COMMIT_FROM_REF) {
                Ok(from) => format!("{from}..{to}"),
                Err(_) => to,
            };
            let mut cmd = git::git_cmd()?;
            cmd.current_dir(hook.work_dir()).arg("log");
            if skip_merges {
                cmd.arg("--no-merges");
            }
            let commits = cmd
                .args([
                    "--no-show-signature",
                    "--format=%h%n%B",
                    "-z",
                    "--end-of-options",
                ])
                .arg(range)
                .arg("--")
                .output()
                .await?;
            let commits = String::from_utf8_lossy(&commits.stdout);
            for record in commits.split('\0').filter(|record| !record.is_empty()) {
                let (source, message) = record
                    .split_once('\n')
                    .context("Invalid commit message record from Git")?;
                messages.push(CommitMessage {
                    source: source.to_string(),
                    message: message.to_string(),
                });
            }
        }
        _ => anyhow::bail!("{} requires the commit-msg or pre-push stage", hook.id),
    }
    Ok(messages)
}
