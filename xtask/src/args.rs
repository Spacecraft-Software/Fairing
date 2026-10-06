// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Minimal argument parsing for the task runner.
//!
//! The grammar is deliberately tiny: `<verb>` followed by `--flag`, `--key value`
//! or `--key=value`. A verb validates its own flag set with [`Args::ensure_known`]
//! so a typo is a usage error rather than a silently ignored option.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::paths;
use crate::report::Failure;

/// Parsed command line: a verb plus its flags.
#[derive(Debug, Clone, Default)]
pub struct Args {
    verb: String,
    flags: BTreeMap<String, Option<String>>,
    raw: Vec<String>,
}

impl Args {
    /// Parses `argv` (program name excluded).
    ///
    /// # Errors
    ///
    /// Returns a usage failure when no verb is given or a positional token
    /// follows the verb.
    pub fn parse<I>(argv: I) -> Result<Self, Failure>
    where
        I: IntoIterator<Item = String>,
    {
        let raw: Vec<String> = argv.into_iter().collect();
        let mut tokens = raw.iter().peekable();
        let verb = tokens
            .next()
            .cloned()
            .ok_or_else(|| Failure::usage("no verb given", "cargo xtask help"))?;
        let mut flags = BTreeMap::new();
        while let Some(token) = tokens.next() {
            let Some(body) = token.strip_prefix("--") else {
                return Err(Failure::usage(
                    format!("unexpected positional argument `{token}`"),
                    "cargo xtask help",
                ));
            };
            if let Some((key, value)) = body.split_once('=') {
                flags.insert(key.to_owned(), Some(value.to_owned()));
                continue;
            }
            let takes_value = tokens.peek().is_some_and(|next| !next.starts_with("--"));
            let value = if takes_value {
                tokens.next().cloned()
            } else {
                None
            };
            flags.insert(body.to_owned(), value);
        }
        Ok(Self { verb, flags, raw })
    }

    /// The verb, e.g. `trace`.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// Whether `--name` was given (with or without a value).
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    /// The value of `--name`, if one was supplied.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(Option::as_deref)
    }

    /// Whether `--json` output was requested.
    #[must_use]
    pub fn json(&self) -> bool {
        self.flag("json")
    }

    /// The repository root: `--root` if given, else the workspace containing `xtask`.
    #[must_use]
    pub fn root(&self) -> PathBuf {
        self.value("root")
            .map_or_else(paths::default_root, PathBuf::from)
    }

    /// The reconstructed invocation, used in envelopes and diagnostics.
    #[must_use]
    pub fn command_line(&self) -> String {
        std::iter::once("xtask")
            .chain(self.raw.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Rejects any flag outside `allowed` (plus the global `--root` / `--json`).
    ///
    /// # Errors
    ///
    /// Returns a usage failure naming the first unknown flag.
    pub fn ensure_known(&self, allowed: &[&str]) -> Result<(), Failure> {
        for key in self.flags.keys() {
            let known = key == "root" || key == "json" || allowed.contains(&key.as_str());
            if !known {
                return Err(Failure::usage(
                    format!("unknown flag `--{key}` for `{}`", self.verb),
                    "cargo xtask help",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Args {
        Args::parse(line.split_whitespace().map(str::to_owned)).unwrap_or_default()
    }

    #[test]
    fn parses_verb_and_flag_forms() {
        let args = parse("trace --json --out target/t --requirements=doc/r.toml");
        assert_eq!(args.verb(), "trace");
        assert!(args.json());
        assert_eq!(args.value("out"), Some("target/t"));
        assert_eq!(args.value("requirements"), Some("doc/r.toml"));
        assert!(!args.flag("check"));
    }

    #[test]
    fn bare_flag_before_another_flag_takes_no_value() {
        let args = parse("req-texi --check --json");
        assert!(args.flag("check"));
        assert_eq!(args.value("check"), None);
        assert!(args.json());
    }

    #[test]
    fn missing_verb_is_usage_error() {
        let failure = Args::parse(Vec::<String>::new()).err();
        assert!(failure.is_some_and(|f| f.exit_code() == 2));
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let args = parse("trace --bogus");
        assert!(args.ensure_known(&["out"]).is_err());
        assert!(args.ensure_known(&["out", "bogus"]).is_ok());
    }
}
