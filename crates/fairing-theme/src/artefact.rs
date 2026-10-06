// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The compiled theme artefact: what `fairing theme compile` writes and the
//! splash loads in the initrd (FRN-SRS-042, FRN-SRS-046, FRN-SRS-047).
//!
//! ```text
//! offset  size  field
//! 0       8     magic, b"FRNTHEME"
//! 8       2     format version, little-endian
//! 10      4     metadata length in bytes, little-endian
//! 14      n     metadata, postcard-encoded [`Meta`]
//! 14+n    rest  pixel blob, addressed by [`ImageMeta`] offsets
//! ```
//!
//! Pixels carry no colour. A role-indexed pixel is a §11.1 role and a
//! coverage byte; a mask pixel is coverage alone, drawn in one role. The
//! palette chosen at boot supplies the values, so one artefact renders
//! correctly in its declared palette, that palette's high-contrast sibling,
//! mono, or any palette the operator selects (§11.6), and no literal colour
//! can reach the screen through a theme (FRN-SRS-044).
//!
//! [`Meta::validate`] is the single definition of a well-formed theme: the
//! compiler runs it before writing and the loader runs it after reading, so
//! nothing the compiler accepts can be refused at boot and nothing the loader
//! accepts was outside the contract.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::fault::{ThemeError, ThemeErrorKind};
use crate::role::Role;
use crate::theme::Theme;

/// First eight bytes of every artefact.
pub const MAGIC: [u8; 8] = *b"FRNTHEME";
/// The format this build reads and writes; bumped on any change to [`Meta`].
pub const FORMAT_VERSION: u16 = 1;
/// The largest artefact, assets included (FRN-SRS-047).
pub const MAX_ARTEFACT_BYTES: usize = 4 * 1024 * 1024;
/// Width of the reference frame every layout is authored in (FRN-SRS-007).
pub const REFERENCE_WIDTH: f32 = 1920.0;
/// Height of the reference frame.
pub const REFERENCE_HEIGHT: f32 = 1080.0;
/// The largest image side, in source pixels.
///
/// Twice the reference frame's long side: enough for a crisp logo on an 8K
/// panel, small enough that one image cannot exhaust the 4 MiB budget alone.
pub const MAX_IMAGE_SIDE: u32 = 3840;
/// The most images (logo plus sequence frames) one artefact may carry.
pub const MAX_IMAGES: usize = 512;
/// The fastest frame sequence (FRN-SRS-051).
pub const MAX_SEQUENCE_FPS: u8 = 30;
/// Longest theme name and shutdown message, in characters.
pub const MAX_TEXT_CHARS: usize = 120;
/// Smallest and largest text size, in reference pixels.
///
/// Below 8 px a glyph is unreadable at 1080p; above 200 px one word fills a
/// third of the frame, which no layout element needs.
pub const TEXT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=200.0;
/// Thickest bar outline, in reference pixels.
pub const MAX_OUTLINE: f32 = 16.0;

const HEADER_LEN: usize = 14;

/// A rectangle in reference pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectSpec {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width, greater than zero.
    pub width: f32,
    /// Height, greater than zero.
    pub height: f32,
}

impl RectSpec {
    /// A rectangle at `(x, y)` of `width` × `height`.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn check(&self, what: &str) -> Result<(), ThemeError> {
        let finite = [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite());
        if !finite
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > REFERENCE_WIDTH
            || self.y + self.height > REFERENCE_HEIGHT
        {
            return Err(layout_error(format!(
                "{what} {self} is not inside the {REFERENCE_WIDTH}x{REFERENCE_HEIGHT} reference frame"
            )));
        }
        Ok(())
    }
}

impl fmt::Display for RectSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {} x {})",
            self.x, self.y, self.width, self.height
        )
    }
}

/// Where the logo comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogoImage {
    /// Fairing's built-in vector mark, drawn at any resolution.
    Builtin,
    /// The image at this index of [`Meta::images`].
    Image(u32),
}

/// The logo: a box and what fills it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogoSpec {
    /// The box, in reference pixels; the image is fitted into it uniformly.
    pub rect: RectSpec,
    /// The image.
    pub image: LogoImage,
}

