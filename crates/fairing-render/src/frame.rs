// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The heap frame the compositor draws into, in the output's byte order.
//!
//! tiny-skia paints premultiplied RGBA in memory order `[r, g, b, a]`. DRM's
//! `XRGB8888` and the common 32-bit fbdev layout are the little-endian word
//! `0xXXRRGGBB`, i.e. memory order `[b, g, r, x]`. Drawing every opaque colour
//! with red and blue swapped lands each channel in its scan-out byte, and the
//! copy to the device is then a plain row copy. The swap lives in exactly one
//! place, [`PixelFormat::encode`] (used by [`Frame::paint`] and the text blit), and is
//! pinned by a test.

use std::fmt;
use std::io::{self, Write};

use fairing_theme::Rgb;
use tiny_skia::{Paint, Pixmap, PixmapMut};

use crate::fault::{RenderError, RenderErrorKind};
use crate::geometry::Size;

/// Bytes per pixel in every format this crate handles.
pub const BYTES_PER_PIXEL: usize = 4;
/// Largest frame allocated: 256 MiB, an 8192x8192 canvas, far beyond any panel.
const MAX_FRAME_BYTES: u64 = 256 * 1024 * 1024;

/// The byte order of a 32-bit pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// `[r, g, b, a]` — tiny-skia's native order; the memory backend and snapshots.
    Rgba8888,
    /// `[b, g, r, x]` — DRM `XRGB8888` and 32-bit fbdev on little-endian hosts.
    Xrgb8888,
}

