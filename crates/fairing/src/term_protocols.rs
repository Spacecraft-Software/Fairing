// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Terminal protocols for the human-mode verbs (spacecraft-tui-protocols 1.0.0).
//!
//! Only OSC 9;4 progress is spoken: `fairing preview` reports its simulated bar
//! in the terminal's tab, title bar or taskbar. It is decorative; the bar is
//! reported in the verb's own output too. The splash never comes through here:
//! it draws on DRM/KMS or the Linux console, which understand none of this.
//!
//! The five rules of the skill, as built:
//!
//! 1. Gate before emitting (§2.1): stderr is a TTY, no agent or CI harness, `TERM`
//!    is neither `dumb` nor unset, and the user has not turned the protocol off.
//! 2. No probe exists for OSC 9;4, so nothing is asked and nothing can block
//!    (§2.2); identity hints stand in.
//! 3. The bar is cleared when the guard drops and from a panic hook (§2.3). A
//!    SIGINT or SIGTERM ends the process by its default action and leaves the bar
//!    to the terminal: handling it would mean the verb's loops stop on a flag,
//!    which a verb that is only a preview does not need.
//! 4. Allow-list only (§2.4): on a terminal that reads `OSC 9 ; text` as a
//!    desktop notification, every update would pop one reading `4;1;50`.
//! 5. `--term-protocols` and `SPACECRAFT_TERM_PROTOCOLS` beat detection (§2.5).
//!
//! Inside tmux, zellij or screen nothing is sent: wrapping in passthrough is
//! optional and skipping is allowed (§3).

use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The environment variable that carries the override list.
pub const ENV: &str = "SPACECRAFT_TERM_PROTOCOLS";

/// Every protocol name the override list accepts (skill §2.5). Only `progress`
/// is spoken today; the others are accepted so one list serves every
/// Spacecraft program, and `-all` turns everything off.
const NAMES: [&str; 8] = [
    "keyboard",
    "graphics",
    "animation",
    "clipboard",
    "clipboard-rich",
    "progress",
    "pointer",
    "all",
];

/// How often a progress update may be written: about five a second (skill,
/// "Emission rules"). A faster bar flickers some taskbars and gains nothing.
const MIN_INTERVAL: Duration = Duration::from_millis(200);

/// OSC 9;4 with state 0: remove the indicator.
const CLEAR: &[u8] = b"\x1b]9;4;0\x1b\\";

/// Set while a bar is showing, so the panic hook knows to clear it. A release
/// build aborts on panic and runs no `Drop`, hence the hook.
static SHOWING: AtomicBool = AtomicBool::new(false);

/// Installs the clearing panic hook once.
static HOOK: Once = Once::new();

/// What an override list says about the protocols Fairing speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overrides {
    /// `+progress` (`Some(true)`), `-progress` (`Some(false)`), or nothing said.
    progress: Option<bool>,
}

/// A rejected override list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Overrides {
    type Err = ParseError;

    /// Parses `+name` and `-name` items separated by commas; a later item
    /// beats an earlier one, and `all` stands for every name.
    ///
    /// Implements: FRN-SRS-109
    fn from_str(list: &str) -> Result<Self, ParseError> {
        let mut overrides = Self::default();
        for item in list.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            let (on, name) = match (item.strip_prefix('+'), item.strip_prefix('-')) {
                (Some(name), _) => (true, name),
                (_, Some(name)) => (false, name),
                _ => {
                    return Err(ParseError(format!(
                        "`{item}` must start with + (force on) or - (force off)"
                    )));
                }
            };
            if !NAMES.contains(&name) {
                return Err(ParseError(format!(
                    "unknown protocol `{name}`; the names are {}",
                    NAMES.join(", ")
                )));
            }
            if name == "progress" || name == "all" {
                overrides.progress = Some(on);
            }
        }
        Ok(overrides)
    }
}

/// What the environment says about the terminal, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    /// `TERM`.
    pub term: Option<String>,
    /// `TERM_PROGRAM`.
    pub term_program: Option<String>,
    /// `TERM_PROGRAM_VERSION`.
    pub term_program_version: Option<String>,
    /// `WT_SESSION` is set: Windows Terminal.
    pub windows_terminal: bool,
    /// `ConEmuPID` is set or `ConEmuANSI=ON`: `ConEmu`.
    pub conemu: bool,
    /// `TMUX`, `ZELLIJ` or `STY` is set.
    pub multiplexer: bool,
}

