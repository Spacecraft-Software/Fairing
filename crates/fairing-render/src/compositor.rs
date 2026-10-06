// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Draws a [`Scene`] into a [`Frame`]: canvas, logo, bar, percentage, status.

use std::fmt;

use fairing_theme::{CompiledTheme, LogoImage, Role, Selection};

use crate::fault::RenderError;
use crate::frame::Frame;
use crate::geometry::{Rect, Viewport, as_f32, round_u32};
use crate::layout::{Layout, REFERENCE_SIZE};
use crate::logo;
use crate::palette::Palette;
use crate::scene::Scene;
use crate::shapes;
use crate::sprite::Sprite;
use crate::text::{MIN_PX, TextRenderer};

/// Marks a status line that had to be shortened to fit.
const ELLIPSIS: &str = "...";

/// Renders scenes for one theme selection and layout.
pub struct Compositor {
    palette: Palette,
    layout: Layout,
    text: TextRenderer,
    /// The theme's logo image; `None` draws the built-in vector mark.
    logo: Option<Sprite>,
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
            None => logo::draw(frame, &self.palette, logo_box),
        }
        let bar = self.draw_bar(frame, &viewport, scene);
        self.draw_percent(frame, &viewport, scene, bar);
        self.draw_status(frame, &viewport, scene);
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
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use fairing_theme::{Rgb, Theme};

    use super::*;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;

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
}
