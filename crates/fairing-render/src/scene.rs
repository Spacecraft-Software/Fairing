// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! What a frame shows: progress and the status line.

use std::fmt;

/// The state the compositor draws. Progress is clamped to `0..=100`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scene {
    percent: u8,
    status: Option<String>,
}

impl Scene {
    /// A scene at `percent` (clamped to 100) with no status line.
    #[must_use]
    pub fn new(percent: u8) -> Self {
        Self {
            percent: percent.min(100),
            status: None,
        }
    }

    /// Sets the status line; an empty string clears it.
    #[must_use]
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        let status = status.into();
        self.status = (!status.is_empty()).then_some(status);
        self
    }

    /// Progress in whole percent, `0..=100`.
    #[must_use]
    pub const fn percent(&self) -> u8 {
        self.percent
    }

    /// Progress as a fraction, `0.0..=1.0`.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        f32::from(self.percent) / 100.0
    }

    /// The status line, if any.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// The percentage as the text the bar shows (FRN-SRS-053), e.g. `42%`.
    #[must_use]
    pub fn percent_text(&self) -> String {
        format!("{}%", self.percent)
    }
}

impl fmt::Display for Scene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.status {
            Some(status) => write!(f, "{}% {status}", self.percent),
            None => write!(f, "{}%", self.percent),
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
    fn empty_status_is_none() {
        assert_eq!(Scene::new(1).with_status("").status(), None);
        assert_eq!(
            Scene::new(1).with_status("Mounting /").status(),
            Some("Mounting /")
        );
        assert_eq!(Scene::new(7).with_status("x").to_string(), "7% x");
    }
}
