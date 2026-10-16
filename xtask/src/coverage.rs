// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask coverage`: the line-coverage floor of every product crate
//! (Steelbore Standard §21).
//!
//! It reads the JSON summary `cargo llvm-cov --json --summary-only` writes
//! and fails any crate under `crates/` whose covered lines fall below the
//! floor, 80 % unless `--min` says otherwise. The files that only run against
//! hardware are left out by name, each with its reason, in [`HARDWARE_ONLY`];
//! nothing else is. A listed file the report does not contain fails the gate
//! too, so the list cannot go stale.
//!
//! `cargo xtask coverage [--report <json>] [--min <percent>]`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure};

/// Where CI writes the llvm-cov summary, relative to the root.
pub const DEFAULT_REPORT: &str = "target/llvm-cov.json";

/// The floor, in percent of lines.
pub const DEFAULT_MIN: f64 = 80.0;

/// Files left out of the floor: they only run against a device, in tests
/// that are `#[ignore]`d and run on the reference machine.
pub const HARDWARE_ONLY: &[(&str, &str)] = &[(
    "crates/fairing-render/src/backend/drm.rs",
    "drives a DRM/KMS device: mode setting, dumb buffers and page flips need a card",
)];

/// One crate's lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Lines {
    /// Lines run by the tests.
    pub covered: u64,
    /// Lines counted.
    pub count: u64,
}

impl Lines {
    /// Covered lines in percent, 100 for a crate with no lines.
    #[must_use]
    pub fn percent(&self) -> f64 {
        if self.count == 0 {
            return 100.0;
        }
        #[expect(clippy::cast_precision_loss, reason = "line counts are far below 2^52")]
        let ratio = self.covered as f64 / self.count as f64;
        ratio * 100.0
    }
}

/// A crate's verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Crate {
    /// The directory name under `crates/`.
    pub name: String,
    /// Its lines, hardware-only files left out.
    pub lines: Lines,
    /// Covered lines in percent.
    pub percent: f64,
    /// Whether it meets the floor.
    pub ok: bool,
}

/// Sums the report's files into crates, leaving out [`HARDWARE_ONLY`].
///
/// # Errors
///
/// A report that is not llvm-cov's JSON export, or a listed file it does not
/// contain.
pub fn crates(report: &serde_json::Value, root: &str, min: f64) -> Result<Vec<Crate>, Failure> {
    let files = report
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(|d| d.get("files"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Failure::internal("the report has no `data[0].files`"))?;
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mut sums: BTreeMap<String, Lines> = BTreeMap::new();
    let mut seen = Vec::new();
    for file in files {
        let name = file
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Failure::internal("a report file has no `filename`"))?;
        let name = name.strip_prefix(&prefix).unwrap_or(name);
        if HARDWARE_ONLY.iter().any(|(path, _)| *path == name) {
            seen.push(name.to_owned());
            continue;
        }
        let Some(krate) = name
            .strip_prefix("crates/")
            .and_then(|rest| rest.split('/').next())
        else {
            continue;
        };
        let lines = file
            .get("summary")
            .and_then(|s| s.get("lines"))
            .ok_or_else(|| Failure::internal(format!("`{name}` has no line summary")))?;
        let field = |key: &str| {
            lines
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        let sum = sums.entry(krate.to_owned()).or_default();
        sum.covered += field("covered");
        sum.count += field("count");
    }
    if let Some((missing, _)) = HARDWARE_ONLY
        .iter()
        .find(|(path, _)| !seen.iter().any(|s| s == path))
    {
        return Err(Failure::gate(
            format!("`{missing}` is listed as hardware-only but the report has no such file"),
            "update HARDWARE_ONLY in xtask/src/coverage.rs",
        ));
    }
    Ok(sums
        .into_iter()
        .map(|(name, lines)| {
            let percent = lines.percent();
            Crate {
                name,
                ok: percent >= min,
                percent,
                lines,
            }
        })
        .collect())
}

#[derive(Debug, Serialize)]
struct Output {
    min: f64,
    excluded: Vec<&'static str>,
    crates: Vec<Crate>,
}

/// `cargo xtask coverage`.
///
/// # Errors
///
/// An unreadable report, a bad `--min`, or a crate under the floor.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["report", "min"])?;
    let root = args.root();
    let min = match args.value("min") {
        None => DEFAULT_MIN,
        Some(value) => value
            .parse::<f64>()
            .ok()
            .filter(|m| (0.0..=100.0).contains(m))
            .ok_or_else(|| {
                Failure::usage(
                    format!("`--min {value}` is not a percentage"),
                    "cargo xtask coverage --min 80",
                )
            })?,
    };
    let path = paths::resolve(&root, args.value("report").unwrap_or(DEFAULT_REPORT));
    let report: serde_json::Value = serde_json::from_str(&paths::read_text(
        &path,
        "cargo llvm-cov --workspace --exclude xtask --json --summary-only --output-path target/llvm-cov.json",
    )?)?;
    let crates = crates(&report, &root.to_string_lossy(), min)?;
    let failing: Vec<String> = crates
        .iter()
        .filter(|c| !c.ok)
        .map(|c| c.name.clone())
        .collect();
    if args.json() {
        let output = Output {
            min,
            excluded: HARDWARE_ONLY.iter().map(|(path, _)| *path).collect(),
            crates,
        };
        Envelope::new(&args.command_line(), &output).print()?;
    } else {
        for (path, reason) in HARDWARE_ONLY {
            println!("[INFO] excluded {path}: {reason}");
        }
        for krate in &crates {
            println!(
                "{} COVERAGE: {} {:.1}% ({}/{} lines, floor {min:.0}%)",
                if krate.ok { "[OK]" } else { "[ERROR]" },
                krate.name,
                krate.percent,
                krate.lines.covered,
                krate.lines.count
            );
        }
    }
    if failing.is_empty() {
        Ok(())
    } else {
        Err(Failure::gate(
            format!(
                "crate(s) under the {min:.0}% line floor: {}",
                failing.join(", ")
            ),
            "cargo xtask coverage --json",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, covered: u64, count: u64) -> serde_json::Value {
        serde_json::json!({
            "filename": format!("/src/{name}"),
            "summary": { "lines": { "covered": covered, "count": count } }
        })
    }

    fn report(files: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({ "data": [ { "files": files } ] })
    }

    #[test]
    fn crates_are_summed_without_the_hardware_files() {
        let report = report(&[
            file("crates/fairing-render/src/frame.rs", 90, 100),
            file("crates/fairing-render/src/backend/drm.rs", 0, 400),
            file("crates/fairing/src/main.rs", 7, 10),
            file("xtask/src/lib.rs", 0, 50),
        ]);
        let crates = crates(&report, "/src", 80.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(crates.len(), 2);
        assert_eq!(crates[0].name, "fairing");
        assert!(!crates[0].ok);
        assert_eq!(crates[1].name, "fairing-render");
        assert_eq!(
            crates[1].lines,
            Lines {
                covered: 90,
                count: 100
            }
        );
        assert!(crates[1].ok);
    }

    #[test]
    fn a_stale_exclusion_fails_the_gate() {
        let report = report(&[file("crates/fairing/src/main.rs", 10, 10)]);
        assert!(crates(&report, "/src", 80.0).is_err());
        assert!(crates(&serde_json::json!({}), "/src", 80.0).is_err());
    }

    #[test]
    fn an_empty_crate_counts_as_covered() {
        assert!((Lines::default().percent() - 100.0).abs() < f64::EPSILON);
    }
}
