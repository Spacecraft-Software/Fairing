// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! PNG decoding for theme assets, at compile time only.
//!
//! Every PNG colour type and bit depth is normalised to 8-bit RGBA: palettes
//! and `tRNS` chunks are expanded to alpha, 16-bit samples are stripped to 8.
//! The decoder's own allocation limit is set from the largest image a theme
//! may carry, and the size is checked from the header before any pixel
//! buffer exists, so a hostile file costs at most one bounded allocation.

use std::io::Cursor;

use crate::artefact::MAX_IMAGE_SIDE;
use crate::compile::Rgba8;
use crate::fault::{ThemeError, ThemeErrorKind};

/// The decoder's allocation budget: one image at the largest permitted size,
/// four bytes per pixel, plus room for the decoder's line buffers.
const DECODE_LIMIT: usize = 4 * (MAX_IMAGE_SIDE as usize) * (MAX_IMAGE_SIDE as usize) + (1 << 20);

/// Decodes a PNG into 8-bit RGBA.
///
/// # Errors
///
/// [`ThemeErrorKind::InvalidSource`] for a file that is not a PNG, is
/// damaged, or is larger than [`MAX_IMAGE_SIDE`] on either side.
pub(crate) fn decode(bytes: &[u8]) -> Result<Rgba8, ThemeError> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: DECODE_LIMIT,
        },
    );
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(|e| png_error(&e))?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(source_error(format!(
            "the PNG is {width}x{height}; each side must be 1..={MAX_IMAGE_SIDE}"
        )));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| source_error("the PNG's decoded size overflows"))?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(|e| png_error(&e))?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(source_error(format!(
            "the PNG decoded to {:?}-bit samples, not 8",
            frame.bit_depth
        )));
    }
    let samples: usize = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(source_error("the PNG's palette was not expanded"));
        }
    };
    let row_bytes = usize::try_from(frame.width).unwrap_or(usize::MAX) * samples;
    let mut pixels = Vec::with_capacity(
        usize::try_from(width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height).unwrap_or(0))
            .saturating_mul(4),
    );
    for row in buffer
        .chunks(frame.line_size)
        .take(usize::try_from(height).unwrap_or(0))
    {
        for pixel in row
            .get(..row_bytes)
            .unwrap_or_default()
            .chunks_exact(samples)
        {
            pixels.extend_from_slice(&match *pixel {
                [g] => [g, g, g, u8::MAX],
                [g, a] => [g, g, g, a],
                [r, g, b] => [r, g, b, u8::MAX],
                [r, g, b, a] => [r, g, b, a],
                _ => [0; 4],
            });
        }
    }
    Ok(Rgba8 {
        width,
        height,
        pixels,
    })
}

fn png_error(error: &png::DecodingError) -> ThemeError {
    source_error(format!("not a readable PNG: {error}"))
}

fn source_error(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorKind::InvalidSource, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        encode_with(width, height, color, data, |_| {})
    }

    fn encode_with(
        width: u32,
        height: u32,
        color: png::ColorType,
        data: &[u8],
        configure: impl FnOnce(&mut png::Encoder<'_, &mut Vec<u8>>),
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        configure(&mut encoder);
        let mut writer = encoder.write_header().unwrap_or_else(|e| panic!("{e}"));
        writer
            .write_image_data(data)
            .unwrap_or_else(|e| panic!("{e}"));
        // The writer finishes the stream when dropped and borrows `out` until then.
        drop(writer);
        out
    }

    #[test]
    fn every_colour_type_becomes_rgba() {
        let rgba = decode(&encode(1, 1, png::ColorType::Rgba, &[1, 2, 3, 4]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rgba.pixels, [1, 2, 3, 4]);
        let rgb = decode(&encode(1, 1, png::ColorType::Rgb, &[5, 6, 7]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rgb.pixels, [5, 6, 7, 255]);
        let grey = decode(&encode(
            2,
            1,
            png::ColorType::GrayscaleAlpha,
            &[9, 128, 10, 0],
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(grey.pixels, [9, 9, 9, 128, 10, 10, 10, 0]);
        let indexed = decode(&encode_with(
            2,
            1,
            png::ColorType::Indexed,
            &[0, 1],
            |encoder| {
                encoder.set_palette(vec![10, 20, 30, 40, 50, 60]);
                encoder.set_trns(vec![255, 0]);
            },
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(indexed.pixels, [10, 20, 30, 255, 40, 50, 60, 0]);
        assert_eq!((indexed.width, indexed.height), (2, 1));
    }

    #[test]
    fn garbage_and_oversized_images_are_refused() {
        let kind = |bytes: &[u8]| decode(bytes).err().map(|e| e.kind());
        assert_eq!(kind(b"not a png"), Some(ThemeErrorKind::InvalidSource));
        let mut truncated = encode(4, 4, png::ColorType::Rgba, &[7; 64]);
        truncated.truncate(truncated.len() / 2);
        assert_eq!(kind(&truncated), Some(ThemeErrorKind::InvalidSource));
        // A valid header that claims 4000x1: refused from the header.
        let wide = encode(4000, 1, png::ColorType::GrayscaleAlpha, &vec![0; 8000]);
        assert_eq!(kind(&wide), Some(ThemeErrorKind::InvalidSource));
    }
}
