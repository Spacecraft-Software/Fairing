// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The built-in Steelbore emblem and wordmark, filled in the accent role.
//!
//! The branding repository's square emblem fills compact logo boxes; its
//! full wordmark fills wide ones. Both use top-left SVG coordinates without
//! transforms and scale uniformly, retaining the active palette's colour.
//!
//! Only bundled path data is parsed. The small path-only assets need no
//! runtime SVG dependency or shared cache in the initrd.

use fairing_theme::Role;
use tiny_skia::{Path, PathBuilder};

use crate::frame::Frame;
use crate::geometry::Rect;
use crate::palette::Palette;
use crate::shapes;

/// The branding assets, recoloured to currentColor when vendored.
const EMBLEM: Mark = Mark {
    svg: include_str!("../assets/logo/steelbore.svg"),
    width: 213.0,
    height: 213.0,
};
const WORDMARK: Mark = Mark {
    svg: include_str!("../assets/logo/steelbore-wordmark.svg"),
    width: 600.0,
    height: 257.0,
};

#[derive(Clone, Copy)]
struct Mark {
    svg: &'static str,
    width: f32,
    height: f32,
}

/// Draws the emblem or wordmark uniformly fitted and centred in `bounds`.
pub(crate) fn draw(frame: &mut Frame, palette: &Palette, bounds: Rect) {
    if bounds.width < 4.0 || bounds.height < 4.0 {
        return;
    }
    // Wide theme boxes use the wordmark; square or tall ones use the emblem.
    let asset = if bounds.width >= 2.0 * bounds.height {
        WORDMARK
    } else {
        EMBLEM
    };
    if let Some(mark) = outline(asset, bounds) {
        shapes::fill(frame, &mark, palette.color(Role::Accent));
    }
}

/// The logo's outline fitted into `bounds`; `None` if the path data is malformed.
fn outline(asset: Mark, bounds: Rect) -> Option<Path> {
    let scale = (bounds.width / asset.width).min(bounds.height / asset.height);
    let left = bounds.x + (bounds.width - asset.width * scale) / 2.0;
    let top = bounds.y + (bounds.height - asset.height * scale) / 2.0;
    let map = |x: f32, y: f32| (left + x * scale, top + y * scale);
    let mut pb = PathBuilder::new();
    for data in path_data(asset.svg) {
        trace(data, &map, &mut pb)?;
    }
    pb.finish()
}

/// The `d` attribute of every `<path>` in `svg`, in document order.
fn path_data(svg: &str) -> impl Iterator<Item = &str> {
    svg.split(" d=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
}

/// Appends one path's data to `pb`, mapping each point through `map`.
///
/// Understands the commands the bundled branding assets use, `M`/`m`, `L`/`l`, `C`/`c` and
/// `Z`/`z`, with implicit repetition; anything else is refused with `None`.
fn trace(data: &str, map: &impl Fn(f32, f32) -> (f32, f32), pb: &mut PathBuilder) -> Option<()> {
    let mut tokens = Tokens { rest: data };
    let mut command = None;
    let (mut x, mut y) = (0.0_f32, 0.0_f32);
    let (mut start_x, mut start_y) = (0.0_f32, 0.0_f32);
    loop {
        tokens.skip_separators();
        if tokens.rest.is_empty() {
            return Some(());
        }
        if let Some(letter) = tokens.letter() {
            command = Some(letter);
            if letter.eq_ignore_ascii_case(&'z') {
                pb.close();
                (x, y) = (start_x, start_y);
                continue;
            }
        }
        let letter = command?;
        let (dx, dy) = if letter.is_ascii_lowercase() {
            (x, y)
        } else {
            (0.0, 0.0)
        };
        match letter.to_ascii_uppercase() {
            'M' => {
                (x, y) = (dx + tokens.number()?, dy + tokens.number()?);
                (start_x, start_y) = (x, y);
                let (px, py) = map(x, y);
                pb.move_to(px, py);
                // Pairs after a moveto are linetos (SVG 1.1 §8.3.2).
                command = Some(if letter == 'm' { 'l' } else { 'L' });
            }
            'L' => {
                (x, y) = (dx + tokens.number()?, dy + tokens.number()?);
                let (px, py) = map(x, y);
                pb.line_to(px, py);
            }
            'C' => {
                let (x1, y1) = map(dx + tokens.number()?, dy + tokens.number()?);
                let (x2, y2) = map(dx + tokens.number()?, dy + tokens.number()?);
                (x, y) = (dx + tokens.number()?, dy + tokens.number()?);
                let (px, py) = map(x, y);
                pb.cubic_to(x1, y1, x2, y2, px, py);
            }
            _ => return None,
        }
    }
}

/// A cursor over SVG path data.
struct Tokens<'a> {
    rest: &'a str,
}

