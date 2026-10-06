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
use crate::geometry::{Size, to_usize};

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

impl Card {
    /// Opens a primary node read-write (the ioctls and the mapping need both).
    fn open(path: &Path) -> io::Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(Card)
    }
}

/// One scan-out buffer: the dumb buffer and the framebuffer object over it.
#[derive(Debug, Clone, Copy)]
struct Scanout {
    bo: DumbBuffer,
    fb: framebuffer::Handle,
}

/// What the CRTC showed before we took it, restored on close.
///
/// `fb` is `None` for a CRTC that was inactive; restoring that state means
/// disabling the CRTC again, not leaving our last buffer attached.
#[derive(Debug, Clone, Copy)]
struct Saved {
    fb: Option<framebuffer::Handle>,
    mode: Option<Mode>,
    position: (u32, u32),
    /// Set once the CRTC has been restored (or was never touched).
    restored: bool,
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
    /// The most specific error across the nodes tried (a held master or a refused
    /// open outranks a connector-less card), or [`RenderErrorKind::NotFound`] when
    /// no node exists.
    pub fn open() -> Result<Self, RenderError> {
        let mut worst: Option<RenderError> = None;
        for n in CARD_RANGE {
            let path = PathBuf::from(format!("/dev/dri/card{n}"));
            if !path.exists() {
                continue;
            }
            match Self::open_path(&path) {
                Ok(backend) => return Ok(backend),
                Err(error) => {
                    if worst
                        .as_ref()
                        .is_none_or(|w| specificity(error.kind()) > specificity(w.kind()))
                    {
                        worst = Some(error);
                    }
                }
            }
        }
        Err(worst.unwrap_or_else(|| {
            RenderError::new(RenderErrorKind::NotFound, "no `/dev/dri/card*` node exists")
        }))
    }

