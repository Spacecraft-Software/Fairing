// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask manifest`: the release manifest (Steelbore Standard §25.3).
//!
//! The manifest is assembled from what the tree already records and what the
//! release build produced: identity from `Cargo.toml` and the commit; the
//! archive and SBOM with their checksums, which must match what the three
//! package definitions carry; toolchain and flake pins; the requirement
//! counts; coverage and mutation figures when their reports are given; the
//! budgets with their margins. The maintainer writes the rest, the
//! validation note, the known issues and the conformance claim, in
//! `doc/releases/<version>.toml`. The result is
//! `doc/releases/fairing-<version>.manifest.json`, committed with the
//! stamped package definitions at the release tag and published with the
//! release; `.gitattributes` keeps the directory out of the archive.
//!
//! - `cargo xtask manifest [--coverage <json>] [--mutants <json>,...]`
//!   writes it from `target/dist`, after `cargo xtask dist` and `sbom`.
//! - `cargo xtask manifest --check` fails unless the committed manifest
//!   names the version and the artefacts `target/dist` holds now: the
//!   release workflow's proof that the tag publishes what it recorded.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::args::Args;
use crate::budgets;
use crate::coverage;
use crate::dist::{self, Checksum};
use crate::mutants;
use crate::paths;
use crate::report::{Envelope, Failure, now_utc};
use crate::requirements::{self, model::Status};
use crate::sbom;

/// What the maintainer writes for a release.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    /// The assurance category (§19.2), with any raised subsystem.
    pub category: String,
    /// What was exercised at G3, on what platform, and what was found (§21.5).
    pub validation: String,
    /// The §19.6 conformance claim.
    pub conformance: String,
    /// Open anomalies (§25.2).
    #[serde(default)]
    pub known_issue: Vec<KnownIssue>,
}

/// One open anomaly.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KnownIssue {
    /// Its identifier in the issue tracker.
    pub id: String,
    /// `S1` to `S4`.
    pub severity: String,
    /// What it is and the workaround.
    pub summary: String,
}

/// One shipped file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Artefact {
    /// Its file name, as published.
    pub name: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// Its SHA-256, hexadecimal.
    pub sha256: String,
}

/// The manifest's identity section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Identity {
    /// `Fairing`.
    pub project: String,
    /// The version released.
    pub version: String,
    /// When the manifest was assembled, ISO 8601 UTC.
    pub released: String,
    /// The signed tag the release is cut from.
    pub tag: String,
    /// The commit the archive holds; the tag's commit adds only the
    /// stamped definitions and this manifest to it.
    pub source_commit: String,
    /// The assurance category.
    pub category: String,
}

/// The manifest's provenance section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Provenance {
    /// The SBOM's file name.
    pub sbom: String,
    /// The tool that generated it.
    pub sbom_tool: String,
    /// The `rust-toolchain.toml` channel.
    pub toolchain: String,
    /// The nixpkgs revision `flake.lock` pins.
    pub nixpkgs: String,
    /// The release profile's flags (§3.2.1).
    pub release_profile: BTreeMap<String, String>,
    /// How to rebuild, and whether the build is bit-reproducible.
    pub reproducibility: String,
}

/// Mutation-testing figures, summed over the reports given.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Mutation {
    /// Mutants the tests failed on.
    pub caught: u64,
    /// Mutants that hung the tests.
    pub timeout: u64,
    /// Mutants that did not build.
    pub unviable: u64,
    /// Survivors, every one triaged in `doc/mutants.toml`.
    pub missed: u64,
    /// `(caught + timeout) / (caught + timeout + missed)`, in percent.
    pub score: f64,
}

/// The manifest's verification section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Verification {
    /// Requirements by status.
    pub requirements: BTreeMap<String, usize>,
    /// Where the traceability matrix is published.
    pub matrix: String,
    /// Line coverage per crate, in percent, if a report was given.
    pub coverage: Option<BTreeMap<String, f64>>,
    /// Mutation figures, if reports were given.
    pub mutation: Option<Mutation>,
}

/// A budget's line in the manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetLine {
    /// The budget's id.
    pub id: String,
    /// The §22.3 line, ceiling, figure and band.
    pub status: String,
    /// The band.
    pub band: String,
}

/// The release manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    /// Who and what.
    pub identity: Identity,
    /// Every shipped file.
    pub artefacts: Vec<Artefact>,
    /// How it was built.
    pub provenance: Provenance,
    /// The evidence.
    pub verification: Verification,
    /// The maintainer's G3 note.
    pub validation: String,
    /// Each budget and its margin.
    pub budgets: Vec<BudgetLine>,
    /// Open anomalies.
    pub known_issues: Vec<KnownIssue>,
    /// The §19.6 claim and where the tailoring register stands.
    pub conformance: String,
}

/// The manifest's path for `version`, relative to the root.
#[must_use]
pub fn manifest_path(version: &str) -> String {
    format!("{}/fairing-{version}.manifest.json", dist::RELEASES_DIR)
}