impl Identity {
    /// Reads the variables.
    #[must_use]
    pub fn read() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Self {
            term: var("TERM"),
            term_program: var("TERM_PROGRAM"),
            term_program_version: var("TERM_PROGRAM_VERSION"),
            windows_terminal: var("WT_SESSION").is_some(),
            conemu: var("ConEmuPID").is_some() || var("ConEmuANSI").as_deref() == Some("ON"),
            multiplexer: ["TMUX", "ZELLIJ", "STY"]
                .iter()
                .any(|name| var(name).is_some()),
        }
    }

    /// `TERM` is `dumb` or unset: no escape sequence of any kind.
    fn dumb(&self) -> bool {
        self.term.as_deref().is_none_or(|term| term == "dumb")
    }

    /// Whether this terminal is on the OSC 9;4 allow-list.
    ///
    /// kitty is left off: its first release with progress support is not
    /// recorded here, and an older one reads OSC 9 as a notification. Whoever
    /// has one that works says so with `+progress`.
    fn handles_progress(&self) -> bool {
        if self.windows_terminal || self.conemu {
            return true;
        }
        if self.term.as_deref().is_some_and(|t| t.starts_with("foot")) {
            return true;
        }
        let Some(program) = self.term_program.as_deref().map(str::to_ascii_lowercase) else {
            return false;
        };
        let version = self.term_program_version.as_deref().unwrap_or_default();
        if ["ghostty", "vscode", "rio", "contour"]
            .iter()
            .any(|name| program.contains(name))
        {
            return true;
        }
        if program.contains("iterm") {
            // iTerm2 reads OSC 9 as a notification before 3.6.
            return major_minor(version) >= Some((3, 6));
        }
        if program.contains("wezterm") {
            // The 20240203 stable release pops a notification; the snapshots
            // from 20250210 on draw the bar.
            return build_date(version) >= Some(20_250_210);
        }
        false
    }
}

/// `3.6.1` as `(3, 6)`.
fn major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// `WezTerm`'s `20250210-101010-abcdef` as `20250210`.
fn build_date(version: &str) -> Option<u32> {
    let digits = version.split('-').next()?;
    (digits.len() == 8).then(|| digits.parse().ok()).flatten()
}

/// The environment's part of the terminal-protocol decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// The raw `SPACECRAFT_TERM_PROTOCOLS` value, if set.
    pub raw: Option<String>,
    /// The terminal's identity.
    pub identity: Identity,
    /// Whether stderr is a terminal.
    pub stderr_tty: bool,
}

impl Source {
    /// Reads the environment and stderr.
    #[must_use]
    pub fn read() -> Self {
        use std::io::IsTerminal as _;
        Self {
            raw: std::env::var(ENV).ok(),
            identity: Identity::read(),
            stderr_tty: io::stderr().is_terminal(),
        }
    }
}

/// The resolved terminal-protocol state a verb consults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Term {
    /// Whether OSC 9;4 progress may be written to stderr.
    pub progress: bool,
    /// Why the environment's override list was ignored, if it was.
    pub warning: Option<String>,
}

impl Term {
    /// Resolves the state: the flag beats the variable, which beats detection.
    ///
    /// A list that does not parse is ignored with a warning; the flag's own
    /// list was validated by clap.
    ///
    /// Implements: FRN-SRS-109
    #[must_use]
    pub fn resolve(flag: Option<Overrides>, source: &Source, agent_environment: bool) -> Self {
        let mut warning = None;
        let overrides = flag.unwrap_or_else(|| {
            source
                .raw
                .as_deref()
                .map_or_else(Overrides::default, |raw| {
                    raw.parse().unwrap_or_else(|e: ParseError| {
                        warning = Some(format!("{ENV} ignored: {e}"));
                        Overrides::default()
                    })
                })
        });
        let identity = &source.identity;
        let progress = overrides.progress != Some(false)
            && source.stderr_tty
            && !agent_environment
            && !identity.dumb()
            && !identity.multiplexer
            && (overrides.progress == Some(true) || identity.handles_progress());
        Self { progress, warning }
    }
}

/// Writes OSC 9;4 progress, and takes it away again.
#[derive(Debug)]
pub struct Progress<W: Write> {
    out: W,
    enabled: bool,
    shown: bool,
    last_percent: Option<u8>,
    last_at: Option<Instant>,
}

