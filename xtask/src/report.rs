// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Envelopes, findings and failures shared by every verb.
//!
//! Machine output follows the Spacecraft CLI Standard shapes: a `metadata` +
//! `data` envelope on stdout, and a single-line `{"error":{...}}` object on
//! stderr for failures. Human output uses the `[ERROR]` / `[WARN]` / `[OK]` tag
//! ladder so meaning never depends on colour (Steelbore Standard §18.2.1).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Returns the current time as `YYYY-MM-DDTHH:MM:SSZ` (Steelbore Standard §14).
///
/// Honours `SOURCE_DATE_EPOCH` so build artifacts can be reproducible.
#[must_use]
pub fn now_utc() -> String {
    let timestamp = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .and_then(|seconds| jiff::Timestamp::from_second(seconds).ok())
        .unwrap_or_else(jiff::Timestamp::now);
    timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The `metadata` half of a response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    /// Always `xtask`.
    pub tool: String,
    /// The crate version.
    pub version: String,
    /// The invocation that produced the response.
    pub command: String,
    /// ISO 8601 UTC timestamp with `Z` suffix.
    pub timestamp: String,
}

/// A response envelope: metadata plus a verb-specific payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope<T> {
    /// Response metadata.
    pub metadata: Metadata,
    /// The payload.
    pub data: T,
}

impl<T: Serialize> Envelope<T> {
    /// Wraps `data` with fresh metadata for `command`.
    pub fn new(command: &str, data: T) -> Self {
        Self {
            metadata: Metadata {
                tool: "xtask".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                command: command.to_owned(),
                timestamp: now_utc(),
            },
            data,
        }
    }

    /// Prints the envelope as one compact JSON document on stdout.
    ///
    /// # Errors
    ///
    /// Returns an internal failure if the payload cannot be serialised.
    pub fn print(&self) -> Result<(), Failure> {
        let line = serde_json::to_string(self)?;
        println!("{line}");
        Ok(())
    }
}

/// One thing a gate found wrong (or, for `lint`, merely suspicious).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    /// Stable upper-snake-case category, e.g. `UNKNOWN_MARKER`.
    pub code: String,
    /// The requirement or need the finding concerns, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// One sentence, lowercase first word, no trailing period.
    pub message: String,
    /// Repository-relative path, if the finding points at a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based line number within `path`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

impl Finding {
    /// Creates a finding without a location.
    pub fn new(code: &str, id: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            id: id.map(str::to_owned),
            message: message.into(),
            path: None,
            line: None,
        }
    }

    /// Attaches a file location.
    #[must_use]
    pub fn at(mut self, path: &str, line: usize) -> Self {
        self.path = Some(path.to_owned());
        self.line = Some(line);
        self
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(path), Some(line)) = (&self.path, self.line) {
            write!(f, "{path}:{line}: ")?;
        }
        if let Some(id) = &self.id {
            write!(f, "{id}: ")?;
        }
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// Why a verb failed, mapped onto the house exit-code table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    /// Bad invocation; do not retry without fixing the arguments (exit 2).
    UsageError,
    /// An input file does not exist (exit 3).
    NotFound,
    /// The gate ran and found violations (exit 1).
    GateFailed,
    /// Unexpected internal error (exit 1).
    InternalError,
}

impl Kind {
    /// The process exit code for this kind.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::UsageError => 2,
            Self::NotFound => 3,
            Self::GateFailed | Self::InternalError => 1,
        }
    }
}

/// A failed verb: what happened, and the command that fixes or investigates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The failure category.
    pub kind: Kind,
    /// One sentence stating what happened.
    pub message: String,
    /// A runnable command, never prose.
    pub hint: String,
}

impl Failure {
    fn new(kind: Kind, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// A usage error (exit 2).
    pub fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(Kind::UsageError, message, hint)
    }

    /// A missing input (exit 3).
    pub fn not_found(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(Kind::NotFound, message, hint)
    }

    /// A gate violation (exit 1).
    pub fn gate(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(Kind::GateFailed, message, hint)
    }

    /// An internal error (exit 1).
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(Kind::InternalError, message, "cargo xtask help")
    }

    /// The process exit code.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        self.kind.exit_code()
    }

    /// Writes the failure to stderr: one-line JSON in machine mode, tagged text otherwise.
    pub fn emit(&self, json: bool, command: &str) {
        if json {
            #[derive(Serialize)]
            struct Body<'a> {
                code: Kind,
                exit_code: u8,
                message: &'a str,
                hint: &'a str,
                timestamp: String,
                command: &'a str,
            }
            #[derive(Serialize)]
            struct Wrapper<'a> {
                error: Body<'a>,
            }
            let wrapper = Wrapper {
                error: Body {
                    code: self.kind,
                    exit_code: self.exit_code(),
                    message: &self.message,
                    hint: &self.hint,
                    timestamp: now_utc(),
                    command,
                },
            };
            match serde_json::to_string(&wrapper) {
                Ok(line) => eprintln!("{line}"),
                Err(_) => eprintln!("[ERROR] {}", self.message),
            }
        } else {
            eprintln!("[ERROR] {}", self.message);
            eprintln!("  hint: {}", self.hint);
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Failure {}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::internal(format!("i/o error: {error}"))
    }
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!("json error: {error}"))
    }
}

impl From<toml::de::Error> for Failure {
    fn from(error: toml::de::Error) -> Self {
        Self::internal(format!("toml error: {}", error.message()))
    }
}

/// Prints findings as `[WARN]`/`[ERROR]`-tagged lines on stderr.
pub fn print_findings(tag: &str, findings: &[Finding]) {
    for finding in findings {
        eprintln!("[{tag}] {finding}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_iso8601_utc_with_z() {
        let stamp = now_utc();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'));
        assert_eq!(&stamp[10..11], "T");
    }

    #[test]
    fn exit_codes_follow_the_house_table() {
        assert_eq!(Failure::usage("a", "b").exit_code(), 2);
        assert_eq!(Failure::not_found("a", "b").exit_code(), 3);
        assert_eq!(Failure::gate("a", "b").exit_code(), 1);
        assert_eq!(Failure::internal("a").exit_code(), 1);
    }

    #[test]
    fn finding_display_carries_location_and_id() {
        let finding = Finding::new("X", Some("FRN-SRS-001"), "bad").at("a.rs", 3);
        assert_eq!(finding.to_string(), "a.rs:3: FRN-SRS-001: bad (X)");
    }
}