    /// Opens one primary node and takes its first connected connector.
    ///
    /// # Errors
    ///
    /// Classified from the failing ioctl: [`RenderErrorKind::Busy`] when another
    /// client holds the DRM master, [`RenderErrorKind::PermissionDenied`] when the
    /// process may not become master, [`RenderErrorKind::NotFound`] without a
    /// connected connector, [`RenderErrorKind::Unsupported`] without dumb buffers.
    pub fn open_path(path: &Path) -> Result<Self, RenderError> {
        let shown = path.display().to_string();
        let card =
            Card::open(path).map_err(|e| RenderError::from_io(format!("open `{shown}`"), e))?;
        let dumb = card
            .get_driver_capability(DriverCapability::DumbBuffer)
            .map_err(|e| RenderError::from_io(format!("query `{shown}` capabilities"), e))?;
        if dumb == 0 {
            return Err(RenderError::new(
                RenderErrorKind::Unsupported,
                format!("`{shown}` has no dumb-buffer support"),
            ));
        }
        // The first opener of a primary node is master already; SET_MASTER then
        // succeeds trivially. Any failure means our frames would never reach the
        // screen, so it is fatal for this node and lets the chain move on: EBUSY,
        // another client (a compositor, a display manager) holds master; EACCES, the
        // process was never master and lacks CAP_SYS_ADMIN. The mode-set and the
        // page flip would otherwise fail later with a far less useful EACCES.
        if let Err(e) = card.acquire_master_lock() {
            let detail = match RenderErrorKind::classify(&e) {
                RenderErrorKind::Busy => format!("another client is DRM master on `{shown}`"),
                RenderErrorKind::PermissionDenied => {
                    format!("cannot become DRM master on `{shown}` (not master, no CAP_SYS_ADMIN)")
                }
                _ => format!("set DRM master on `{shown}`"),
            };
            return Err(RenderError::from_io(detail, e));
        }

        let resources = card
            .resource_handles()
            .map_err(|e| RenderError::from_io(format!("enumerate `{shown}`"), e))?;
        // A cheap pass first; if it finds nothing, a forced probe re-reads EDID (slow,
        // may flicker), which is what an unprobed connector needs right after the
        // device appeared.
        let output = match find_output(&card, &resources, false)? {
            Some(output) => output,
            None => find_output(&card, &resources, true)?.ok_or_else(|| {
                RenderError::new(
                    RenderErrorKind::NotFound,
                    format!("`{shown}` has no connected connector with a mode"),
                )
            })?,
        };
        let (width, height) = output.mode.size();
        let size = Size::new(u32::from(width), u32::from(height))?;
        let saved = card
            .get_crtc(output.crtc)
            .map(|info| Saved {
                fb: info.framebuffer(),
                mode: info.mode(),
                position: info.position(),
                restored: false,
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
    ///
    /// The buffer is mapped per frame: `DumbMapping` borrows the `DumbBuffer`
    /// mutably, so a mapping kept for the backend's lifetime would need a
    /// self-referential owner. The two extra syscalls and the first-touch faults
    /// cost well under a millisecond at 1080p on the sandbox; the Steelbore §3.2
    /// measurement on the reference machine (TODO T-029) decides whether a kept
    /// mapping is worth the restructuring.
    fn upload(&self, scanout: &mut Scanout, frame: &Frame) -> Result<(), RenderError> {
        let pitch = to_usize(scanout.bo.pitch());
        let row_bytes = to_usize(self.size.width()) * BYTES_PER_PIXEL;
        let mut map = self
            .card
            .map_dumb_buffer(&mut scanout.bo)
            .map_err(|e| RenderError::from_io("map the dumb buffer", e))?;
        let bytes: &mut [u8] = &mut map;
        for y in 0..self.size.height() {
            let Some(row) = frame.row(y) else { break };
            let start = to_usize(y) * pitch;
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
        let restored = self.restore_crtc();
        self.release();
        restored.map_err(|e| RenderError::from_io("restore the CRTC", e).on(BackendKind::Drm))
    }
}

impl DrmBackend {
    /// Puts the CRTC back as it was before us, once: on its previous framebuffer,
    /// or disabled again if it was scanning nothing. A CRTC we never mode-set is
    /// left alone.
    fn restore_crtc(&mut self) -> io::Result<()> {
        if self.saved.restored {
            return Ok(());
        }
        self.saved.restored = true;
        if self.presented == 0 {
            return Ok(());
        }
        match self.saved.fb {
            Some(saved_fb) => self.card.set_crtc(
                self.output.crtc,
                Some(saved_fb),
                self.saved.position,
                &[self.output.connector],
                self.saved.mode,
            ),
            None => self
                .card
                .set_crtc(self.output.crtc, None, (0, 0), &[], None),
        }
    }

    /// Frees the scan-out buffers and drops master, once.
    fn release(&mut self) {
        if let Some(scanouts) = self.scanouts.take() {
            for scanout in scanouts {
                destroy_scanout(&self.card, scanout);
            }
            let _ = self.card.release_master_lock();
        }
    }
}

impl Drop for DrmBackend {
    /// `close` is the orderly path; on an error path this still restores the
    /// console and frees the kernel objects, best effort.
    fn drop(&mut self) {
        let _ = self.restore_crtc();
        self.release();
    }
}

/// How much a failure tells the operator: a held master or a refused open outranks
/// "no connector here", which every headless render node reports.
const fn specificity(kind: RenderErrorKind) -> u8 {
    match kind {
        RenderErrorKind::Busy | RenderErrorKind::PermissionDenied => 3,
        RenderErrorKind::Unsupported | RenderErrorKind::Lost | RenderErrorKind::Io => 2,
        RenderErrorKind::NotFound => 1,
        _ => 0,
    }
}

/// The first connected connector with a mode, its preferred mode, and a CRTC for it.
///
/// With `force_probe` false the kernel answers from the connector's last probe
/// (the fast path at boot); with it true the connector is re-probed, which can
/// take hundreds of milliseconds and flicker, so it is only used when the cheap
/// pass found nothing.
fn find_output(
    card: &Card,
    resources: &ResourceHandles,
    force_probe: bool,
) -> Result<Option<Output>, RenderError> {
    for &handle in resources.connectors() {
        let info = card
            .get_connector(handle, force_probe)
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

/// The CRTC the connector's current encoder drives, else the first one any of its
/// encoders may use; `None` when no encoder of this connector can reach a CRTC, so
/// the connector is skipped instead of failing the mode-set with `EINVAL`.
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
    Ok(None)
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

    /// Drives a real DRM device at the connector's preferred mode; a hardware smoke
    /// run, not CI evidence. `cargo test -p fairing-render -- --ignored` on a free
    /// text console; it is red without a device, as it should be.
    #[test]
    #[ignore = "needs a DRM device and a free text console"]
    fn presents_two_frames_on_real_hardware_at_the_preferred_mode() {
        let mut backend = DrmBackend::open().unwrap_or_else(|e| panic!("{e}"));
        // Read the preferred mode independently of the backend's own choice.
        let card = Card::open(backend.path()).unwrap_or_else(|e| panic!("{e}"));
        let resources = card.resource_handles().unwrap_or_else(|e| panic!("{e}"));
        let (w, h) = resources
            .connectors()
            .iter()
            .filter_map(|&h| card.get_connector(h, false).ok())
            .find(|info| info.state() == connector::State::Connected)
            .and_then(|info| preferred_mode(&info))
            .map_or_else(|| panic!("no connected connector"), |mode| mode.size());
        assert_eq!(
            (backend.size().width(), backend.size().height()),
            (u32::from(w), u32::from(h))
        );
        let frame =
            Frame::new(backend.size(), PixelFormat::Xrgb8888).unwrap_or_else(|e| panic!("{e}"));
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        backend.present(&frame).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(backend.presented(), 2);
        backend.close().unwrap_or_else(|e| panic!("{e}"));
    }
}
