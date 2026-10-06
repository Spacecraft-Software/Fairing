// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! DRM/KMS output: a dumb buffer per frame, a legacy mode-set, then page flips
//! (FRN-SRS-001).
//!
//! The device is opened read-write; the first opener of a primary node is DRM
//! master, so no privilege dance is needed in the initrd. The connected
//! connector's preferred mode is set on the CRTC its encoder already drives.
//! Two `XRGB8888` dumb buffers alternate: the frame is copied row by row,
//! honouring the driver's pitch, then the buffer is flipped at vblank and the
//! flip-complete event is awaited with a bounded `poll`. `ENODEV` from any
//! ioctl means the device was unplugged under us — the simpledrm → native
//! driver handover — and surfaces as [`RenderErrorKind::Lost`] so the presenter
//! re-acquires (FRN-SRS-006). On close the CRTC is returned to whatever it
//! scanned out before, and the device is released for the next master.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use drm::buffer::{Buffer as _, DrmFourcc};
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::{
    Device as ControlDevice, Event, Mode, ModeTypeFlags, PageFlipFlags, ResourceHandles, connector,
    crtc, framebuffer,
};
use drm::{Device as BasicDevice, DriverCapability};
use rustix::event::{PollFd, PollFlags, Timespec, poll};

use crate::backend::{Backend, BackendKind};
use crate::fault::{RenderError, RenderErrorKind};
use crate::frame::{BYTES_PER_PIXEL, Frame, PixelFormat};
use crate::geometry::Size;

/// Primary nodes tried by [`DrmBackend::open`], in order.
const CARD_RANGE: std::ops::Range<u32> = 0..16;
/// Bits per pixel of the dumb buffers.
const BPP: u32 = 32;
/// Colour depth passed to `ADDFB`; with 32 bpp the kernel infers `XRGB8888`.
const DEPTH: u32 = 24;
/// How long to wait for a flip-complete event before probing the device (FRN-SRS-006).
const FLIP_TIMEOUT: Duration = Duration::from_millis(250);

/// The opened primary node; both DRM traits are default-method only.
#[derive(Debug)]
struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl BasicDevice for Card {}
impl ControlDevice for Card {}

/// One scan-out buffer: the dumb buffer and the framebuffer object over it.
#[derive(Debug, Clone, Copy)]
struct Scanout {
    bo: DumbBuffer,
    fb: framebuffer::Handle,
}

/// What the CRTC showed before we took it, restored on close.
#[derive(Debug, Clone, Copy)]
struct Saved {
    fb: Option<framebuffer::Handle>,
    mode: Option<Mode>,
    position: (u32, u32),
}

/// A connected output and the CRTC that drives it.
#[derive(Debug, Clone, Copy)]
struct Output {
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: Mode,
}

/// DRM/KMS with dumb buffers.
#[derive(Debug)]
pub struct DrmBackend {
    card: Card,
    path: PathBuf,
    output: Output,
    size: Size,
    scanouts: Option<[Scanout; 2]>,
    front: usize,
    saved: Saved,
    presented: u64,
}

impl DrmBackend {
    /// Opens the first primary node with a connected connector.
    ///
    /// # Errors
    ///
    /// The last node's error when every `/dev/dri/cardN` fails, or
    /// [`RenderErrorKind::NotFound`] when none exists.
    pub fn open() -> Result<Self, RenderError> {
        let mut last: Option<RenderError> = None;
        for n in CARD_RANGE {
            let path = PathBuf::from(format!("/dev/dri/card{n}"));
            if !path.exists() {
                continue;
            }
            match Self::open_path(&path) {
                Ok(backend) => return Ok(backend),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| {
            RenderError::new(RenderErrorKind::NotFound, "no `/dev/dri/card*` node exists")
        }))
    }