/// The progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BarSpec {
    /// The track.
    pub rect: RectSpec,
    /// Corner radius, at most half the height.
    pub radius: f32,
    /// Outline width; zero for none.
    pub outline: f32,
    /// The filled part.
    pub fill: Role,
    /// The empty part.
    pub track: Role,
    /// The outline.
    pub border: Role,
}

/// The percentage beside the bar (FRN-SRS-053).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PercentSpec {
    /// Gap between the bar's right edge and the text.
    pub gap: f32,
    /// Text size.
    pub size: f32,
    /// Text colour; a foreground-class role.
    pub color: Role,
}

/// The status line (FRN-SRS-050).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StatusSpec {
    /// Whether the line is drawn at all.
    pub show: bool,
    /// Top of the line; the line is centred horizontally.
    pub top: f32,
    /// Text size.
    pub size: f32,
    /// Text colour; a foreground-class role.
    pub color: Role,
}

/// Where the password prompt is drawn (FRN-SRS-021, from M3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PromptSpec {
    /// The prompt region.
    pub rect: RectSpec,
    /// Text size of the message and the field.
    pub size: f32,
}

/// A frame sequence played in a box (FRN-SRS-051, from M4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SequenceSpec {
    /// The box frames are fitted into.
    pub rect: RectSpec,
    /// Frames per second, `1..=30`.
    pub fps: u8,
    /// Index of the first frame in [`Meta::images`].
    pub first: u32,
    /// Number of frames.
    pub count: u32,
}

/// The boot layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootLayout {
    /// The logo.
    pub logo: LogoSpec,
    /// The bar.
    pub bar: BarSpec,
    /// The percentage text.
    pub percent: PercentSpec,
    /// The status line.
    pub status: StatusSpec,
    /// The prompt region.
    pub prompt: PromptSpec,
    /// An optional frame sequence.
    pub sequence: Option<SequenceSpec>,
}

impl BootLayout {
    /// The built-in layout: logo above a centred bar, percentage beside it,
    /// status below, the prompt region under the status line.
    #[must_use]
    pub const fn builtin() -> Self {
        Self {
            logo: LogoSpec {
                rect: RectSpec::new(840.0, 300.0, 240.0, 240.0),
                image: LogoImage::Builtin,
            },
            bar: BarSpec {
                rect: RectSpec::new(640.0, 640.0, 640.0, 14.0),
                radius: 7.0,
                outline: 1.5,
                fill: Role::Accent,
                track: Role::Surface,
                border: Role::Border,
            },
            percent: PercentSpec {
                gap: 28.0,
                size: 30.0,
                color: Role::Foreground,
            },
            status: StatusSpec {
                show: true,
                top: 704.0,
                size: 26.0,
                color: Role::Foreground,
            },
            prompt: PromptSpec {
                rect: RectSpec::new(560.0, 780.0, 800.0, 140.0),
                size: 28.0,
            },
            sequence: None,
        }
    }
}

/// The shutdown layout (FRN-SRS-035, from M4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownLayout {
    /// The logo.
    pub logo: LogoSpec,
    /// The line under the logo; its text is [`ShutdownLayout::message`].
    pub status: StatusSpec,
    /// What the line says.
    pub message: String,
    /// An optional frame sequence.
    pub sequence: Option<SequenceSpec>,
}

impl ShutdownLayout {
    /// The built-in shutdown layout: the logo and one line.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            logo: BootLayout::builtin().logo,
            status: StatusSpec {
                show: true,
                top: 640.0,
                size: 26.0,
                color: Role::Foreground,
            },
            message: "Shutting down".to_owned(),
            sequence: None,
        }
    }
}

/// How an image's pixels are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    /// One coverage byte per pixel, drawn in this role.
    Mask(Role),
    /// Two bytes per pixel: a role index into [`Role::ALL`], then coverage.
    RoleIndexed,
}

impl Encoding {
    /// Bytes per pixel.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Mask(_) => 1,
            Self::RoleIndexed => 2,
        }
    }
}

