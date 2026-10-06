// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Theme compilation: an evaluated theme (the JSON Nickel exports once the
//! contract is applied) becomes a validated [`CompiledTheme`].
//!
//! This is where the rules a contract cannot express are enforced:
//!
//! - the declared palette is a registered colour theme (FRN-SRS-043);
//! - a token reference names a token of that palette that binds a role, and
//!   becomes the role (FRN-SRS-043);
//! - no string anywhere in the theme is a literal colour, and every opaque
//!   pixel of a role-indexed image is a value of the declared palette
//!   (FRN-SRS-044);
//! - the layout passes [`Meta::validate`] and the artefact fits in 4 MiB
//!   (FRN-SRS-046, FRN-SRS-047).
//!
//! Compilation is build-time only (`fairing theme compile`, the NixOS module);
//! nothing here runs in the initrd.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::artefact::{
    BarSpec, BootLayout, CompiledTheme, Encoding, ImageMeta, LogoImage, LogoSpec,
    MAX_ARTEFACT_BYTES, MAX_IMAGE_SIDE, MAX_IMAGES, Meta, PercentSpec, PromptSpec, RectSpec,
    SequenceSpec, ShutdownLayout, StatusSpec,
};
use crate::fault::{ThemeError, ThemeErrorKind};
use crate::role::Role;
use crate::theme::Theme;
use crate::tokens::role_of_token;

/// The contract every theme satisfies, embedded so `fairing` applies the very
/// contract it was built with (FRN-SRS-040).
pub const CONTRACT: &str = include_str!("../../../contracts/fairing-theme.ncl");

/// The contract version this build understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Reads the files a theme refers to (PNG images).
///
/// [`ThemeDir`] reads from the theme file's directory; tests supply their own
/// (M-MOCKABLE-SYSCALLS).
pub trait Assets {
    /// The bytes of `path`, as written in the theme.
    ///
    /// # Errors
    ///
    /// The I/O error, as a [`ThemeErrorKind::InvalidSource`] naming the path.
    fn read(&self, path: &str) -> Result<Vec<u8>, ThemeError>;
}

/// Assets relative to a theme file's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeDir {
    root: PathBuf,
}

impl ThemeDir {
    /// Assets of the theme at `theme_file`, resolved against its directory.
    #[must_use]
    pub fn of(theme_file: &Path) -> Self {
        let root = theme_file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self { root }
    }
}

/// The largest asset file read, before decoding.
///
/// The whole artefact is capped at 4 MiB; a source PNG three times that is
/// already far beyond anything that could fit once quantised.
const MAX_ASSET_BYTES: u64 = 12 * 1024 * 1024;

impl Assets for ThemeDir {
    fn read(&self, path: &str) -> Result<Vec<u8>, ThemeError> {
        use std::io::Read as _;
        let full = self.root.join(path);
        let mut bytes = Vec::new();
        std::fs::File::open(&full)
            .and_then(|file| file.take(MAX_ASSET_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| source_error(format!("cannot read `{}`: {e}", full.display())))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ASSET_BYTES {
            return Err(source_error(format!(
                "`{}` is larger than {MAX_ASSET_BYTES} bytes",
                full.display()
            )));
        }
        Ok(bytes)
    }
}

/// A decoded image: 8-bit RGBA, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8 {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width × height × 4` bytes.
    pub pixels: Vec<u8>,
}

/// Evaluates the Nickel theme at `path` and compiles it (FRN-SRS-040 to FRN-SRS-047).
///
/// Assets are read relative to the theme's directory.
///
/// # Errors
///
/// [`ThemeErrorKind::Contract`] with Nickel's diagnostic when the theme does
/// not evaluate against the contract; otherwise as [`compile_value`] fails.
pub fn compile_file(path: &Path) -> Result<CompiledTheme, ThemeError> {
    let evaluated = crate::nickel::evaluate(path)?;
    compile_value(&evaluated, &ThemeDir::of(path), &crate::png_asset::decode)
}

/// Compiles theme JSON (what `nickel export` prints for a theme with the
/// contract applied) without evaluating Nickel.
///
/// # Errors
///
/// [`ThemeErrorKind::InvalidSource`] for text that is not JSON; otherwise as
/// [`compile_file`].
pub fn compile_json(json: &str, assets: &dyn Assets) -> Result<CompiledTheme, ThemeError> {
    let evaluated: Value =
        serde_json::from_str(json).map_err(|e| source_error(format!("not JSON: {e}")))?;
    compile_value(&evaluated, assets, &crate::png_asset::decode)
}

