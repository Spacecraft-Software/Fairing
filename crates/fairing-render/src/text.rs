// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Glyph rasterisation and blitting with the bundled Inconsolata (OFL-1.1).
//!
//! Glyph coverage from fontdue is blended straight into the frame — an exact
//! per-channel lerp between the opaque text colour and the pixel underneath,
//! which is what tiny-skia's mask pipeline computes for opaque sources without
//! needing a frame-sized mask. Rasterised glyphs are cached by character and
//! pixel size; the status charset is small and the cache is bounded by it.

use std::collections::HashMap;
use std::fmt;

use fairing_theme::Rgb;
use fontdue::{Font, FontSettings, LineMetrics, Metrics};

use crate::fault::{RenderError, RenderErrorKind};
use crate::frame::{BYTES_PER_PIXEL, Frame};
use crate::geometry::{as_f32, round_i32};

/// Inconsolata Regular (OFL-1.1; the licence text ships beside it in `assets/fonts/`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/Inconsolata-Regular.ttf");

/// The glyph drawn for a character the font lacks.
const SUBSTITUTE: char = '?';

/// Smallest size worth rasterising.
pub(crate) const MIN_PX: u32 = 6;

/// A rasterised glyph: metrics plus row-major coverage, `width * height` bytes.
struct Glyph {
    metrics: Metrics,
    coverage: Vec<u8>,
}

/// Text measurement and drawing with the bundled font.
pub(crate) struct TextRenderer {
    font: Font,
    cache: HashMap<(char, u32), Glyph>,
}

impl TextRenderer {
    /// Parses the bundled font.
    ///
    /// # Errors
    ///
    /// [`RenderErrorKind::Font`] if the bundled file does not parse (a build defect).
    pub(crate) fn new() -> Result<Self, RenderError> {
        let settings = FontSettings {
            scale: 30.0,
            ..FontSettings::default()
        };
        let font = Font::from_bytes(FONT_BYTES, settings)
            .map_err(|reason| RenderError::new(RenderErrorKind::Font, reason))?;
        Ok(Self {
            font,
            cache: HashMap::new(),
        })
    }

    /// Ascent, descent and line height at `px`.
    pub(crate) fn line_metrics(&self, px: u32) -> LineMetrics {
        let size = as_f32(px.max(MIN_PX));
        self.font
            .horizontal_line_metrics(size)
            .unwrap_or(LineMetrics {
                // A font without an hhea table; approximate the usual proportions.
                ascent: size * 0.8,
                descent: -size * 0.2,
                line_gap: 0.0,
                new_line_size: size,
            })
    }

    /// The advance of `text` at `px`, in pixels.
    pub(crate) fn measure(&mut self, text: &str, px: u32) -> f32 {
        text.chars()
            .map(|ch| self.glyph(ch, px).metrics.advance_width)
            .sum()
    }

    /// Draws `text` with its pen at (`x`, `baseline`) and returns the pen's final x.
    pub(crate) fn draw(
        &mut self,
        frame: &mut Frame,
        text: &str,
        x: f32,
        baseline: f32,
        px: u32,
        color: Rgb,
    ) -> f32 {
        let px = px.max(MIN_PX);
        let src = frame.encode(color);
        let mut pen = x;
        for ch in text.chars() {
            let glyph = self.glyph(ch, px);
            // `xmin`/`ymin` are whole pixels, so rounding the pen first loses nothing.
            let origin_x = round_i32(pen).saturating_add(glyph.metrics.xmin);
            let height = i32::try_from(glyph.metrics.height).unwrap_or(i32::MAX);
            let top = round_i32(baseline)
                .saturating_sub(glyph.metrics.ymin)
                .saturating_sub(height);
            blit(frame, glyph, origin_x, top, src);
            pen += glyph.metrics.advance_width;
        }
        pen
    }

    fn glyph(&mut self, ch: char, px: u32) -> &Glyph {
        let px = px.max(MIN_PX);
        let ch = if self.font.has_glyph(ch) {
            ch
        } else {
            SUBSTITUTE
        };
        self.cache.entry((ch, px)).or_insert_with(|| {
            let (metrics, coverage) = self.font.rasterize(ch, as_f32(px));
            Glyph { metrics, coverage }
        })
    }
}

impl fmt::Debug for TextRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextRenderer")
            .field("font", &self.font.name())
            .field("cached_glyphs", &self.cache.len())
            .finish()
    }
}