/// Where one image lives in the blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageMeta {
    /// Width in source pixels.
    pub width: u32,
    /// Height in source pixels.
    pub height: u32,
    /// Pixel storage.
    pub encoding: Encoding,
    /// Byte offset into the blob.
    pub offset: u32,
}

impl ImageMeta {
    /// Byte length of the pixels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pixel_count()
            .saturating_mul(self.encoding.bytes_per_pixel())
    }

    /// Whether the image has no pixels (never true of a valid image).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn pixel_count(&self) -> usize {
        usize::try_from(self.width)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::try_from(self.height).unwrap_or(usize::MAX))
    }
}

/// Everything in the artefact except the pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// The theme's name, for reports.
    pub name: String,
    /// The registered palette slug the theme declares (FRN-SRS-043): the
    /// declared default of the §11.6 resolution (FRN-SRS-045).
    pub palette: String,
    /// The boot layout.
    pub boot: BootLayout,
    /// The shutdown layout.
    pub shutdown: ShutdownLayout,
    /// Every image: the logo, then sequence frames.
    pub images: Vec<ImageMeta>,
}

impl Meta {
    /// The theme Fairing uses when none is given.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            name: "builtin".to_owned(),
            palette: crate::DEFAULT_SLUG.to_owned(),
            boot: BootLayout::builtin(),
            shutdown: ShutdownLayout::builtin(),
            images: Vec::new(),
        }
    }

    /// Checks every rule a theme must satisfy against a blob of `blob_len` bytes.
    ///
    /// # Errors
    ///
    /// [`ThemeErrorKind::InvalidLayout`] naming the first rule broken.
    ///
    /// Implements: FRN-SRS-043, FRN-SRS-046
    pub fn validate(&self, blob_len: usize) -> Result<(), ThemeError> {
        check_text("name", &self.name)?;
        check_text("shutdown message", &self.shutdown.message)?;
        let palette = Theme::find(&self.palette).ok_or_else(|| {
            layout_error(format!(
                "palette `{}` is not a registered colour theme",
                self.palette
            ))
        })?;
        if self.images.len() > MAX_IMAGES {
            return Err(layout_error(format!(
                "{} images; at most {MAX_IMAGES}",
                self.images.len()
            )));
        }
        for (index, image) in self.images.iter().enumerate() {
            check_image(index, image, blob_len)?;
        }
        let boot = &self.boot;
        self.check_logo("boot logo", &boot.logo)?;
        check_bar(&boot.bar, palette)?;
        check_size("percent size", boot.percent.size)?;
        check_text_role("percent", boot.percent.color)?;
        if !(boot.percent.gap.is_finite() && boot.percent.gap >= 0.0) {
            return Err(layout_error("percent gap must be a non-negative number"));
        }
        // Room for "100%" at four ems beside the bar (FRN-SRS-053).
        let percent_right =
            boot.bar.rect.x + boot.bar.rect.width + boot.percent.gap + 4.0 * boot.percent.size;
        if percent_right > REFERENCE_WIDTH {
            return Err(layout_error(
                "the percentage beside the bar runs off the reference frame",
            ));
        }
        check_status("boot status", &boot.status)?;
        boot.prompt.rect.check("prompt region")?;
        check_size("prompt size", boot.prompt.size)?;
        self.check_sequence("boot sequence", boot.sequence.as_ref())?;
        self.check_logo("shutdown logo", &self.shutdown.logo)?;
        check_status("shutdown status", &self.shutdown.status)?;
        self.check_sequence("shutdown sequence", self.shutdown.sequence.as_ref())
    }

    fn check_logo(&self, what: &str, logo: &LogoSpec) -> Result<(), ThemeError> {
        logo.rect.check(what)?;
        if let LogoImage::Image(index) = logo.image
            && self.image(index).is_none()
        {
            return Err(layout_error(format!("{what} names missing image {index}")));
        }
        Ok(())
    }

    fn check_sequence(
        &self,
        what: &str,
        sequence: Option<&SequenceSpec>,
    ) -> Result<(), ThemeError> {
        let Some(sequence) = sequence else {
            return Ok(());
        };
        sequence.rect.check(what)?;
        if !(1..=MAX_SEQUENCE_FPS).contains(&sequence.fps) {
            return Err(layout_error(format!(
                "{what} runs at {} fps; the range is 1..={MAX_SEQUENCE_FPS}",
                sequence.fps
            )));
        }
        if sequence.count == 0 {
            return Err(layout_error(format!("{what} has no frames")));
        }
        let first = self
            .image(sequence.first)
            .ok_or_else(|| layout_error(format!("{what} starts at a missing image")))?;
        for offset in 0..sequence.count {
            let frame = sequence
                .first
                .checked_add(offset)
                .and_then(|index| self.image(index))
                .ok_or_else(|| layout_error(format!("{what} runs past the last image")))?;
            if (frame.width, frame.height) != (first.width, first.height) {
                return Err(layout_error(format!(
                    "{what} frame {offset} is {}x{}, not {}x{} like the first",
                    frame.width, frame.height, first.width, first.height
                )));
            }
        }
        Ok(())
    }

    fn image(&self, index: u32) -> Option<&ImageMeta> {
        self.images.get(usize::try_from(index).ok()?)
    }
}

