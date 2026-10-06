// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Reading a compiled theme from disk, bounded, with the error mapped onto
//! the exit-code table.

use std::io::{self, Read as _};
use std::path::Path;

use fairing_theme::{CompiledTheme, MAX_ARTEFACT_BYTES};

use crate::error::AppError;

/// Reads and validates the compiled theme at `path`.
///
/// At most one byte more than [`MAX_ARTEFACT_BYTES`] is read, so a wrong path
/// pointing at a large file costs nothing (FRN-SRS-047).
///
/// # Errors
///
/// `NOT_FOUND` or `PERMISSION_DENIED` by the I/O error; `INVALID_ARGUMENT`
/// for a file that is not a valid compiled theme of this format version.
pub fn load(path: &Path, hint: &str, invocation: &str) -> Result<CompiledTheme, AppError> {
    let bytes = read_bounded(path).map_err(|e| {
        let message = format!("cannot read theme `{}`: {e}", path.display());
        match e.kind() {
            io::ErrorKind::NotFound => AppError::not_found(message, hint, invocation),
            io::ErrorKind::PermissionDenied => {
                AppError::permission_denied(message, hint, invocation)
            }
            _ => AppError::invalid_argument(message, hint, invocation),
        }
    })?;
    CompiledTheme::from_bytes(&bytes).map_err(|e| {
        AppError::invalid_argument(format!("theme `{}`: {e}", path.display()), hint, invocation)
    })
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_ARTEFACT_BYTES).unwrap_or(u64::MAX);
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ARTEFACT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("larger than {MAX_ARTEFACT_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_damaged_and_good_themes_map_onto_the_table() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let missing = load(&dir.path().join("none.fairing"), "h", "c")
            .err()
            .map(|e| e.exit_code);
        assert_eq!(missing, Some(3));
        let damaged = dir.path().join("bad.fairing");
        std::fs::write(&damaged, b"FRNTHEME\x09\x00").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(load(&damaged, "h", "c").err().map(|e| e.exit_code), Some(2));
        let good = dir.path().join("good.fairing");
        let bytes = CompiledTheme::builtin()
            .to_bytes()
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&good, bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(load(&good, "h", "c").is_ok());
    }
}