impl<W: Write> Progress<W> {
    /// A reporter that writes to `out` when `enabled`, and nothing otherwise.
    #[must_use]
    pub fn new(out: W, enabled: bool) -> Self {
        if enabled {
            HOOK.call_once(install_hook);
        }
        Self {
            out,
            enabled,
            shown: false,
            last_percent: None,
            last_at: None,
        }
    }

    /// Reports `percent` (0 to 100): only when it changed, and not faster than
    /// five times a second.
    ///
    /// Implements: FRN-SRS-109
    pub fn update(&mut self, percent: u8) {
        self.update_at(percent, Instant::now());
    }

    fn update_at(&mut self, percent: u8, now: Instant) {
        if !self.enabled {
            return;
        }
        let percent = percent.min(100);
        let unchanged = self.last_percent == Some(percent);
        let too_soon = percent < 100
            && self
                .last_at
                .is_some_and(|at| now.duration_since(at) < MIN_INTERVAL);
        if unchanged || too_soon {
            return;
        }
        self.last_percent = Some(percent);
        self.last_at = Some(now);
        SHOWING.store(true, Ordering::Relaxed);
        self.shown = true;
        self.write(format!("\x1b]9;4;1;{percent}\x1b\\").as_bytes());
    }

    /// Removes the indicator, if one is showing.
    pub fn clear(&mut self) {
        if self.shown {
            self.shown = false;
            SHOWING.store(false, Ordering::Relaxed);
            self.write(CLEAR);
        }
    }

    /// A write that fails ends the reporting: the terminal went away.
    fn write(&mut self, bytes: &[u8]) {
        if self
            .out
            .write_all(bytes)
            .and_then(|()| self.out.flush())
            .is_err()
        {
            self.enabled = false;
        }
    }
}