fn check_text(what: &str, text: &str) -> Result<(), ThemeError> {
    let chars = text.chars().count();
    if chars > MAX_TEXT_CHARS || text.chars().any(char::is_control) {
        return Err(layout_error(format!(
            "{what} must be at most {MAX_TEXT_CHARS} characters with no control characters"
        )));
    }
    Ok(())
}

fn check_size(what: &str, size: f32) -> Result<(), ThemeError> {
    if TEXT_SIZE_RANGE.contains(&size) {
        Ok(())
    } else {
        Err(layout_error(format!(
            "{what} {size} is outside {}..={}",
            TEXT_SIZE_RANGE.start(),
            TEXT_SIZE_RANGE.end()
        )))
    }
}

/// Text is drawn on the canvas; a surface or the canvas itself is never a
/// text colour (Steelbore Standard §11, class rules).
fn check_text_role(what: &str, role: Role) -> Result<(), ThemeError> {
    if role == Role::Background || role.is_surface() {
        return Err(layout_error(format!(
            "{what} text cannot be `{role}`: surfaces and the canvas are never text colours"
        )));
    }
    Ok(())
}

fn check_status(what: &str, status: &StatusSpec) -> Result<(), ThemeError> {
    check_size(&format!("{what} size"), status.size)?;
    check_text_role(what, status.color)?;
    if !(status.top.is_finite()
        && status.top >= 0.0
        && status.top + status.size <= REFERENCE_HEIGHT)
    {
        return Err(layout_error(format!(
            "{what} at {} runs off the reference frame",
            status.top
        )));
    }
    Ok(())
}

/// The minimum contrast between the bar's fill and its track: the WCAG 2.2
/// non-text floor (1.4.11), which the filled part is.
const BAR_CONTRAST: f64 = 3.0;

fn check_bar(bar: &BarSpec, palette: &'static Theme) -> Result<(), ThemeError> {
    bar.rect.check("bar")?;
    if !(bar.radius.is_finite() && (0.0..=bar.rect.height / 2.0).contains(&bar.radius)) {
        return Err(layout_error(
            "bar radius must lie between 0 and half the bar's height",
        ));
    }
    if !(bar.outline.is_finite() && (0.0..=MAX_OUTLINE).contains(&bar.outline)) {
        return Err(layout_error(format!(
            "bar outline must lie between 0 and {MAX_OUTLINE}"
        )));
    }
    // Measured in the declared palette and its high-contrast sibling, the two
    // variants a theme author chose; colours are roles, so every other
    // palette is the operator's choice (§11.6).
    let variants = [
        Some(palette.base_theme()),
        palette.base_theme().high_contrast(),
    ];
    for theme in variants.into_iter().flatten() {
        let ratio = theme.color(bar.fill).contrast_ratio(theme.color(bar.track));
        if ratio < BAR_CONTRAST {
            return Err(layout_error(format!(
                "bar fill `{}` on track `{}` is {ratio:.2}:1 in `{}`; non-text contrast needs {BAR_CONTRAST}:1",
                bar.fill, bar.track, theme.slug
            )));
        }
    }
    Ok(())
}

