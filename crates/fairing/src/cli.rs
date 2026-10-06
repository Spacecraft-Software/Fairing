// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Command-line surface: global flags (CLI Standard §3) and the verb tree.
//!
//! `--version` is implemented by hand so both renderings carry the Steelbore
//! Standard §15.2 attribution block.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;
use crate::output::write_line;

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
  fairing preview --seconds 3   draw the splash on this VT for three seconds
  fairing preview --backend memory --snapshot frame.ppm --json

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

/// Output backend for `preview`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendChoice {
    /// DRM/KMS, then `/dev/fb0`.
    Auto,
    /// DRM/KMS only.
    Drm,
    /// `/dev/fb0` only.
    Fbdev,
    /// A heap buffer; pair with `--snapshot`.
    Memory,
}

impl BackendChoice {
    /// The render crate's chain for this choice.
    #[must_use]
    pub const fn chain(self) -> fairing_render::Choice {
        match self {
            Self::Auto => fairing_render::Choice::Auto,
            Self::Drm => fairing_render::Choice::Drm,
            Self::Fbdev => fairing_render::Choice::Fbdev,
            Self::Memory => fairing_render::Choice::Memory,
        }
    }
}

/// Arguments of `fairing preview`.
#[derive(Debug, Args)]
pub struct PreviewArgs {
    /// How long to run, in seconds; fractions allowed, `0` draws a single frame.
    #[arg(long, default_value = "5", value_name = "n", value_parser = parse_seconds)]
    pub seconds: f64,
    /// Which backend draws; `auto` tries DRM/KMS, then `/dev/fb0`.
    #[arg(long, value_enum, default_value_t = BackendChoice::Auto, value_name = "backend")]
    pub backend: BackendChoice,
    /// Write the final frame to this path as a binary PPM (`P6`).
    #[arg(long, value_name = "file.ppm")]
    pub snapshot: Option<PathBuf>,
    /// Registered palette slug to draw with; default is the §11.6 resolution.
    #[arg(long, value_name = "slug")]
    pub palette: Option<String>,
    /// Status line shown under the bar.
    #[arg(
        long,
        value_name = "text",
        default_value = "Starting Steelbore OS Bravais"
    )]
    pub status: String,
    /// Frame size for the memory backend, `WxH`.
    #[arg(long, value_name = "WxH", default_value = "1920x1080", value_parser = parse_size)]
    pub size: (u32, u32),
    /// Frames per second.
    #[arg(long, default_value_t = 30, value_name = "hz", value_parser = clap::value_parser!(u32).range(1..=240))]
    pub fps: u32,
    /// Compiled theme to preview; arrives with the theme format (M2) and is hidden until then.
    #[arg(long, value_name = "path", hide = true)]
    pub theme: Option<PathBuf>,
}

/// Seconds as a finite number in `0.0..=3600.0`.
fn parse_seconds(text: &str) -> Result<f64, String> {
    let seconds: f64 = text
        .parse()
        .map_err(|_e| format!("`{text}` is not a number of seconds"))?;
    if !(0.0..=3600.0).contains(&seconds) {
        return Err(format!("`{text}` is outside 0..=3600 seconds"));
    }
    Ok(seconds)
}

/// `WxH` with both sides in `1..=16384`.
fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let invalid = || format!("`{text}` is not `WxH`");
    let (w, h) = text.split_once('x').ok_or_else(invalid)?;
    // Digits only, as the schema pattern promises: no sign, no whitespace.
    let digits = |side: &str| -> Result<u32, String> {
        if side.is_empty() || !side.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        side.parse().map_err(|_e| invalid())
    };
    let width = digits(w)?;
    let height = digits(h)?;
    if !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) {
        return Err(format!("`{text}` is outside 1x1..=16384x16384"));
    }
    Ok((width, height))
}

/// The verb tree. Splash, shutdown and theme arrive with M2–M4.
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
    /// Draw the splash with a simulated bar for a few seconds, then restore the console.
    #[command(
        after_help = "Examples:\n  fairing preview --seconds 3\n  fairing preview --backend memory --snapshot frame.ppm --json\n  fairing preview --palette steelbore-high-contrast --status \"Mounting /home\""
    )]
    Preview(PreviewArgs),
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

/// Prints `--version` in the mode's rendering; `fields` narrows the envelope.
///
/// # Errors
///
/// Returns an internal error when the envelope cannot be serialised or stdout
/// cannot be written.
pub fn print_version(
    context: &Context,
    invocation: &str,
    fields: &[String],
) -> Result<(), AppError> {
    let data = VersionData {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        maintainer: MAINTAINER,
        website: WEBSITE,
        copyright: COPYRIGHT,
        license: "GPL-3.0-or-later",
    };
    if context.mode.is_machine() {
        return Response::new(invocation, data)
            .with_context(context, false)
            .emit(context, fields);
    }
    let lines = [
        format!("{} {}", data.name, data.version),
        format!("Maintained by {MAINTAINER}"),
        COPYRIGHT.to_owned(),
        WEBSITE.to_owned(),
    ];
    for line in &lines {
        write_line(line, invocation)?;
    }
    Ok(())
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
    fn preview_value_parsers_bound_their_inputs() {
        assert_eq!(parse_seconds("0"), Ok(0.0));
        assert_eq!(parse_seconds("2.5"), Ok(2.5));
        assert!(parse_seconds("-1").is_err() && parse_seconds("inf").is_err());
        assert!(parse_seconds("abc").is_err());
        assert_eq!(parse_size("640x360"), Ok((640, 360)));
        assert!(parse_size("640").is_err() && parse_size("0x1").is_err());
        assert!(parse_size("99999x1").is_err());
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