impl<W: Write> Drop for Progress<W> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Chains a hook in front of the current one that clears a showing bar.
fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if SHOWING.swap(false, Ordering::Relaxed) {
            let mut err = io::stderr();
            let _ = err.write_all(CLEAR).and_then(|()| err.flush());
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(program: &str, version: &str) -> Identity {
        Identity {
            term: Some("xterm-256color".into()),
            term_program: Some(program.into()),
            term_program_version: Some(version.into()),
            ..Identity::default()
        }
    }

    fn source(identity: Identity) -> Source {
        Source {
            raw: None,
            identity,
            stderr_tty: true,
        }
    }

    #[test]
    fn overrides_parse_in_order_and_all_covers_progress() {
        // Verifies: FRN-SRS-109
        let parse = |s: &str| s.parse::<Overrides>().map(|o| o.progress);
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("+progress"), Ok(Some(true)));
        assert_eq!(parse("-all"), Ok(Some(false)));
        assert_eq!(parse("-all,+progress"), Ok(Some(true)));
        assert_eq!(parse("+progress,-all"), Ok(Some(false)));
        assert_eq!(parse("-graphics, +clipboard-rich"), Ok(None));
    }

    #[test]
    fn overrides_refuse_bare_and_unknown_names() {
        // Verifies: FRN-SRS-109
        let bare = "progress"
            .parse::<Overrides>()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(bare.contains("must start with + "), "{bare}");
        let unknown = "+cursor"
            .parse::<Overrides>()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(unknown.contains("unknown protocol `cursor`"), "{unknown}");
        assert!(unknown.contains("progress"), "{unknown}");
    }

    #[test]
    fn the_allow_list_names_terminals_that_draw_the_bar() {
        // Verifies: FRN-SRS-109
        for good in [
            identity("ghostty", ""),
            identity("vscode", "1.99"),
            identity("iTerm.app", "3.6.0"),
            identity("WezTerm", "20250210-101010-abc"),
        ] {
            assert!(good.handles_progress(), "{good:?}");
        }
        let windows = Identity {
            windows_terminal: true,
            ..Identity::default()
        };
        assert!(windows.handles_progress());
        let foot = Identity {
            term: Some("foot-direct".into()),
            ..Identity::default()
        };
        assert!(foot.handles_progress());
    }

    #[test]
    fn the_allow_list_leaves_out_terminals_that_pop_a_notification() {
        // Verifies: FRN-SRS-109
        for bad in [
            identity("WezTerm", "20240203-110809-5046fc22"),
            identity("WezTerm", ""),
            identity("iTerm.app", "3.5.11"),
            identity("iTerm.app", ""),
            identity("Apple_Terminal", "455"),
            identity("kitty", "0.40.0"),
            Identity::default(),
        ] {
            assert!(!bad.handles_progress(), "{bad:?}");
        }
    }

    #[test]
    fn detection_needs_a_tty_a_human_and_a_real_terminal() {
        // Verifies: FRN-SRS-109
        let ghostty = identity("ghostty", "");
        assert!(Term::resolve(None, &source(ghostty.clone()), false).progress);
        let piped = Source {
            stderr_tty: false,
            ..source(ghostty.clone())
        };
        assert!(!Term::resolve(None, &piped, false).progress);
        assert!(!Term::resolve(None, &source(ghostty.clone()), true).progress);
        for term in [None, Some("dumb".to_string())] {
            let dumb = Identity {
                term,
                ..ghostty.clone()
            };
            assert!(!Term::resolve(None, &source(dumb), false).progress);
        }
        let tmux = Identity {
            multiplexer: true,
            ..ghostty
        };
        assert!(!Term::resolve(None, &source(tmux), false).progress);
    }

    #[test]
    fn a_user_override_beats_detection_but_not_the_gates() {
        // Verifies: FRN-SRS-109
        let on: Overrides = "+progress".parse().unwrap_or_else(|e| panic!("{e:?}"));
        let off: Overrides = "-all".parse().unwrap_or_else(|e| panic!("{e:?}"));
        let unknown = source(identity("Apple_Terminal", "455"));
        assert!(Term::resolve(Some(on), &unknown, false).progress);
        assert!(
            !Term::resolve(Some(on), &unknown, true).progress,
            "agents get nothing"
        );
        let piped = Source {
            stderr_tty: false,
            ..unknown.clone()
        };
        assert!(
            !Term::resolve(Some(on), &piped, false).progress,
            "no TTY, no sequence"
        );
        let ghostty = source(identity("ghostty", ""));
        assert!(!Term::resolve(Some(off), &ghostty, false).progress);
    }

    #[test]
    fn the_flag_beats_the_variable_and_a_bad_variable_warns() {
        // Verifies: FRN-SRS-109
        let mut from_env = source(identity("Apple_Terminal", "455"));
        from_env.raw = Some("+progress".into());
        assert!(Term::resolve(None, &from_env, false).progress);
        let off: Overrides = "-progress".parse().unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!Term::resolve(Some(off), &from_env, false).progress);

        from_env.raw = Some("progress".into());
        let term = Term::resolve(None, &from_env, false);
        assert!(!term.progress, "an unparsed list changes nothing");
        assert!(term.warning.is_some_and(|w| w.starts_with(ENV)));
    }

    fn written(progress: Progress<Vec<u8>>) -> String {
        // Dropping the reporter clears a showing bar, so take the buffer by hand.
        let mut progress = progress;
        progress.clear();
        let bytes = std::mem::take(&mut progress.out);
        String::from_utf8(bytes).unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn disabled_progress_writes_nothing() {
        // Verifies: FRN-SRS-109
        let mut progress = Progress::new(Vec::new(), false);
        progress.update(10);
        progress.update(100);
        assert_eq!(written(progress), "");
    }

    #[test]
    fn progress_writes_changes_at_most_five_times_a_second_and_clears() {
        // Verifies: FRN-SRS-109
        let mut progress = Progress::new(Vec::new(), true);
        let t0 = Instant::now();
        progress.update_at(10, t0);
        progress.update_at(10, t0 + MIN_INTERVAL * 2); // unchanged
        progress.update_at(11, t0 + Duration::from_millis(50)); // too soon
        progress.update_at(12, t0 + MIN_INTERVAL);
        progress.update_at(100, t0 + MIN_INTERVAL + Duration::from_millis(1)); // the end is not rate-limited
        assert_eq!(
            written(progress),
            "\x1b]9;4;1;10\x1b\\\x1b]9;4;1;12\x1b\\\x1b]9;4;1;100\x1b\\\x1b]9;4;0\x1b\\"
        );
    }

    #[test]
    fn a_reporter_that_never_showed_clears_nothing() {
        // Verifies: FRN-SRS-109
        assert_eq!(written(Progress::new(Vec::new(), true)), "");
    }

    #[test]
    fn a_failed_write_ends_the_reporting() {
        // Verifies: FRN-SRS-109
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut progress = Progress::new(Broken, true);
        progress.update(1);
        assert!(!progress.enabled);
    }
}
