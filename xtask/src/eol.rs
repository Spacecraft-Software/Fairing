// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask check-eol`: the Steelbore Standard §6.5 text-file gate.
//!
//! Reads the **index** through `git ls-files --eol`, as §6.5 requires, so a
//! path pinned `eol=crlf` (stored LF, checked out CRLF) is not a false positive.
//! `i/crlf` and `i/mixed` fail. Paths declared binary (`-text`) and symbolic
//! links are skipped. Every remaining text file must have no UTF-8 BOM and must
//! end with a newline.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::args::Args;
use crate::report::{Envelope, Failure, Finding, print_findings};

/// Result of the gate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EolReport {
    /// Tracked text files examined.
    pub checked: usize,
    /// Violations.
    pub findings: Vec<Finding>,
}

/// Runs the verb.
///
/// # Errors
///
/// Usage failure for unknown flags; gate failure when any file violates §6.5.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&[])?;
    let root = args.root();
    let report = check(&root)?;
    if args.json() {
        Envelope::new(&args.command_line(), &report).print()?;
    } else {
        println!(
            "[{}] checked {} tracked text file(s)",
            if report.findings.is_empty() {
                "OK"
            } else {
                "ERROR"
            },
            report.checked
        );
        print_findings("ERROR", &report.findings);
    }
    if report.findings.is_empty() {
        Ok(())
    } else {
        Err(Failure::gate(
            format!(
                "{} file(s) violate the §6.5 text-file rules",
                report.findings.len()
            ),
            "git ls-files --eol | rg 'i/(crlf|mixed)'",
        ))
    }
}

/// Checks every tracked file under `root`.
///
/// # Errors
///
/// Returns an internal failure if `git` cannot be run.
pub fn check(root: &Path) -> Result<EolReport, Failure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--eol", "-z"])
        .output()?;
    if !output.status.success() {
        return Err(Failure::internal(format!(
            "`git ls-files --eol` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    let mut checked = 0;
    let mut findings = Vec::new();
    for entry in listing.split('\0').filter(|e| !e.is_empty()) {
        let Some((info, rel)) = entry.split_once('\t') else {
            continue;
        };
        let index_eol = info
            .split_whitespace()
            .find_map(|t| t.strip_prefix("i/"))
            .unwrap_or("");
        let declared_binary = info.contains("-text");
        let path = root.join(rel);
        let is_symlink = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
        if declared_binary || is_symlink || index_eol.is_empty() || index_eol == "-text" {
            continue;
        }
        checked += 1;
        match index_eol {
            "crlf" | "mixed" => {
                findings.push(
                    Finding::new(
                        "STORED_CRLF",
                        None,
                        format!("stored with `i/{index_eol}` line endings"),
                    )
                    .at(rel, 1),
                );
                continue;
            }
            _ => {}
        }
        if let Ok(bytes) = std::fs::read(&path) {
            findings.extend(check_bytes(rel, &bytes));
        }
    }
    Ok(EolReport { checked, findings })
}

/// Byte-level checks on one text file: BOM and final newline.
#[must_use]
pub fn check_bytes(rel: &str, bytes: &[u8]) -> Vec<Finding> {
    let mut findings = Vec::new();
    if bytes.is_empty() {
        return findings;
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        findings.push(
            Finding::new("UTF8_BOM", None, "file starts with a UTF-8 byte-order mark").at(rel, 1),
        );
    }
    if bytes.last() != Some(&b'\n') {
        let line = bytes.split(|b| *b == b'\n').count();
        findings.push(
            Finding::new("NO_FINAL_NEWLINE", None, "file does not end with a newline")
                .at(rel, line),
        );
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_and_missing_newline_are_findings() {
        let codes: Vec<String> = check_bytes("f", &[0xEF, 0xBB, 0xBF, b'a'])
            .into_iter()
            .map(|f| f.code)
            .collect();
        assert_eq!(codes, vec!["UTF8_BOM", "NO_FINAL_NEWLINE"]);
        assert!(check_bytes("f", b"ok\n").is_empty());
        assert!(check_bytes("f", b"").is_empty());
    }
}
