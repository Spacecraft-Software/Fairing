// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The structured error (CLI Standard `exit-codes-errors.md`).
//!
//! Every failure is one `AppError`; `report` renders it as a single-line
//! `{"error":{...}}` on stderr in machine mode or a `[ERROR]` line with a
//! `hint:` continuation in human mode, and returns the exit code to use.

use std::fmt;

use serde::Serialize;

use crate::output::mode::Context;
use crate::time::now_iso8601;

/// Stable error codes (CLI Standard §3 of `exit-codes-errors.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Referenced resource does not exist (exit 3).
    NotFound,
    /// Argument failed validation (exit 2).
    InvalidArgument,
    /// Required argument missing in non-interactive mode (exit 2).
    MissingArgument,
    /// Requested feature is not built into this release (exit 1).
    FeatureUnavailable,
    /// Unexpected failure; the bug-report code (exit 1).
    InternalError,
}

impl ErrorCode {
    /// The canonical exit code for the error code.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::NotFound => 3,
            Self::InvalidArgument | Self::MissingArgument => 2,
            Self::FeatureUnavailable | Self::InternalError => 1,
        }
    }
}

/// A structured application error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppError {
    /// Stable code.
    pub code: ErrorCode,
    /// Matches the process exit code.
    pub exit_code: u8,
    /// One sentence, lowercase first word, no trailing period.
    pub message: String,
    /// A runnable command, never prose.
    pub hint: String,
    /// ISO 8601 UTC.
    pub timestamp: String,
    /// The invocation that failed.
    pub command: String,
}

impl AppError {
    fn new(
        code: ErrorCode,
        message: impl Into<String>,
        hint: impl Into<String>,
        command: &str,
    ) -> Self {
        Self {
            code,
            exit_code: code.exit_code(),
            message: message.into(),
            hint: hint.into(),
            timestamp: now_iso8601(),
            command: command.to_owned(),
        }
    }

    /// `NOT_FOUND` (exit 3).
    pub fn not_found(message: impl Into<String>, hint: impl Into<String>, command: &str) -> Self {
        Self::new(ErrorCode::NotFound, message, hint, command)
    }

    /// `INVALID_ARGUMENT` (exit 2).
    pub fn invalid_argument(
        message: impl Into<String>,
        hint: impl Into<String>,
        command: &str,
    ) -> Self {
        Self::new(ErrorCode::InvalidArgument, message, hint, command)
    }

    /// `MISSING_ARGUMENT` (exit 2).
    pub fn missing_argument(
        message: impl Into<String>,
        hint: impl Into<String>,
        command: &str,
    ) -> Self {
        Self::new(ErrorCode::MissingArgument, message, hint, command)
    }

    /// `FEATURE_UNAVAILABLE` (exit 1).
    pub fn feature_unavailable(message: impl Into<String>, command: &str) -> Self {
        Self::new(
            ErrorCode::FeatureUnavailable,
            message,
            "fairing describe --json",
            command,
        )
    }

    /// `INTERNAL_ERROR` (exit 1).
    pub fn internal(message: impl Into<String>, command: &str) -> Self {
        Self::new(ErrorCode::InternalError, message, "fairing --help", command)
    }

    /// Writes the error to stderr in the context's rendering and returns the exit code.
    #[must_use]
    pub fn report(&self, context: &Context) -> u8 {
        if context.mode.is_machine() {
            #[derive(Serialize)]
            struct Wrapper<'a> {
                error: &'a AppError,
            }
            match serde_json::to_string(&Wrapper { error: self }) {
                Ok(line) => eprintln!("{line}"),
                Err(_) => eprintln!("[ERROR] {}", self.message),
            }
        } else {
            eprintln!("[ERROR] {}", self.message);
            eprintln!("  hint: {}", self.hint);
        }
        self.exit_code
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_the_canonical_map() {
        assert_eq!(AppError::not_found("a", "b", "c").exit_code, 3);
        assert_eq!(AppError::invalid_argument("a", "b", "c").exit_code, 2);
        assert_eq!(AppError::missing_argument("a", "b", "c").exit_code, 2);
        assert_eq!(AppError::feature_unavailable("a", "c").exit_code, 1);
        assert_eq!(AppError::internal("a", "c").exit_code, 1);
    }

    #[test]
    fn serialises_with_screaming_snake_code() {
        let value = serde_json::to_value(AppError::not_found("x", "y", "z")).unwrap_or_default();
        assert_eq!(value["code"], "NOT_FOUND");
        assert_eq!(value["exit_code"], 3);
    }
}
