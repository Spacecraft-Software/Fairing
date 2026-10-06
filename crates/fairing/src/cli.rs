// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Command-line surface: global flags (CLI Standard §3) and the verb tree.
//!
//! `--version` is implemented by hand so both renderings carry the Steelbore
//! Standard §15.2 attribution block.

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;

/// Project URL (Steelbore Standard §15.1).
pub const WEBSITE: &str = "https://Fairing.SpacecraftSoftware.org/";
/// Maintainer line (Steelbore Standard §15.2).
pub const MAINTAINER: &str = "Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>";
/// Copyright line (Steelbore Standard §15.2).
pub const COPYRIGHT: &str =
    "Copyright (C) 2026 Mohamed Hammad & Spacecraft Software  |  License: GPL-3.0-or-later";

const AFTER_HELP: &str = "\
Examples:
  fairing describe              capability manifest, human-readable
  fairing describe --json       the same manifest as a JSON envelope
  fairing schema describe       JSON Schema for one command

Maintained by Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
https://Fairing.SpacecraftSoftware.org/";

/// `fairing` — boot splash for Steelbore OS Bravais.
#[derive(Debug, Parser)]
#[command(
    name = "fairing",
    bin_name = "fairing",
    about = "Boot splash for Steelbore OS Bravais: real systemd progress, in-splash LUKS prompt, greetd handoff",
    disable_version_flag = true,
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP
)]
pub struct Cli {
    /// Global flags, identical across every Spacecraft Software CLI.
    #[command(flatten)]
    pub global: GlobalFlags,
    /// The verb.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Output format (CLI Standard §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Human-oriented text (the TTY default).
    Human,
    /// One JSON document with the `metadata` + `data` envelope.
    Json,
    /// Newline-delimited JSON.
    Jsonl,
    /// YAML 1.2 (not available in this milestone).
    Yaml,
    /// RFC 4180 CSV (not available in this milestone).
    Csv,
    /// Interactive TUI (not available in this milestone).
    Explore,
}

/// Colour policy (CLI Standard §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorChoice {
    /// Decide from the environment and the TTY.
    Auto,
    /// Always emit colour.
    Always,
    /// Never emit colour.
    Never,
}

/// Global flags (CLI Standard §3), accepted before or after the verb.
#[derive(Debug, Args)]
pub struct GlobalFlags {
    /// Print version and attribution, then exit.
    #[arg(long, short = 'V', global = true)]
    pub version: bool,
    /// Alias for `--format json`.
    #[arg(long, global = true, conflicts_with = "format")]
    pub json: bool,
    /// Output format.
    #[arg(long, global = true, value_enum)]
    pub format: Option<Format>,
    /// Restrict machine output to these top-level fields (comma-separated).
    #[arg(long, global = true, value_delimiter = ',', value_name = "f1,f2,...")]
    pub fields: Vec<String>,
    /// Emit the action plan without side effects.
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// Lower the severity floor to `info`.
    #[arg(long, short = 'v', global = true, conflicts_with = "quiet")]
    pub verbose: bool,
    /// Raise the severity floor to `error`.
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,
    /// Disable ANSI colour (same as `--color never`).
    #[arg(long, global = true, conflicts_with = "color")]
    pub no_color: bool,
    /// Colour policy.
    #[arg(long, global = true, value_enum, value_name = "when")]
    pub color: Option<ColorChoice>,
    /// Render timestamps as absolute ISO 8601 UTC in human mode (the only form Fairing uses).
    #[arg(long, global = true)]
    pub absolute_time: bool,
    /// NUL-delimited output for `xargs -0`.
    #[arg(long, short = '0', global = true)]
    pub print0: bool,
    /// Skip confirmations in non-TTY mode.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Skip confirmations and overwrite.
    #[arg(long, global = true)]
    pub force: bool,
}

/// The verb tree. Splash, shutdown, theme and preview arrive with M1–M4.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Emit the capability manifest (safe, no side effects).
    #[command(
        after_help = "Examples:\n  fairing describe\n  fairing describe --json\n  fairing describe --json --fields tool,version,commands"
    )]
    Describe,
    /// Emit JSON Schema (Draft 2020-12) for the tool or one command.
    #[command(
        after_help = "Examples:\n  fairing schema\n  fairing schema describe --json\n  fairing schema schema"
    )]
    Schema {
        /// Command path to describe, e.g. `describe`; omit for the whole tool.
        #[arg(value_name = "command")]
        command: Vec<String>,
    },
}

/// Reconstructs the invocation as the envelope reports it: `fairing <args…>`.
#[must_use]
pub fn invocation(argv: &[String]) -> String {
    let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    if rest.is_empty() {
        "fairing".to_owned()
    } else {
        format!("fairing {}", rest.join(" "))
    }
}

#[derive(Debug, Serialize)]
struct VersionData {
    name: &'static str,
    version: &'static str,
    maintainer: &'static str,
    website: &'static str,
    copyright: &'static str,
    license: &'static str,
}

/// Prints `--version` in the mode's rendering.
///
/// # Errors
///
/// Returns an internal error when the envelope cannot be serialised.
pub fn print_version(context: &Context, invocation: &str) -> Result<(), AppError> {
    let data = VersionData {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        maintainer: MAINTAINER,
        website: WEBSITE,
        copyright: COPYRIGHT,
        license: "GPL-3.0-or-later",
    };
    if context.mode.is_machine() {
        Response::new(invocation, data).emit(context, &[])
    } else {
        println!("{} {}", data.name, data.version);
        println!("Maintained by {MAINTAINER}");
        println!("{COPYRIGHT}");
        println!("{WEBSITE}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    #[test]
    fn command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn invocation_round_trips_arguments() {
        let argv = [
            "/usr/bin/fairing".to_owned(),
            "describe".to_owned(),
            "--json".to_owned(),
        ];
        assert_eq!(invocation(&argv), "fairing describe --json");
        assert_eq!(invocation(&argv[..1]), "fairing");
    }
}
