// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The crate's error type for a refused `ask.*` file (M-ERRORS-CANONICAL-STRUCTS).
//!
//! A refusal never carries file content: the `Message=` text is written by the
//! process asking for a password and may echo a device label, so diagnostics
//! name the key and line, never the value.

use std::fmt;
use std::io;

/// Why an `ask.*` file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// The file is larger than [`MAX_REQUEST_BYTES`](crate::MAX_REQUEST_BYTES).
    TooLarge,
    /// The file holds a byte sequence that is not UTF-8.
    NotUtf8,
    /// No `[Ask]` section, or no `Socket=` key in it.
    MissingSocket,
    /// A key appears twice in the `[Ask]` section.
    DuplicateKey,
    /// A known key has a value that does not parse.
    InvalidValue,
    /// Reading the file failed.
    Io,
}

impl ParseErrorKind {
    /// Stable lowercase name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too-large",
            Self::NotUtf8 => "not-utf8",
            Self::MissingSocket => "missing-socket",
            Self::DuplicateKey => "duplicate-key",
            Self::InvalidValue => "invalid-value",
            Self::Io => "io",
        }
    }
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An `ask.*` file the agent will not answer.
///
/// The agent leaves such a request to `systemd-tty-ask-password-agent`.
#[derive(Debug)]
pub struct ParseError {
    kind: ParseErrorKind,
    key: Option<&'static str>,
    line: Option<usize>,
    source: Option<io::Error>,
}

impl ParseError {
    pub(crate) const fn new(kind: ParseErrorKind) -> Self {
        Self {
            kind,
            key: None,
            line: None,
            source: None,
        }
    }

    pub(crate) const fn at(kind: ParseErrorKind, key: &'static str, line: usize) -> Self {
        Self {
            kind,
            key: Some(key),
            line: Some(line),
            source: None,
        }
    }

    pub(crate) fn from_io(source: io::Error) -> Self {
        Self {
            kind: ParseErrorKind::Io,
            key: None,
            line: None,
            source: Some(source),
        }
    }

    /// What went wrong.
    #[must_use]
    pub const fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    /// The key whose line was refused, when one was.
    #[must_use]
    pub const fn key(&self) -> Option<&'static str> {
        self.key
    }

    /// The one-based line number of the refused line, when there is one.
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        self.line
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ParseErrorKind::TooLarge => write!(
                f,
                "ask file is larger than {} bytes",
                crate::MAX_REQUEST_BYTES
            )?,
            ParseErrorKind::NotUtf8 => f.write_str("ask file is not UTF-8")?,
            ParseErrorKind::MissingSocket => f.write_str("ask file has no [Ask] Socket= key")?,
            ParseErrorKind::DuplicateKey => f.write_str("ask file repeats a key")?,
            ParseErrorKind::InvalidValue => f.write_str("ask file has an invalid value")?,
            ParseErrorKind::Io => f.write_str("cannot read ask file")?,
        }
        if let (Some(key), Some(line)) = (self.key, self.line) {
            write!(f, " ({key}= on line {line})")?;
        }
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}
