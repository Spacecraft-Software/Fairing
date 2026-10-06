// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Repository-root discovery and repository-relative path rendering.

use std::path::{Path, PathBuf};

use crate::report::Failure;

/// The workspace root: the directory above the `xtask` crate.
#[must_use]
pub fn default_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("..");
    root.canonicalize().unwrap_or(root)
}

/// Resolves `value` against `root` unless it is already absolute.
#[must_use]
pub fn resolve(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Renders `path` relative to `root` with forward slashes, for reports.
#[must_use]
pub fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Reads a UTF-8 text file, mapping a missing file to a not-found failure.
///
/// # Errors
///
/// Returns [`Failure::not_found`] when the file does not exist and an internal
/// failure for any other I/O or encoding problem.
pub fn read_text(path: &Path, hint: &str) -> Result<String, Failure> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
            Failure::internal(format!("`{}` is not valid UTF-8: {error}", path.display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Failure::not_found(
            format!("`{}` does not exist", path.display()),
            hint,
        )),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_uses_forward_slashes() {
        let root = Path::new("/repo");
        assert_eq!(
            relative(root, Path::new("/repo/crates/a/src/lib.rs")),
            "crates/a/src/lib.rs"
        );
        assert_eq!(relative(root, Path::new("other/x")), "other/x");
    }

    #[test]
    fn resolve_keeps_absolute_paths() {
        let root = Path::new("/repo");
        assert_eq!(resolve(root, "/abs"), PathBuf::from("/abs"));
        assert_eq!(
            resolve(root, "doc/x.toml"),
            PathBuf::from("/repo/doc/x.toml")
        );
    }
}
