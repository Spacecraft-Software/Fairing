// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The legacy framebuffer, `/dev/fb0`, without a single ioctl (FRN-SRS-002).
//!
//! Geometry comes from sysfs (`modes`, `virtual_size`, `bits_per_pixel`,
//! `stride`) and pixels go in with positional writes, so the whole backend is
//! plain file I/O in safe Rust. Only 32-bit depths are driven; the byte order
//! is the little-endian `XRGB8888` that DRM's fbdev emulation produces by
//! construction at 32 bpp (a bare `efifb` on RGB-ordered firmware is the one
//! known exception). The contents found on the device are saved on open and
//! written back on close, so a preview leaves the console as it was; the
//! console cursor blink is paused through sysfs for the same reason, where
//! that attribute is writable.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::FileExt as _;
use std::path::{Path, PathBuf};

use crate::backend::{Backend, BackendKind};
use crate::fault::{RenderError, RenderErrorKind};
use crate::frame::{BYTES_PER_PIXEL, Frame, PixelFormat};
use crate::geometry::Size;

/// Where the kernel describes the first framebuffer.
pub const DEFAULT_SYSFS: &str = "/sys/class/graphics/fb0";
/// The first framebuffer device.
pub const DEFAULT_DEVICE: &str = "/dev/fb0";
/// The fbcon cursor-blink switch (`0` pauses the 200 ms cursor timer).
pub const CURSOR_BLINK: &str = "/sys/class/graphics/fbcon/cursor_blink";
/// Largest dimension accepted from sysfs; anything bigger is a corrupt attribute, not a panel.
const MAX_DIMENSION: u32 = 16_384;

/// The sysfs facts the backend needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FbInfo {
    /// Visible size: the first `modes` line, else `virtual_size`.
    pub size: Size,
    /// Colour depth.
    pub bits_per_pixel: u32,
    /// Bytes per scanline.
    pub stride: usize,
    /// The driver's identification (`fix.id`), e.g. `simpledrmdrmfb` or `EFI VGA`.
    pub name: String,
}

impl FbInfo {
    /// Reads the geometry attributes from a sysfs directory.
    ///
    /// The kernel publishes no `xres`/`yres`; the visible size is the first
    /// `modes` line (`U:1920x1080p-0`), falling back to `virtual_size`, which
    /// can exceed the panel on multi-head devices.
    ///
    /// # Errors
    ///
    /// [`RenderErrorKind::NotFound`] when the attributes are missing,
    /// [`RenderErrorKind::Unsupported`] when they do not parse, exceed sane
    /// bounds, the stride is too short, or the device is not running.
    pub fn read(sysfs: &Path) -> Result<Self, RenderError> {
        let attribute = |name: &str| -> Result<String, RenderError> {
            std::fs::read_to_string(sysfs.join(name))
                .map(|s| s.trim().to_owned())
                .map_err(|e| {
                    RenderError::from_io(format!("read `{}`", sysfs.join(name).display()), e)
                })
        };
        let unsupported = |what: String| RenderError::new(RenderErrorKind::Unsupported, what);
        let virtual_size = attribute("virtual_size")?;
        let (width, height) = parse_pair(&virtual_size, ',')
            .ok_or_else(|| unsupported(format!("virtual_size `{virtual_size}` is not `W,H`")))?;
        let (width, height) = attribute("modes")
            .ok()
            .and_then(|modes| modes.lines().next().and_then(parse_mode_line))
            .unwrap_or((width, height));
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(unsupported(format!(
                "{width}x{height} exceeds {MAX_DIMENSION} pixels"
            )));
        }
        let size = Size::new(width, height)?;
        let bits_per_pixel = attribute("bits_per_pixel")?
            .parse::<u32>()
            .map_err(|e| unsupported(format!("bits_per_pixel does not parse: {e}")))?;
        let stride = attribute("stride")?
            .parse::<usize>()
            .map_err(|e| unsupported(format!("stride does not parse: {e}")))?;
        if stride < width as usize * BYTES_PER_PIXEL && bits_per_pixel == 32 {
            return Err(unsupported(format!(
                "stride {stride} is shorter than {width} 32-bit pixels"
            )));
        }
        if stride > MAX_DIMENSION as usize * BYTES_PER_PIXEL * 2 {
            return Err(unsupported(format!("stride {stride} is implausible")));
        }
        // `state` is 0 while the device scans out; anything else is suspended.
        if let Ok(state) = attribute("state")
            && state != "0"
        {
            return Err(unsupported(format!(
                "framebuffer state is `{state}`, not running"
            )));
        }
        let name = attribute("name").unwrap_or_default();
        Ok(Self {
            size,
            bits_per_pixel,
            stride,
            name,
        })
    }
}

