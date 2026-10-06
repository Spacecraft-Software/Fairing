// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Marker discovery: walks the evidence-bearing parts of the tree.
//!
//! Prose (`*.md`, `*.texi`, `doc/requirements.toml`) is never scanned, so a
//! requirement cannot "verify itself" by mentioning its own identifier.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::report::Failure;

/// Directories and files scanned, relative to the repository root.
///
/// `xtask/` is deliberately absent: the task runner implements no product
/// requirement, and its tests carry marker strings as test data.
pub const SCAN_ROOTS: &[&str] = &[
    "crates",
    ".github/workflows",
    "packaging",
    "flake.nix",
    "Makefile",
    "doc/Makefile",
];

/// File extensions that may carry markers.
const EXTENSIONS: &[&str] = &["rs", "yml", "yaml", "nix", "ncl", "service", "mk"];

/// Directory names never descended into.
const SKIP_DIRS: &[&str] = &["target", "fixtures"];

/// Captures loosely so a mistyped identifier is reported as unknown rather than
/// silently ignored; the matrix validates each captured id strictly.
const MARKER_PATTERN: &str = r"\b(?P<kind>Verifies|Implements):[ \t]*(?P<ids>[A-Z]{2,6}-(?:SRS|NEED)-[0-9]{1,4}(?:[ \t]*,[ \t]*[A-Z]{2,6}-(?:SRS|NEED)-[0-9]{1,4})*)";

/// Whether a marker is evidence or a design pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    /// `Verifies:` — a test or analysis that discharges the requirement.
    Verifies,
    /// `Implements:` — the design element that realises it.
    Implements,
}

/// One marker occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// Evidence or design pointer.
    pub kind: MarkerKind,
    /// Identifiers cited on this line, in order.
    pub ids: Vec<String>,
    /// Repository-relative path.
    pub path: String,
    /// 1-based line number.
    pub line: usize,
}

/// Scans the standard roots under `root`.
///
/// # Errors
///
/// Returns an internal failure on an I/O error other than a missing root.
pub fn scan(root: &Path) -> Result<Vec<Marker>, Failure> {
    let re = marker_regex();
    let mut markers = Vec::new();
    for entry in SCAN_ROOTS {
        let path = root.join(entry);
        if path.exists() {
            scan_path(root, &path, &re, &mut markers)?;
        }
    }
    markers.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    Ok(markers)
}

/// Extracts markers from `text`, attributing them to `path`.
#[must_use]
pub fn scan_text(path: &str, text: &str) -> Vec<Marker> {
    let re = marker_regex();
    extract(path, text, &re)
}

fn marker_regex() -> Regex {
    // A constant pattern; failing to compile it is a programming error (M-PANIC-ON-BUG).
    #[expect(clippy::expect_used, reason = "pattern is a module constant")]
    Regex::new(MARKER_PATTERN).expect("valid marker regex")
}

fn scan_path(root: &Path, path: &Path, re: &Regex, out: &mut Vec<Marker>) -> Result<(), Failure> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        let mut children: Vec<PathBuf> = std::fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            scan_path(root, &child, re, out)?;
        }
        return Ok(());
    }
    if !eligible(path) {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    out.extend(extract(&paths::relative(root, path), &text, re));
    Ok(())
}

fn eligible(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name == "Makefile" {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext))
}

fn extract(path: &str, text: &str, re: &Regex) -> Vec<Marker> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for caps in re.captures_iter(line) {
            let kind = match &caps["kind"] {
                "Verifies" => MarkerKind::Verifies,
                _ => MarkerKind::Implements,
            };
            let ids = caps["ids"]
                .split(',')
                .map(|id| id.trim().to_owned())
                .collect();
            out.push(Marker {
                kind,
                ids,
                path: path.to_owned(),
                line: index + 1,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_both_kinds_and_lists() {
        let text = "/// Resolve.\n///\n/// Implements: FRN-SRS-031, FRN-SRS-032\nfn f() {}\n\n#[test]\nfn t() {\n    // Verifies: FRN-SRS-032\n}\n";
        let markers = scan_text("a.rs", text);
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].kind, MarkerKind::Implements);
        assert_eq!(markers[0].ids, vec!["FRN-SRS-031", "FRN-SRS-032"]);
        assert_eq!(markers[0].line, 3);
        assert_eq!(markers[1].kind, MarkerKind::Verifies);
        assert_eq!(markers[1].line, 8);
    }

    #[test]
    fn captures_malformed_ids_for_later_rejection() {
        let markers = scan_text(
            "a.rs",
            "// Verifies: FRN-SRS-12\n// Verifies: FRN-NEED-001\n",
        );
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].ids, vec!["FRN-SRS-12"]);
    }

    #[test]
    fn ignores_text_without_marker_keyword() {
        assert!(scan_text("a.rs", "id = \"FRN-SRS-001\"\n").is_empty());
    }
}
