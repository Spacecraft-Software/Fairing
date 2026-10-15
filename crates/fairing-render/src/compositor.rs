// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Draws a [`Scene`] into a [`Frame`]: canvas, logo, bar, percentage, status
//! and, while a question is open, the password prompt.

use std::fmt;
use std::time::Duration;

use fairing_theme::{CompiledTheme, LogoImage, Role, Selection, SequenceSpec};

use crate::fault::RenderError;
use crate::frame::Frame;
use crate::geometry::{Rect, Viewport, as_f32, round_u32};
use crate::layout::{Layout, REFERENCE_SIZE};
use crate::logo;
use crate::palette::Palette;
use crate::scene::{Field, OverlayView, PromptView, Scene};
use crate::shapes;
use crate::sprite::Sprite;
use crate::text::{MIN_PX, TextRenderer};

/// Marks a status line that had to be shortened to fit.
const ELLIPSIS: &str = "...";

/// The journal overlay's panel, in reference pixels: the frame less a margin.
const OVERLAY_PANEL: Rect = Rect::new(80.0, 60.0, 1760.0, 960.0);

/// Text size of the overlay, in reference pixels.
const OVERLAY_TEXT: f32 = 22.0;

/// The overlay's heading.
const OVERLAY_TITLE: &str = "Journal - Esc closes; Up/Down, PgUp/PgDn, j/k scroll";

/// Renders scenes for one theme selection and layout.
pub struct Compositor {
    palette: Palette,
    layout: Layout,
    text: TextRenderer,
    /// The theme's logo image; `None` draws the built-in vector mark.
    logo: Option<Sprite>,
    /// The theme's frame sequence, if it declares one.
    sequence: Option<Sequence>,
    /// The last frame drew an echoed answer.
    echoed: bool,
}

/// A frame sequence with its frames copied out of the theme (FRN-SRS-051).
#[derive(Debug)]
struct Sequence {
    /// The box frames are fitted into, in reference pixels.
    rect: Rect,
    /// Frames per second, `1..=30`.
    fps: u8,
    frames: Vec<Sprite>,
}

impl Sequence {
    /// The frames `spec` names in `theme`; `None` if any is missing (the
    /// artefact's validation makes that impossible).
    fn new(theme: &CompiledTheme, spec: &SequenceSpec) -> Option<Self> {
        let frames = (0..spec.count)
            .map(|offset| {
                spec.first
                    .checked_add(offset)
                    .and_then(|index| theme.image(index))
                    .map(Sprite::new)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            rect: Rect::new(spec.rect.x, spec.rect.y, spec.rect.width, spec.rect.height),
            fps: spec.fps,
            frames,
        })
    }
}

/// The frame of a looping sequence of `count` frames at `fps` shown `time`
/// into it: frame `n` from `n / fps` seconds, back to the first after the
/// last (FRN-SRS-051).
///
/// Implements: FRN-SRS-051
#[must_use]
pub fn sequence_frame(time: Duration, fps: u8, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let shown = time.as_millis() * u128::from(fps) / 1000;
    let count = u128::try_from(count).unwrap_or(u128::MAX);
    usize::try_from(shown % count).unwrap_or(0)
}

impl Compositor {
    /// A compositor for `selection` with the built-in layout.
    ///
    /// # Errors
    ///
    /// Propagates a bundled-font parse failure.
    pub fn new(selection: Selection) -> Result<Self, RenderError> {
        Self::with_layout(selection, Layout::builtin())
    }

    /// A compositor for `selection` with an explicit layout.
    ///
    /// # Errors
    ///
    /// Propagates a bundled-font parse failure.
    pub fn with_layout(selection: Selection, layout: Layout) -> Result<Self, RenderError> {
        Ok(Self {
            palette: Palette::from_selection(selection),
            layout,
            text: TextRenderer::new()?,
            logo: None,
            sequence: None,
            echoed: false,
        })
    }

    /// A compositor for `selection` drawing a compiled theme's boot layout.
    ///
    /// The theme's images are copied out, so the theme need not outlive the
    /// compositor.
    ///
    /// # Errors
    ///
    /// Propagates a bundled-font parse failure.
    pub fn with_theme(selection: Selection, theme: &CompiledTheme) -> Result<Self, RenderError> {
        let boot = &theme.meta().boot;
        let mut compositor = Self::with_layout(selection, Layout::from_spec(boot))?;
        compositor.logo = match boot.logo.image {
            LogoImage::Builtin => None,
            LogoImage::Image(index) => theme.image(index).map(Sprite::new),
        };
        compositor.sequence = boot
            .sequence
            .as_ref()
            .and_then(|spec| Sequence::new(theme, spec));
        Ok(compositor)
    }

