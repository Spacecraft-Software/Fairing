// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! A backend that keeps the last frame in memory: tests, agents, snapshots.

use crate::backend::{Backend, BackendKind};
use crate::fault::{RenderError, RenderErrorKind};
use crate::frame::{Frame, PixelFormat};
use crate::geometry::Size;

/// Presents into memory and remembers the most recent frame.
#[derive(Debug)]
pub struct MemoryBackend {
    size: Size,
    format: PixelFormat,
    last: Option<Frame>,
    presented: u64,
    #[cfg(test)]
    fail_next: Option<RenderErrorKind>,
}

impl MemoryBackend {
    /// A memory output of `size` in `format`.
    ///
    /// # Errors
    ///
    /// Never fails today; the signature matches the other backends.
    pub fn new(size: Size, format: PixelFormat) -> Result<Self, RenderError> {
        Ok(Self {
            size,
            format,
            last: None,
            presented: 0,
            #[cfg(test)]
            fail_next: None,
        })
    }

    /// Makes the next `present` fail with `kind` (tests of the lost-output path).
    #[cfg(test)]
    pub(crate) const fn fail_next(&mut self, kind: RenderErrorKind) {
        self.fail_next = Some(kind);
    }

    /// The most recently presented frame.
    #[must_use]
    pub const fn last_frame(&self) -> Option<&Frame> {
        self.last.as_ref()
    }

    /// How many frames were presented.
    #[must_use]
    pub const fn presented(&self) -> u64 {
        self.presented
    }
}

impl Backend for MemoryBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Memory
    }

    fn size(&self) -> Size {
        self.size
    }

    fn format(&self) -> PixelFormat {
        self.format
    }

    fn present(&mut self, frame: &Frame) -> Result<(), RenderError> {
        #[cfg(test)]
        if let Some(kind) = self.fail_next.take() {
            return Err(RenderError::new(kind, "injected failure").on(BackendKind::Memory));
        }
        if frame.size() != self.size || frame.format() != self.format {
            return Err(RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!(
                    "frame is {} {} but the output is {} {}",
                    frame.size(),
                    frame.format(),
                    self.size,
                    self.format
                ),
            )
            .on(BackendKind::Memory));
        }
        self.last = Some(frame.clone());
        self.presented += 1;
        Ok(())
    }

    fn close(self) -> Result<(), RenderError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_the_last_frame_and_rejects_mismatches() {
        let size = Size::new(8, 4).unwrap_or_else(|e| panic!("{e}"));
        let mut backend =
            MemoryBackend::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        assert!(backend.last_frame().is_none());
        let frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(backend.presented(), 1);
        assert_eq!(backend.last_frame().map(Frame::size), Some(size));
        let wrong = Frame::new(size, PixelFormat::Xrgb8888).unwrap_or_else(|e| panic!("{e}"));
        let error = backend
            .present(&wrong)
            .err()
            .unwrap_or_else(|| panic!("expected error"));
        assert_eq!(error.kind(), RenderErrorKind::InvalidGeometry);
        assert_eq!(error.backend(), Some(BackendKind::Memory));
        backend.close().unwrap_or_else(|e| panic!("{e}"));
    }
}
