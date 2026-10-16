// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask mutants`: the mutation-testing triage gate (Steelbore
//! Standard §21).
//!
//! cargo-mutants writes `outcomes.json`; every mutant it reports as missed
//! (the tests still pass with it applied) must be named in
//! `doc/mutants.toml` with its verdict and the reason, or the gate fails. A
//! survivor is triaged once, by killing it with a test (and then it is not
//! listed) or by recording why no test can tell it apart. A mutant that
//! makes the tests time out is detected: the suite would hang, not pass.
//! A baseline that did not pass fails the gate, since then every mutant
//! looks caught.
//!
//! Survivors are matched by file, function and replacement, never by line,
//! so an edit elsewhere in the file does not stale the triage.
//!
//! `cargo xtask mutants --outcomes <outcomes.json> [--triage <toml>]`

use serde::{Deserialize, Serialize};

use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure};

/// The triage file, relative to the repository root.
pub const TRIAGE_PATH: &str = "doc/mutants.toml";

/// The triage file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triage {
    /// Every survivor that is not killed by a test.
    #[serde(default)]
    pub survivor: Vec<Survivor>,
}

/// Why a survivor stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The mutant behaves the same as the original for every input.
    Equivalent,
    /// The difference cannot be observed from a safe test: it shows only on
    /// a device no test has, or in memory already freed.
    Unobservable,
}

/// One triaged survivor.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Survivor {
    /// The file, relative to the root.
    pub file: String,
    /// The function the mutant is in.
    pub function: String,
    /// What cargo-mutants put in its place, empty for a deletion.
    #[serde(default)]
    pub replacement: String,
    /// Why it stays.
    pub verdict: Verdict,
    /// The argument for the verdict.
    pub reason: String,
}

/// A missed mutant from `outcomes.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Missed {
    /// cargo-mutants' name for it, with line and column.
    pub name: String,
    /// The file.
    pub file: String,
    /// The function, empty for a mutant outside one.
    pub function: String,
    /// The replacement.
    pub replacement: String,
}

impl Missed {
    fn matches(&self, survivor: &Survivor) -> bool {
        self.file == survivor.file
            && self.function == survivor.function
            && self.replacement == survivor.replacement
    }
}

/// The run's counts and its missed mutants.
#[derive(Debug, Clone, Serialize)]
pub struct Outcomes {
    /// Whether the unmutated tree passed its tests.
    pub baseline_passed: bool,
    /// Mutants the tests failed on.
    pub caught: u64,
    /// Mutants that hung the tests.
    pub timeout: u64,
    /// Mutants that did not build.
    pub unviable: u64,
    /// Mutants the tests passed with.
    pub missed: Vec<Missed>,
}

/// Reads cargo-mutants' `outcomes.json`.
///
/// # Errors
///
/// A document that is not cargo-mutants' outcomes.
pub fn outcomes(value: &serde_json::Value) -> Result<Outcomes, Failure> {
    let list = value
        .get("outcomes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Failure::internal("the document has no `outcomes`"))?;
    let count = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let text = |v: Option<&serde_json::Value>| {
        v.and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut baseline = None;
    let mut missed = Vec::new();
    for outcome in list {
        let summary = outcome.get("summary").and_then(serde_json::Value::as_str);
        let scenario = outcome.get("scenario");
        if scenario.and_then(serde_json::Value::as_str) == Some("Baseline") {
            baseline = Some(summary == Some("Success"));
            continue;
        }
        if summary != Some("MissedMutant") {
            continue;
        }
        let mutant = scenario
            .and_then(|s| s.get("Mutant"))
            .ok_or_else(|| Failure::internal("a missed outcome names no mutant"))?;
        missed.push(Missed {
            name: text(mutant.get("name")),
            file: text(mutant.get("file")),
            function: text(mutant.get("function").and_then(|f| f.get("function_name"))),
            replacement: text(mutant.get("replacement")),
        });
    }
    Ok(Outcomes {
        // A run with `--baseline skip` has no baseline outcome; one that
        // ran it and failed has stopped before any mutant.
        baseline_passed: baseline.unwrap_or(true),
        caught: count("caught"),
        timeout: count("timeout"),
        unviable: count("unviable"),
        missed,
    })
}

/// The missed mutants the triage does not name.
#[must_use]
pub fn untriaged<'a>(outcomes: &'a Outcomes, triage: &Triage) -> Vec<&'a Missed> {
    outcomes
        .missed
        .iter()
        .filter(|m| !triage.survivor.iter().any(|s| m.matches(s)))
        .collect()
}

/// Reads and checks the triage file: every entry names its file and
/// function and has a reason. The replacement may be empty: cargo-mutants
/// writes `""` for a mutant that deletes code, such as `delete !`.
///
/// # Errors
///
/// An unreadable file, or an entry with an empty field.
pub fn load_triage(path: &std::path::Path) -> Result<Triage, Failure> {
    let triage: Triage = toml::from_str(&paths::read_text(path, "cargo xtask mutants")?)?;
    if let Some(bad) = triage.survivor.iter().find(|s| {
        [&s.file, &s.function, &s.reason]
            .iter()
            .any(|f| f.trim().is_empty())
    }) {
        return Err(Failure::gate(
            format!(
                "a survivor in `{}` has an empty field: {} in {}",
                path.display(),
                bad.replacement,
                bad.file
            ),
            "every survivor names file, function and reason",
        ));
    }
    Ok(triage)
}

