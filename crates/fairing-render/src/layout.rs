// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The built-in layout, authored in the 1920×1080 reference frame.
//!
//! Milestone M2 makes the layout theme data (the Nickel contract); until then
//! this is the one layout Fairing draws. Everything here is in reference
//! pixels and goes through a [`Viewport`](crate::Viewport) before it is drawn
//! (FRN-SRS-007).

use crate::geometry::{Rect, Size};

/// The reference frame every layout is authored in.
pub const REFERENCE_SIZE: Size = match Size::checked(1920, 1080) {
    Some(size) => size,
    None => panic!("the reference frame is non-empty"),
};

/// Positions and sizes of the splash elements in reference pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The square the logo is drawn in.
    pub logo: Rect,
    /// The progress-bar track.
    pub bar: Rect,
    /// Corner radius of the bar.
    pub bar_radius: f32,
    /// Stroke width of the bar outline.
    pub bar_outline: f32,
    /// Gap between the bar's right edge and the percentage text.
    pub percent_gap: f32,
    /// Font size of the percentage text (FRN-SRS-053).
    pub percent_size: f32,
    /// Top of the status line.
    pub status_top: f32,
    /// Font size of the status line.
    pub status_size: f32,
}

impl Layout {
    /// The built-in layout: logo above a centred bar, percentage beside it, status below.
    #[must_use]
    pub const fn builtin() -> Self {
        Self {
            logo: Rect::new(840.0, 300.0, 240.0, 240.0),
            bar: Rect::new(640.0, 640.0, 640.0, 14.0),
            bar_radius: 7.0,
            bar_outline: 1.5,
            percent_gap: 28.0,
            percent_size: 30.0,
            status_top: 704.0,
            status_size: 26.0,
        }
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::as_f32;

    #[test]
    fn builtin_fits_the_reference_frame() {
        let layout = Layout::builtin();
        let width = as_f32(REFERENCE_SIZE.width());
        let height = as_f32(REFERENCE_SIZE.height());
        for rect in [layout.logo, layout.bar] {
            assert!(rect.x >= 0.0 && rect.right() <= width, "{rect:?}");
            assert!(rect.y >= 0.0 && rect.bottom() <= height, "{rect:?}");
        }
        assert!((layout.logo.center_x() - width / 2.0).abs() < f32::EPSILON);
        assert!((layout.bar.center_x() - width / 2.0).abs() < f32::EPSILON);
        assert!(layout.bar.right() + layout.percent_gap + 4.0 * layout.percent_size <= width);
        assert!(layout.status_top > layout.bar.bottom());
    }
}
