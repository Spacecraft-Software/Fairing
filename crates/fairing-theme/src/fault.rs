// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The crate's one error type (M-ERRORS-CANONICAL-STRUCTS).

use std::fmt;

/// What went wrong when parsing a palette name or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ThemeErrorKind {
    /// Not one of the eleven §11.1 role names.
    UnknownRole,
    /// Not one of the ANSI slot names the palette file uses.
    UnknownAnsi,
    /// Not a `#RRGGBB` colour.
    InvalidHex,
    /// Not a registered theme slug.
    UnknownTheme,
    /// Not a token of any palette in the palette file.
    UnknownToken,
    /// A token of another palette than the one the theme declared (§11.4).
    ForeignToken,
    /// A token of the theme's own palette that no role binds.
    UnboundToken,
    /// A theme whose layout or images break a rule of the contract.
    InvalidLayout,
    /// Bytes that are not a readable compiled theme.
    InvalidArtefact,
    /// A theme source that is not valid Nickel or breaks the contract; the
    /// input is Nickel's own diagnostic (FRN-SRS-041).
    Contract,
    /// An evaluated theme or asset the compiler cannot use.
    InvalidSource,
    /// A literal colour in a theme, or an image pixel outside the palette
    /// (FRN-SRS-044).
    LiteralColor,
}

/// A theme-contract violation, carrying the offending input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    kind: ThemeErrorKind,
    input: String,
}

impl ThemeError {
    pub(crate) fn new(kind: ThemeErrorKind, input: impl Into<String>) -> Self {
        Self {
            kind,
            input: input.into(),
        }
    }

    /// The category of failure.
    #[must_use]
    pub fn kind(&self) -> ThemeErrorKind {
        self.kind
    }

    /// The input that was rejected, verbatim.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // These carry a sentence, not a name to quote.
        match self.kind {
            ThemeErrorKind::InvalidLayout | ThemeErrorKind::InvalidSource => {
                return write!(f, "invalid theme: {}", self.input);
            }
            ThemeErrorKind::InvalidArtefact => {
                return write!(f, "unreadable compiled theme: {}", self.input);
            }
            ThemeErrorKind::LiteralColor => return write!(f, "{}", self.input),
            ThemeErrorKind::Contract => {
                return write!(f, "the theme breaks the Fairing theme contract");
            }
            _ => {}
        }
        let what = match self.kind {
            ThemeErrorKind::UnknownRole => "unknown role",
            ThemeErrorKind::UnknownAnsi => "unknown ANSI slot",
            ThemeErrorKind::InvalidHex => "invalid `#RRGGBB` colour",
            ThemeErrorKind::UnknownTheme => "unknown theme",
            ThemeErrorKind::UnknownToken => "unknown palette token",
            ThemeErrorKind::ForeignToken => "token of another palette",
            ThemeErrorKind::UnboundToken => "token that binds no role",
            ThemeErrorKind::InvalidLayout
            | ThemeErrorKind::InvalidArtefact
            | ThemeErrorKind::Contract
            | ThemeErrorKind::InvalidSource
            | ThemeErrorKind::LiteralColor => "invalid",
        };
        write!(f, "{what} `{}`", self.input)
    }
}

impl std::error::Error for ThemeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_input() {
        let error = ThemeError::new(ThemeErrorKind::UnknownTheme, "nope");
        assert_eq!(error.to_string(), "unknown theme `nope`");
        assert_eq!(error.kind(), ThemeErrorKind::UnknownTheme);
        assert_eq!(error.input(), "nope");
    }
}