/// Compiles an evaluated theme into an artefact.
///
/// `evaluated` is the theme after the contract was applied and evaluated, in
/// the JSON data model; `decode` turns an asset's bytes into pixels.
///
/// # Errors
///
/// [`ThemeErrorKind::InvalidSource`] for a shape the contract should have
/// refused or an unreadable asset; [`ThemeErrorKind::LiteralColor`] for a
/// literal colour or an off-palette pixel; token and palette errors from
/// [`role_of_token`]; [`ThemeErrorKind::InvalidLayout`] from
/// [`Meta::validate`]; [`ThemeErrorKind::InvalidArtefact`] when the result
/// exceeds 4 MiB.
///
/// Implements: FRN-SRS-043, FRN-SRS-044, FRN-SRS-046, FRN-SRS-047
pub(crate) fn compile_value(
    evaluated: &Value,
    assets: &dyn Assets,
    decode: &dyn Fn(&[u8]) -> Result<Rgba8, ThemeError>,
) -> Result<CompiledTheme, ThemeError> {
    reject_literal_colours(evaluated, &mut String::from("theme"))?;
    let source: Source = Source::deserialize(evaluated).map_err(|e| {
        source_error(format!(
            "the evaluated theme does not match the contract: {e}"
        ))
    })?;
    if source.schema_version != SCHEMA_VERSION {
        return Err(source_error(format!(
            "schema_version {} is not {SCHEMA_VERSION}",
            source.schema_version
        )));
    }
    let palette = Theme::find(&source.palette)
        .ok_or_else(|| ThemeError::new(ThemeErrorKind::UnknownTheme, source.palette.clone()))?;
    let mut builder = Builder {
        palette,
        assets,
        decode,
        images: Vec::new(),
        blob: Vec::new(),
    };
    let boot = builder.boot(&source.layouts.boot)?;
    let shutdown = builder.shutdown(&source.layouts.shutdown)?;
    let meta = Meta {
        name: source.name,
        palette: source.palette,
        boot,
        shutdown,
        images: builder.images,
    };
    let theme = CompiledTheme::new(meta, builder.blob)?;
    // Encoding enforces the 4 MiB ceiling (FRN-SRS-047).
    theme.to_bytes()?;
    Ok(theme)
}