impl Tokens<'_> {
    fn skip_separators(&mut self) {
        self.rest = self
            .rest
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
    }

    /// Takes a command letter, if one is next.
    fn letter(&mut self) -> Option<char> {
        let letter = self.rest.chars().next().filter(char::is_ascii_alphabetic)?;
        self.rest = &self.rest[letter.len_utf8()..];
        Some(letter)
    }

    /// Takes a number: an optional sign, digits, and an optional fraction.
    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let bytes = self.rest.as_bytes();
        let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
        let mut seen_point = false;
        while let Some(&byte) = bytes.get(end) {
            match byte {
                b'0'..=b'9' => {}
                b'.' if !seen_point => seen_point = true,
                _ => break,
            }
            end += 1;
        }
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        number.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use fairing_theme::Theme;

    use super::*;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;

    #[test]
    fn branding_assets_have_the_expected_coordinate_system() {
        for (asset, view_box, count) in [(EMBLEM, "0 0 213 213", 7), (WORDMARK, "0 0 600 257", 30)]
        {
            assert!(asset.svg.contains(&format!("viewBox=\"{view_box}\"")));
            assert!(!asset.svg.contains("transform="));
            assert!(
                !asset.svg.contains("<polygon"),
                "polygons must be normalised to paths"
            );
            assert!(asset.svg.contains("fill=\"currentColor\""));
            assert_eq!(path_data(asset.svg).count(), count);
        }
    }

    #[test]
    fn both_outlines_fit_wide_and_tall_boxes_without_distortion() {
        for asset in [EMBLEM, WORDMARK] {
            for bounds in [
                Rect::new(10.0, 20.0, 600.0, 300.0),
                Rect::new(10.0, 20.0, 200.0, 500.0),
            ] {
                let mark = outline(asset, bounds).unwrap_or_else(|| panic!("bundled paths parse"));
                let b = mark.bounds();
                assert!(b.left() >= bounds.x && b.top() >= bounds.y, "{b:?}");
                assert!(
                    b.right() <= bounds.x + bounds.width && b.bottom() <= bounds.y + bounds.height,
                    "{b:?}"
                );
                assert!(b.width() > bounds.width.min(bounds.height) * 0.8, "{b:?}");
            }
        }
    }

    #[test]
    fn wide_boxes_draw_the_wordmark_in_the_active_role() {
        let theme = Theme::family_default();
        let palette = Palette::from_theme(theme);
        let size = Size::new(800, 400).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        let background = theme.color(Role::Background);
        shapes::fill_rect(
            &mut frame,
            Rect::new(0.0, 0.0, 800.0, 400.0),
            background,
            false,
        );
        draw(&mut frame, &palette, Rect::new(100.0, 50.0, 600.0, 257.0));
        let accent = theme.color(Role::Accent);
        let (mut left, mut right) = (800_u32, 0_u32);
        for y in 0..400 {
            for x in 0..800 {
                let pixel = frame.pixel(x, y);
                if !(100..700).contains(&x) || !(50..307).contains(&y) {
                    assert_eq!(pixel, Some(background));
                }
                if pixel == Some(accent) {
                    left = left.min(x);
                    right = right.max(x);
                }
            }
        }
        // The wordmark spans the wide box; a centred square emblem cannot.
        assert!(right.saturating_sub(left) > 500);
    }

    #[test]
    fn malformed_path_data_is_refused() {
        let map = |x: f32, y: f32| (x, y);
        for data in ["10 10", "M10", "M10 10 q1 2 3 4", "M10 10 l1 x"] {
            assert!(
                trace(data, &map, &mut PathBuilder::new()).is_none(),
                "{data}"
            );
        }
        assert!(
            trace(
                "M1 2 3 4 c1 1 2 2 3 3 l-1-1z",
                &map,
                &mut PathBuilder::new()
            )
            .is_some()
        );
    }

    #[test]
    fn mark_paints_accent_inside_its_box_only() {
        let theme = Theme::family_default();
        let palette = Palette::from_theme(theme);
        let size = Size::new(320, 320).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        let background = theme.color(Role::Background);
        shapes::fill_rect(
            &mut frame,
            Rect::new(0.0, 0.0, 320.0, 320.0),
            background,
            false,
        );
        draw(&mut frame, &palette, Rect::new(40.0, 40.0, 240.0, 240.0));
        let accent = theme.color(Role::Accent);
        let mut inside = 0_u32;
        for y in 0..320 {
            for x in 0..320 {
                let pixel = frame.pixel(x, y);
                let in_box = (40..280).contains(&x) && (40..280).contains(&y);
                if !in_box {
                    assert_eq!(pixel, Some(background), "({x}, {y})");
                } else if pixel == Some(accent) {
                    inside += 1;
                }
            }
        }
        // A solid mark: a fair share of the box is pure accent.
        assert!(inside > 240 * 240 / 5, "{inside} accent pixels");
    }
}
