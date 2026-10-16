// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! What a frame shows: progress, the status line and a password prompt.

use std::fmt;
use std::time::Duration;

use zeroize::Zeroize;

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
    prompt: Option<PromptView>,
    /// How far into its frame sequence the theme's animation is.
    time: Duration,
    overlay: Option<OverlayView>,
    /// Whether the bar and the percentage are drawn: not at shutdown.
    progress: bool,
}

/// The journal overlay as the compositor draws it (FRN-SRS-060).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverlayView {
    /// The entries, oldest first, each one line of plain text.
    pub lines: Vec<String>,
    /// How many lines the view is scrolled up from the newest (FRN-SRS-061).
    pub scroll: usize,
}

/// What the input field of a prompt shows.
///
/// A masked field carries only how many characters were typed: the secret
/// itself never leaves the password agent (FRN-SRS-021, FRN-SRS-025).
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "an echoed answer stays inline: boxing it would put the answer on the heap"
)]
pub enum Field {
    /// One mark per character typed.
    Masked(usize),
    /// The input as typed, for a question asked with `Echo=1` (FRN-SRS-026).
    Clear(EchoText),
}

/// Bytes an echoed answer may hold, as many as the agent's secret.
pub const ECHO_CAPACITY: usize = 512;

/// The text of an `Echo=1` answer, in a fixed buffer zeroised when cleared
/// and when dropped.
///
/// An echoed answer is still the answer: crypttab `password-echo=yes` echoes
/// a LUKS passphrase. So it is never copied to the heap, where freed memory
/// would keep it, and is wiped like the agent's own secret (FRN-SRS-024).
#[derive(Clone, PartialEq, Eq)]
pub struct EchoText {
    bytes: [u8; ECHO_CAPACITY],
    len: usize,
}

impl EchoText {
    /// Copies as much of `text` as fits, cut at a character boundary.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut end = text.len().min(ECHO_CAPACITY);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut echo = Self {
            bytes: [0; ECHO_CAPACITY],
            len: end,
        };
        echo.bytes[..end].copy_from_slice(&text.as_bytes()[..end]);
        echo
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only whole characters are copied in.
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }

    /// Zeroises every byte and empties the text.
    pub fn clear(&mut self) {
        self.bytes.zeroize();
        self.len = 0;
    }
}

impl Drop for EchoText {
    fn drop(&mut self) {
        self.clear();
    }
}

impl fmt::Debug for EchoText {
    /// The length only, like the agent's secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EchoText")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

/// A password prompt as the compositor draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptView {
    /// The question's `Message=`.
    pub message: String,
    /// The input field.
    pub field: Field,
    /// A notice beside the prompt, such as a rejected answer (FRN-SRS-029).
    pub notice: Option<String>,
}

impl Scene {
    /// A scene at `percent` (clamped to 100) with no status line.
    #[must_use]
    pub fn new(percent: u8) -> Self {
        Self {
            steps: u16::from(percent.min(100)) * (STEPS / 100),
            status: None,
            prompt: None,
            time: Duration::ZERO,
            overlay: None,
            progress: true,
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
            prompt: None,
            time: Duration::ZERO,
            overlay: None,
            progress: true,
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

    /// Shows `prompt`, or none.
    #[must_use]
    pub fn with_prompt(mut self, prompt: Option<PromptView>) -> Self {
        self.prompt = prompt;
        self
    }

    /// Sets the animation time: how long the theme's frame sequence has been
    /// playing. Zero, the default, is its first frame; reduced motion keeps
    /// it there (FRN-SRS-052).
    #[must_use]
    pub const fn with_time(mut self, time: Duration) -> Self {
        self.time = time;
        self
    }

    /// Hides the bar and the percentage: the shutdown layout has neither
    /// (FRN-SRS-035).
    #[must_use]
    pub const fn without_progress(mut self) -> Self {
        self.progress = false;
        self
    }

    /// Whether the bar and the percentage are drawn.
    #[must_use]
    pub const fn shows_progress(&self) -> bool {
        self.progress
    }

    /// Shows the journal overlay, or none.
    #[must_use]
    pub fn with_overlay(mut self, overlay: Option<OverlayView>) -> Self {
        self.overlay = overlay;
        self
    }

    /// The journal overlay, if shown.
    #[must_use]
    pub const fn overlay(&self) -> Option<&OverlayView> {
        self.overlay.as_ref()
    }

    /// The animation time.
    #[must_use]
    pub const fn time(&self) -> Duration {
        self.time
    }

    /// The prompt, if one is open.
    #[must_use]
    pub const fn prompt(&self) -> Option<&PromptView> {
        self.prompt.as_ref()
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
    fn echo_text_is_cut_whole_and_cleared() {
        // Verifies: FRN-SRS-024
        let mut echo = EchoText::new(&"é".repeat(ECHO_CAPACITY));
        assert_eq!(echo.as_str().chars().count(), ECHO_CAPACITY / 2);
        assert!(!format!("{echo:?}").contains('é'));
        echo.clear();
        assert!(echo.as_str().is_empty());
        assert!(echo.bytes.iter().all(|&b| b == 0));
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