fn check_image(index: usize, image: &ImageMeta, blob_len: usize) -> Result<(), ThemeError> {
    if image.width == 0
        || image.height == 0
        || image.width > MAX_IMAGE_SIDE
        || image.height > MAX_IMAGE_SIDE
    {
        return Err(layout_error(format!(
            "image {index} is {}x{}; each side must be 1..={MAX_IMAGE_SIDE}",
            image.width, image.height
        )));
    }
    let start = usize::try_from(image.offset).unwrap_or(usize::MAX);
    let end = start.checked_add(image.len());
    if end.is_none_or(|end| end > blob_len) {
        return Err(layout_error(format!(
            "image {index} lies outside the pixel data"
        )));
    }
    Ok(())
}

fn layout_error(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorKind::InvalidLayout, message)
}

fn artefact_error(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorKind::InvalidArtefact, message)
}

/// A loaded, validated theme.
#[derive(Clone, PartialEq)]
pub struct CompiledTheme {
    meta: Meta,
    blob: Vec<u8>,
}

impl CompiledTheme {
    /// A theme from metadata and pixels, validated.
    ///
    /// # Errors
    ///
    /// [`ThemeErrorKind::InvalidLayout`] when the metadata breaks a rule.
    pub fn new(meta: Meta, blob: Vec<u8>) -> Result<Self, ThemeError> {
        meta.validate(blob.len())?;
        Ok(Self { meta, blob })
    }

    /// The built-in theme: built-in layouts, no images.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            meta: Meta::builtin(),
            blob: Vec::new(),
        }
    }

    /// Reads an artefact.
    ///
    /// Every length is checked before it is used, and nothing is allocated
    /// beyond the input's own size, so a hostile file costs at most its bytes.
    ///
    /// # Errors
    ///
    /// [`ThemeErrorKind::InvalidArtefact`] for a wrong magic, an unknown version,
    /// an oversized or truncated file or undecodable metadata;
    /// [`ThemeErrorKind::InvalidLayout`] for metadata that breaks a rule.
    ///
    /// Implements: FRN-SRS-042, FRN-SRS-047
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ThemeError> {
        if bytes.len() > MAX_ARTEFACT_BYTES {
            return Err(artefact_error(format!(
                "{} bytes; a theme is at most {MAX_ARTEFACT_BYTES}",
                bytes.len()
            )));
        }
        let (header, rest) = bytes
            .split_at_checked(HEADER_LEN)
            .ok_or_else(|| artefact_error("shorter than the header"))?;
        if header[..8] != MAGIC {
            return Err(artefact_error("not a Fairing theme (bad magic)"));
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != FORMAT_VERSION {
            return Err(artefact_error(format!(
                "format version {version}; this Fairing reads {FORMAT_VERSION}"
            )));
        }
        let meta_len = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);
        let (meta_bytes, blob) = usize::try_from(meta_len)
            .ok()
            .and_then(|len| rest.split_at_checked(len))
            .ok_or_else(|| artefact_error("metadata runs past the end of the file"))?;
        let meta: Meta = postcard::from_bytes(meta_bytes)
            .map_err(|e| artefact_error(format!("metadata does not decode: {e}")))?;
        Self::new(meta, blob.to_vec())
    }

    /// Serialises the theme into artefact bytes.
    ///
    /// # Errors
    ///
    /// [`ThemeErrorKind::InvalidArtefact`] when the result would exceed
    /// [`MAX_ARTEFACT_BYTES`] or the metadata cannot be encoded.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ThemeError> {
        let meta = postcard::to_extend(&self.meta, Vec::new())
            .map_err(|e| artefact_error(format!("metadata does not encode: {e}")))?;
        let total = HEADER_LEN + meta.len() + self.blob.len();
        if total > MAX_ARTEFACT_BYTES {
            return Err(artefact_error(format!(
                "{total} bytes; a theme is at most {MAX_ARTEFACT_BYTES} (FRN-SRS-047)"
            )));
        }
        let meta_len = u32::try_from(meta.len())
            .map_err(|_e| artefact_error("metadata is larger than 4 GiB"))?;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&meta_len.to_le_bytes());
        bytes.extend_from_slice(&meta);
        bytes.extend_from_slice(&self.blob);
        Ok(bytes)
    }

    /// The metadata.
    #[must_use]
    pub const fn meta(&self) -> &Meta {
        &self.meta
    }

    /// The registered theme the artefact declares (its §11.6 declared default).
    #[must_use]
    pub fn palette(&self) -> &'static Theme {
        Theme::find(&self.meta.palette).unwrap_or_else(Theme::family_default)
    }

    /// The image at `index` with its pixels.
    #[must_use]
    pub fn image(&self, index: u32) -> Option<ImageView<'_>> {
        let meta = *self.meta.image(index)?;
        let start = usize::try_from(meta.offset).ok()?;
        let pixels = self.blob.get(start..start.checked_add(meta.len())?)?;
        Some(ImageView { meta, pixels })
    }

    /// Size of the pixel data in bytes.
    #[must_use]
    pub fn blob_len(&self) -> usize {
        self.blob.len()
    }
}

