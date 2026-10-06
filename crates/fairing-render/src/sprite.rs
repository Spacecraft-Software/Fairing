// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Theme images: role-indexed or mask pixels, coloured by the active palette.
//!
//! A compiled theme stores no colour (FRN-SRS-044); a [`Sprite`] turns its
//! roles into the palette's values once per pixel format, premultiplied and
//! in the frame's byte order, and then draws that bitmap fitted into a box
//! with bilinear filtering. The coloured bitmap is cached, so a frame costs
//! one scaled blit, not a recolouring.

use fairing_theme::{Encoding, ImageView};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::frame::{BYTES_PER_PIXEL, Frame, PixelFormat};
use crate::geometry::{Rect, as_f32};
use crate::palette::Palette;

/// An owned theme image with its coloured bitmap.
#[derive(Clone)]
pub(crate) struct Sprite {
    width: u32,
    height: u32,
    encoding: Encoding,
    pixels: Vec<u8>,
    coloured: Option<(PixelFormat, Pixmap)>,
}

impl Sprite {
    /// Copies an image out of a compiled theme.
    pub(crate) fn new(view: ImageView<'_>) -> Self {
        Self {
            width: view.width(),
            height: view.height(),
            encoding: view.encoding(),
            pixels: view.pixels().to_vec(),
            coloured: None,
        }
    }

    /// Draws the image fitted uniformly into `bounds` (output pixels), centred.
    pub(crate) fn draw(&mut self, frame: &mut Frame, palette: &Palette, bounds: Rect) {
        if bounds.width < 1.0 || bounds.height < 1.0 {
            return;
        }
        let format = frame.format();
        if self.coloured.as_ref().is_none_or(|(f, _)| *f != format) {
            self.coloured = self.colour(palette, format).map(|p| (format, p));
        }
        let Some((_, bitmap)) = &self.coloured else {
            return;
        };
        let (w, h) = (as_f32(self.width), as_f32(self.height));
        let scale = (bounds.width / w).min(bounds.height / h);
        let x = bounds.center_x() - w * scale / 2.0;
        let y = bounds.center_y() - h * scale / 2.0;
        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        frame.pixmap_mut().draw_pixmap(
            0,
            0,
            bitmap.as_ref(),
            &paint,
            Transform::from_row(scale, 0.0, 0.0, scale, x, y),
            None,
        );
    }

    /// Builds the premultiplied bitmap in `format`'s byte order.
    fn colour(&self, palette: &Palette, format: PixelFormat) -> Option<Pixmap> {
        let mut bitmap = Pixmap::new(self.width, self.height)?;
        let per_pixel = self.encoding.bytes_per_pixel();
        for (out, source) in bitmap
            .data_mut()
            .chunks_exact_mut(BYTES_PER_PIXEL)
            .zip(self.pixels.chunks_exact(per_pixel))
        {
            let (role, coverage) = match self.encoding {
                Encoding::Mask(role) => (Some(role), source[0]),
                Encoding::RoleIndexed => (
                    fairing_theme::Role::ALL
                        .get(usize::from(source[0]))
                        .copied(),
                    source[1],
                ),
            };
            let Some(role) = role else {
                out.copy_from_slice(&[0; BYTES_PER_PIXEL]);
                continue;
            };
            let [c0, c1, c2, _] = format.encode(palette.color(role));
            out.copy_from_slice(&[
                premultiply(c0, coverage),
                premultiply(c1, coverage),
                premultiply(c2, coverage),
                coverage,
            ]);
        }
        Some(bitmap)
    }
}

impl std::fmt::Debug for Sprite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sprite")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("encoding", &self.encoding)
            .field(
                "coloured",
                &self.coloured.as_ref().map(|(format, _)| *format),
            )
            .finish_non_exhaustive()
    }
}

/// `channel × coverage / 255`, rounded.
fn premultiply(channel: u8, coverage: u8) -> u8 {
    let product = u16::from(channel) * u16::from(coverage);
    u8::try_from((product + 127) / 255).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use fairing_theme::{CompiledTheme, ImageMeta, LogoImage, Meta, Role, Selection, Theme};

    use super::*;
    use crate::geometry::Size;

    fn theme_with(encoding: Encoding, pixels: Vec<u8>) -> CompiledTheme {
        let mut meta = Meta::builtin();
        meta.images.push(ImageMeta {
            width: 4,
            height: 4,
            encoding,
            offset: 0,
        });
        meta.boot.logo.image = LogoImage::Image(0);
        CompiledTheme::new(meta, pixels).unwrap_or_else(|e| panic!("{e}"))
    }

    fn draw(theme: &CompiledTheme, format: PixelFormat) -> (Frame, Theme) {
        let palette_theme = *Theme::family_default();
        let palette = Palette::from_selection(Selection::Color(Theme::family_default()));
        let mut sprite = Sprite::new(theme.image(0).unwrap_or_else(|| panic!("image")));
        let size = Size::new(40, 20).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, format).unwrap_or_else(|e| panic!("{e}"));
        sprite.draw(&mut frame, &palette, Rect::new(0.0, 0.0, 40.0, 20.0));
        (frame, palette_theme)
    }

    #[test]
    fn role_indexed_pixels_take_the_palette_value_in_both_byte_orders() {
        // Every pixel accent, fully covered.
        let accent = u8::try_from(
            Role::ALL
                .iter()
                .position(|r| *r == Role::Accent)
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let pixels = [accent, 255].repeat(16);
        for format in [PixelFormat::Rgba8888, PixelFormat::Xrgb8888] {
            let (frame, theme) = draw(&theme_with(Encoding::RoleIndexed, pixels.clone()), format);
            // Fitted uniformly: a 20x20 square centred in the 40x20 frame.
            assert_eq!(
                frame.pixel(20, 10),
                Some(theme.color(Role::Accent)),
                "{format}"
            );
            assert_ne!(
                frame.pixel(2, 10),
                Some(theme.color(Role::Accent)),
                "{format}"
            );
        }
    }

    #[test]
    fn masks_draw_coverage_in_their_role() {
        let (frame, theme) = draw(
            &theme_with(Encoding::Mask(Role::Structure), vec![255; 16]),
            PixelFormat::Rgba8888,
        );
        assert_eq!(frame.pixel(20, 10), Some(theme.color(Role::Structure)));
        let (empty, _) = draw(
            &theme_with(Encoding::Mask(Role::Structure), vec![0; 16]),
            PixelFormat::Rgba8888,
        );
        assert_ne!(empty.pixel(20, 10), Some(theme.color(Role::Structure)));
    }

    #[test]
    fn premultiplication_rounds() {
        assert_eq!(premultiply(255, 255), 255);
        assert_eq!(premultiply(255, 0), 0);
        assert_eq!(premultiply(200, 128), 100);
    }
}
