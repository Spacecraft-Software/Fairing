// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! What every drawing verb decides before it touches a device: which backend
//! the agent rule allows (FRN-SRS-084) and which palette §11.6 selects
//! (FRN-SRS-045). `preview` and `splash` share these so the two can never
//! disagree about either.

use fairing_render::Choice;
use fairing_theme::{Request, Resolution, Source};

use crate::cli::BackendChoice;
use crate::diagnostic::{Diagnostic, Severity};
use crate::error::AppError;
use crate::output::mode::Context;

/// Applies the agent rule (FRN-SRS-084) to the requested backend.
///
/// Under an agent or CI harness no VT is opened: `auto` becomes the memory
/// backend with a warning and an explicit device backend is a usage error. A
/// plan opens nothing, so under `--dry-run` an agent may inspect the device
/// chain it could not run; the plan carries a warning instead of a refusal.
///
/// # Errors
///
/// `INVALID_ARGUMENT` for a device backend under an agent harness.
///
/// Implements: FRN-SRS-084
pub fn choose_backend(
    requested: BackendChoice,
    hint: &str,
    context: &Context,
    invocation: &str,
    dry_run: bool,
) -> Result<Choice, AppError> {
    if !context.is_agent_environment() {
        return Ok(requested.chain());
    }
    match requested {
        BackendChoice::Memory => Ok(Choice::Memory),
        BackendChoice::Drm | BackendChoice::Fbdev if dry_run => {
            Diagnostic::new(
                Severity::Warn,
                "AGENT_DEVICE_PLAN_ONLY",
                format!(
                    "agent environment detected; `--backend {}` is planned here but would be refused without --dry-run",
                    requested.chain()
                ),
                invocation,
            )
            .with_hint(hint)
            .emit(context);
            Ok(requested.chain())
        }
        BackendChoice::Auto => {
            Diagnostic::new(
                Severity::Warn,
                "AGENT_MEMORY_BACKEND",
                "agent environment detected; rendering to memory instead of a VT",
                invocation,
            )
            .with_hint(hint)
            .emit(context);
            Ok(Choice::Memory)
        }
        BackendChoice::Drm | BackendChoice::Fbdev => Err(AppError::invalid_argument(
            format!(
                "`--backend {}` opens a VT, which an agent environment must not",
                requested.chain()
            ),
            hint,
            invocation,
        )),
    }
}

/// Where a palette may come from, besides the environment and `NO_COLOR`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaletteSources<'a> {
    /// `--palette`: source 1, and a usage error when unregistered.
    pub explicit: Option<&'a str>,
    /// The kernel command line, searched for `fairing.theme=`.
    pub kernel_cmdline: Option<&'a str>,
    /// The compiled theme's declared palette.
    pub declared_default: Option<&'a str>,
    /// The verb, for the hint (`preview`, `splash`).
    pub verb: &'a str,
}

/// Resolves the palette (Steelbore Standard §11.6, FRN-SRS-045) and reports it
/// under `--verbose`.
///
/// `SPACECRAFT_THEME` is read here and nowhere else.
///
/// # Errors
///
/// `INVALID_ARGUMENT` when `--palette` is empty or not a registered theme; a
/// slug from any other source is skipped instead, as §11.6 requires.
pub fn resolve_palette(
    sources: &PaletteSources<'_>,
    context: &Context,
    invocation: &str,
) -> Result<Resolution, AppError> {
    let environment = std::env::var(fairing_theme::ENV_VAR).ok();
    let request = Request {
        explicit: sources.explicit,
        kernel_parameter: sources
            .kernel_cmdline
            .and_then(fairing_theme::kernel_parameter),
        environment: environment.as_deref(),
        declared_default: sources.declared_default,
        no_color: context.no_color,
        accessible: false,
    };
    let resolution = fairing_theme::resolve(&request);
    if let Some(slug) = sources.explicit
        && (slug.trim().is_empty()
            || resolution
                .skipped
                .iter()
                .any(|s| s.source == Source::Explicit))
    {
        return Err(AppError::invalid_argument(
            format!("palette `{slug}` is not a registered theme"),
            format!(
                "fairing {} --palette {}",
                sources.verb,
                fairing_theme::DEFAULT_SLUG
            ),
            invocation,
        ));
    }
    Diagnostic::new(
        Severity::Info,
        "PALETTE_RESOLVED",
        format!(
            "palette `{}` from {} (base `{}`, overlay {})",
            resolution.selection, resolution.source, resolution.base, resolution.overlay
        ),
        invocation,
    )
    .emit(context);
    Ok(resolution)
}
