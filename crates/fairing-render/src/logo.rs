// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The built-in vector mark: a payload fairing, drawn from role tokens only.
//!
//! Theme files supply their own logo from M2; this mark is what Fairing draws
//! until then and whenever a theme has none. It is pure geometry in the
//! `surface`, `structure`, `accent` and `foreground` roles, so it survives
//! every palette including mono.

use fairing_theme::Role;
use tiny_skia::PathBuilder;

use crate::frame::Frame;
use crate::geometry::Rect;
use crate::palette::Palette;
use crate::shapes;

/// Draws the mark inside `bounds` (a square in output pixels).
pub(crate) fn draw(frame: &mut Frame, palette: &Palette, bounds: Rect) {
    let w = bounds.width;
    let h = bounds.height;
    if w < 4.0 || h < 4.0 {
        return;
    }
    let cx = bounds.center_x();
    let left = bounds.x + 0.22 * w;
    let right = bounds.x + 0.78 * w;
    let base = bounds.y + 0.92 * h;
    let apex = bounds.y + 0.04 * h;

    // The ogive: two mirrored cubics from the base to the apex.
    let mut pb = PathBuilder::new();
    pb.move_to(left, base);
    pb.cubic_to(
        left,
        bounds.y + 0.42 * h,
        cx - 0.14 * w,
        apex + 0.06 * h,
        cx,
        apex,
    );
    pb.cubic_to(
        cx + 0.14 * w,
        apex + 0.06 * h,
        right,
        bounds.y + 0.42 * h,
        right,
        base,
    );
    pb.close();
    if let Some(shell) = pb.finish() {
        shapes::fill(frame, &shell, palette.color(Role::Surface));
        shapes::stroke(frame, &shell, palette.color(Role::Structure), 0.025 * w);
    }

    // The separation seam, in the accent.
    let seam = Rect::new(cx - 0.20 * w, bounds.y + 0.64 * h, 0.40 * w, 0.045 * h);
    if let Some(band) = shapes::rounded_rect(seam, seam.height / 2.0) {
        shapes::fill(frame, &band, palette.color(Role::Accent));
    }

    // The window.
    if let Some(window) = shapes::circle(cx, bounds.y + 0.40 * h, 0.055 * w) {
        shapes::fill(frame, &window, palette.color(Role::Foreground));
    }
}

#[cfg(test)]
mod tests {
    use fairing_theme::Theme;

    use super::*;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;

    #[test]
    fn mark_paints_accent_and_structure_inside_its_box() {
        let theme = Theme::family_default();
        let palette = Palette::from_theme(theme);
        let size = Size::new(240, 240).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        shapes::fill_rect(
            &mut frame,
            Rect::new(0.0, 0.0, 240.0, 240.0),
            theme.color(Role::Background),
            false,
        );
        draw(&mut frame, &palette, Rect::new(0.0, 0.0, 240.0, 240.0));
        // Seam centre is pure accent; the window centre is foreground; corners stay canvas.
        assert_eq!(frame.pixel(120, 159), Some(theme.color(Role::Accent)));
        assert_eq!(frame.pixel(120, 96), Some(theme.color(Role::Foreground)));
        assert_eq!(frame.pixel(2, 2), Some(theme.color(Role::Background)));
        assert_eq!(frame.pixel(237, 237), Some(theme.color(Role::Background)));
        // The shell interior is surface.
        assert_eq!(frame.pixel(120, 200), Some(theme.color(Role::Surface)));
    }
}