impl fmt::Debug for CompiledTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledTheme")
            .field("meta", &self.meta)
            .field("blob_len", &self.blob.len())
            .finish()
    }
}

/// An image and its pixels, borrowed from a [`CompiledTheme`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageView<'a> {
    meta: ImageMeta,
    pixels: &'a [u8],
}

impl<'a> ImageView<'a> {
    /// Width in source pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.meta.width
    }

    /// Height in source pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.meta.height
    }

    /// Pixel storage.
    #[must_use]
    pub const fn encoding(&self) -> Encoding {
        self.meta.encoding
    }

    /// The raw pixels, row-major, [`Encoding::bytes_per_pixel`] bytes each.
    #[must_use]
    pub const fn pixels(&self) -> &'a [u8] {
        self.pixels
    }

    /// The role and coverage of the pixel at `(x, y)`, if inside the image.
    ///
    /// A role-indexed pixel whose role byte is out of range reads as
    /// uncovered: the loader cannot reject such bytes without scanning every
    /// pixel, and drawing nothing is the safe reading.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<(Role, u8)> {
        if x >= self.meta.width || y >= self.meta.height {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.meta.width).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?;
        match self.meta.encoding {
            Encoding::Mask(role) => Some((role, *self.pixels.get(index)?)),
            Encoding::RoleIndexed => {
                let at = index.checked_mul(2)?;
                let role = Role::ALL.get(usize::from(*self.pixels.get(at)?));
                let coverage = *self.pixels.get(at + 1)?;
                Some(role.map_or((Role::Background, 0), |role| (*role, coverage)))
            }
        }
    }
}

impl Serialize for Role {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_logo_image() -> CompiledTheme {
        let mut meta = Meta::builtin();
        meta.name = "test".to_owned();
        meta.images.push(ImageMeta {
            width: 2,
            height: 2,
            encoding: Encoding::RoleIndexed,
            offset: 0,
        });
        meta.boot.logo.image = LogoImage::Image(0);
        let blob = vec![4, 255, 3, 128, 0, 0, 99, 255];
        CompiledTheme::new(meta, blob).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn builtin_theme_is_valid_and_round_trips() {
        let theme = CompiledTheme::builtin();
        theme.meta().validate(0).unwrap_or_else(|e| panic!("{e}"));
        let bytes = theme.to_bytes().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&bytes[..8], b"FRNTHEME");
        let back = CompiledTheme::from_bytes(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back, theme);
        assert_eq!(back.palette().slug, crate::DEFAULT_SLUG);
    }

    #[test]
    fn images_round_trip_and_read_by_role() {
        let theme = with_logo_image();
        let bytes = theme.to_bytes().unwrap_or_else(|e| panic!("{e}"));
        let back = CompiledTheme::from_bytes(&bytes).unwrap_or_else(|e| panic!("{e}"));
        let image = back.image(0).unwrap_or_else(|| panic!("image 0"));
        assert_eq!((image.width(), image.height()), (2, 2));
        assert_eq!(image.pixel(0, 0), Some((Role::Accent, 255)));
        assert_eq!(image.pixel(1, 0), Some((Role::Foreground, 128)));
        assert_eq!(
            image.pixel(1, 1),
            Some((Role::Background, 0)),
            "role 99 is uncovered"
        );
        assert_eq!(image.pixel(2, 0), None);
        assert!(back.image(1).is_none());
    }