/// `W<sep>H` as two numbers.
fn parse_pair(text: &str, separator: char) -> Option<(u32, u32)> {
    let (w, h) = text.split_once(separator)?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// The resolution in a `modes` line such as `U:1920x1080p-60`.
pub(crate) fn parse_mode_line(line: &str) -> Option<(u32, u32)> {
    let (_, rest) = line.trim().split_once(':')?;
    let digits_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == 'x'))
        .unwrap_or(rest.len());
    parse_pair(&rest[..digits_end], 'x')
}

/// `/dev/fb0` driven through positional writes.
#[derive(Debug)]
pub struct FbdevBackend {
    device: File,
    path: PathBuf,
    info: FbInfo,
    saved: Vec<u8>,
    cursor_blink: Option<(PathBuf, String)>,
    presented: u64,
}

impl FbdevBackend {
    /// Opens the first framebuffer and pauses the console cursor.
    ///
    /// # Errors
    ///
    /// See [`FbdevBackend::open_at`].
    pub fn open() -> Result<Self, RenderError> {
        let mut backend = Self::open_at(Path::new(DEFAULT_SYSFS), Path::new(DEFAULT_DEVICE))?;
        backend.pause_cursor_blink(Path::new(CURSOR_BLINK));
        Ok(backend)
    }

    /// Opens the framebuffer described by `sysfs` at `device`.
    ///
    /// # Errors
    ///
    /// Sysfs failures as in [`FbInfo::read`]; [`RenderErrorKind::Unsupported`] for a
    /// depth other than 32 bits; the classified open error otherwise.
    pub fn open_at(sysfs: &Path, device: &Path) -> Result<Self, RenderError> {
        let info = FbInfo::read(sysfs)?;
        if info.bits_per_pixel != 32 {
            return Err(RenderError::new(
                RenderErrorKind::Unsupported,
                format!("{} bits per pixel; only 32 is driven", info.bits_per_pixel),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(device)
            .map_err(|e| RenderError::from_io(format!("open `{}`", device.display()), e))?;
        let saved = read_all_at(&file, info.stride * info.size.height() as usize)
            .map_err(|e| RenderError::from_io(format!("save `{}`", device.display()), e))?;
        Ok(Self {
            device: file,
            path: device.to_owned(),
            info,
            saved,
            cursor_blink: None,
            presented: 0,
        })
    }

    /// Pauses the fbcon cursor timer through `switch` (best effort; restored on close).
    ///
    /// In `KD_TEXT` the console shares this buffer and its cursor would blink
    /// over the frame every 200 ms; a kernel without the attribute is left alone.
    pub fn pause_cursor_blink(&mut self, switch: &Path) {
        if let Ok(previous) = std::fs::read_to_string(switch)
            && std::fs::write(switch, "0\n").is_ok()
        {
            self.cursor_blink = Some((switch.to_owned(), previous));
        }
    }

    /// The device path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The sysfs facts the backend was opened with.
    #[must_use]
    pub const fn info(&self) -> &FbInfo {
        &self.info
    }

    /// Frames presented so far.
    #[must_use]
    pub const fn presented(&self) -> u64 {
        self.presented
    }
}

impl AsFd for FbdevBackend {
    /// The device descriptor, so a caller may `poll` it beside its own descriptors.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.device.as_fd()
    }
}

impl Backend for FbdevBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Fbdev
    }

