// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The built-in vector mark: the Steelbore logo, drawn in the accent role.
//!
//! Fairing draws it whenever a theme names `'builtin` and has no PNG of its
//! own. The outline comes from `assets/logo/steelbore.svg` (a potrace trace,
//! paths only, recoloured to `currentColor`) and is filled in one role, so it
//! scales to any mode and survives every palette, mono included.
//!
//! The path data is read again on every draw rather than cached: it is nine
//! short paths, a few microseconds of parsing next to the fill itself, and it
//! keeps the mark free of shared state.

use fairing_theme::Role;
use tiny_skia::{Path, PathBuilder};

use crate::frame::Frame;
use crate::geometry::Rect;
use crate::palette::Palette;
use crate::shapes;

/// The logo, as the branding repository ships it bar the fill colour.
const SVG: &str = include_str!("../assets/logo/steelbore.svg");

/// Side of the SVG's square `viewBox`.
const VIEW_BOX: f32 = 213.0;

/// Size of one path-data unit in `viewBox` units.
///
/// Potrace writes path data in tenths of a unit with y pointing up; the SVG's
/// group transform, `translate(0,213) scale(0.1,-0.1)`, undoes both. A test
/// pins that transform, so a re-traced logo with another one fails loudly.
const UNIT: f32 = 0.1;

/// Draws the mark inside `bounds` (a square in output pixels).
pub(crate) fn draw(frame: &mut Frame, palette: &Palette, bounds: Rect) {
    if bounds.width < 4.0 || bounds.height < 4.0 {
        return;
    }
    if let Some(mark) = outline(bounds) {
        shapes::fill(frame, &mark, palette.color(Role::Accent));
    }
}

/// The logo's outline fitted into `bounds`; `None` if the path data is malformed.
fn outline(bounds: Rect) -> Option<Path> {
    let scale = bounds.width.min(bounds.height) / VIEW_BOX;
    let map = |x: f32, y: f32| {
        (
            bounds.x + x * UNIT * scale,
            bounds.y + (VIEW_BOX - y * UNIT) * scale,
        )
    };
    let mut pb = PathBuilder::new();
    for data in path_data(SVG) {
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
/// Understands the commands potrace emits, `M`/`m`, `L`/`l`, `C`/`c` and
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
    fn the_svg_is_the_potrace_layout_the_mark_assumes() {
        assert!(SVG.contains(r#"viewBox="0 0 213.000000 213.000000""#));
        assert!(SVG.contains("translate(0.000000,213.000000) scale(0.100000,-0.100000)"));
        assert!(SVG.contains(r#"fill="currentColor""#), "no literal colour");
        assert_eq!(path_data(SVG).count(), 9);
    }

    #[test]
    fn the_outline_parses_and_fits_its_box() {
        let bounds = Rect::new(10.0, 20.0, 213.0, 213.0);
        let mark = outline(bounds).unwrap_or_else(|| panic!("the logo's path data parses"));
        let b = mark.bounds();
        assert!(b.left() >= 10.0 && b.top() >= 20.0, "{b:?}");
        assert!(b.right() <= 223.0 && b.bottom() <= 233.0, "{b:?}");
        // The glyph spans most of the box, not a corner of it.
        assert!(b.width() > 150.0 && b.height() > 150.0, "{b:?}");
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