    #[test]
    fn damaged_artefacts_are_refused_without_panicking() {
        // Verifies: FRN-SRS-047
        let good = with_logo_image()
            .to_bytes()
            .unwrap_or_else(|e| panic!("{e}"));
        let kind = |bytes: &[u8]| CompiledTheme::from_bytes(bytes).err().map(|e| e.kind());
        assert_eq!(kind(&good[..5]), Some(ThemeErrorKind::InvalidArtefact));
        let mut magic = good.clone();
        magic[0] = b'X';
        assert_eq!(kind(&magic), Some(ThemeErrorKind::InvalidArtefact));
        let mut version = good.clone();
        version[8] = 9;
        assert_eq!(kind(&version), Some(ThemeErrorKind::InvalidArtefact));
        let mut long_meta = good.clone();
        long_meta[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(kind(&long_meta), Some(ThemeErrorKind::InvalidArtefact));
        let truncated_blob = &good[..good.len() - 1];
        assert_eq!(kind(truncated_blob), Some(ThemeErrorKind::InvalidLayout));
        let oversized = vec![0_u8; MAX_ARTEFACT_BYTES + 1];
        assert_eq!(kind(&oversized), Some(ThemeErrorKind::InvalidArtefact));
        // A deterministic sweep of single-byte corruptions: refuse or accept, never panic.
        for at in 0..good.len() {
            let mut bytes = good.clone();
            bytes[at] = bytes[at].wrapping_add(0x5B);
            let _ = CompiledTheme::from_bytes(&bytes);
        }
    }

    #[test]
    fn layout_rules_are_enforced() {
        // Verifies: FRN-SRS-046
        let refuse = |edit: fn(&mut Meta)| {
            let mut meta = Meta::builtin();
            edit(&mut meta);
            meta.validate(0).err().map(|e| e.kind())
        };
        let layout = Some(ThemeErrorKind::InvalidLayout);
        assert_eq!(refuse(|m| m.palette = "solarized-dark".to_owned()), layout);
        assert_eq!(refuse(|m| m.palette = "steelbore-mono".to_owned()), layout);
        assert_eq!(refuse(|m| m.boot.bar.rect.x = 1800.0), layout);
        assert_eq!(refuse(|m| m.boot.bar.rect.width = f32::NAN), layout);
        assert_eq!(refuse(|m| m.boot.bar.radius = 30.0), layout);
        assert_eq!(refuse(|m| m.boot.percent.color = Role::Surface), layout);
        assert_eq!(refuse(|m| m.boot.status.color = Role::Background), layout);
        assert_eq!(refuse(|m| m.boot.percent.size = 2.0), layout);
        assert_eq!(
            refuse(|m| m.boot.bar.track = Role::Accent),
            layout,
            "no contrast"
        );
        assert_eq!(refuse(|m| m.boot.logo.image = LogoImage::Image(0)), layout);
        assert_eq!(refuse(|m| m.shutdown.message = "a\nb".to_owned()), layout);
        assert_eq!(
            refuse(|m| {
                m.boot.sequence = Some(SequenceSpec {
                    rect: RectSpec::new(0.0, 0.0, 10.0, 10.0),
                    fps: 60,
                    first: 0,
                    count: 1,
                });
            }),
            layout
        );
        assert_eq!(
            refuse(|m| m.boot.percent.gap = 900.0),
            layout,
            "percent runs off"
        );
        assert_eq!(refuse(|_| {}), None);
    }

    #[test]
    fn roles_serialise_by_name() {
        // postcard writes the name as a length-prefixed string, so reordering
        // `Role` can never silently recolour an artefact.
        let bytes =
            postcard::to_extend(&Role::SurfaceAlt, Vec::new()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&bytes[1..], b"surface-alt");
        let back: Role = postcard::from_bytes(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back, Role::SurfaceAlt);
        assert!(postcard::from_bytes::<Role>(b"\x03xyz").is_err());
    }
}