    fn size(&self) -> Size {
        self.info.size
    }

    fn format(&self) -> PixelFormat {
        PixelFormat::Xrgb8888
    }

    fn present(&mut self, frame: &Frame) -> Result<(), RenderError> {
        if frame.size() != self.info.size || frame.format() != PixelFormat::Xrgb8888 {
            return Err(RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!(
                    "frame is {} {} but the framebuffer is {} xrgb8888",
                    frame.size(),
                    frame.format(),
                    self.info.size
                ),
            )
            .on(BackendKind::Fbdev));
        }
        for y in 0..self.info.size.height() {
            let Some(row) = frame.row(y) else { break };
            let offset = u64::try_from(y as usize * self.info.stride).unwrap_or(u64::MAX);
            self.device.write_all_at(row, offset).map_err(|e| {
                RenderError::from_io(format!("write row {y}"), e).on(BackendKind::Fbdev)
            })?;
        }
        self.presented += 1;
        Ok(())
    }

    fn close(self) -> Result<(), RenderError> {
        if let Some((switch, previous)) = &self.cursor_blink {
            let _ = std::fs::write(switch, previous);
        }
        if self.saved.is_empty() {
            return Ok(());
        }
        self.device
            .write_all_at(&self.saved, 0)
            .map_err(|e| RenderError::from_io("restore the console", e).on(BackendKind::Fbdev))
    }
}

