// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! What a frame shows: progress and the status line.

use std::fmt;

use crate::geometry::round_u32;

/// Progress resolution: the bar moves in steps of one ten-thousandth.
///
/// Whole percent (6.4 reference pixels on the built-in 640-pixel bar) is too
/// coarse for an eased bar; ten-thousandths keep sub-pixel motion at 8K while
/// leaving the scene `Eq` (an `f32` field would not be).
const STEPS: u16 = 10_000;

/// The state the compositor draws. Progress is clamped to `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scene {
    /// Progress in ten-thousandths, `0..=STEPS`.
    steps: u16,
    status: Option<String>,
}

impl Scene {
    /// A scene at `percent` (clamped to 100) with no status line.
    #[must_use]
    pub fn new(percent: u8) -> Self {
        Self {
            steps: u16::from(percent.min(100)) * (STEPS / 100),
            status: None,
        }
    }

    /// A scene at `fraction` of the bar, clamped to `0.0..=1.0`; NaN is empty.
    ///
    /// The percentage text is the fraction rounded down, so a bar short of
    /// full never reads `100%` (FRN-SRS-017).
    #[must_use]
    pub fn from_fraction(fraction: f32) -> Self {
        let clamped = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        let steps = u16::try_from(round_u32(clamped * f32::from(STEPS))).unwrap_or(STEPS);
        Self {
            steps: steps.min(STEPS),
            status: None,
        }
    }

    /// Sets the status line; an empty string clears it.
    #[must_use]
    ///
    /// Control characters (a trailing newline from a unit description, a tab)
    /// have no glyph and would draw as the substitute mark; they become spaces.
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        let status: String = status
            .into()
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        self.status = (!status.trim().is_empty()).then_some(status);
        self
    }

    /// Progress in whole percent, rounded down, `0..=100`.
    #[must_use]
    pub fn percent(&self) -> u8 {
        u8::try_from(self.steps / (STEPS / 100)).unwrap_or(100)
    }

    /// Progress as a fraction, `0.0..=1.0`.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        f32::from(self.steps) / f32::from(STEPS)
    }

    /// Whether any progress is shown at all.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.steps > 0
    }

    /// The status line, if any.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// The percentage as the text the bar shows (FRN-SRS-053), e.g. `42%`.
    #[must_use]
    pub fn percent_text(&self) -> String {
        format!("{}%", self.percent())
    }
}

impl fmt::Display for Scene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.status {
            Some(status) => write!(f, "{}% {status}", self.percent()),
            None => write!(f, "{}%", self.percent()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_is_clamped_and_rendered_as_text() {
        assert_eq!(Scene::new(250).percent(), 100);
        assert_eq!(Scene::new(42).percent_text(), "42%");
        assert!((Scene::new(50).fraction() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn fractions_keep_sub_percent_steps_and_round_the_text_down() {
        let scene = Scene::from_fraction(0.4567);
        assert_eq!(scene.percent(), 45);
        assert_eq!(scene.percent_text(), "45%");
        assert!((scene.fraction() - 0.4567).abs() < 1e-4);
        assert_eq!(Scene::from_fraction(0.9999).percent(), 99);
        assert_eq!(Scene::from_fraction(1.0).percent(), 100);
        assert_eq!(Scene::from_fraction(7.0), Scene::new(100));
        assert_eq!(Scene::from_fraction(-1.0), Scene::new(0));
        assert_eq!(Scene::from_fraction(f32::NAN), Scene::new(0));
        assert!(!Scene::new(0).is_started());
        assert!(Scene::from_fraction(0.0002).is_started());
        assert_eq!(Scene::new(37).percent(), 37);
    }

    #[test]
    fn empty_status_is_none() {
        assert_eq!(Scene::new(1).with_status("").status(), None);
        assert_eq!(Scene::new(1).with_status("\n\t").status(), None);
        assert_eq!(
            Scene::new(1).with_status("Starting foo\n").status(),
            Some("Starting foo ")
        );
        assert_eq!(
            Scene::new(1).with_status("Mounting /").status(),
            Some("Mounting /")
        );
        assert_eq!(Scene::new(7).with_status("x").to_string(), "7% x");
    }
}