fn artefact(dir: &Path, name: &str) -> Result<Artefact, Failure> {
    let path = dir.join(name);
    let bytes = std::fs::metadata(&path)
        .map_err(|e| {
            Failure::not_found(
                format!("`{}`: {e}", path.display()),
                "run `cargo xtask dist` and `cargo xtask sbom` first",
            )
        })?
        .len();
    let printed = dist::run(dir, "sha256sum", &[name])?;
    let sha256 = printed
        .split_whitespace()
        .next()
        .and_then(Checksum::from_hex)
        .ok_or_else(|| Failure::internal(format!("`sha256sum` printed `{printed}`")))?;
    Ok(Artefact {
        name: name.to_owned(),
        bytes,
        sha256: sha256.hex,
    })
}

/// The archive and the SBOM in `out`, checked against the definitions.
///
/// # Errors
///
/// A missing file, or a package definition that does not carry the
/// archive's checksum.
pub fn artefacts(root: &Path, out: &Path, version: &str) -> Result<Vec<Artefact>, Failure> {
    let archive = artefact(out, &format!("fairing-{version}.tar.gz"))?;
    let sum = Checksum::from_hex(&archive.sha256)
        .ok_or_else(|| Failure::internal("the archive's checksum is not hexadecimal"))?;
    for name in dist::DEFINITIONS {
        let text = paths::read_text(&root.join(name), "cargo xtask dist")?;
        if dist::stamp(name, &text, version, &sum)? != text {
            return Err(Failure::gate(
                format!("`{name}` does not carry the checksum of the archive in target/dist"),
                "cargo xtask dist",
            ));
        }
    }
    let sbom = artefact(out, &format!("fairing-{version}.cdx.json"))?;
    Ok(vec![archive, sbom])
}

fn toolchain(root: &Path) -> Result<String, Failure> {
    let text = paths::read_text(&root.join("rust-toolchain.toml"), "rust-toolchain.toml")?;
    let value: toml::Value = toml::from_str(&text)?;
    Ok(value
        .get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(toml::Value::as_str)
        .unwrap_or("unpinned")
        .to_owned())
}

fn nixpkgs(root: &Path) -> Result<String, Failure> {
    let lock: serde_json::Value = serde_json::from_str(&paths::read_text(
        &root.join("flake.lock"),
        "nix flake lock",
    )?)?;
    Ok(lock
        .pointer("/nodes/nixpkgs/locked/rev")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unlocked")
        .to_owned())
}