#[derive(Debug, Serialize)]
struct Output<'a> {
    outcomes: &'a Outcomes,
    untriaged: Vec<&'a Missed>,
}

/// `cargo xtask mutants`.
///
/// # Errors
///
/// An unreadable file, a failed baseline, or an untriaged survivor.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["outcomes", "triage"])?;
    let root = args.root();
    let Some(path) = args.value("outcomes") else {
        return Err(Failure::usage(
            "`--outcomes <outcomes.json>` is required",
            "cargo xtask mutants --outcomes mutants.out/outcomes.json",
        ));
    };
    let path = paths::resolve(&root, path);
    let document: serde_json::Value =
        serde_json::from_str(&paths::read_text(&path, "cargo mutants")?)?;
    let outcomes = outcomes(&document)?;
    let triage = load_triage(&paths::resolve(
        &root,
        args.value("triage").unwrap_or(TRIAGE_PATH),
    ))?;
    let untriaged = untriaged(&outcomes, &triage);
    let failed: Vec<String> = untriaged.iter().map(|m| m.name.clone()).collect();
    if args.json() {
        Envelope::new(
            &args.command_line(),
            Output {
                outcomes: &outcomes,
                untriaged,
            },
        )
        .print()?;
    } else {
        println!(
            "[INFO] MUTANTS: {} caught, {} timed out, {} unviable, {} missed",
            outcomes.caught,
            outcomes.timeout,
            outcomes.unviable,
            outcomes.missed.len()
        );
        for name in &failed {
            println!("[ERROR] untriaged survivor: {name}");
        }
    }
    if !outcomes.baseline_passed {
        return Err(Failure::gate(
            "the unmutated baseline failed its tests",
            "cargo test --workspace --locked",
        ));
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Failure::gate(
            format!(
                "{} survivor(s) neither killed nor triaged in {TRIAGE_PATH}",
                failed.len()
            ),
            "add a test that kills it, or a [[survivor]] entry with its reason",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutant(summary: &str, function: &str) -> serde_json::Value {
        serde_json::json!({
            "scenario": { "Mutant": {
                "name": format!("crates/x/src/a.rs:3:5: replace {function} with ()"),
                "file": "crates/x/src/a.rs",
                "function": { "function_name": function },
                "replacement": "()",
            }},
            "summary": summary,
        })
    }

    fn document(baseline: &str, outcomes: &[serde_json::Value]) -> serde_json::Value {
        let mut list = vec![serde_json::json!({ "scenario": "Baseline", "summary": baseline })];
        list.extend(outcomes.iter().cloned());
        serde_json::json!({ "outcomes": list, "caught": 4, "timeout": 1, "unviable": 2 })
    }

    #[test]
    fn only_untriaged_survivors_fail() {
        let doc = document(
            "Success",
            &[
                mutant("CaughtMutant", "a"),
                mutant("MissedMutant", "b"),
                mutant("MissedMutant", "c"),
                mutant("Timeout", "d"),
            ],
        );
        let outcomes = outcomes(&doc).unwrap_or_else(|e| panic!("{e}"));
        assert!(outcomes.baseline_passed);
        assert_eq!(outcomes.caught, 4);
        assert_eq!(outcomes.missed.len(), 2);
        let triage: Triage = toml::from_str(
            r#"
            [[survivor]]
            file = "crates/x/src/a.rs"
            function = "b"
            replacement = "()"
            verdict = "equivalent"
            reason = "b has no effect a test can see"
            "#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let left = untriaged(&outcomes, &triage);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].function, "c");
    }

    #[test]
    fn a_failed_baseline_is_reported() {
        let outcomes = outcomes(&document("Failure", &[])).unwrap_or_else(|e| panic!("{e}"));
        assert!(!outcomes.baseline_passed);
        assert!(super::outcomes(&serde_json::json!({})).is_err());
    }

    #[test]
    fn a_deletion_is_triaged_with_an_empty_replacement() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("mutants.toml");
        let entry = |reason: &str| {
            format!(
                "[[survivor]]\nfile = \"a.rs\"\nfunction = \"f\"\nreplacement = \"\"\n\
                 verdict = \"unobservable\"\nreason = \"{reason}\"\n"
            )
        };
        std::fs::write(&path, entry("needs a device")).unwrap_or_else(|e| panic!("{e}"));
        assert!(load_triage(&path).is_ok());
        std::fs::write(&path, entry(" ")).unwrap_or_else(|e| panic!("{e}"));
        assert!(load_triage(&path).is_err());
    }

    #[test]
    fn the_committed_triage_is_well_formed() {
        let root = crate::paths::default_root();
        load_triage(&root.join(TRIAGE_PATH)).unwrap_or_else(|e| panic!("{e}"));
    }
}
