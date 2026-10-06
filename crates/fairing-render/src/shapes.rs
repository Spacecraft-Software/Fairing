// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Path construction and solid fills on a [`Frame`].
//!
//! tiny-skia reports misuse (empty paths, zero strokes) only through `log`,
//! so every helper here validates its geometry first and draws nothing for a
//! degenerate shape.

use fairing_theme::Rgb;
use tiny_skia::{FillRule, Path, PathBuilder, Stroke, Transform};

use crate::frame::Frame;
use crate::geometry::Rect;

/// Cubic Bézier control distance for a quarter circle: `4/3 · (√2 − 1)`.
const KAPPA: f32 = 0.552_284_8;

/// Shapes thinner than this are invisible and would only trip tiny-skia's warnings.
const MIN_EXTENT: f32 = 0.5;

/// A rounded rectangle from four cubic arcs; `None` when the rectangle is degenerate.
///
/// The radius is clamped to half the shorter side, so a short progress fill
/// stays a capsule instead of a self-intersecting path.
#[must_use]
pub(crate) fn rounded_rect(rect: Rect, radius: f32) -> Option<Path> {
    if !(rect.width >= MIN_EXTENT && rect.height >= MIN_EXTENT) {
        return None;
    }
    let rad = radius.clamp(0.0, rect.width.min(rect.height) / 2.0);
    let ctrl = rad * KAPPA;
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(left + rad, top);
    pb.line_to(right - rad, top);
    pb.cubic_to(
        right - rad + ctrl,
        top,
        right,
        top + rad - ctrl,
        right,
        top + rad,
    );
    pb.line_to(right, bottom - rad);
    pb.cubic_to(
        right,
        bottom - rad + ctrl,
        right - rad + ctrl,
        bottom,
        right - rad,
        bottom,
    );
    pb.line_to(left + rad, bottom);
    pb.cubic_to(
        left + rad - ctrl,
        bottom,
        left,
        bottom - rad + ctrl,
        left,
        bottom - rad,
    );
    pb.line_to(left, top + rad);
    pb.cubic_to(
        left,
        top + rad - ctrl,
        left + rad - ctrl,
        top,
        left + rad,
        top,
    );
    pb.close();
    pb.finish()
}

/// A circle, or `None` for a non-positive radius.
#[must_use]
pub(crate) fn circle(cx: f32, cy: f32, radius: f32) -> Option<Path> {
    if radius < MIN_EXTENT {
        return None;
    }
    PathBuilder::from_circle(cx, cy, radius)
}

/// Fills `path` with an opaque, anti-aliased colour.
pub(crate) fn fill(frame: &mut Frame, path: &Path, color: Rgb) {
    let paint = frame.paint(color, true);
    frame
        .pixmap_mut()
        .fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// Strokes `path` with an opaque, anti-aliased line of `width` pixels (nothing for `width < 0.5`).
pub(crate) fn stroke(frame: &mut Frame, path: &Path, color: Rgb, width: f32) {
    if width < MIN_EXTENT {
        return;
    }
    let paint = frame.paint(color, true);
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    frame
        .pixmap_mut()
        .stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

/// Fills an axis-aligned rectangle; `anti_alias = false` on whole pixels is a plain memset.
pub(crate) fn fill_rect(frame: &mut Frame, rect: Rect, color: Rgb, anti_alias: bool) {
    let Some(rect) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) else {
        return;
    };
    if rect.width() < MIN_EXTENT || rect.height() < MIN_EXTENT {
        return;
    }
    let paint = frame.paint(color, anti_alias);
    frame
        .pixmap_mut()
        .fill_rect(rect, &paint, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_shapes_are_none() {
        assert!(rounded_rect(Rect::new(0.0, 0.0, 0.2, 10.0), 3.0).is_none());
        assert!(rounded_rect(Rect::new(0.0, 0.0, 10.0, f32::NAN), 3.0).is_none());
        assert!(circle(1.0, 1.0, 0.0).is_none());
        assert!(rounded_rect(Rect::new(0.0, 0.0, 10.0, 4.0), 100.0).is_some());
    }

    #[test]
    fn rounded_rect_bounds_match_the_rectangle() {
        let path =
            rounded_rect(Rect::new(10.0, 20.0, 100.0, 14.0), 7.0).unwrap_or_else(|| panic!("path"));
        let bounds = path.bounds();
        assert!((bounds.left() - 10.0).abs() < 1e-3 && (bounds.top() - 20.0).abs() < 1e-3);
        assert!((bounds.right() - 110.0).abs() < 1e-3 && (bounds.bottom() - 34.0).abs() < 1e-3);
    }
}
