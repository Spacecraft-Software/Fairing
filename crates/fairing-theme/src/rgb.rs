// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! An opaque sRGB colour, the value type of every role token.

use std::fmt;
use std::str::FromStr;

use crate::fault::{ThemeError, ThemeErrorKind};

/// An opaque 8-bit-per-channel sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// Builds a colour from its channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses the `#RRGGBB` form the palette file uses (case-insensitive digits).
    ///
    /// # Errors
    ///
    /// [`ThemeErrorKind::InvalidHex`] for anything but `#` and six hex digits.
    pub fn parse_hex(text: &str) -> Result<Self, ThemeError> {
        let invalid = || ThemeError::new(ThemeErrorKind::InvalidHex, text);
        let digits = text
            .strip_prefix('#')
            .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(invalid)?;
        // The digits were validated above, so the radix parse cannot fail in practice.
        let channel =
            |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_hex| invalid());
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }

    /// WCAG 2.2 relative luminance in `0.0..=1.0` (sRGB linearisation).
    #[must_use]
    pub fn relative_luminance(self) -> f64 {
        fn linear(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG 2.2 contrast ratio between two colours, in `1.0..=21.0`.
    #[must_use]
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }
}

impl fmt::Display for Rgb {
    /// Renders as `#RRGGBB`, the palette file's spelling.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl FromStr for Rgb {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_hex(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let colour = Rgb::parse_hex("#0e2A47").unwrap_or(Rgb::new(0, 0, 0));
        assert_eq!(colour, Rgb::new(0x0E, 0x2A, 0x47));
        assert_eq!(colour.to_string(), "#0E2A47");
        assert_eq!("#0E2A47".parse::<Rgb>(), Ok(colour));
    }

    #[test]
    fn malformed_hex_is_rejected() {
        for bad in ["0E2A47", "#0E2A4", "#0E2A477", "#GGGGGG", "", "#"] {
            let error = Rgb::parse_hex(bad);
            assert!(
                error
                    .as_ref()
                    .is_err_and(|e| e.kind() == ThemeErrorKind::InvalidHex),
                "{bad}: {error:?}"
            );
        }
    }

    #[test]
    fn contrast_matches_wcag_bounds() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        assert!((white.contrast_ratio(black) - 21.0).abs() < 1e-9);
        assert!((black.contrast_ratio(black) - 1.0).abs() < 1e-9);
        assert!((white.contrast_ratio(black) - black.contrast_ratio(white)).abs() < 1e-12);
    }
}
