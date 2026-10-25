use std::borrow::Cow;
use std::fmt;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use fancy_regex::Regex;

use crate::hook::Hook;
use crate::hooks::builtin_hooks::commit_messages::commit_messages;
use crate::hooks::{HookOutput, parse_hook_args};

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
#[command(disable_version_flag = true)]
#[command(disable_help_flag = true)]
#[command(group(clap::ArgGroup::new("rules").required(true).multiple(true).args(["require", "deny"])))]
pub(crate) struct Args {
    /// Require a non-empty trailer value, optionally matching a regex. Repeat for multiple rules.
    #[arg(long, value_name = "KEY[=REGEX]", value_parser = parse_rule)]
    require: Vec<Rule>,
    /// Deny a trailer, or any of its values matching a regex. Repeat for multiple rules.
    #[arg(long, value_name = "KEY[=REGEX]", value_parser = parse_rule)]
    deny: Vec<Rule>,
}

#[derive(Clone, Debug)]
struct Rule {
    key: String,
    pattern: Option<Regex>,
}

impl Rule {
    fn matches(&self, trailer: &Trailer<'_>) -> Result<bool> {
        if !self.key.eq_ignore_ascii_case(trailer.key) {
            return Ok(false);
        }
        match &self.pattern {
            Some(pattern) => pattern
                .is_match(trailer.value.as_ref())
                .with_context(|| format!("Failed to match trailer rule `{self}`")),
            None => Ok(true),
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key)?;
        if let Some(pattern) = &self.pattern {
            write!(f, "={}", pattern.as_str())?;
        }
        Ok(())
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn parse_rule(value: &str) -> Result<Rule, String> {
    let (key, pattern) = match value.split_once('=') {
        Some((key, pattern)) => {
            if pattern.is_empty() {
                return Err("expected a regular expression after `=`".to_string());
            }
            (
                key,
                Some(Regex::new(pattern).map_err(|err| err.to_string())?),
            )
        }
        None => (value, None),
    };
    if !valid_key(key) {
        return Err(
            "trailer keys must contain only ASCII letters, digits, and hyphens".to_string(),
        );
    }
    Ok(Rule {
        key: key.to_string(),
        pattern,
    })
}

pub(crate) async fn run(hook: &Hook, filenames: &[&Path]) -> Result<HookOutput> {
    let args: Args = parse_hook_args(hook)?;
    for denied in &args.deny {
        if denied.pattern.is_none()
            && args
                .require
                .iter()
                .any(|rule| rule.key.eq_ignore_ascii_case(&denied.key))
        {
            anyhow::bail!(
                "Trailer `{}` cannot be both required and denied",
                denied.key
            );
        }
    }

    let mut output = Vec::new();
    for commit in commit_messages(hook, filenames, true).await? {
        check_message(&commit.source, &commit.message, &args, &mut output)?;
    }
    Ok(HookOutput::unchanged(i32::from(!output.is_empty()), output))
}

struct Trailer<'a> {
    key: &'a str,
    value: Cow<'a, str>,
}

struct TrailerBlock<'a> {
    trailers: Vec<Trailer<'a>>,
    malformed: bool,
}

fn parse_trailers(message: &str) -> TrailerBlock<'_> {
    let mut block = TrailerBlock {
        trailers: Vec::new(),
        malformed: false,
    };
    let mut lines = message.trim_ascii_end().lines().rev();
    let paragraph: Vec<_> = lines
        .by_ref()
        .take_while(|line| !line.trim_ascii().is_empty())
        .collect();
    // The first paragraph is the subject, even if it looks like a trailer.
    if !lines.any(|line| !line.trim_ascii().is_empty()) {
        return block;
    }
    for line in paragraph.into_iter().rev() {
        if line.starts_with([' ', '\t']) {
            if let Some(trailer) = block.trailers.last_mut() {
                let value = trailer.value.to_mut();
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(line.trim_ascii());
            } else {
                block.malformed = true;
            }
        } else if let Some((key, value)) = line.split_once(':')
            && valid_key(key.trim_end_matches([' ', '\t']))
        {
            block.trailers.push(Trailer {
                key: key.trim_end_matches([' ', '\t']),
                value: Cow::Borrowed(value.trim_ascii()),
            });
        } else {
            block.malformed = true;
        }
    }
    block
}

fn check_message(source: &str, message: &str, args: &Args, output: &mut Vec<u8>) -> Result<()> {
    let block = parse_trailers(message);
    if block.malformed
        && block.trailers.iter().any(|trailer| {
            args.require
                .iter()
                .chain(&args.deny)
                .any(|rule| rule.key.eq_ignore_ascii_case(trailer.key))
        })
    {
        // Treating a malformed candidate as absent would let deny rules pass silently.
        writeln!(
            output,
            "{source}: trailer block contains extra text; move it above the trailers"
        )?;
        return Ok(());
    }
    for rule in &args.require {
        let mut found = false;
        for trailer in &block.trailers {
            if !trailer.value.is_empty() && rule.matches(trailer)? {
                found = true;
                break;
            }
        }
        if !found {
            if !block
                .trailers
                .iter()
                .any(|trailer| rule.key.eq_ignore_ascii_case(trailer.key))
            {
                writeln!(output, "{source}: missing required trailer `{}`", rule.key)?;
            } else if let Some(pattern) = &rule.pattern {
                writeln!(
                    output,
                    "{source}: trailer `{}` needs a value matching `{pattern}`",
                    rule.key
                )?;
            } else {
                writeln!(output, "{source}: trailer `{}` needs a value", rule.key)?;
            }
        }
    }
    for rule in &args.deny {
        for trailer in &block.trailers {
            if rule.matches(trailer)? {
                if rule.pattern.is_some() {
                    writeln!(
                        output,
                        "{source}: value `{}` is not allowed for trailer `{}`",
                        trailer.value, trailer.key
                    )?;
                } else {
                    writeln!(output, "{source}: trailer `{}` is not allowed", trailer.key)?;
                }
                break;
            }
        }
    }
    Ok(())
}
