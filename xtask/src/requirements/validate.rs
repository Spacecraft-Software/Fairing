// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Structural validation (errors) and wording lint (warnings) for a requirement set.
//!
//! Validation enforces what §20.3 and §20.4 make mechanical: identifier shape
//! and uniqueness, resolvable references, every need covered, and Texinfo node
//! names that will build. The lint flags §20.5 wording problems but never fails
//! the gate, because wording is the maintainer's call at G1.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use super::model::RequirementSet;
use crate::report::Finding;

/// Longest title the generated `@subheading` lines accept.
pub const MAX_TITLE_CHARS: usize = 60;

/// Characters Texinfo node names must not contain.
const NODE_FORBIDDEN: &[char] = &[',', ':', '.', '(', ')', '@'];

/// §20.5 words that hide an unmeasurable or open-ended obligation.
const WEASEL_WORDS: &[&str] = &[
    "fast",
    "robust",
    "efficient",
    "user-friendly",
    "secure",
    "modern",
    "seamless",
    "etc.",
    "and so on",
    "as appropriate",
    "including but not limited to",
    "where possible",
    "if practical",
    "as far as reasonable",
];

/// Returns every structural error in `set`; an empty vector means the set is valid.
#[must_use]
pub fn validate(set: &RequirementSet) -> Vec<Finding> {
    let prefix = regex::escape(&set.meta.prefix);
    let patterns = Patterns {
        need: compile(&format!("^{prefix}-NEED-[0-9]{{3}}$")),
        srs: compile(&format!("^{prefix}-SRS-[0-9]{{3}}$")),
        milestone: compile("^M[0-9]+$"),
    };
    let mut findings = Vec::new();
    let mut seen_ids = BTreeSet::new();
    validate_groups(set, &mut findings);
    validate_needs(set, &patterns, &mut seen_ids, &mut findings);
    let coverage = validate_requirements(set, &patterns, &mut seen_ids, &mut findings);
    for (need, count) in &coverage {
        if *count == 0 {
            findings.push(Finding::new(
                "UNCOVERED_NEED",
                Some(need),
                "no requirement traces to this need (§20.4 set completeness)",
            ));
        }
    }
    findings
}

struct Patterns {
    need: Regex,
    srs: Regex,
    milestone: Regex,
}

fn validate_groups(set: &RequirementSet, findings: &mut Vec<Finding>) {
    let mut seen_nodes = BTreeSet::new();
    for group in &set.group {
        if group.node.chars().any(|c| NODE_FORBIDDEN.contains(&c)) {
            findings.push(Finding::new(
                "BAD_NODE_NAME",
                Some(&group.id),
                format!("node name `{}` contains one of `, : . ( ) @`", group.node),
            ));
        }
        if !seen_nodes.insert(group.node.as_str()) {
            findings.push(Finding::new(
                "DUPLICATE_NODE",
                Some(&group.id),
                format!("node name `{}` is used by another group", group.node),
            ));
        }
    }
}

fn validate_needs<'a>(
    set: &'a RequirementSet,
    patterns: &Patterns,
    seen_ids: &mut BTreeSet<&'a str>,
    findings: &mut Vec<Finding>,
) {
    for need in &set.need {
        if !patterns.need.is_match(&need.id) {
            findings.push(Finding::new(
                "BAD_ID",
                Some(&need.id),
                format!("identifier does not match `{}-NEED-NNN`", set.meta.prefix),
            ));
        }
        if !seen_ids.insert(need.id.as_str()) {
            findings.push(Finding::new(
                "DUPLICATE_ID",
                Some(&need.id),
                "identifier is reused",
            ));
        }
    }
}

/// Validates every requirement and returns how many requirements cite each need.
fn validate_requirements<'a>(
    set: &'a RequirementSet,
    patterns: &Patterns,
    seen_ids: &mut BTreeSet<&'a str>,
    findings: &mut Vec<Finding>,
) -> BTreeMap<&'a str, usize> {
    let groups: BTreeSet<&str> = set.group.iter().map(|g| g.id.as_str()).collect();
    let mut coverage: BTreeMap<&str, usize> = set.need.iter().map(|n| (n.id.as_str(), 0)).collect();
    for req in &set.requirement {
        if !patterns.srs.is_match(&req.id) {
            findings.push(Finding::new(
                "BAD_ID",
                Some(&req.id),
                format!("identifier does not match `{}-SRS-NNN`", set.meta.prefix),
            ));
        }
        if !seen_ids.insert(req.id.as_str()) {
            findings.push(Finding::new(
                "DUPLICATE_ID",
                Some(&req.id),
                "identifier is reused",
            ));
        }
        if !groups.contains(req.group.as_str()) {
            findings.push(Finding::new(
                "UNKNOWN_GROUP",
                Some(&req.id),
                format!("group `{}` is not declared", req.group),
            ));
        }
        if !patterns.milestone.is_match(&req.milestone) {
            findings.push(Finding::new(
                "BAD_MILESTONE",
                Some(&req.id),
                format!("milestone `{}` does not match `M<n>`", req.milestone),
            ));
        }
        validate_wording(req, findings);
        for source in &req.source {
            if patterns.need.is_match(source) {
                match coverage.get_mut(source.as_str()) {
                    Some(count) => *count += 1,
                    None => findings.push(Finding::new(
                        "UNKNOWN_NEED",
                        Some(&req.id),
                        format!("source `{source}` is not a declared need"),
                    )),
                }
            }
        }
    }
    coverage
}