    /// Opens one primary node and takes its first connected connector.
    ///
    /// # Errors
    ///
    /// Classified from the failing ioctl: [`RenderErrorKind::PermissionDenied`] when
    /// another client is master, [`RenderErrorKind::NotFound`] without a connected
    /// connector, [`RenderErrorKind::Unsupported`] without dumb buffers.
    pub fn open_path(path: &Path) -> Result<Self, RenderError> {
        let shown = path.display().to_string();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| RenderError::from_io(format!("open `{shown}`"), e))?;
        let card = Card(file);
        let dumb = card
            .get_driver_capability(DriverCapability::DumbBuffer)
            .map_err(|e| RenderError::from_io(format!("query `{shown}` capabilities"), e))?;
        if dumb == 0 {
            return Err(RenderError::new(
                RenderErrorKind::Unsupported,
                format!("`{shown}` has no dumb-buffer support"),
            ));
        }
        // The first opener is master already. EBUSY means another client (a compositor,
        // a display manager) holds master: nothing we draw would reach the screen, so
        // say so now rather than failing the mode-set later. EACCES means we were never
        // master and lack CAP_SYS_ADMIN; the mode-set below reports that case.
        if let Err(e) = card.acquire_master_lock()
            && RenderErrorKind::classify(&e) == RenderErrorKind::Busy
        {
            return Err(RenderError::from_io(
                format!("another client is DRM master on `{shown}`"),
                e,
            ));
        }

        let resources = card
            .resource_handles()
            .map_err(|e| RenderError::from_io(format!("enumerate `{shown}`"), e))?;
        let output = find_output(&card, &resources)?.ok_or_else(|| {
            RenderError::new(
                RenderErrorKind::NotFound,
                format!("`{shown}` has no connected connector with a mode"),
            )
        })?;
        let (width, height) = output.mode.size();
        let size = Size::new(u32::from(width), u32::from(height))?;
        let saved = card
            .get_crtc(output.crtc)
            .map(|info| Saved {
                fb: info.framebuffer(),
                mode: info.mode(),
                position: info.position(),
            })
            .map_err(|e| RenderError::from_io("read the current CRTC state", e))?;

        let first = create_scanout(&card, size)?;
        let second = match create_scanout(&card, size) {
            Ok(scanout) => scanout,
            Err(error) => {
                destroy_scanout(&card, first);
                return Err(error);
            }
        };
        Ok(Self {
            card,
            path: path.to_owned(),
            output,
            size,
            scanouts: Some([first, second]),
            front: 0,
            saved,
            presented: 0,
        })
    }

    /// The device path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The mode in use: width, height and refresh rate in Hz.
    #[must_use]
    pub fn mode(&self) -> (u32, u32, u32) {
        let (w, h) = self.output.mode.size();
        (u32::from(w), u32::from(h), self.output.mode.vrefresh())
    }

    /// Frames presented so far.
    #[must_use]
    pub const fn presented(&self) -> u64 {
        self.presented
    }

    /// Copies `frame` into `scanout`'s dumb buffer, honouring the driver's pitch.
    fn upload(&self, scanout: &mut Scanout, frame: &Frame) -> Result<(), RenderError> {
        let pitch = scanout.bo.pitch() as usize;
        let row_bytes = self.size.width() as usize * BYTES_PER_PIXEL;
        let mut map = self
            .card
            .map_dumb_buffer(&mut scanout.bo)
            .map_err(|e| RenderError::from_io("map the dumb buffer", e))?;
        let bytes: &mut [u8] = &mut map;
        for y in 0..self.size.height() {
            let Some(row) = frame.row(y) else { break };
            let start = y as usize * pitch;
            if let Some(dst) = bytes.get_mut(start..start + row_bytes) {
                dst.copy_from_slice(&row[..row_bytes]);
            }
        }
        Ok(())
    }

    /// Blocks until this CRTC's flip completes, bounded by [`FLIP_TIMEOUT`].
    fn wait_flip(&self) -> Result<(), RenderError> {
        let deadline = Instant::now() + FLIP_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Distinguish a slow vblank from a device that vanished (FRN-SRS-006).
                return match self
                    .card
                    .get_driver_capability(DriverCapability::DumbBuffer)
                {
                    Ok(_) => Err(RenderError::new(
                        RenderErrorKind::Timeout,
                        format!("no flip event within {FLIP_TIMEOUT:?}"),
                    )),
                    Err(e) => Err(RenderError::from_io(
                        "probe the device after a flip timeout",
                        e,
                    )),
                };
            }
            let timeout = Timespec {
                tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(remaining.subsec_nanos()),
            };
            let mut fds = [PollFd::new(&self.card, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => continue,
                Ok(_) => {}
                Err(errno) if errno == rustix::io::Errno::INTR => continue,
                Err(errno) => {
                    return Err(RenderError::from_io(
                        "poll the DRM device",
                        io::Error::from(errno),
                    ));
                }
            }
            let events = self
                .card
                .receive_events()
                .map_err(|e| RenderError::from_io("read DRM events", e))?;
            for event in events {
                if let Event::PageFlip(flip) = event
                    && flip.crtc == self.output.crtc
                {
                    return Ok(());
                }
            }
        }
    }
}

