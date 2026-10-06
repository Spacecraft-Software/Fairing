// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The output-mode cascade (CLI Standard §5) and colour precedence (§6).
//!
//! Agent detection is presence-based: `AI_AGENT` or `AGENT` set to any
//! non-empty value other than `0`/`false` means an agent; only `CI` carries a
//! truthy value. `CLAUDECODE`, `CURSOR_AGENT` and `GEMINI_CLI` are informational
//! (FRN-SRS-084): they make Fairing non-interactive and colourless but never
//! change the format on their own.

use std::io::IsTerminal as _;

use crate::cli::{ColorChoice, Format, GlobalFlags};
use crate::diagnostic::Severity;

/// How stdout is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Human-oriented text.
    Human,
    /// Single JSON document.
    Json,
    /// Newline-delimited JSON.
    Jsonl,
    /// YAML (unavailable in this milestone; reported as `FEATURE_UNAVAILABLE`).
    Yaml,
    /// CSV (unavailable in this milestone).
    Csv,
}

impl Mode {
    /// Whether the mode is machine-readable (suppresses colour, prompts and TUI).
    #[must_use]
    pub const fn is_machine(self) -> bool {
        !matches!(self, Self::Human)
    }

    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Yaml => "yaml",
            Self::Csv => "csv",
        }
    }
}

/// Which reader we believe we are serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// A person at a terminal.
    Human,
    /// An AI agent (`AI_AGENT` / `AGENT`).
    Agent,
    /// A CI pipeline (`CI` truthy).
    Ci,
}

impl Profile {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
            Self::Ci => "ci",
        }
    }
}

/// The resolved rendering context, computed once at entry and threaded everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// stdout rendering.
    pub mode: Mode,
    /// Whether ANSI colour may be emitted (always false in machine mode).
    pub color: bool,
    /// Whether prompts may be shown.
    pub interactive: bool,
    /// Minimum diagnostic severity written to stderr.
    pub floor: Severity,
    /// Who we think is reading.
    pub profile: Profile,
    /// Detected harness label for `metadata.tool_agent`, if any.
    pub tool_agent: Option<&'static str>,
    /// Whether a TUI was requested and refused.
    pub explore_requested: bool,
}

impl Context {
    /// Resolves the context from parsed flags and the environment.
    #[must_use]
    pub fn resolve(flags: &GlobalFlags) -> Self {
        let explicit = if flags.json {
            Some(Format::Json)
        } else {
            flags.format
        };
        let color_flag = if flags.no_color {
            Some(ColorChoice::Never)
        } else {
            flags.color
        };
        Self::compute(
            explicit,
            color_flag,
            flags.quiet,
            flags.verbose,
            &Env::read(),
        )
    }

    /// Best-effort context before clap has parsed anything (used to render parse errors).
    #[must_use]
    pub fn from_argv(argv: &[String]) -> Self {
        let json = argv.iter().any(|a| a == "--json")
            || argv
                .windows(2)
                .any(|w| w[0] == "--format" && w[1] != "human")
            || argv
                .iter()
                .any(|a| a.starts_with("--format=") && a != "--format=human");
        let explicit = json.then_some(Format::Json);
        Self::compute(explicit, None, false, false, &Env::read())
    }

    fn compute(
        explicit: Option<Format>,
        color_flag: Option<ColorChoice>,
        quiet: bool,
        verbose: bool,
        env: &Env,
    ) -> Self {
        let profile = if env.agent {
            Profile::Agent
        } else if env.ci {
            Profile::Ci
        } else {
            Profile::Human
        };
        let stdout_tty = std::io::stdout().is_terminal();
        let mut explore_requested = false;
        let default_mode = if profile == Profile::Human && stdout_tty {
            Mode::Human
        } else {
            Mode::Json
        };
        let mode = match explicit {
            Some(Format::Human) => Mode::Human,
            Some(Format::Json) => Mode::Json,
            Some(Format::Jsonl) => Mode::Jsonl,
            Some(Format::Yaml) => Mode::Yaml,
            Some(Format::Csv) => Mode::Csv,
            Some(Format::Explore) => {
                // The TUI does not exist yet, and would be refused for agents,
                // dumb terminals and pipes anyway (CLI Standard §5).
                explore_requested = true;
                Mode::Json
            }
            None => default_mode,
        };
        let interactive = mode == Mode::Human
            && profile == Profile::Human
            && !env.informational_agent
            && stdout_tty;
        let color = !mode.is_machine()
            && !env.informational_agent
            && match color_flag {
                Some(ColorChoice::Never) => false,
                Some(ColorChoice::Always) => true,
                Some(ColorChoice::Auto) | None => {
                    if env.force_color {
                        true
                    } else if env.no_color || env.clicolor_zero || env.dumb_term {
                        false
                    } else {
                        stdout_tty
                    }
                }
            };
        let floor = if quiet {
            Severity::Error
        } else if verbose {
            Severity::Info
        } else if profile == Profile::Agent {
            Severity::Warn
        } else {
            Severity::Ok
        };
        Self {
            mode,
            color,
            interactive,
            floor,
            profile,
            tool_agent: env.tool_agent,
            explore_requested,
        }
    }
}