    /// A compositor for `selection` drawing a compiled theme's shutdown
    /// layout: its logo, its line and its frame sequence (FRN-SRS-035).
    ///
    /// # Errors
    ///
    /// Propagates a bundled-font parse failure.
    ///
    /// Implements: FRN-SRS-035
    pub fn with_shutdown(selection: Selection, theme: &CompiledTheme) -> Result<Self, RenderError> {
        let shutdown = &theme.meta().shutdown;
        let mut compositor = Self::with_layout(selection, Layout::from_shutdown(shutdown))?;
        compositor.logo = match shutdown.logo.image {
            LogoImage::Builtin => None,
            LogoImage::Image(index) => theme.image(index).map(Sprite::new),
        };
        compositor.sequence = shutdown
            .sequence
            .as_ref()
            .and_then(|spec| Sequence::new(theme, spec));
        Ok(compositor)
    }

    /// The palette in use.
    #[must_use]
    pub const fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The layout in use, in reference pixels.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Draws `scene` over the whole of `frame`.
    ///
    /// Implements: FRN-SRS-007, FRN-SRS-053
    pub fn render(&mut self, frame: &mut Frame, scene: &Scene) {
        let viewport = Viewport::fit(REFERENCE_SIZE, frame.size());
        let size = frame.size();

        // Canvas: the background role over everything, letterbox bands included.
        shapes::fill_rect(
            frame,
            Rect::new(0.0, 0.0, as_f32(size.width()), as_f32(size.height())),
            self.palette.color(Role::Background),
            false,
        );

        let logo_box = viewport.rect(self.layout.logo);
        match &mut self.logo {
            Some(sprite) => sprite.draw(frame, &self.palette, logo_box),
            // The mark is square: fitted uniformly, centred, like an image.
            None => logo::draw(frame, &self.palette, square_in(logo_box)),
        }
        if let Some(sequence) = &mut self.sequence {
            let index = sequence_frame(scene.time(), sequence.fps, sequence.frames.len());
            let bounds = viewport.rect(sequence.rect);
            if let Some(sprite) = sequence.frames.get_mut(index) {
                sprite.draw(frame, &self.palette, bounds);
            }
        }
        if scene.shows_progress() {
            let bar = self.draw_bar(frame, &viewport, scene);
            self.draw_percent(frame, &viewport, scene, bar);
        }
        self.draw_status(frame, &viewport, scene);
        if let Some(prompt) = scene.prompt() {
            self.draw_prompt(frame, &viewport, prompt);
        }
        if let Some(overlay) = scene.overlay() {
            self.draw_overlay(frame, &viewport, overlay);
        }
        let echoed = scene
            .prompt()
            .is_some_and(|prompt| matches!(prompt.field, Field::Clear(_)));
        if self.echoed && !echoed {
            self.text.forget();
        }
        self.echoed = echoed;
    }

    /// The journal overlay over everything: a surface panel with a border,
    /// its heading, and as many entries as fit, the newest at the bottom
    /// unless scrolled (FRN-SRS-060, FRN-SRS-061). The text is the foreground
    /// role on the surface, a verified pairing in every palette; each entry
    /// carries its severity as a tag, never as colour alone.
    ///
    /// Implements: FRN-SRS-060, FRN-SRS-061
    fn draw_overlay(&mut self, frame: &mut Frame, viewport: &Viewport, overlay: &OverlayView) {
        let panel = viewport.rect(OVERLAY_PANEL);
        if let Some(path) = shapes::rounded_rect(panel, viewport.length(12.0)) {
            shapes::fill(frame, &path, self.palette.color(Role::Surface));
            shapes::stroke(
                frame,
                &path,
                self.palette.color(Role::Border),
                viewport.length(2.0),
            );
        }
        let px = round_u32(viewport.length(OVERLAY_TEXT)).max(MIN_PX);
        let metrics = self.text.line_metrics(px);
        let pitch = metrics.new_line_size.max(1.0);
        let inset = viewport.length(24.0);
        let width = (panel.width - 2.0 * inset).max(0.0);
        let foreground = self.palette.color(Role::Foreground);
        let title = self.fit(OVERLAY_TITLE, px, width);
        let top = panel.y + inset + metrics.ascent;
        self.text
            .draw(frame, &title, panel.x + inset, top, px, foreground);
        // Entries fill upwards from the bottom of the panel.
        let first_baseline = top + 1.5 * pitch;
        let mut baseline = panel.bottom() - inset + metrics.descent;
        let end = overlay.lines.len().saturating_sub(overlay.scroll);
        for line in overlay.lines[..end].iter().rev() {
            if baseline < first_baseline {
                break;
            }
            let text = self.fit(line, px, width);
            self.text
                .draw(frame, &text, panel.x + inset, baseline, px, foreground);
            baseline -= pitch;
        }
    }

