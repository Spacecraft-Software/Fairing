// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The layout the compositor draws, in the 1920×1080 reference frame.
//!
//! A theme's boot layout ([`BootLayout`]) arrives from the compiled theme
//! artefact; [`Layout::from_spec`] turns it into rectangles and roles here.
//! Without a theme, [`Layout::builtin`] is the same layout the reference theme
//! declares. Everything is in reference pixels and goes through a
//! [`Viewport`](crate::Viewport) before it is drawn (FRN-SRS-007).

use fairing_theme::{BootLayout, RectSpec, Role};

use crate::geometry::{Rect, Size};

/// The reference frame every layout is authored in.
pub const REFERENCE_SIZE: Size = match Size::checked(1920, 1080) {
    Some(size) => size,
    None => panic!("the reference frame is non-empty"),
};

/// Positions, sizes and roles of the splash elements, in reference pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The box the logo is fitted into.
    pub logo: Rect,
    /// The progress-bar track.
    pub bar: Rect,
    /// Corner radius of the bar.
    pub bar_radius: f32,
    /// Stroke width of the bar outline.
    pub bar_outline: f32,
    /// The bar's filled part.
    pub bar_fill: Role,
    /// The bar's empty part.
    pub bar_track: Role,
    /// The bar's outline.
    pub bar_border: Role,
    /// Gap between the bar's right edge and the percentage text.
    pub percent_gap: f32,
    /// Font size of the percentage text (FRN-SRS-053).
    pub percent_size: f32,
    /// Colour of the percentage text.
    pub percent_color: Role,
    /// Whether the status line is drawn (FRN-SRS-050).
    pub show_status: bool,
    /// Top of the status line.
    pub status_top: f32,
    /// Font size of the status line.
    pub status_size: f32,
    /// Colour of the status line.
    pub status_color: Role,
}

impl Layout {
    /// The built-in layout: logo above a centred bar, percentage beside it, status below.
    #[must_use]
    pub const fn builtin() -> Self {
        Self::from_spec(&BootLayout::builtin())
    }

    /// The layout a compiled theme's boot layout describes.
    #[must_use]
    pub const fn from_spec(spec: &BootLayout) -> Self {
        Self {
            logo: rect(spec.logo.rect),
            bar: rect(spec.bar.rect),
            bar_radius: spec.bar.radius,
            bar_outline: spec.bar.outline,
            bar_fill: spec.bar.fill,
            bar_track: spec.bar.track,
            bar_border: spec.bar.border,
            percent_gap: spec.percent.gap,
            percent_size: spec.percent.size,
            percent_color: spec.percent.color,
            show_status: spec.status.show,
            status_top: spec.status.top,
            status_size: spec.status.size,
            status_color: spec.status.color,
        }
    }
}

const fn rect(spec: RectSpec) -> Rect {
    Rect::new(spec.x, spec.y, spec.width, spec.height)
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