/// Refuses any string that spells a colour, wherever it is (FRN-SRS-044).
///
/// The contract already types every colour field as a role or a token; this
/// pass also covers fields a future contract might add and catches a hex
/// value smuggled into a token name.
fn reject_literal_colours(value: &Value, path: &mut String) -> Result<(), ThemeError> {
    match value {
        Value::String(text) if is_literal_colour(text) => Err(ThemeError::new(
            ThemeErrorKind::LiteralColor,
            format!("`{path}` is the literal colour `{text}`; name a role or a palette token"),
        )),
        Value::Object(fields) => {
            for (key, field) in fields {
                let len = path.len();
                path.push('.');
                path.push_str(key);
                reject_literal_colours(field, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let len = path.len();
                // Writing to a `String` cannot fail.
                let _ = write!(path, "[{index}]");
                reject_literal_colours(item, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `0xrrggbb`, or a CSS colour function.
fn is_literal_colour(text: &str) -> bool {
    let text = text.trim();
    let hex_digits = |digits: &str, lengths: &[usize]| {
        lengths.contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_hexdigit())
    };
    if let Some(digits) = text.strip_prefix('#') {
        return hex_digits(digits, &[3, 4, 6, 8]);
    }
    if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return hex_digits(digits, &[6, 8]);
    }
    let lower = text.to_ascii_lowercase();
    [
        "rgb(", "rgba(", "hsl(", "hsla(", "hwb(", "lab(", "lch(", "oklab(", "oklch(", "color(",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn source_error(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorKind::InvalidSource, message)
}

/// Accumulates images and their pixels while the layouts are built.
struct Builder<'a> {
    palette: &'static Theme,
    assets: &'a dyn Assets,
    decode: &'a dyn Fn(&[u8]) -> Result<Rgba8, ThemeError>,
    images: Vec<ImageMeta>,
    blob: Vec<u8>,
}

impl Builder<'_> {
    fn boot(&mut self, boot: &BootSource) -> Result<BootLayout, ThemeError> {
        Ok(BootLayout {
            logo: self.logo(&boot.logo)?,
            bar: BarSpec {
                rect: boot.bar.rect(),
                radius: boot.bar.radius,
                outline: boot.bar.outline,
                fill: self.color(&boot.bar.fill)?,
                track: self.color(&boot.bar.track)?,
                border: self.color(&boot.bar.border)?,
            },
            percent: PercentSpec {
                gap: boot.percent.gap,
                size: boot.percent.size,
                color: self.color(&boot.percent.color)?,
            },
            status: self.status(&boot.status)?,
            prompt: PromptSpec {
                rect: boot.prompt.rect(),
                size: boot.prompt.size,
            },
            sequence: boot
                .sequence
                .as_ref()
                .map(|s| self.sequence(s))
                .transpose()?,
        })
    }

    fn shutdown(&mut self, shutdown: &ShutdownSource) -> Result<ShutdownLayout, ThemeError> {
        Ok(ShutdownLayout {
            logo: self.logo(&shutdown.logo)?,
            status: self.status(&shutdown.status)?,
            message: shutdown.message.clone(),
            sequence: shutdown
                .sequence
                .as_ref()
                .map(|s| self.sequence(s))
                .transpose()?,
        })
    }

    fn status(&self, status: &StatusSource) -> Result<StatusSpec, ThemeError> {
        Ok(StatusSpec {
            show: status.show,
            top: status.top,
            size: status.size,
            color: self.color(&status.color)?,
        })
    }

    fn logo(&mut self, logo: &LogoSource) -> Result<LogoSpec, ThemeError> {
        // Every colour is checked against the palette, whatever it applies to
        // (FRN-SRS-043).
        let tint = logo.tint.as_ref().map(|c| self.color(c)).transpose()?;
        let image = match &logo.image {
            ImageSource::Named(name) if name == "builtin" => {
                if tint.is_some() {
                    return Err(source_error(
                        "logo tint applies to a PNG logo; the built-in mark is drawn in its own roles",
                    ));
                }
                LogoImage::Builtin
            }
            ImageSource::Named(name) => {
                return Err(source_error(format!(
                    "logo image `{name}` is neither 'builtin nor {{ file = \"...\" }}"
                )));
            }
            ImageSource::File { file } => LogoImage::Image(self.image(file, tint)?),
        };
        Ok(LogoSpec {
            rect: RectSpec::new(logo.x, logo.y, logo.width, logo.height),
            image,
        })
    }

    fn sequence(&mut self, sequence: &SequenceSource) -> Result<SequenceSpec, ThemeError> {
        // A whole number of frames per second; the range is Meta::validate's to enforce.
        let fps = (1..=u8::MAX)
            .find(|n| (f32::from(*n) - sequence.fps).abs() < f32::EPSILON)
            .ok_or_else(|| {
                source_error(format!(
                    "sequence fps {} is not a whole number of frames per second",
                    sequence.fps
                ))
            })?;
        let tint = sequence.tint.as_ref().map(|c| self.color(c)).transpose()?;
        let first =
            u32::try_from(self.images.len()).map_err(|_e| source_error("too many images"))?;
        for frame in &sequence.frames {
            self.image(frame, tint)?;
        }
        Ok(SequenceSpec {
            rect: RectSpec::new(sequence.x, sequence.y, sequence.width, sequence.height),
            fps,
            first,
            count: u32::try_from(sequence.frames.len())
                .map_err(|_e| source_error("too many frames"))?,
        })
    }

    fn color(&self, color: &ColorSource) -> Result<Role, ThemeError> {
        match color {
            ColorSource::Role(name) => name.replace('_', "-").parse(),
            ColorSource::Token { token } => role_of_token(self.palette, token),
        }
    }

    /// Decodes, quantises and stores one image; returns its index.
    ///
    /// The artefact's limits are checked as images arrive, not at the end: a
    /// theme past them is refused after at most one more decode, so a list of
    /// large frames cannot pile up gigabytes before the 4 MiB check.
    fn image(&mut self, path: &str, tint: Option<Role>) -> Result<u32, ThemeError> {
        if self.images.len() >= MAX_IMAGES {
            return Err(source_error(format!(
                "more than {MAX_IMAGES} images (logo and sequence frames together)"
            )));
        }
        let bytes = self.assets.read(path)?;
        let decoded = (self.decode)(&bytes).map_err(|e| source_error(format!("`{path}`: {e}")))?;
        if decoded.width == 0
            || decoded.height == 0
            || decoded.width > MAX_IMAGE_SIDE
            || decoded.height > MAX_IMAGE_SIDE
        {
            return Err(source_error(format!(
                "`{path}` is {}x{}; each side must be 1..={MAX_IMAGE_SIDE}",
                decoded.width, decoded.height
            )));
        }
        let encoding = tint.map_or(Encoding::RoleIndexed, Encoding::Mask);
        let needed = usize::try_from(u64::from(decoded.width) * u64::from(decoded.height))
            .unwrap_or(usize::MAX)
            .saturating_mul(encoding.bytes_per_pixel());
        if self.blob.len().saturating_add(needed) > MAX_ARTEFACT_BYTES {
            return Err(ThemeError::new(
                ThemeErrorKind::InvalidArtefact,
                format!(
                    "`{path}` takes the images past {MAX_ARTEFACT_BYTES} bytes; a theme is at most 4 MiB (FRN-SRS-047)"
                ),
            ));
        }
        let offset =
            u32::try_from(self.blob.len()).map_err(|_e| source_error("the images exceed 4 GiB"))?;
        quantise(&decoded, encoding, self.palette, path, &mut self.blob)?;
        let index =
            u32::try_from(self.images.len()).map_err(|_e| source_error("too many images"))?;
        self.images.push(ImageMeta {
            width: decoded.width,
            height: decoded.height,
            encoding,
            offset,
        });
        Ok(index)
    }
}

/// Appends `image`'s pixels to `blob` in `encoding`.
///
/// A mask keeps only coverage. A role-indexed image maps every pixel with any
/// coverage to the role whose value it equals exactly in the declared
/// palette; anything else is a literal colour (FRN-SRS-044).
fn quantise(
    image: &Rgba8,
    encoding: Encoding,
    palette: &'static Theme,
    path: &str,
    blob: &mut Vec<u8>,
) -> Result<(), ThemeError> {
    let base = palette.base_theme();
    let width = usize::try_from(image.width).unwrap_or(usize::MAX);
    for (index, pixel) in image.pixels.chunks_exact(4).enumerate() {
        let coverage = pixel[3];
        match encoding {
            Encoding::Mask(_) => blob.push(coverage),
            Encoding::RoleIndexed if coverage == 0 => blob.extend_from_slice(&[0, 0]),
            Encoding::RoleIndexed => {
                let value = crate::rgb::Rgb::new(pixel[0], pixel[1], pixel[2]);
                let role = Role::ALL
                    .iter()
                    .position(|role| base.color(*role) == value)
                    .and_then(|i| u8::try_from(i).ok())
                    .ok_or_else(|| {
                        ThemeError::new(
                            ThemeErrorKind::LiteralColor,
                            format!(
                                "`{path}` pixel ({}, {}) is {value}, not a role value of `{}`; \
                                 recolour it or give the image a `tint`",
                                index % width,
                                index / width,
                                base.slug
                            ),
                        )
                    })?;
                blob.extend_from_slice(&[role, coverage]);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    schema_version: u32,
    name: String,
    palette: String,
    layouts: Layouts,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layouts {
    boot: BootSource,
    shutdown: ShutdownSource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootSource {
    logo: LogoSource,
    bar: BarSource,
    percent: PercentSource,
    status: StatusSource,
    prompt: PromptSource,
    #[serde(default)]
    sequence: Option<SequenceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShutdownSource {
    logo: LogoSource,
    status: StatusSource,
    message: String,
    #[serde(default)]
    sequence: Option<SequenceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LogoSource {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    image: ImageSource,
    #[serde(default)]
    tint: Option<ColorSource>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ImageSource {
    Named(String),
    File { file: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ColorSource {
    Role(String),
    Token { token: String },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BarSource {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    outline: f32,
    fill: ColorSource,
    track: ColorSource,
    border: ColorSource,
}

impl BarSource {
    const fn rect(&self) -> RectSpec {
        RectSpec::new(self.x, self.y, self.width, self.height)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PercentSource {
    gap: f32,
    size: f32,
    color: ColorSource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusSource {
    #[serde(default = "shown")]
    show: bool,
    top: f32,
    size: f32,
    color: ColorSource,
}

const fn shown() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptSource {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    size: f32,
}

impl PromptSource {
    const fn rect(&self) -> RectSpec {
        RectSpec::new(self.x, self.y, self.width, self.height)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SequenceSource {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fps: f32,
    frames: Vec<String>,
    #[serde(default)]
    tint: Option<ColorSource>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

    /// The reference theme as `nickel export` prints it once the contract is applied.
    fn reference() -> Value {
        json!({
            "schema_version": 1,
            "name": "steelbore",
            "palette": "steelbore",
            "layouts": {
                "boot": {
                    "logo": { "x": 840, "y": 300, "width": 240, "height": 240, "image": "builtin" },
                    "bar": { "x": 640, "y": 640, "width": 640, "height": 14, "radius": 7,
                             "outline": 1.5, "fill": "accent", "track": "surface", "border": "border" },
                    "percent": { "gap": 28, "size": 30, "color": "foreground" },
                    "status": { "show": true, "top": 704, "size": 26, "color": "foreground" },
                    "prompt": { "x": 560, "y": 780, "width": 800, "height": 140, "size": 28 }
                },
                "shutdown": {
                    "logo": { "x": 840, "y": 300, "width": 240, "height": 240, "image": "builtin" },
                    "status": { "show": true, "top": 640, "size": 26, "color": "foreground" },
                    "message": "Shutting down"
                }
            }
        })
    }

    /// In-memory assets: each name maps to a decoded image (the "bytes" are its key).
    struct Fake(HashMap<&'static str, Rgba8>);

    impl Assets for Fake {
        fn read(&self, path: &str) -> Result<Vec<u8>, ThemeError> {
            self.0
                .contains_key(path)
                .then(|| path.as_bytes().to_vec())
                .ok_or_else(|| source_error(format!("no asset `{path}`")))
        }
    }

    fn compile_with(theme: &Value, assets: &Fake) -> Result<CompiledTheme, ThemeError> {
        let decode = |bytes: &[u8]| {
            let key = std::str::from_utf8(bytes).unwrap_or_default();
            assets
                .0
                .get(key)
                .cloned()
                .ok_or_else(|| source_error("undecodable"))
        };
        compile_value(theme, assets, &decode)
    }

    fn plain(theme: &Value) -> Result<CompiledTheme, ThemeError> {
        compile_with(theme, &Fake(HashMap::new()))
    }

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Rgba8 {
        let count = usize::try_from(width * height).unwrap_or(0);
        Rgba8 {
            width,
            height,
            pixels: rgba.repeat(count),
        }
    }

    fn kind(result: Result<CompiledTheme, ThemeError>) -> Option<ThemeErrorKind> {
        result.err().map(|e| e.kind())
    }

    #[test]
    fn reference_theme_compiles_to_the_builtin_layouts() {
        let theme = plain(&reference()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(theme.meta().name, "steelbore");
        assert_eq!(theme.meta().boot, BootLayout::builtin());
        assert_eq!(theme.meta().shutdown, ShutdownLayout::builtin());
        assert!(theme.meta().images.is_empty());
    }

    #[test]
    fn tokens_of_the_declared_palette_become_roles_and_others_are_refused() {
        // Verifies: FRN-SRS-043
        let mut own = reference();
        own["layouts"]["boot"]["bar"]["fill"] = json!({ "token": "Plasma Orange" });
        let theme = plain(&own).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(theme.meta().boot.bar.fill, Role::Accent);

        let mut foreign = reference();
        foreign["layouts"]["boot"]["bar"]["fill"] = json!({ "token": "Electric Blue" });
        assert_eq!(kind(plain(&foreign)), Some(ThemeErrorKind::ForeignToken));

        let mut unregistered = reference();
        unregistered["palette"] = json!("solarized-dark");
        assert_eq!(
            kind(plain(&unregistered)),
            Some(ThemeErrorKind::UnknownTheme)
        );
    }

    #[test]
    fn literal_colours_are_refused_wherever_they_appear() {
        // Verifies: FRN-SRS-044
        for (pointer, value) in [
            ("/layouts/boot/bar/fill", json!("#FF5E00")),
            ("/name", json!("#ff5e00")),
            (
                "/layouts/boot/percent/color",
                json!({ "token": "rgb(255, 94, 0)" }),
            ),
            ("/layouts/shutdown/message", json!("0xFF5E00")),
        ] {
            let mut theme = reference();
            if let Some(slot) = theme.pointer_mut(pointer) {
                *slot = value;
            }
            let error = plain(&theme)
                .err()
                .unwrap_or_else(|| panic!("{pointer} accepted"));
            assert_eq!(error.kind(), ThemeErrorKind::LiteralColor, "{pointer}");
        }
        assert!(!is_literal_colour("Shutting down"));
        assert!(!is_literal_colour("#hashtag"));
        assert!(is_literal_colour(" #abc "));
    }

    #[test]
    fn images_are_quantised_to_roles_or_kept_as_masks() {
        // Verifies: FRN-SRS-044
        let modern = Theme::family_default();
        let accent = modern.color(Role::Accent);
        let mut assets = HashMap::new();
        assets.insert("logo.png", solid(3, 2, [accent.r, accent.g, accent.b, 200]));
        assets.insert("off.png", solid(2, 2, [1, 2, 3, 255]));
        assets.insert("clear.png", solid(2, 2, [1, 2, 3, 0]));
        let assets = Fake(assets);

        let mut indexed = reference();
        indexed["layouts"]["boot"]["logo"]["image"] = json!({ "file": "logo.png" });
        let theme = compile_with(&indexed, &assets).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(theme.meta().boot.logo.image, LogoImage::Image(0));
        let image = theme.image(0).unwrap_or_else(|| panic!("image"));
        assert_eq!(image.encoding(), Encoding::RoleIndexed);
        assert_eq!(image.pixel(2, 1), Some((Role::Accent, 200)));

        let mut off = reference();
        off["layouts"]["boot"]["logo"]["image"] = json!({ "file": "off.png" });
        let error = compile_with(&off, &assets)
            .err()
            .unwrap_or_else(|| panic!("off-palette"));
        assert_eq!(error.kind(), ThemeErrorKind::LiteralColor);
        assert!(
            error.to_string().contains("pixel (0, 0) is #010203"),
            "{error}"
        );

        // Fully transparent pixels carry no colour, so any value is fine.
        let mut clear = reference();
        clear["layouts"]["boot"]["logo"]["image"] = json!({ "file": "clear.png" });
        assert!(compile_with(&clear, &assets).is_ok());

        let mut masked = off;
        masked["layouts"]["boot"]["logo"]["tint"] = json!("structure");
        let theme = compile_with(&masked, &assets).unwrap_or_else(|e| panic!("{e}"));
        let image = theme.image(0).unwrap_or_else(|| panic!("image"));
        assert_eq!(image.encoding(), Encoding::Mask(Role::Structure));
        assert_eq!(image.pixel(1, 1), Some((Role::Structure, 255)));
    }

    #[test]
    fn sequences_are_stored_after_the_logo_and_their_rate_is_checked() {
        let mut assets = HashMap::new();
        assets.insert("a.png", solid(4, 4, [0, 0, 0, 0]));
        assets.insert("b.png", solid(4, 4, [0, 0, 0, 0]));
        assets.insert("c.png", solid(5, 4, [0, 0, 0, 0]));
        let assets = Fake(assets);
        let sequence = |frames: Value, fps: f32| {
            let mut theme = reference();
            theme["layouts"]["boot"]["sequence"] = json!({
                "x": 0, "y": 0, "width": 100, "height": 100,
                "fps": fps, "frames": frames, "tint": "accent"
            });
            theme
        };
        let theme = compile_with(&sequence(json!(["a.png", "b.png"]), 24.0), &assets)
            .unwrap_or_else(|e| panic!("{e}"));
        let spec = theme
            .meta()
            .boot
            .sequence
            .unwrap_or_else(|| panic!("sequence"));
        assert_eq!((spec.first, spec.count, spec.fps), (0, 2, 24));
        assert_eq!(
            kind(compile_with(&sequence(json!(["a.png"]), 60.0), &assets)),
            Some(ThemeErrorKind::InvalidLayout),
            "faster than 30 fps"
        );
        assert_eq!(
            kind(compile_with(&sequence(json!(["a.png"]), 2.5), &assets)),
            Some(ThemeErrorKind::InvalidSource)
        );
        assert_eq!(
            kind(compile_with(
                &sequence(json!(["a.png", "c.png"]), 10.0),
                &assets
            )),
            Some(ThemeErrorKind::InvalidLayout),
            "frames of different sizes"
        );
    }

    #[test]
    fn artefacts_over_four_mebibytes_are_refused() {
        // Verifies: FRN-SRS-047
        let mut assets = HashMap::new();
        // 1500 x 1500 masks are 2.25 MB each; two exceed the ceiling.
        assets.insert("big.png", solid(1500, 1500, [0, 0, 0, 255]));
        let assets = Fake(assets);
        let mut theme = reference();
        theme["layouts"]["boot"]["sequence"] = json!({
            "x": 0, "y": 0, "width": 100, "height": 100,
            "fps": 10, "frames": ["big.png", "big.png"], "tint": "accent"
        });
        let error = compile_with(&theme, &assets)
            .err()
            .unwrap_or_else(|| panic!("too big"));
        assert_eq!(error.kind(), ThemeErrorKind::InvalidArtefact);
        assert!(
            error.to_string().contains(&MAX_ARTEFACT_BYTES.to_string()),
            "{error}"
        );
    }

    #[test]
    fn limits_stop_a_long_sequence_before_its_frames_pile_up() {
        // Twenty 1500 x 1500 masks would be 45 MB of pixels; the third frame
        // is never decoded, because the second already passes 4 MiB.
        let frame = solid(1500, 1500, [0, 0, 0, 255]);
        let decodes = std::cell::Cell::new(0_u32);
        let decode = |_bytes: &[u8]| {
            decodes.set(decodes.get() + 1);
            Ok(frame.clone())
        };
        let assets = Fake(HashMap::from([("big.png", frame.clone())]));
        let mut theme = reference();
        theme["layouts"]["boot"]["sequence"] = json!({
            "x": 0, "y": 0, "width": 100, "height": 100,
            "fps": 10, "frames": vec!["big.png"; 20], "tint": "accent"
        });
        let error = compile_value(&theme, &assets, &decode)
            .err()
            .unwrap_or_else(|| panic!("too big"));
        assert_eq!(error.kind(), ThemeErrorKind::InvalidArtefact);
        assert_eq!(decodes.get(), 2, "decoded past the limit");
    }

    #[test]
    fn a_builtin_logo_takes_no_tint_and_every_tint_is_checked() {
        // Verifies: FRN-SRS-043
        let mut foreign = reference();
        foreign["layouts"]["boot"]["logo"]["tint"] = json!({ "token": "Electric Blue" });
        assert_eq!(kind(plain(&foreign)), Some(ThemeErrorKind::ForeignToken));
        let mut own = reference();
        own["layouts"]["boot"]["logo"]["tint"] = json!("accent");
        let error = plain(&own)
            .err()
            .unwrap_or_else(|| panic!("tinted builtin"));
        assert!(error.to_string().contains("built-in mark"), "{error}");
    }

    #[test]
    fn shapes_the_contract_forbids_are_still_refused() {
        let mut extra = reference();
        extra["layouts"]["boot"]["glow"] = json!(true);
        assert_eq!(kind(plain(&extra)), Some(ThemeErrorKind::InvalidSource));
        let mut version = reference();
        version["schema_version"] = json!(2);
        assert_eq!(kind(plain(&version)), Some(ThemeErrorKind::InvalidSource));
        let mut surface_text = reference();
        surface_text["layouts"]["boot"]["status"]["color"] = json!("surface_alt");
        assert_eq!(
            kind(plain(&surface_text)),
            Some(ThemeErrorKind::InvalidLayout)
        );
        let mut unknown_role = reference();
        unknown_role["layouts"]["boot"]["bar"]["fill"] = json!("glow");
        assert_eq!(
            kind(plain(&unknown_role)),
            Some(ThemeErrorKind::UnknownRole)
        );
    }
}