/// Blends one glyph into the frame at (`x0`, `y0`), clipped to the frame.
fn blit(frame: &mut Frame, glyph: &Glyph, x0: i32, y0: i32, src: [u8; BYTES_PER_PIXEL]) {
    let width = glyph.metrics.width;
    let height = glyph.metrics.height;
    if width == 0 || height == 0 || glyph.coverage.len() < width * height {
        return;
    }
    let frame_w = i64::from(frame.size().width());
    let frame_h = i64::from(frame.size().height());
    let stride = frame.stride();
    let data = frame.data_mut();
    for row in 0..height {
        let y = i64::from(y0) + i64::try_from(row).unwrap_or(i64::MAX);
        if y < 0 || y >= frame_h {
            continue;
        }
        let Ok(y) = usize::try_from(y) else { continue };
        for col in 0..width {
            let cover = glyph.coverage[row * width + col];
            if cover == 0 {
                continue;
            }
            let x = i64::from(x0) + i64::try_from(col).unwrap_or(i64::MAX);
            if x < 0 || x >= frame_w {
                continue;
            }
            let Ok(x) = usize::try_from(x) else { continue };
            let offset = y * stride + x * BYTES_PER_PIXEL;
            let Some(dst) = data.get_mut(offset..offset + BYTES_PER_PIXEL) else {
                continue;
            };
            for channel in 0..3 {
                dst[channel] = lerp(dst[channel], src[channel], cover);
            }
            dst[3] = 0xFF;
        }
    }
}

/// `dst + (src - dst) * cover / 255`, rounded to nearest.
fn lerp(dst: u8, src: u8, cover: u8) -> u8 {
    let (d, s, c) = (u32::from(dst), u32::from(src), u32::from(cover));
    let blended = (s * c + d * (255 - c) + 127) / 255;
    u8::try_from(blended).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;

    fn frame(w: u32, h: u32) -> Frame {
        Frame::new(
            Size::new(w, h).unwrap_or_else(|e| panic!("{e}")),
            PixelFormat::Rgba8888,
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn bundled_font_loads_and_is_monospace() {
        let mut text = TextRenderer::new().unwrap_or_else(|e| panic!("{e}"));
        let one = text.measure("0", 30);
        let many = text.measure("100%", 30);
        assert!((many - 4.0 * one).abs() < 1e-3, "{one} {many}");
        let metrics = text.line_metrics(30);
        assert!(metrics.ascent > 0.0 && metrics.descent < 0.0);
        assert!(text.font.has_glyph('%') && text.font.has_glyph('é'));
        // The asset is Inconsolata Regular 3.100, the static instance (964 glyphs).
        assert_eq!(text.font.name(), Some("Inconsolata Regular"));
        assert_eq!(text.font.glyph_count(), 964);
        assert_eq!(FONT_BYTES.len(), 105_284);
    }

    #[test]
    fn lerp_is_exact_at_the_ends_and_rounds() {
        assert_eq!(lerp(10, 200, 0), 10);
        assert_eq!(lerp(10, 200, 255), 200);
        assert_eq!(lerp(0, 255, 128), 128);
    }

    #[test]
    fn drawing_paints_the_text_colour_inside_the_glyph_box_only() {
        let mut text = TextRenderer::new().unwrap_or_else(|e| panic!("{e}"));
        let mut frame = frame(120, 60);
        let color = Rgb::new(0xD9, 0xDE, 0xE5);
        let end = text.draw(&mut frame, "42%", 10.0, 40.0, 30, color);
        assert!(end > 10.0 + 3.0 * 10.0, "pen advanced: {end}");
        let painted = (0..60)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) != Some(Rgb::new(0, 0, 0)))
            .count();
        assert!(painted > 50, "{painted} pixels painted");
        // Nothing lands above the ascent or left of the pen.
        for y in 0..8 {
            for x in 0..120 {
                assert_eq!(frame.pixel(x, y), Some(Rgb::new(0, 0, 0)), "({x},{y})");
            }
        }
        for y in 0..60 {
            for x in 0..8 {
                assert_eq!(frame.pixel(x, y), Some(Rgb::new(0, 0, 0)), "({x},{y})");
            }
        }
        // Fully covered pixels carry the exact colour.
        assert!(
            (0..60)
                .flat_map(|y| (0..120).map(move |x| (x, y)))
                .any(|(x, y)| frame.pixel(x, y) == Some(color))
        );
    }

    #[test]
    fn glyphs_off_the_frame_are_clipped_not_panicking() {
        let mut text = TextRenderer::new().unwrap_or_else(|e| panic!("{e}"));
        let mut frame = frame(16, 16);
        text.draw(&mut frame, "Clipped", -30.0, -30.0, 40, Rgb::new(1, 2, 3));
        text.draw(&mut frame, "Clipped", 1000.0, 1000.0, 40, Rgb::new(1, 2, 3));
        text.draw(&mut frame, "\u{1F680}", 2.0, 12.0, 12, Rgb::new(1, 2, 3));
    }
}