    /// The prompt, top to bottom in its region: the message on the canvas,
    /// the field on a surface with the focus colour round it, and any notice
    /// in the error colour on the canvas (FRN-SRS-021, FRN-SRS-026,
    /// FRN-SRS-029). Every text sits on a pairing the palette verifies.
    ///
    /// Implements: FRN-SRS-021, FRN-SRS-026, FRN-SRS-029
    fn draw_prompt(&mut self, frame: &mut Frame, viewport: &Viewport, prompt: &PromptView) {
        let region = viewport.rect(self.layout.prompt);
        let size = viewport.length(self.layout.prompt_size);
        let px = round_u32(size).max(MIN_PX);
        let ascent = self.text.line_metrics(px).ascent;

        self.draw_centred(
            frame,
            &prompt.message,
            region,
            region.y + ascent,
            px,
            Role::Foreground,
        );

        let field = Rect::new(region.x, region.y + 1.5 * size, region.width, 1.8 * size);
        if let Some(path) = shapes::rounded_rect(field, 0.3 * size) {
            shapes::fill(frame, &path, self.palette.color(Role::Surface));
            shapes::stroke(
                frame,
                &path,
                self.palette.color(Role::Focus),
                viewport.length(2.0),
            );
        }
        let inset = 0.5 * size;
        let inner = (field.width - 2.0 * inset).max(0.0);
        let foreground = self.palette.color(Role::Foreground);
        match &prompt.field {
            Field::Masked(count) => {
                let pitch = 0.6 * size;
                let fits = (inner / pitch).floor();
                let shown = u32::try_from(*count)
                    .unwrap_or(u32::MAX)
                    .min(round_u32(fits));
                for i in 0..shown {
                    let cx = field.x + inset + pitch * (as_f32(i) + 0.5);
                    if let Some(dot) = shapes::circle(cx, field.center_y(), 0.18 * size) {
                        shapes::fill(frame, &dot, foreground);
                    }
                }
            }
            Field::Clear(text) => {
                let tail = self.fit_tail(text.as_str(), px, inner);
                let baseline =
                    field.center_y() + f32::midpoint(ascent, self.text.line_metrics(px).descent);
                self.text
                    .draw(frame, tail, field.x + inset, baseline, px, foreground);
            }
        }

        if let Some(notice) = &prompt.notice {
            let baseline = field.bottom() + 0.5 * size + ascent;
            self.draw_centred(frame, notice, region, baseline, px, Role::Error);
        }
    }

    /// One line centred in `region`, shortened to its width.
    fn draw_centred(
        &mut self,
        frame: &mut Frame,
        text: &str,
        region: Rect,
        baseline: f32,
        px: u32,
        role: Role,
    ) {
        let text = self.fit(text, px, region.width);
        let width = self.text.measure(&text, px);
        let x = (region.center_x() - width / 2.0).floor();
        self.text
            .draw(frame, &text, x, baseline, px, self.palette.color(role));
    }