fn validate_wording(req: &super::model::Requirement, findings: &mut Vec<Finding>) {
    if req.title.chars().count() > MAX_TITLE_CHARS {
        findings.push(Finding::new(
            "TITLE_TOO_LONG",
            Some(&req.id),
            format!("title exceeds {MAX_TITLE_CHARS} characters"),
        ));
    }
    if !req.text.trim_end().ends_with('.') {
        findings.push(Finding::new(
            "TEXT_NOT_SENTENCE",
            Some(&req.id),
            "requirement text does not end with a full stop",
        ));
    }
    if !req
        .text
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word == "shall")
    {
        findings.push(Finding::new(
            "MISSING_SHALL",
            Some(&req.id),
            "requirement text carries no `shall` (§20.2)",
        ));
    }
}

/// Returns §20.5 wording warnings; never fails the gate.
#[must_use]
pub fn lint(set: &RequirementSet) -> Vec<Finding> {
    let mut findings = Vec::new();
    for req in &set.requirement {
        let lower = req.text.to_lowercase();
        for word in WEASEL_WORDS {
            if contains_word(&lower, word) {
                findings.push(Finding::new(
                    "WEASEL_WORD",
                    Some(&req.id),
                    format!("requirement text contains `{word}` (§20.5)"),
                ));
            }
        }
    }
    findings
}

/// Word-boundary containment for a lowercase phrase.
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = start == 0
            || !haystack[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let after_ok = end == haystack.len()
            || !haystack[end..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        before_ok && after_ok
    })
}

/// Compiles a regex that is a compile-time constant of this module.
fn compile(pattern: &str) -> Regex {
    // The patterns are built from a validated prefix and fixed text; a failure
    // here is a programming error, so stopping the program is correct
    // (M-PANIC-ON-BUG).
    #[expect(clippy::expect_used, reason = "pattern is a module constant")]
    Regex::new(pattern).expect("valid regex")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::requirements::model::{
        Group, Meta, Need, Priority, Requirement, Status, Verification,
    };

    fn sample() -> RequirementSet {
        RequirementSet {
            meta: Meta {
                project: "Fairing".into(),
                prefix: "FRN".into(),
                standard: "2.12".into(),
                category: "B".into(),
                updated: "2026-10-06".into(),
                needs_preamble: String::new(),
                requirements_preamble: String::new(),
            },
            group: vec![Group {
                id: "display".into(),
                node: "Display Requirements".into(),
                title: "Display".into(),
                intro: String::new(),
            }],
            need: vec![Need {
                id: "FRN-NEED-001".into(),
                title: "Splash".into(),
                text: "A splash.".into(),
                who: "Maintainer".into(),
            }],
            requirement: vec![Requirement {
                id: "FRN-SRS-001".into(),
                group: "display".into(),
                milestone: "M1".into(),
                title: "Draws through KMS".into(),
                text: "The renderer shall draw through DRM/KMS.".into(),
                rationale: "KMS works before fbcon.".into(),
                source: vec!["FRN-NEED-001".into(), "decision 2".into()],
                priority: Priority::Mandatory,
                verification: Verification::Test,
                status: Status::Draft,
                category: None,
                notes: None,
            }],
        }
    }

    #[test]
    fn valid_sample_has_no_findings() {
        assert!(validate(&sample()).is_empty());
    }

    #[test]
    fn detects_bad_id_unknown_group_and_uncovered_need() {
        let mut set = sample();
        set.requirement[0].id = "FRN-SRS-12".into();
        set.requirement[0].group = "nope".into();
        set.requirement[0].source = vec!["decision 2".into()];
        let findings = validate(&set);
        let codes: Vec<&str> = findings.iter().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"BAD_ID"));
        assert!(codes.contains(&"UNKNOWN_GROUP"));
        assert!(codes.contains(&"UNCOVERED_NEED"));
    }

    #[test]
    fn detects_duplicate_ids_and_bad_node_names() {
        let mut set = sample();
        set.requirement.push(set.requirement[0].clone());
        set.group[0].node = "Display: stuff".into();
        let findings = validate(&set);
        let codes: Vec<&str> = findings.iter().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"DUPLICATE_ID"));
        assert!(codes.contains(&"BAD_NODE_NAME"));
    }

    #[test]
    fn lint_flags_weasel_words_on_word_boundaries() {
        let mut set = sample();
        set.requirement[0].text = "The renderer shall be fast and robust.".into();
        assert_eq!(lint(&set).len(), 2);
        set.requirement[0].text = "The renderer shall use fastboot.".into();
        assert!(lint(&set).is_empty());
    }
}
