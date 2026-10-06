// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Non-error diagnostics: the `[INFO]`/`[OK]`/`[WARN]` ladder and severity floor.
//!
//! Colour for the tags arrives with `fairing-theme` (M2), which carries the
//! §11.1 role tokens; until then every tag is plain text, which is already the
//! accessible rendering (Steelbore Standard §18.2.1).

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
        }
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
        if context.mode.is_machine() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