impl AsFd for DrmBackend {
    /// The device descriptor, so a caller may `poll` it beside its own descriptors.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.card.as_fd()
    }
}

impl Backend for DrmBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Drm
    }

    fn size(&self) -> Size {
        self.size
    }

    fn format(&self) -> PixelFormat {
        PixelFormat::Xrgb8888
    }

    /// Implements: FRN-SRS-001
    fn present(&mut self, frame: &Frame) -> Result<(), RenderError> {
        if frame.size() != self.size || frame.format() != PixelFormat::Xrgb8888 {
            return Err(RenderError::new(
                RenderErrorKind::InvalidGeometry,
                format!(
                    "frame is {} {} but the mode is {} xrgb8888",
                    frame.size(),
                    frame.format(),
                    self.size
                ),
            )
            .on(BackendKind::Drm));
        }
        let Some(mut scanouts) = self.scanouts else {
            return Err(
                RenderError::new(RenderErrorKind::Lost, "the output was closed")
                    .on(BackendKind::Drm),
            );
        };
        let back = 1 - self.front;
        let result = (|| {
            self.upload(&mut scanouts[back], frame)?;
            if self.presented == 0 {
                self.card
                    .set_crtc(
                        self.output.crtc,
                        Some(scanouts[back].fb),
                        (0, 0),
                        &[self.output.connector],
                        Some(self.output.mode),
                    )
                    .map_err(|e| RenderError::from_io("set the CRTC mode", e))?;
            } else {
                let mut attempts = 0;
                loop {
                    match self.card.page_flip(
                        self.output.crtc,
                        scanouts[back].fb,
                        PageFlipFlags::EVENT,
                        None,
                    ) {
                        Ok(()) => break,
                        Err(e)
                            if RenderErrorKind::classify(&e) == RenderErrorKind::Busy
                                && attempts < 3 =>
                        {
                            // A previous flip is still pending; let it land first.
                            attempts += 1;
                            self.wait_flip()?;
                        }
                        Err(e) => return Err(RenderError::from_io("queue the page flip", e)),
                    }
                }
                self.wait_flip()?;
            }
            Ok(())
        })();
        self.scanouts = Some(scanouts);
        result.map_err(|e| e.on(BackendKind::Drm))?;
        self.front = back;
        self.presented += 1;
        Ok(())
    }

    fn close(mut self) -> Result<(), RenderError> {
        let mut first_error = None;
        if let Some(saved_fb) = self.saved.fb
            && let Err(e) = self.card.set_crtc(
                self.output.crtc,
                Some(saved_fb),
                self.saved.position,
                &[self.output.connector],
                self.saved.mode,
            )
        {
            first_error
                .get_or_insert(RenderError::from_io("restore the CRTC", e).on(BackendKind::Drm));
        }
        if let Some(scanouts) = self.scanouts.take() {
            for scanout in scanouts {
                destroy_scanout(&self.card, scanout);
            }
        }
        let _ = self.card.release_master_lock();
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for DrmBackend {
    fn drop(&mut self) {
        // `close` is the orderly path; this only frees kernel objects if it was skipped.
        if let Some(scanouts) = self.scanouts.take() {
            for scanout in scanouts {
                destroy_scanout(&self.card, scanout);
            }
        }
    }
}

/// The first connected connector with a mode, its preferred mode, and a CRTC for it.
fn find_output(card: &Card, resources: &ResourceHandles) -> Result<Option<Output>, RenderError> {
    for &handle in resources.connectors() {
        // `force_probe = false`: never trigger a slow EDID re-read during boot.
        let info = card
            .get_connector(handle, false)
            .map_err(|e| RenderError::from_io("read a connector", e))?;
        if info.state() != connector::State::Connected {
            continue;
        }
        let Some(mode) = preferred_mode(&info) else {
            continue;
        };
        let Some(crtc) = pick_crtc(card, resources, &info)? else {
            continue;
        };
        return Ok(Some(Output {
            connector: handle,
            crtc,
            mode,
        }));
    }
    Ok(None)
}

/// The mode flagged preferred, else the first one.
fn preferred_mode(info: &connector::Info) -> Option<Mode> {
    info.modes()
        .iter()
        .copied()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| info.modes().first().copied())
}