    /// The end of `text` that fits in `available` pixels: an echoed field
    /// shows where the typing is. A slice of `text`, never a copy: the text
    /// may be an echoed answer (FRN-SRS-024).
    fn fit_tail<'t>(&mut self, text: &'t str, px: u32, available: f32) -> &'t str {
        let mut width = 0.0;
        let mut start = text.len();
        for (index, ch) in text.char_indices().rev() {
            let advance = self.text.advance(ch, px);
            if width + advance > available {
                break;
            }
            width += advance;
            start = index;
        }
        &text[start..]
    }

    /// Track, outline and fill; returns the track rectangle in output pixels.
    fn draw_bar(&self, frame: &mut Frame, viewport: &Viewport, scene: &Scene) -> Rect {
        let bar = viewport.rect(self.layout.bar);
        let radius = viewport.length(self.layout.bar_radius);
        let track = shapes::rounded_rect(bar, radius);
        if let Some(track) = &track {
            shapes::fill(frame, track, self.palette.color(self.layout.bar_track));
        }
        // The fill is never narrower than a full pill, so its caps coincide with the
        // track's and nothing spills past the rounded corners at low percentages.
        if scene.is_started() {
            let fill_width = (bar.width * scene.fraction()).max(2.0 * radius);
            if let Some(fill) = shapes::rounded_rect(bar.with_width(fill_width), radius) {
                shapes::fill(frame, &fill, self.palette.color(self.layout.bar_fill));
            }
        }
        // The outline goes on last so the fill never thins it.
        if let Some(track) = &track {
            shapes::stroke(
                frame,
                track,
                self.palette.color(self.layout.bar_border),
                viewport.length(self.layout.bar_outline),
            );
        }
        bar
    }

    /// The percentage as text beside the bar, vertically centred on it (FRN-SRS-053).
    fn draw_percent(&mut self, frame: &mut Frame, viewport: &Viewport, scene: &Scene, bar: Rect) {
        let px = round_u32(viewport.length(self.layout.percent_size)).max(MIN_PX);
        let metrics = self.text.line_metrics(px);
        // Digits sit on the baseline and rise to roughly the ascent; centre that span.
        let baseline = bar.center_y() + f32::midpoint(metrics.ascent, metrics.descent);
        let x = bar.right() + viewport.length(self.layout.percent_gap);
        self.text.draw(
            frame,
            &scene.percent_text(),
            x,
            baseline,
            px,
            self.palette.color(self.layout.percent_color),
        );
    }

    /// The status line, centred under the bar and shortened to the content width.
    fn draw_status(&mut self, frame: &mut Frame, viewport: &Viewport, scene: &Scene) {
        if !self.layout.show_status {
            return;
        }
        let Some(status) = scene.status() else { return };
        let px = round_u32(viewport.length(self.layout.status_size)).max(MIN_PX);
        let content = viewport.rect(Rect::new(
            0.0,
            self.layout.status_top,
            as_f32(REFERENCE_SIZE.width()),
            0.0,
        ));
        let margin = viewport.length(40.0);
        let available = (content.width - 2.0 * margin).max(0.0);
        let text = self.fit(status, px, available);
        let width = self.text.measure(&text, px);
        let x = (content.center_x() - width / 2.0).floor();
        let baseline = content.y + self.text.line_metrics(px).ascent;
        self.text.draw(
            frame,
            &text,
            x,
            baseline,
            px,
            self.palette.color(self.layout.status_color),
        );
    }

    /// Shortens `text` with an ellipsis until it measures at most `available` pixels.
    fn fit(&mut self, text: &str, px: u32, available: f32) -> String {
        if self.text.measure(text, px) <= available {
            return text.to_owned();
        }
        let ellipsis_width = self.text.measure(ELLIPSIS, px);
        let mut kept = String::new();
        let mut width = 0.0;
        for ch in text.chars() {
            let advance = self.text.advance(ch, px);
            if width + advance + ellipsis_width > available {
                break;
            }
            kept.push(ch);
            width += advance;
        }
        kept.push_str(ELLIPSIS);
        kept
    }
}

impl fmt::Debug for Compositor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compositor")
            .field("palette", &self.palette)
            .field("layout", &self.layout)
            .field("text", &self.text)
            .field("logo", &self.logo)
            .field("sequence", &self.sequence)
            .finish()
    }
}

/// The largest square centred in `rect`.
fn square_in(rect: Rect) -> Rect {
    let side = rect.width.min(rect.height);
    Rect::new(
        rect.x + (rect.width - side) / 2.0,
        rect.y + (rect.height - side) / 2.0,
        side,
        side,
    )
}

#[cfg(test)]
mod tests {
    use fairing_theme::{Rgb, Theme};

    use super::*;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;
    use crate::scene::EchoText;