/// The environment variables the cascade reads (CLI Standard §5–§6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Env {
    agent: bool,
    ci: bool,
    informational_agent: bool,
    tool_agent: Option<&'static str>,
    force_color: bool,
    no_color: bool,
    clicolor_zero: bool,
    dumb_term: bool,
}

impl Env {
    fn read() -> Self {
        let present = |name: &str| {
            std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0" && v != "false")
        };
        let set_non_empty = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let tool_agent = [
            ("CLAUDECODE", "claude-code"),
            ("CURSOR_AGENT", "cursor"),
            ("GEMINI_CLI", "gemini-cli"),
        ]
        .into_iter()
        .find(|(var, _)| set_non_empty(var))
        .map(|(_, label)| label);
        Self {
            agent: present("AI_AGENT") || present("AGENT"),
            ci: present("CI"),
            informational_agent: tool_agent.is_some(),
            tool_agent,
            force_color: set_non_empty("FORCE_COLOR"),
            no_color: set_non_empty("NO_COLOR"),
            clicolor_zero: std::env::var("CLICOLOR").as_deref() == Ok("0"),
            dumb_term: std::env::var("TERM").as_deref() == Ok("dumb"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env::default()
    }

    #[test]
    fn explicit_json_wins_over_everything() {
        let ctx = Context::compute(
            Some(Format::Json),
            Some(ColorChoice::Always),
            false,
            false,
            &env(),
        );
        assert_eq!(ctx.mode, Mode::Json);
        assert!(!ctx.color, "machine mode never carries colour");
    }

    #[test]
    fn agent_env_forces_json_and_warn_floor() {
        let ctx = Context::compute(
            None,
            None,
            false,
            false,
            &Env {
                agent: true,
                ..env()
            },
        );
        assert_eq!(
            (ctx.mode, ctx.profile, ctx.floor),
            (Mode::Json, Profile::Agent, Severity::Warn)
        );
        assert!(!ctx.interactive);
    }

    #[test]
    fn ci_forces_json_with_default_floor() {
        let ctx = Context::compute(None, None, false, false, &Env { ci: true, ..env() });
        assert_eq!(
            (ctx.mode, ctx.profile, ctx.floor),
            (Mode::Json, Profile::Ci, Severity::Ok)
        );
    }

    #[test]
    fn force_color_beats_no_color_in_human_mode() {
        let e = Env {
            force_color: true,
            no_color: true,
            ..env()
        };
        let ctx = Context::compute(Some(Format::Human), None, false, false, &e);
        assert!(ctx.color);
        let e = Env {
            no_color: true,
            ..env()
        };
        let ctx = Context::compute(Some(Format::Human), None, false, false, &e);
        assert!(!ctx.color);
    }

    #[test]
    fn informational_agent_disables_colour_but_not_format() {
        let e = Env {
            informational_agent: true,
            tool_agent: Some("claude-code"),
            force_color: true,
            ..env()
        };
        let ctx = Context::compute(Some(Format::Human), None, false, false, &e);
        assert_eq!(ctx.mode, Mode::Human);
        assert!(!ctx.color && !ctx.interactive);
        assert_eq!(ctx.tool_agent, Some("claude-code"));
    }

    #[test]
    fn quiet_and_verbose_set_the_floor() {
        assert_eq!(
            Context::compute(None, None, true, false, &env()).floor,
            Severity::Error
        );
        assert_eq!(
            Context::compute(None, None, false, true, &env()).floor,
            Severity::Info
        );
    }

    #[test]
    fn explore_falls_back_to_json() {
        let ctx = Context::compute(Some(Format::Explore), None, false, false, &env());
        assert_eq!(ctx.mode, Mode::Json);
        assert!(ctx.explore_requested);
    }
}