impl PixelFormat {
    /// Stable lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rgba8888 => "rgba8888",
            Self::Xrgb8888 => "xrgb8888",
        }
    }

    /// The four bytes of an opaque colour in this format.
    #[must_use]
    pub const fn encode(self, color: Rgb) -> [u8; BYTES_PER_PIXEL] {
        match self {
            Self::Rgba8888 => [color.r, color.g, color.b, 0xFF],
            Self::Xrgb8888 => [color.b, color.g, color.r, 0xFF],
        }
    }

    /// The colour of four bytes in this format (the fourth byte is ignored).
    #[must_use]
    pub const fn decode(self, bytes: [u8; BYTES_PER_PIXEL]) -> Rgb {
        match self {
            Self::Rgba8888 => Rgb::new(bytes[0], bytes[1], bytes[2]),
            Self::Xrgb8888 => Rgb::new(bytes[2], bytes[1], bytes[0]),
        }
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A heap pixel buffer of one output's size and byte order.
#[derive(Clone)]
pub struct Frame {
    pixmap: Pixmap,
    format: PixelFormat,
    size: Size,
}

impl Frame {
    /// Allocates a zeroed frame.
    ///
    /// # Errors
    ///
    /// [`RenderErrorKind::InvalidGeometry`] when the frame would exceed 256 MiB or
    /// tiny-skia's limits.
    pub fn new(size: Size, format: PixelFormat) -> Result<Self, RenderError> {
        // A corrupt mode or sysfs attribute must fail here, not abort the
        // process in the allocator (the release profile cannot unwind).
        let bytes = u64::from(size.width())
            * u64::from(size.height())
            * u64::try_from(BYTES_PER_PIXEL).unwrap_or(u64::MAX);
        if bytes > MAX_FRAME_BYTES {
            return Err(RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!(
                    "a {size} frame needs {bytes} bytes, above the {} MiB limit",
                    MAX_FRAME_BYTES / (1024 * 1024)
                ),
            ));
        }
        let pixmap = Pixmap::new(size.width(), size.height()).ok_or_else(|| {
            RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!("cannot allocate a {size} frame"),
            )
        })?;
        Ok(Self {
            pixmap,
            format,
            size,
        })
    }

    /// The frame's size.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// The frame's byte order.
    #[must_use]
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    /// Bytes per row (`width * 4`; frames are never padded).
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.size.width() as usize * BYTES_PER_PIXEL
    }

    /// The whole buffer, row-major, `stride()` bytes per row.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Row `y` of the buffer, or `None` past the bottom edge.
    #[must_use]
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        if y >= self.size.height() {
            return None;
        }
        let stride = self.stride();
        let start = y as usize * stride;
        self.data().get(start..start + stride)
    }

    /// The colour at (`x`, `y`), or `None` outside the frame.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        if x >= self.size.width() {
            return None;
        }
        let row = self.row(y)?;
        let start = x as usize * BYTES_PER_PIXEL;
        let bytes: [u8; BYTES_PER_PIXEL] =
            row.get(start..start + BYTES_PER_PIXEL)?.try_into().ok()?;
        Some(self.format.decode(bytes))
    }

    /// An opaque solid paint for `color` in this frame's byte order.
    #[must_use]
    pub(crate) fn paint(&self, color: Rgb, anti_alias: bool) -> Paint<'static> {
        let [c0, c1, c2, a] = self.format.encode(color);
        let mut paint = Paint::default();
        paint.set_color_rgba8(c0, c1, c2, a);
        paint.anti_alias = anti_alias;
        paint
    }

    /// The four bytes `color` occupies in this frame.
    #[must_use]
    pub const fn encode(&self, color: Rgb) -> [u8; BYTES_PER_PIXEL] {
        self.format.encode(color)
    }

    /// The tiny-skia view for drawing (crate-internal: the raster library never leaks).
    pub(crate) fn pixmap_mut(&mut self) -> PixmapMut<'_> {
        self.pixmap.as_mut()
    }

    /// The raw bytes for direct blits (text), row-major with `stride()` bytes per row.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.pixmap.data_mut()
    }

    /// Writes the frame as a binary PPM (`P6`), the dependency-free snapshot format.
    ///
    /// # Errors
    ///
    /// Any write failure on `out`.
    pub fn write_ppm(&self, out: &mut impl Write) -> io::Result<()> {
        write!(
            out,
            "P6\n{} {}\n255\n",
            self.size.width(),
            self.size.height()
        )?;
        let mut row = Vec::with_capacity(self.size.width() as usize * 3);
        for y in 0..self.size.height() {
            row.clear();
            for x in 0..self.size.width() {
                let color = self.pixel(x, y).unwrap_or(Rgb::new(0, 0, 0));
                row.extend_from_slice(&[color.r, color.g, color.b]);
            }
            out.write_all(&row)?;
        }
        out.flush()
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("size", &self.size)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use tiny_skia::{Rect, Transform};

    use super::*;

    fn frame(format: PixelFormat) -> Frame {
        Frame::new(Size::new(4, 2).unwrap_or_else(|e| panic!("{e}")), format)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn xrgb_paint_lands_channels_in_scanout_bytes() {
        let mut frame = frame(PixelFormat::Xrgb8888);
        let color = Rgb::new(0xAA, 0xBB, 0xCC);
        let paint = frame.paint(color, false);
        let rect = Rect::from_xywh(0.0, 0.0, 4.0, 2.0).unwrap_or_else(|| panic!("rect"));
        frame
            .pixmap_mut()
            .fill_rect(rect, &paint, Transform::identity(), None);
        let bytes: [u8; 4] = frame.data()[..4].try_into().unwrap_or([0; 4]);
        assert_eq!(u32::from_le_bytes(bytes), 0xFFAA_BBCC, "XRGB8888 word");
        assert_eq!(frame.pixel(3, 1), Some(color));
        assert_eq!(frame.pixel(4, 0), None);
        assert_eq!(frame.encode(color), [0xCC, 0xBB, 0xAA, 0xFF]);
    }

    #[test]
    fn rgba_paint_is_natural_order() {
        let mut frame = frame(PixelFormat::Rgba8888);
        let color = Rgb::new(1, 2, 3);
        let paint = frame.paint(color, false);
        let rect = Rect::from_xywh(0.0, 0.0, 4.0, 2.0).unwrap_or_else(|| panic!("rect"));
        frame
            .pixmap_mut()
            .fill_rect(rect, &paint, Transform::identity(), None);
        assert_eq!(&frame.data()[..4], &[1, 2, 3, 255]);
        assert_eq!(frame.pixel(0, 0), Some(color));
        assert_eq!(frame.row(1).map(<[u8]>::len), Some(16));
        assert!(frame.row(2).is_none());
    }

    #[test]
    fn ppm_has_header_and_rgb_payload() {
        let mut frame = frame(PixelFormat::Xrgb8888);
        let paint = frame.paint(Rgb::new(9, 8, 7), false);
        let rect = Rect::from_xywh(0.0, 0.0, 4.0, 2.0).unwrap_or_else(|| panic!("rect"));
        frame
            .pixmap_mut()
            .fill_rect(rect, &paint, Transform::identity(), None);
        let mut out = Vec::new();
        frame.write_ppm(&mut out).unwrap_or_else(|e| panic!("{e}"));
        assert!(out.starts_with(b"P6\n4 2\n255\n"));
        assert_eq!(out.len(), b"P6\n4 2\n255\n".len() + 4 * 2 * 3);
        assert_eq!(&out[out.len() - 3..], &[9, 8, 7]);
    }
}