fn release_profile(root: &Path) -> Result<BTreeMap<String, String>, Failure> {
    let manifest: toml::Value =
        toml::from_str(&paths::read_text(&root.join("Cargo.toml"), "Cargo.toml")?)?;
    Ok(manifest
        .get("profile")
        .and_then(|p| p.get("release"))
        .and_then(toml::Value::as_table)
        .map(|table| {
            table
                .iter()
                .map(|(key, value)| {
                    let text = value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned);
                    (key.clone(), text)
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Sums mutation reports; every survivor must be triaged.
///
/// # Errors
///
/// An unreadable report, a failed baseline, or an untriaged survivor.
pub fn mutation(root: &Path, reports: &[&str]) -> Result<Mutation, Failure> {
    let triage = mutants::load_triage(&root.join(mutants::TRIAGE_PATH))?;
    let mut sum = Mutation::default();
    for report in reports {
        let path = paths::resolve(root, report);
        let document: serde_json::Value =
            serde_json::from_str(&paths::read_text(&path, "cargo mutants")?)?;
        let outcomes = mutants::outcomes(&document)?;
        if !outcomes.baseline_passed || !mutants::untriaged(&outcomes, &triage).is_empty() {
            return Err(Failure::gate(
                format!("`{report}` has a failed baseline or an untriaged survivor"),
                "cargo xtask mutants --outcomes <report>",
            ));
        }
        sum.caught += outcomes.caught;
        sum.timeout += outcomes.timeout;
        sum.unviable += outcomes.unviable;
        sum.missed += u64::try_from(outcomes.missed.len()).unwrap_or(u64::MAX);
    }
    let detected = sum.caught + sum.timeout;
    sum.score = coverage::Lines {
        covered: detected,
        count: detected + sum.missed,
    }
    .percent();
    Ok(sum)
}

/// Assembles the manifest.
///
/// # Errors
///
/// Any input that is missing or does not check out.
pub fn assemble(
    root: &Path,
    out: &Path,
    coverage_report: Option<&str>,
    mutation_reports: &[&str],
) -> Result<Manifest, Failure> {
    let version = dist::version(root)?;
    let notes_path = root.join(format!("{}/{version}.toml", dist::RELEASES_DIR));
    let notes: Notes = toml::from_str(&paths::read_text(
        &notes_path,
        "write the release notes: category, validation, conformance, known issues",
    )?)?;
    let commit = dist::run(root, "git", &["rev-parse", "HEAD"])?;
    let set = requirements::load(&root.join(requirements::DEFAULT_PATH))?;
    let mut counts = BTreeMap::new();
    for requirement in &set.requirement {
        let status = match requirement.status {
            Status::Draft => "draft",
            Status::Baselined => "baselined",
            Status::Implemented => "implemented",
            Status::Verified => "verified",
            Status::Withdrawn => "withdrawn",
        };
        *counts.entry(status.to_owned()).or_insert(0) += 1;
    }
    let coverage = coverage_report
        .map(|report| -> Result<BTreeMap<String, f64>, Failure> {
            let document: serde_json::Value = serde_json::from_str(&paths::read_text(
                &paths::resolve(root, report),
                "cargo llvm-cov",
            )?)?;
            Ok(
                coverage::crates(&document, &root.to_string_lossy(), coverage::DEFAULT_MIN)?
                    .into_iter()
                    .map(|c| (c.name, (c.percent * 10.0).floor() / 10.0))
                    .collect(),
            )
        })
        .transpose()?;
    let mutation = if mutation_reports.is_empty() {
        None
    } else {
        Some(mutation(root, mutation_reports)?)
    };
    let (register, measurements) = budgets::load(root)?;
    let budgets = budgets::status(&register, &measurements, &BTreeMap::new())
        .into_iter()
        .map(|s| BudgetLine {
            band: s.band.as_str().to_owned(),
            status: s.line(),
            id: s.id,
        })
        .collect();
    Ok(Manifest {
        identity: Identity {
            project: set.meta.project.clone(),
            released: now_utc(),
            tag: format!("v{version}"),
            source_commit: commit.trim().to_owned(),
            category: notes.category,
            version: version.clone(),
        },
        artefacts: artefacts(root, out, &version)?,
        provenance: Provenance {
            sbom: format!("fairing-{version}.cdx.json"),
            sbom_tool: format!(
                "cargo-cyclonedx {}, CycloneDX {}",
                sbom::CYCLONEDX_VERSION,
                sbom::SPEC_VERSION
            ),
            toolchain: toolchain(root)?,
            nixpkgs: nixpkgs(root)?,
            release_profile: release_profile(root)?,
            reproducibility: "The source archive and the SBOM are bit-reproducible: \
                `cargo xtask dist --ref v<version>` and `cargo xtask sbom --ref v<version>` \
                rebuild them byte for byte, and the release workflow does so before it \
                publishes. Binaries are built by each package manager from the archive \
                with the pinned toolchain and are not claimed bit-reproducible."
                .to_owned(),
        },
        verification: Verification {
            requirements: counts,
            matrix: "matrix.json and matrix.md, published with the release \
                (`cargo xtask trace`)"
                .to_owned(),
            coverage,
            mutation,
        },
        validation: notes.validation,
        budgets,
        known_issues: notes.known_issue,
        conformance: notes.conformance,
    })
}

/// `cargo xtask manifest`.
///
/// # Errors
///
/// See [`assemble`]; under `--check`, a committed manifest that names other
/// artefacts.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["coverage", "mutants", "out", "check"])?;
    let root = args.root();
    let out = paths::resolve(&root, args.value("out").unwrap_or(dist::DEFAULT_OUT));
    let version = dist::version(&root)?;
    let path = root.join(manifest_path(&version));
    if args.flag("check") {
        let committed: Manifest =
            serde_json::from_str(&paths::read_text(&path, "cargo xtask manifest")?)?;
        let now = artefacts(&root, &out, &version)?;
        if committed.identity.version != version || committed.artefacts != now {
            return Err(Failure::gate(
                format!(
                    "`{}` does not name the artefacts in target/dist",
                    manifest_path(&version)
                ),
                "cargo xtask dist && cargo xtask sbom && cargo xtask manifest",
            ));
        }
        if args.json() {
            Envelope::new(&args.command_line(), &committed).print()?;
        } else {
            println!("[OK] {} matches target/dist", manifest_path(&version));
        }
        return Ok(());
    }
    let reports: Vec<&str> = args
        .value("mutants")
        .map(|list| list.split(',').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let manifest = assemble(&root, &out, args.value("coverage"), &reports)?;
    let mut text = serde_json::to_string_pretty(&manifest)?;
    text.push('\n');
    std::fs::write(&path, text)?;
    if args.json() {
        Envelope::new(&args.command_line(), &manifest).print()?;
    } else {
        println!("[OK] {} written", manifest_path(&version));
        for artefact in &manifest.artefacts {
            println!("[OK] {} {}", artefact.sha256, artefact.name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_notes_parse() {
        let root = crate::paths::default_root();
        let version = dist::version(&root).unwrap_or_else(|e| panic!("{e}"));
        let path = root.join(format!("{}/{version}.toml", dist::RELEASES_DIR));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let notes: Notes = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert!(!notes.category.is_empty());
        assert!(
            notes
                .known_issue
                .iter()
                .all(|i| matches!(i.severity.as_str(), "S1" | "S2" | "S3" | "S4")),
            "severities are S1 to S4"
        );
    }

    #[test]
    fn the_manifest_lives_beside_the_notes() {
        assert_eq!(
            manifest_path("0.1.0"),
            "doc/releases/fairing-0.1.0.manifest.json"
        );
    }
}
