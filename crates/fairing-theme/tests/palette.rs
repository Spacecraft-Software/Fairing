// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The generated palette agrees with the vendored palette file, value for value.
//!
//! This is the §11.6 "never retype hexes" guarantee as a test: every colour the
//! binary can draw is compared against the TOML it was generated from, so a
//! generator defect cannot ship a wrong colour silently (Category B).

use fairing_theme::{Ansi, MonoRoles, Polarity, Role, Theme, Variant};

const ASSET: &str = include_str!("../assets/steelbore.toml");

fn table() -> toml::Table {
    ASSET
        .parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("vendored palette does not parse: {e}"))
}

fn sub<'a>(table: &'a toml::Table, key: &str) -> &'a toml::Table {
    table
        .get(key)
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("missing table `{key}`"))
}

fn text<'a>(table: &'a toml::Table, key: &str) -> &'a str {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("missing string `{key}`"))
}

#[test]
fn registered_set_is_generated_in_full_and_in_order() {
    let table = table();
    let meta = sub(&table, "meta");
    let registered: Vec<&str> = meta["registered-set"]
        .as_array()
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    let colour: Vec<&str> = registered
        .iter()
        .copied()
        .filter(|s| *s != fairing_theme::MONO_SLUG)
        .collect();
    let generated: Vec<&str> = Theme::registered().iter().map(|t| t.slug).collect();
    assert_eq!(generated, colour);
    assert_eq!(fairing_theme::DEFAULT_SLUG, text(meta, "default-theme"));
    assert_eq!(fairing_theme::MONO_SLUG, text(meta, "mono-theme"));
    assert_eq!(fairing_theme::PALETTE_VERSION, text(meta, "version"));
    // A re-vendored palette file is a reviewed event: `steelbore-blackpinkpanther` changed
    // meaning at v2.08, so this pin moves only on purpose.
    assert_eq!(fairing_theme::PALETTE_VERSION, "3.5.0");
    assert_eq!(Theme::family_default().slug, fairing_theme::DEFAULT_SLUG);
}

#[test]
fn every_role_of_every_theme_equals_the_file() {
    let table = table();
    let themes = sub(&table, "themes");
    for theme in Theme::registered() {
        let source = sub(themes, theme.slug);
        for role in Role::ALL {
            let expected = text(source, role.as_str());
            assert_eq!(
                theme.color(role).to_string().to_ascii_uppercase(),
                expected.to_ascii_uppercase(),
                "{} {role}",
                theme.slug
            );
        }
        let expected_variant = if theme.slug.ends_with("-high-contrast") {
            Variant::HighContrast
        } else {
            Variant::Base
        };
        assert_eq!(theme.variant, expected_variant, "{}", theme.slug);
        assert!(theme.slug.starts_with(theme.base), "{}", theme.slug);
    }
}

#[test]
fn polarity_and_counterparts_equal_the_file() {
    let table = table();
    let resolution = sub(&table, "resolution");
    let polarity = sub(resolution, "polarity");
    let pair = sub(resolution, "pair");
    for theme in Theme::registered() {
        let expected = match text(polarity, theme.base) {
            "dark" => Polarity::Dark,
            "light" => Polarity::Light,
            other => panic!("{other}"),
        };
        assert_eq!(theme.polarity, expected, "{}", theme.slug);
        assert_eq!(theme.counterpart, text(pair, theme.base), "{}", theme.slug);
    }
    assert_eq!(fairing_theme::ENV_VAR, text(resolution, "env-var"));
    assert_eq!(fairing_theme::SYSTEM_FILE, text(resolution, "system-file"));
    assert_eq!(fairing_theme::USER_FILE, text(resolution, "user-file"));
}

#[test]
fn mono_bindings_equal_the_file() {
    let table = table();
    let mono = sub(sub(&table, "themes"), fairing_theme::MONO_SLUG);
    let selection = fairing_theme::resolve(&fairing_theme::Request {
        no_color: true,
        ..fairing_theme::Request::default()
    });
    let bindings: &MonoRoles = match selection.selection {
        fairing_theme::Selection::Mono(mono) => mono,
        fairing_theme::Selection::Color(_) => panic!("NO_COLOR did not select mono"),
    };
    for role in Role::ALL {
        match bindings.get(role) {
            Some(slot) => assert_eq!(slot.as_str(), text(mono, role.as_str()), "{role}"),
            None => assert!(
                role.is_surface() && mono.get(role.as_str()).is_none(),
                "{role}"
            ),
        }
    }
    assert_eq!(bindings.focus, Ansi::ReverseVideo);
}

#[test]
fn documented_contrast_floors_hold_for_every_base_theme() {
    // Spot-checks the §11.0.1 class rules the file records: on every conforming
    // base palette, body text clears 4.5:1 on the canvas and the two
    // surfaces are not text colours (below 3:1 against the canvas or each
    // other is permitted, since they are fills).
    for theme in Theme::registered()
        .iter()
        .filter(|t| t.variant == Variant::Base)
    {
        let body = theme
            .roles
            .foreground
            .contrast_ratio(theme.roles.background);
        assert!(
            body >= 4.5,
            "{}: foreground on background {body:.2}:1",
            theme.slug
        );
    }
    for theme in Theme::registered()
        .iter()
        .filter(|t| t.variant == Variant::HighContrast)
    {
        for role in [
            Role::Foreground,
            Role::Accent,
            Role::Structure,
            Role::Success,
            Role::Error,
            Role::Warning,
        ] {
            let ratio = theme.color(role).contrast_ratio(theme.roles.background);
            assert!(
                ratio >= 7.0,
                "{}: {role} on background {ratio:.2}:1",
                theme.slug
            );
        }
    }
}

#[test]
fn polarity_follows_the_canvas_luminance() {
    // The file states that `[resolution.polarity]` is derivable from each background's
    // WCAG relative luminance; derive it and hold the generated table to that.
    let black = fairing_theme::Rgb::new(0, 0, 0);
    let white = fairing_theme::Rgb::new(0xFF, 0xFF, 0xFF);
    for theme in Theme::registered() {
        let background = theme.color(Role::Background);
        let derived = if background.contrast_ratio(black) > background.contrast_ratio(white) {
            Polarity::Light
        } else {
            Polarity::Dark
        };
        assert_eq!(theme.polarity, derived, "{}", theme.slug);
    }
}

#[test]
fn recorded_contrast_ratios_match_the_wcag_math() {
    // Every `contrast.vs-background` row of the file is an independent oracle for
    // `Rgb::contrast_ratio`; the generated colours must reproduce them to the hundredth.
    let table = table();
    let themes = sub(&table, "themes");
    let mut checked = 0;
    for theme in Theme::registered() {
        let Some(contrast) = themes
            .get(theme.slug)
            .and_then(toml::Value::as_table)
            .and_then(|t| t.get("contrast"))
            .and_then(toml::Value::as_table)
            .and_then(|c| c.get("vs-background"))
            .and_then(toml::Value::as_table)
        else {
            continue;
        };
        let background = theme.color(Role::Background);
        for (key, value) in contrast {
            let Some(role) = Role::ALL.iter().copied().find(|r| r.as_str() == key) else {
                continue;
            };
            let recorded: f64 = value
                .as_str()
                .and_then(|s| s.strip_suffix(":1"))
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("{}: `{key}` = {value} is not `N:1`", theme.slug));
            let computed = theme.color(role).contrast_ratio(background);
            assert!(
                (computed - recorded).abs() < 0.011,
                "{}: {key} recorded {recorded} computed {computed:.3}",
                theme.slug
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} ratios checked");
}