/// Reads up to `len` bytes from offset 0, stopping early at end of file.
fn read_all_at(file: &File, len: usize) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; len];
    let mut filled = 0;
    while filled < len {
        match file.read_at(
            &mut buffer[filled..],
            u64::try_from(filled).unwrap_or(u64::MAX),
        ) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    buffer.truncate(filled);
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use fairing_theme::Rgb;
    use tempfile::TempDir;

    use super::*;

    fn fake_sysfs(dir: &Path, virtual_size: &str, bpp: &str, stride: &str) {
        std::fs::write(dir.join("virtual_size"), format!("{virtual_size}\n"))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.join("bits_per_pixel"), format!("{bpp}\n"))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.join("stride"), format!("{stride}\n")).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn sysfs_attributes_are_parsed() {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        fake_sysfs(dir.path(), "1920,1080", "32", "7680");
        let info = FbInfo::read(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(info.size.width(), 1920);
        assert_eq!(info.size.height(), 1080);
        assert_eq!((info.bits_per_pixel, info.stride), (32, 7680));
        assert_eq!(info.name, "");

        // The visible size comes from `modes` when present; `name` and `state` are read.
        std::fs::write(dir.path().join("modes"), "U:1280x800p-0\nV:640x480p-60\n")
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("name"), "simpledrmdrmfb\n")
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("state"), "0\n").unwrap_or_else(|e| panic!("{e}"));
        let info = FbInfo::read(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((info.size.width(), info.size.height()), (1280, 800));
        assert_eq!(info.name, "simpledrmdrmfb");
        std::fs::write(dir.path().join("state"), "1\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            FbInfo::read(dir.path()).err().map(|e| e.kind()),
            Some(RenderErrorKind::Unsupported)
        );
        std::fs::remove_file(dir.path().join("state")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_file(dir.path().join("modes")).unwrap_or_else(|e| panic!("{e}"));
        fake_sysfs(dir.path(), "99999,1080", "32", "7680");
        assert_eq!(
            FbInfo::read(dir.path()).err().map(|e| e.kind()),
            Some(RenderErrorKind::Unsupported)
        );

        fake_sysfs(dir.path(), "1920x1080", "32", "7680");
        assert_eq!(
            FbInfo::read(dir.path()).err().map(|e| e.kind()),
            Some(RenderErrorKind::Unsupported)
        );
        fake_sysfs(dir.path(), "1920,1080", "32", "100");
        assert_eq!(
            FbInfo::read(dir.path()).err().map(|e| e.kind()),
            Some(RenderErrorKind::Unsupported)
        );
        let missing = FbInfo::read(&dir.path().join("nope"))
            .err()
            .map(|e| e.kind());
        assert_eq!(missing, Some(RenderErrorKind::NotFound));
    }

    #[test]
    fn presents_rows_at_the_stride_and_restores_on_close() {
        // Verifies: FRN-SRS-002
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let stride = 4 * 4 + 8; // four pixels plus padding
        fake_sysfs(dir.path(), "4,2", "32", &stride.to_string());
        let device = dir.path().join("fb0");
        let original: Vec<u8> = (0..u8::try_from(stride * 2).unwrap_or(u8::MAX)).collect();
        File::create(&device)
            .and_then(|mut f| f.write_all(&original))
            .unwrap_or_else(|e| panic!("{e}"));

        let mut backend =
            FbdevBackend::open_at(dir.path(), &device).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(backend.format(), PixelFormat::Xrgb8888);
        assert_eq!(backend.path(), device);
        let size = Size::new(4, 2).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Xrgb8888).unwrap_or_else(|e| panic!("{e}"));
        let color = Rgb::new(0x11, 0x22, 0x33);
        let paint = frame.paint(color, false);
        let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 2.0).unwrap_or_else(|| panic!("rect"));
        frame
            .pixmap_mut()
            .fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(backend.presented(), 1);

        let written = std::fs::read(&device).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            &written[..4],
            &[0x33, 0x22, 0x11, 0xFF],
            "first pixel, XRGB order"
        );
        assert_eq!(
            &written[stride..stride + 4],
            &[0x33, 0x22, 0x11, 0xFF],
            "second row at stride"
        );
        assert_eq!(
            &written[16..stride],
            &original[16..stride],
            "padding untouched"
        );

        let wrong = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            backend.present(&wrong).err().map(|e| e.kind()),
            Some(RenderErrorKind::InvalidGeometry)
        );

        backend.close().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            std::fs::read(&device).unwrap_or_else(|e| panic!("{e}")),
            original,
            "restored"
        );
    }

    #[test]
    fn mode_lines_parse() {
        assert_eq!(parse_mode_line("U:1920x1080p-0"), Some((1920, 1080)));
        assert_eq!(parse_mode_line("D:1024x768i-60\n"), Some((1024, 768)));
        assert_eq!(parse_mode_line("garbage"), None);
        assert_eq!(parse_mode_line("U:1920"), None);
    }

    #[test]
    fn cursor_blink_is_paused_and_restored() {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        fake_sysfs(dir.path(), "4,1", "32", "16");
        let device = dir.path().join("fb0");
        File::create(&device)
            .and_then(|mut f| f.write_all(&[0; 16]))
            .unwrap_or_else(|e| panic!("{e}"));
        let switch = dir.path().join("cursor_blink");
        std::fs::write(&switch, "1\n").unwrap_or_else(|e| panic!("{e}"));
        let mut backend =
            FbdevBackend::open_at(dir.path(), &device).unwrap_or_else(|e| panic!("{e}"));
        backend.pause_cursor_blink(&switch);
        assert_eq!(std::fs::read_to_string(&switch).unwrap_or_default(), "0\n");
        backend.close().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(std::fs::read_to_string(&switch).unwrap_or_default(), "1\n");
        // A missing switch is simply skipped.
        let mut backend =
            FbdevBackend::open_at(dir.path(), &device).unwrap_or_else(|e| panic!("{e}"));
        backend.pause_cursor_blink(&dir.path().join("absent"));
        backend.close().unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn unsupported_depth_is_refused_before_opening_the_device() {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        fake_sysfs(dir.path(), "4,2", "16", "8");
        let error = FbdevBackend::open_at(dir.path(), &dir.path().join("absent"))
            .err()
            .unwrap_or_else(|| panic!("expected error"));
        assert_eq!(error.kind(), RenderErrorKind::Unsupported);
    }
}