    fn render(width: u32, height: u32, scene: &Scene, format: PixelFormat) -> (Frame, Viewport) {
        let size = Size::new(width, height).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, format).unwrap_or_else(|e| panic!("{e}"));
        let mut compositor = Compositor::new(Selection::Color(Theme::family_default()))
            .unwrap_or_else(|e| panic!("{e}"));
        compositor.render(&mut frame, scene);
        (frame, Viewport::fit(REFERENCE_SIZE, size))
    }

    fn count(frame: &Frame, rect: Rect, color: Rgb) -> usize {
        let (x0, y0) = (round_u32(rect.x), round_u32(rect.y));
        let (x1, y1) = (round_u32(rect.right()), round_u32(rect.bottom()));
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) == Some(color))
            .count()
    }

    /// A theme whose sequence blinks a 100-pixel square in the top-left
    /// corner at 10 fps: frame 0 is filled with the accent, frame 1 empty.
    fn blinking_theme() -> CompiledTheme {
        use fairing_theme::{Encoding, ImageMeta, Meta, RectSpec};
        let mut meta = Meta::builtin();
        let image = |offset| ImageMeta {
            width: 2,
            height: 2,
            encoding: Encoding::Mask(Role::Accent),
            offset,
        };
        meta.images = vec![image(0), image(4)];
        meta.boot.sequence = Some(SequenceSpec {
            rect: RectSpec::new(0.0, 0.0, 100.0, 100.0),
            fps: 10,
            first: 0,
            count: 2,
        });
        let mut blob = vec![255; 4];
        blob.extend([0; 4]);
        CompiledTheme::new(meta, blob).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn sequences_loop_at_their_declared_rate() {
        // Verifies: FRN-SRS-051
        let at = |ms: u64| sequence_frame(Duration::from_millis(ms), 10, 3);
        assert_eq!([at(0), at(99), at(100), at(250), at(299)], [0, 0, 1, 2, 2]);
        assert_eq!(at(300), 0, "back to the first after the last");
        assert_eq!(at(1_000_000), 1);
        assert_eq!(sequence_frame(Duration::from_secs(1), 30, 30), 0);
        assert_eq!(sequence_frame(Duration::from_millis(500), 30, 0), 0);

        let theme = blinking_theme();
        let accent = Theme::family_default().color(Role::Accent);
        let mut compositor =
            Compositor::with_theme(Selection::Color(Theme::family_default()), &theme)
                .unwrap_or_else(|e| panic!("{e}"));
        let size = Size::new(192, 108).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        let mut corner_at = |ms: u64| {
            let scene = Scene::new(10).with_time(Duration::from_millis(ms));
            compositor.render(&mut frame, &scene);
            frame.pixel(5, 5)
        };
        assert_eq!(corner_at(0), Some(accent));
        assert_ne!(corner_at(100), Some(accent));
        assert_eq!(corner_at(200), Some(accent));
    }

    #[test]
    fn the_overlay_draws_the_newest_entries_at_the_bottom_and_scrolls() {
        // Verifies: FRN-SRS-060, FRN-SRS-061
        let theme = Theme::family_default();
        let overlay = |scroll| OverlayView {
            lines: vec![
                "   1.000 [INFO] one".to_owned(),
                "   2.000 [WARN] two".to_owned(),
                "   3.000 [ERROR] three".to_owned(),
            ],
            scroll,
        };
        let viewport = Viewport::fit(REFERENCE_SIZE, REFERENCE_SIZE);
        let panel = viewport.rect(OVERLAY_PANEL);
        // The last line's band, just above the panel's bottom inset.
        let band = Rect::new(panel.x + 24.0, panel.bottom() - 60.0, 600.0, 40.0);
        let foreground = theme.color(Role::Foreground);
        let (newest, _) = render(
            1920,
            1080,
            &Scene::new(50).with_overlay(Some(overlay(0))),
            PixelFormat::Rgba8888,
        );
        assert_eq!(
            newest.pixel(round_u32(panel.center_x()), round_u32(panel.center_y())),
            Some(theme.color(Role::Surface)),
            "the panel covers the splash"
        );
        assert!(
            count(&newest, band, foreground) > 0,
            "an entry at the bottom"
        );
        let (scrolled, _) = render(
            1920,
            1080,
            &Scene::new(50).with_overlay(Some(overlay(3))),
            PixelFormat::Rgba8888,
        );
        assert_eq!(
            count(&scrolled, band, foreground),
            0,
            "scrolled past every entry"
        );
        let (none, _) = render(1920, 1080, &Scene::new(50), PixelFormat::Rgba8888);
        assert_eq!(
            none.pixel(round_u32(panel.x + 30.0), round_u32(panel.y + 30.0)),
            Some(theme.color(Role::Background)),
            "no overlay unless asked"
        );
    }

    #[test]
    fn the_shutdown_layout_draws_its_line_and_no_bar() {
        // Verifies: FRN-SRS-035
        let theme = Theme::family_default();
        let size = Size::new(1920, 1080).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        let mut compositor =
            Compositor::with_shutdown(Selection::Color(theme), &CompiledTheme::builtin())
                .unwrap_or_else(|e| panic!("{e}"));
        let scene = Scene::new(50)
            .without_progress()
            .with_status("Shutting down");
        compositor.render(&mut frame, &scene);
        let viewport = Viewport::fit(REFERENCE_SIZE, size);
        let bar = viewport.rect(Layout::builtin().bar);
        assert_eq!(
            count(&frame, bar, theme.color(Role::Accent)),
            0,
            "no bar fill at shutdown"
        );
        let line = Rect::new(0.0, 640.0, 1920.0, 40.0);
        assert!(
            count(&frame, line, theme.color(Role::Foreground)) > 0,
            "the line"
        );
    }

    #[test]
    fn an_echoed_answers_glyphs_go_when_its_prompt_closes() {
        // Verifies: FRN-SRS-024
        let size = Size::new(640, 360).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}"));
        let mut compositor = Compositor::new(Selection::Color(Theme::family_default()))
            .unwrap_or_else(|e| panic!("{e}"));
        let echoed = Scene::new(10).with_prompt(Some(PromptView {
            message: "Token".to_owned(),
            field: Field::Clear(EchoText::new("zq")),
            notice: None,
        }));
        compositor.render(&mut frame, &echoed);
        assert!(compositor.text.has_cached('z'));
        compositor.render(&mut frame, &Scene::new(10));
        assert!(!compositor.text.has_cached('z'));
        assert!(!compositor.text.has_cached('q'));
    }

    #[test]
    fn bar_fill_follows_the_percentage_in_both_byte_orders() {
        for format in [PixelFormat::Rgba8888, PixelFormat::Xrgb8888] {
            let theme = Theme::family_default();
            let (frame, viewport) = render(1920, 1080, &Scene::new(50), format);
            let bar = viewport.rect(Layout::builtin().bar);
            let y = round_u32(bar.center_y());
            let quarter = round_u32(bar.x + bar.width * 0.25);
            let three_quarters = round_u32(bar.x + bar.width * 0.75);
            assert_eq!(
                frame.pixel(quarter, y),
                Some(theme.color(Role::Accent)),
                "{format}"
            );
            assert_eq!(
                frame.pixel(three_quarters, y),
                Some(theme.color(Role::Surface)),
                "{format}"
            );
            assert_eq!(
                frame.pixel(0, 0),
                Some(theme.color(Role::Background)),
                "{format}"
            );
            assert_eq!(
                frame.pixel(1919, 1079),
                Some(theme.color(Role::Background)),
                "{format}"
            );
        }
    }

    #[test]
    fn zero_and_full_progress_are_drawn() {
        let theme = Theme::family_default();
        let layout = Layout::builtin();
        let (empty, viewport) = render(1920, 1080, &Scene::new(0), PixelFormat::Rgba8888);
        let bar = viewport.rect(layout.bar);
        assert_eq!(count(&empty, bar, theme.color(Role::Accent)), 0);
        let (full, _) = render(1920, 1080, &Scene::new(100), PixelFormat::Rgba8888);
        let inner = Rect::new(bar.x + 8.0, bar.y + 4.0, bar.width - 16.0, bar.height - 8.0);
        let accent = count(&full, inner, theme.color(Role::Accent));
        let expected = round_u32(inner.width * inner.height * 0.95) as usize;
        assert!(accent > expected, "{accent} <= {expected}");
    }

    #[test]
    fn percentage_text_is_rendered_beside_the_bar() {
        // Verifies: FRN-SRS-053
        let theme = Theme::family_default();
        let layout = Layout::builtin();
        let (frame, viewport) = render(1920, 1080, &Scene::new(42), PixelFormat::Rgba8888);
        let bar = viewport.rect(layout.bar);
        let region = Rect::new(bar.right(), bar.y - 30.0, 160.0, bar.height + 60.0);
        let painted = count(&frame, region, theme.color(Role::Foreground));
        assert!(painted > 40, "{painted} foreground pixels beside the bar");
        // The same region is empty canvas when nothing is drawn there at 0%... no: it always
        // shows text, so compare with the mirror region left of the bar, which is canvas.
        let mirror = Rect::new(bar.x - 160.0, bar.y - 30.0, 160.0, bar.height + 60.0);
        assert_eq!(count(&frame, mirror, theme.color(Role::Foreground)), 0);
    }

    #[test]
    fn status_line_is_centred_and_fits() {
        let theme = Theme::family_default();
        let scene = Scene::new(10).with_status("Mounting /home");
        let (frame, viewport) = render(1920, 1080, &scene, PixelFormat::Rgba8888);
        let top = viewport.rect(Rect::new(0.0, Layout::builtin().status_top, 1920.0, 40.0));
        let left = Rect::new(0.0, top.y, 960.0, 40.0);
        let right = Rect::new(960.0, top.y, 960.0, 40.0);
        let l = count(&frame, left, theme.color(Role::Foreground));
        let r = count(&frame, right, theme.color(Role::Foreground));
        assert!(l > 0 && r > 0, "{l} {r}");
        let imbalance =
            (as_f32(u32::try_from(l).unwrap_or(0)) - as_f32(u32::try_from(r).unwrap_or(0))).abs()
                / as_f32(u32::try_from(l + r).unwrap_or(1));
        assert!(imbalance < 0.35, "centred: {l} vs {r}");

        let long = "x".repeat(400);
        let scene = Scene::new(10).with_status(long);
        let (frame, _) = render(640, 480, &scene, PixelFormat::Rgba8888);
        assert_eq!(frame.pixel(0, 0), Some(theme.color(Role::Background)));
    }

    #[test]
    fn layout_scales_uniformly_into_other_panels() {
        // Verifies: FRN-SRS-007
        let theme = Theme::family_default();
        let layout = Layout::builtin();
        for (w, h) in [
            (1280, 800),
            (1024, 768),
            (2560, 1080),
            (3840, 2160),
            (640, 480),
        ] {
            let (frame, viewport) = render(w, h, &Scene::new(50), PixelFormat::Xrgb8888);
            let bar = viewport.rect(layout.bar);
            let reference_ratio = layout.bar.width / layout.bar.height;
            assert!(
                (bar.width / bar.height - reference_ratio).abs() < 1e-3,
                "{w}x{h}"
            );
            let y = round_u32(bar.center_y());
            assert_eq!(
                frame.pixel(round_u32(bar.x + bar.width * 0.25), y),
                Some(theme.color(Role::Accent)),
                "{w}x{h}"
            );
            assert_eq!(
                frame.pixel(round_u32(bar.x + bar.width * 0.75), y),
                Some(theme.color(Role::Surface)),
                "{w}x{h}"
            );
            assert_eq!(
                frame.pixel(0, 0),
                Some(theme.color(Role::Background)),
                "{w}x{h}"
            );
            assert_eq!(
                frame.pixel(w - 1, h - 1),
                Some(theme.color(Role::Background)),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn mono_selection_renders_without_colour_tokens() {
        let resolution = fairing_theme::resolve(&fairing_theme::Request {
            no_color: true,
            ..fairing_theme::Request::default()
        });
        let size = Size::new(800, 600).unwrap_or_else(|e| panic!("{e}"));
        let mut frame = Frame::new(size, PixelFormat::Xrgb8888).unwrap_or_else(|e| panic!("{e}"));
        let mut compositor =
            Compositor::new(resolution.selection).unwrap_or_else(|e| panic!("{e}"));
        compositor.render(&mut frame, &Scene::new(75).with_status("Starting greetd"));
        assert_eq!(compositor.palette().slug(), fairing_theme::MONO_SLUG);
        let bar = Viewport::fit(REFERENCE_SIZE, size).rect(Layout::builtin().bar);
        let filled = frame.pixel(
            round_u32(bar.x + bar.width * 0.5),
            round_u32(bar.center_y()),
        );
        assert_eq!(filled, Some(compositor.palette().color(Role::Accent)));
    }

    fn prompt(field: Field, notice: Option<&str>) -> Scene {
        Scene::new(30).with_prompt(Some(PromptView {
            message: "Please enter passphrase for disk root".to_owned(),
            field,
            notice: notice.map(str::to_owned),
        }))
    }

    /// The field box in output pixels, at 1920x1080.
    fn field_box() -> Rect {
        let layout = Layout::builtin();
        let size = layout.prompt_size;
        let region = layout.prompt;
        Rect::new(region.x, region.y + 1.5 * size, region.width, 1.8 * size)
    }

    #[test]
    fn no_prompt_leaves_its_region_as_canvas() {
        let theme = Theme::family_default();
        let (frame, viewport) = render(1920, 1080, &Scene::new(30), PixelFormat::Rgba8888);
        let region = viewport.rect(Layout::builtin().prompt);
        let area = round_u32(region.width * region.height) as usize;
        assert_eq!(count(&frame, region, theme.color(Role::Background)), area);
    }

    #[test]
    fn the_prompt_draws_message_and_a_masked_field() {
        // Verifies: FRN-SRS-021
        let theme = Theme::family_default();
        let layout = Layout::builtin();
        let (frame, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(6), None),
            PixelFormat::Rgba8888,
        );
        let message = Rect::new(
            layout.prompt.x,
            layout.prompt.y,
            layout.prompt.width,
            1.4 * layout.prompt_size,
        );
        assert!(count(&frame, message, theme.color(Role::Foreground)) > 200);
        let field = field_box();
        assert!(count(&frame, field, theme.color(Role::Surface)) > 10_000);
        assert!(
            count(&frame, field, theme.color(Role::Focus)) > 100,
            "focus outline"
        );
        let six = count(&frame, field, theme.color(Role::Foreground));
        let (frame, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(3), None),
            PixelFormat::Rgba8888,
        );
        let three = count(&frame, field, theme.color(Role::Foreground));
        assert!(
            three > 0 && six > 3 * three / 2,
            "one mark per character: {three} vs {six}"
        );
        // A very long secret stays inside the field.
        let (frame, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(4096), None),
            PixelFormat::Rgba8888,
        );
        let right = Rect::new(field.right() + 1.0, field.y, 40.0, field.height);
        assert_eq!(count(&frame, right, theme.color(Role::Foreground)), 0);
    }

    #[test]
    fn echo_draws_the_input_as_text() {
        // Verifies: FRN-SRS-026
        let theme = Theme::family_default();
        let field = field_box();
        let (masked, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(0), None),
            PixelFormat::Rgba8888,
        );
        assert_eq!(count(&masked, field, theme.color(Role::Foreground)), 0);
        let (clear, _) = render(
            1920,
            1080,
            &prompt(Field::Clear(EchoText::new("operator")), None),
            PixelFormat::Rgba8888,
        );
        assert!(count(&clear, field, theme.color(Role::Foreground)) > 100);
        // Longer than the field: the tail is shown and nothing spills out.
        let long = Field::Clear(EchoText::new(&"y".repeat(500)));
        let (frame, _) = render(1920, 1080, &prompt(long, None), PixelFormat::Rgba8888);
        let right = Rect::new(field.right() + 1.0, field.y, 40.0, field.height);
        assert_eq!(count(&frame, right, theme.color(Role::Foreground)), 0);
    }

    #[test]
    fn a_notice_is_drawn_below_the_field_in_the_error_colour() {
        // Verifies: FRN-SRS-029
        let theme = Theme::family_default();
        let field = field_box();
        let below = Rect::new(field.x, field.bottom() + 2.0, field.width, 50.0);
        let (plain, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(0), None),
            PixelFormat::Rgba8888,
        );
        assert_eq!(count(&plain, below, theme.color(Role::Error)), 0);
        let (noticed, _) = render(
            1920,
            1080,
            &prompt(Field::Masked(0), Some("Wrong passphrase. Try again.")),
            PixelFormat::Rgba8888,
        );
        assert!(count(&noticed, below, theme.color(Role::Error)) > 100);
    }

    #[test]
    fn the_builtin_mark_is_fitted_into_a_centred_square() {
        let wide = square_in(Rect::new(360.0, 300.0, 1200.0, 240.0));
        assert_eq!(wide, Rect::new(840.0, 300.0, 240.0, 240.0));
        let tall = square_in(Rect::new(0.0, 0.0, 100.0, 300.0));
        assert_eq!(tall, Rect::new(0.0, 100.0, 100.0, 100.0));
    }
}
