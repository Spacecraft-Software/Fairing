// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Non-error diagnostics: the `[INFO]`/`[OK]`/`[WARN]` ladder and severity floor.
//!
//! Every tag is plain text, which is already the accessible rendering
//! (Steelbore Standard §18.2.1); terminal colour for the tags is deferred.
//!
//! Under systemd, stderr is the journal. There a diagnostic is one text line
//! led by a syslog priority (`<5>[WARN] …`), which journald strips and records
//! as the entry's priority (`SyslogLevelPrefix=yes`, the default), whatever
//! the output mode: the journal is read by people, not parsed as JSON.

use serde::Serialize;

use crate::output::mode::Context;
use crate::time::now_iso8601;

/// Severity ladder; ordered so a floor comparison is a plain `<`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Debugging narration; shown under `--verbose`.
    Info,
    /// A side effect completed.
    Ok,
    /// Degraded, deprecated, or fell back.
    Warn,
    /// Only used as a floor; errors go through `AppError`.
    Error,
}

impl Severity {
    /// The §18.2.1 text tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Info => "[INFO]",
            Self::Ok => "[OK]",
            Self::Warn => "[WARN]",
            Self::Error => "[ERROR]",
        }
    }
}

/// A syslog priority, as journald records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// `LOG_ERR` (3).
    Error,
    /// `LOG_WARNING` (4).
    Warning,
    /// `LOG_NOTICE` (5): normal but significant.
    Notice,
    /// `LOG_INFO` (6).
    Info,
}

impl Priority {
    /// The numeric syslog level.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 4,
            Self::Notice => 5,
            Self::Info => 6,
        }
    }
}

impl Severity {
    /// The journal priority a diagnostic of this severity is recorded at.
    #[must_use]
    pub const fn priority(self) -> Priority {
        match self {
            Self::Info | Self::Ok => Priority::Info,
            Self::Warn => Priority::Warning,
            Self::Error => Priority::Error,
        }
    }
}

/// A non-error diagnostic (CLI Standard `diagnostics.md` §3).
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// `info`, `ok` or `warn`.
    pub severity: Severity,
    /// Stable upper-snake-case code.
    pub code: &'static str,
    /// What happened; lowercase first word, no trailing period.
    pub message: String,
    /// A runnable command, if there is something to run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// ISO 8601 UTC.
    pub timestamp: String,
    /// The invocation.
    pub command: String,
    /// Journal priority when it differs from the severity's own.
    #[serde(skip)]
    pub priority: Option<Priority>,
}

impl Diagnostic {
    /// Creates a diagnostic for `command`.
    pub fn new(
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
        command: &str,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            hint: None,
            timestamp: now_iso8601(),
            command: command.to_owned(),
            priority: None,
        }
    }

    /// Records the diagnostic at `priority` in the journal.
    ///
    /// Requirements fix some priorities independently of how alarming the
    /// message reads: "no backend could draw" is a warning to a person at a
    /// terminal and a notice in the journal (FRN-SRS-003).
    #[must_use]
    pub const fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// The journal line: `<N>[TAG] message`, the hint on the same line.
    #[must_use]
    pub fn journal_line(&self) -> String {
        let priority = self.priority.unwrap_or_else(|| self.severity.priority());
        let hint = self
            .hint
            .as_deref()
            .map(|hint| format!(" (hint: {hint})"))
            .unwrap_or_default();
        format!(
            "<{}>{} {}{hint}",
            priority.level(),
            self.severity.tag(),
            single_line(&self.message)
        )
    }

    /// Attaches a runnable hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Writes to stderr unless below the floor: one JSON line in machine mode, tagged text otherwise.
    pub fn emit(&self, context: &Context) {
        if self.severity < context.floor {
            return;
        }
        if context.journal {
            eprintln!("{}", self.journal_line());
        } else if context.mode.is_machine() {
            #[derive(Serialize)]
            struct Wrapper<'a> {
                diagnostic: &'a Diagnostic,
            }
            if let Ok(line) = serde_json::to_string(&Wrapper { diagnostic: self }) {
                eprintln!("{line}");
            }
        } else {
            eprintln!("{} {}", self.severity.tag(), self.message);
            if let Some(hint) = &self.hint {
                eprintln!("  hint: {hint}");
            }
        }
    }
}

/// `text` with every line break replaced by a space: journald splits on newlines,
/// and a second line would lose the priority prefix.
#[must_use]
pub fn single_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_lines_carry_the_priority_and_stay_on_one_line() {
        let warn = Diagnostic::new(Severity::Warn, "X", "no backend\ncould draw", "c")
            .with_hint("fairing preview --backend memory");
        assert_eq!(
            warn.journal_line(),
            "<4>[WARN] no backend could draw (hint: fairing preview --backend memory)"
        );
        let notice = warn.with_priority(Priority::Notice);
        assert!(notice.journal_line().starts_with("<5>[WARN] "));
        assert_eq!(
            Diagnostic::new(Severity::Info, "Y", "first frame", "c").journal_line(),
            "<6>[INFO] first frame"
        );
        let json = serde_json::to_string(&notice).unwrap_or_default();
        assert!(!json.contains("priority"), "{json}");
    }

    #[test]
    fn ladder_is_ordered() {
        assert!(
            Severity::Info < Severity::Ok
                && Severity::Ok < Severity::Warn
                && Severity::Warn < Severity::Error
        );
        assert_eq!(Severity::Warn.tag(), "[WARN]");
    }
}