/// The CRTC the connector's current encoder drives, else the first one any of its encoders may use.
fn pick_crtc(
    card: &Card,
    resources: &ResourceHandles,
    info: &connector::Info,
) -> Result<Option<crtc::Handle>, RenderError> {
    if let Some(encoder) = info.current_encoder() {
        let encoder = card
            .get_encoder(encoder)
            .map_err(|e| RenderError::from_io("read the current encoder", e))?;
        if let Some(crtc) = encoder.crtc() {
            return Ok(Some(crtc));
        }
    }
    for &handle in info.encoders() {
        let encoder = card
            .get_encoder(handle)
            .map_err(|e| RenderError::from_io("read an encoder", e))?;
        if let Some(&crtc) = resources.filter_crtcs(encoder.possible_crtcs()).first() {
            return Ok(Some(crtc));
        }
    }
    Ok(resources.crtcs().first().copied())
}

/// A dumb buffer of `size` with a framebuffer object over it.
fn create_scanout(card: &Card, size: Size) -> Result<Scanout, RenderError> {
    let bo = card
        .create_dumb_buffer((size.width(), size.height()), DrmFourcc::Xrgb8888, BPP)
        .map_err(|e| RenderError::from_io(format!("create a {size} dumb buffer"), e))?;
    match card.add_framebuffer(&bo, DEPTH, BPP) {
        Ok(fb) => Ok(Scanout { bo, fb }),
        Err(e) => {
            let _ = card.destroy_dumb_buffer(bo);
            Err(RenderError::from_io("add a framebuffer", e))
        }
    }
}

/// Frees a scan-out buffer, framebuffer first; errors are ignored on teardown.
fn destroy_scanout(card: &Card, scanout: Scanout) {
    let _ = card.destroy_framebuffer(scanout.fb);
    let _ = card.destroy_dumb_buffer(scanout.bo);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_device_is_not_found() {
        let error = DrmBackend::open_path(Path::new("/dev/dri/card-does-not-exist"))
            .err()
            .unwrap_or_else(|| panic!("expected an error"));
        assert_eq!(error.kind(), RenderErrorKind::NotFound);
        assert!(error.to_string().contains("card-does-not-exist"), "{error}");
    }

    /// Drives a real DRM device; runs only when a VT is available for the test.
    ///
    /// `FAIRING_HW_TESTS=1 cargo test -p fairing-render -- --ignored` on a text console.
    #[test]
    #[ignore = "needs a DRM device and a free VT; run with FAIRING_HW_TESTS=1"]
    fn presents_two_frames_on_real_hardware() {
        // Verifies: FRN-SRS-001
        if std::env::var_os("FAIRING_HW_TESTS").is_none() {
            return;
        }
        let mut backend = DrmBackend::open().unwrap_or_else(|e| panic!("{e}"));
        let size = backend.size();
        let frame = Frame::new(size, PixelFormat::Xrgb8888).unwrap_or_else(|e| panic!("{e}"));
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(backend.presented(), 2);
        backend.close().unwrap_or_else(|e| panic!("{e}"));
    }
}
