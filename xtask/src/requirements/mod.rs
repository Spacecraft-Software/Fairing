// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The requirement set: its TOML model, validation and Texinfo rendering.
//!
//! `doc/requirements.toml` is the authoring source. The canonical published form
//! (Steelbore Standard §20.1) is the generated `doc/needs.texi` and
//! `doc/requirements.texi`, which `doc/fairing.texi` includes. Nothing is
//! maintained twice: the generated files are never hand-edited, and
//! `cargo xtask req-texi --check` fails CI when they are stale.

pub mod command;
pub mod model;
pub mod texinfo;
pub mod validate;

use std::path::Path;

use crate::paths;
use crate::report::Failure;

pub use model::{Need, Priority, Requirement, RequirementSet, Status, Verification};

/// Default location of the requirement set, relative to the repository root.
pub const DEFAULT_PATH: &str = "doc/requirements.toml";

/// Loads and parses the requirement set at `path`.
///
/// # Errors
///
/// Returns a not-found failure when the file is missing and an internal failure
/// when it is not valid TOML for the [`RequirementSet`] schema.
pub fn load(path: &Path) -> Result<RequirementSet, Failure> {
    let text = paths::read_text(
        path,
        "cargo xtask req-texi --requirements doc/requirements.toml",
    )?;
    let set: RequirementSet = toml::from_str(&text).map_err(|error| {
        Failure::internal(format!(
            "`{}` does not match the requirement schema: {}",
            path.display(),
            error.message()
        ))
    })?;
    Ok(set)
}
