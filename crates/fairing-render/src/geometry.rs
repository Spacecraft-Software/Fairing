// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Sizes, rectangles and the reference-frame viewport (FRN-SRS-007).

use std::fmt;

use crate::fault::{RenderError, RenderErrorKind};

/// A non-empty pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    width: u32,
    height: u32,
}

impl Size {
    /// Builds a size; both dimensions must be non-zero.
    ///
    /// # Errors
    ///
    /// [`RenderErrorKind::InvalidGeometry`] when either dimension is zero.
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        Self::checked(width, height).ok_or_else(|| {
            RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!("size {width}x{height} has an empty dimension"),
            )
        })
    }

    /// Builds a size in constant context; `None` when either dimension is zero.
    #[must_use]
    pub const fn checked(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self { width, height })
        }
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    /// Pixel count.
    #[must_use]
    pub const fn area(self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// An axis-aligned rectangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Builds a rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Horizontal centre.
    #[must_use]
    pub fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    /// Vertical centre.
    #[must_use]
    pub fn center_y(self) -> f32 {
        self.y + self.height / 2.0
    }

    /// The same rectangle with `width` replaced.
    #[must_use]
    pub const fn with_width(self, width: f32) -> Self {
        Self { width, ..self }
    }
}

/// Maps the 1920×1080 reference frame onto an output of another size.
///
/// The scale is uniform (`min(w/1920, h/1080)`) and the content is centred,
/// so proportions survive every panel and the margins are letterbox bands in
/// the canvas colour (FRN-SRS-007).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    target: Size,
}

impl Viewport {
    /// Fits `reference` into `target`.
    #[must_use]
    pub fn fit(reference: Size, target: Size) -> Self {
        let sx = as_f32(target.width()) / as_f32(reference.width());
        let sy = as_f32(target.height()) / as_f32(reference.height());
        let scale = sx.min(sy);
        let offset_x = (as_f32(target.width()) - as_f32(reference.width()) * scale) / 2.0;
        let offset_y = (as_f32(target.height()) - as_f32(reference.height()) * scale) / 2.0;
        Self {
            scale,
            offset_x,
            offset_y,
            target,
        }
    }

    /// The uniform scale factor.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// The output size this viewport maps onto.
    #[must_use]
    pub const fn target(&self) -> Size {
        self.target
    }

    /// Maps a reference-frame rectangle to output pixels.
    #[must_use]
    pub fn rect(&self, reference: Rect) -> Rect {
        Rect::new(
            self.offset_x + reference.x * self.scale,
            self.offset_y + reference.y * self.scale,
            reference.width * self.scale,
            reference.height * self.scale,
        )
    }

    /// Maps a reference-frame length (a font size, a stroke width) to output pixels.
    #[must_use]
    pub fn length(&self, reference: f32) -> f32 {
        reference * self.scale
    }
}

/// `u32` pixel counts as `f32`; exact below 2^24, far beyond any display.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "pixel counts are far below 2^24, so the conversion is exact"
)]
pub(crate) fn as_f32(value: u32) -> f32 {
    value as f32
}

/// Rounds a finite pixel coordinate to the nearest integer, saturating.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped to the i32 range first"
)]
pub(crate) fn round_i32(value: f32) -> i32 {
    value.round().clamp(-2_147_483_648.0, 2_147_483_520.0) as i32
}

/// Converts a non-negative, finite length to whole pixels, rounding to nearest.
#[must_use]
pub(crate) fn round_u32(value: f32) -> u32 {
    u32::try_from(round_i32(value.max(0.0))).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: u32, h: u32) -> Size {
        Size::new(w, h).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn empty_sizes_are_rejected() {
        assert!(Size::new(0, 1).is_err());
        assert!(Size::new(1, 0).is_err());
        assert_eq!(size(3, 2).area(), 6);
        assert_eq!(size(1920, 1080).to_string(), "1920x1080");
    }

    #[test]
    fn viewport_scales_uniformly_and_centres() {
        // Verifies: FRN-SRS-007
        let reference = size(1920, 1080);
        let identity = Viewport::fit(reference, reference);
        assert!((identity.scale() - 1.0).abs() < f32::EPSILON);
        let r = identity.rect(Rect::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!((r.x, r.y, r.width, r.height), (10.0, 20.0, 30.0, 40.0));

        // 4:3 panel: the 16:9 frame is width-limited, so the bands sit above and below.
        let v = Viewport::fit(reference, size(1024, 768));
        let scale = 1024.0 / 1920.0;
        assert!((v.scale() - scale).abs() < 1e-5, "{}", v.scale());
        let full = v.rect(Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!(
            (full.width / full.height - 1920.0 / 1080.0).abs() < 1e-4,
            "aspect preserved"
        );
        assert!((full.center_x() - 512.0).abs() < 1e-3 && (full.center_y() - 384.0).abs() < 1e-3);
        assert!(
            full.x.abs() < 1e-3 && full.y > 0.0,
            "letterboxed vertically"
        );

        // Ultra-wide: width would overflow, so width limits and bands are top/bottom.
        let v = Viewport::fit(reference, size(2560, 1080));
        assert!((v.scale() - 1.0).abs() < 1e-6);
        let full = v.rect(Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!((full.x - 320.0).abs() < 1e-3 && full.y.abs() < 1e-3);
        assert!((v.length(30.0) - 30.0).abs() < 1e-6);
    }

    #[test]
    fn rounding_helpers_saturate() {
        assert_eq!(round_i32(2.5), 3);
        assert_eq!(round_i32(-2.5), -3);
        assert_eq!(round_u32(-4.0), 0);
        assert_eq!(round_u32(7.4), 7);
        assert_eq!(round_i32(f32::MAX), 2_147_483_520);
    }
}
