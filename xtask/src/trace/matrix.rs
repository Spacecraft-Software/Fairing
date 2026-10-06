// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Join markers against the requirement set and decide per-requirement verdicts.
//!
//! Verdict rules (§21.3):
//! - a marker citing an identifier absent from the set, a need, or a withdrawn
//!   requirement is **unknown** and always fails;
//! - a `baselined` or `implemented` requirement with no `Verifies:` marker is an
//!   **orphan** and fails;
//! - a `verified` requirement with no `Verifies:` marker is a **claim without
//!   evidence** and fails;
//! - `draft` requirements owe nothing yet, so a pre-G1 set with zero evidence
//!   passes while still catching every unknown identifier.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::markers::{Marker, MarkerKind};
use crate::requirements::{RequirementSet, Status};

/// Where a marker was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Repository-relative path.
    pub path: String,
    /// 1-based line.
    pub line: usize,
}

/// Verdict for one requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Not yet baselined; no evidence owed.
    Draft,
    /// Withdrawn; keeps its identifier, enters no count.
    Withdrawn,
    /// Baselined or implemented with at least one `Verifies:` marker.
    Evidenced,
    /// Verified with evidence.
    Verified,
    /// Baselined or implemented without evidence (fails).
    Orphan,
    /// Marked verified without evidence (fails).
    ClaimWithoutEvidence,
}

impl Verdict {
    /// Whether this verdict fails the gate.
    #[must_use]
    pub const fn fails(self) -> bool {
        matches!(self, Self::Orphan | Self::ClaimWithoutEvidence)
    }
}

/// One row of the forward matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// Requirement identifier.
    pub id: String,
    /// Short title.
    pub title: String,
    /// Group key.
    pub group: String,
    /// Milestone label.
    pub milestone: String,
    /// Priority name.
    pub priority: String,
    /// Verification method name.
    pub verification: String,
    /// Status name.
    pub status: String,
    /// Decided verdict.
    pub verdict: Verdict,
    /// `Implements:` locations.
    pub implements: Vec<Location>,
    /// `Verifies:` locations.
    pub verifies: Vec<Location>,
}

/// A marker whose identifier the set cannot accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownMarker {
    /// The cited identifier.
    pub id: String,
    /// Why it is rejected.
    pub reason: String,
    /// Where it was found.
    pub path: String,
    /// 1-based line.
    pub line: usize,
}

/// Per-milestone counts feeding §17.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneSummary {
    /// Mandatory + expected, not withdrawn.
    pub counted: usize,
    /// Of those, baselined / implemented / verified.
    pub baselined: usize,
    /// Of those, verified with evidence.
    pub verified: usize,
}

/// Whole-set counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Every requirement in the set.
    pub total: usize,
    /// Mandatory + expected, not withdrawn.
    pub counted: usize,
    /// Withdrawn requirements.
    pub withdrawn: usize,
    /// Baselined / implemented / verified requirements.
    pub baselined: usize,
    /// Verified with evidence.
    pub verified: usize,
    /// Orphans (fail).
    pub orphans: usize,
    /// Verified without evidence (fail).
    pub claims_without_evidence: usize,
    /// Unknown marker identifiers (fail).
    pub unknown: usize,
    /// Counts per milestone label.
    pub by_milestone: BTreeMap<String, MilestoneSummary>,
}

/// The traceability matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    /// Whether the gate passes.
    pub ok: bool,
    /// Counts.
    pub summary: Summary,
    /// Forward table, one row per requirement in set order.
    pub requirements: Vec<Row>,
    /// Rejected markers.
    pub unknown_markers: Vec<UnknownMarker>,
    /// Every marker found (backward table).
    pub markers: Vec<Marker>,
}

/// Builds the matrix for `set` from `markers`.
#[must_use]
pub fn build(set: &RequirementSet, markers: &[Marker]) -> Matrix {
    let mut rows: Vec<Row> = set
        .requirement
        .iter()
        .map(|req| Row {
            id: req.id.clone(),
            title: req.title.clone(),
            group: req.group.clone(),
            milestone: req.milestone.clone(),
            priority: req.priority.as_str().to_owned(),
            verification: req.verification.as_str().to_owned(),
            status: req.status.as_str().to_owned(),
            verdict: Verdict::Draft,
            implements: Vec::new(),
            verifies: Vec::new(),
        })
        .collect();
    let unknown_markers = join_markers(set, markers, &mut rows);
    let mut summary = summarize(set, &mut rows);
    summary.unknown = unknown_markers.len();
    let ok = summary.unknown == 0 && summary.orphans == 0 && summary.claims_without_evidence == 0;
    Matrix {
        ok,
        summary,
        requirements: rows,
        unknown_markers,
        markers: markers.to_vec(),
    }
}

/// Attaches each marker to its requirement row; returns the markers that cannot be accepted.
fn join_markers(set: &RequirementSet, markers: &[Marker], rows: &mut [Row]) -> Vec<UnknownMarker> {
    let index: BTreeMap<&str, usize> = set
        .requirement
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut unknown = Vec::new();
    for marker in markers {
        for id in &marker.ids {
            let location = Location {
                path: marker.path.clone(),
                line: marker.line,
            };
            let reject = |reason: &str| UnknownMarker {
                id: id.clone(),
                reason: reason.to_owned(),
                path: marker.path.clone(),
                line: marker.line,
            };
            match index.get(id.as_str()) {
                Some(&i) if set.requirement[i].status == Status::Withdrawn => {
                    unknown.push(reject("cites a withdrawn requirement"));
                }
                Some(&i) => match marker.kind {
                    MarkerKind::Verifies => rows[i].verifies.push(location),
                    MarkerKind::Implements => rows[i].implements.push(location),
                },
                None if set.need.iter().any(|n| &n.id == id) => {
                    unknown.push(reject(
                        "a need cannot be a verification target; cite its requirements",
                    ));
                }
                None => unknown.push(reject("identifier is not in the requirement set")),
            }
        }
    }
    unknown
}

/// Decides every verdict and accumulates the counts.
fn summarize(set: &RequirementSet, rows: &mut [Row]) -> Summary {
    let mut summary = Summary {
        total: set.requirement.len(),
        ..Summary::default()
    };
    for (row, req) in rows.iter_mut().zip(&set.requirement) {
        row.verdict = decide(req.status, !row.verifies.is_empty());
        let counted = req.is_counted();
        let entry = summary
            .by_milestone
            .entry(req.milestone.clone())
            .or_default();
        if counted {
            summary.counted += 1;
            entry.counted += 1;
        }
        if req.status == Status::Withdrawn {
            summary.withdrawn += 1;
        } else if req.status.is_baselined() {
            summary.baselined += 1;
            if counted {
                entry.baselined += 1;
            }
        }
        match row.verdict {
            Verdict::Verified => {
                summary.verified += 1;
                if counted {
                    entry.verified += 1;
                }
            }
            Verdict::Orphan => summary.orphans += 1,
            Verdict::ClaimWithoutEvidence => summary.claims_without_evidence += 1,
            Verdict::Draft | Verdict::Withdrawn | Verdict::Evidenced => {}
        }
    }
    summary
}

const fn decide(status: Status, has_evidence: bool) -> Verdict {
    match (status, has_evidence) {
        (Status::Draft, _) => Verdict::Draft,
        (Status::Withdrawn, _) => Verdict::Withdrawn,
        (Status::Baselined | Status::Implemented, true) => Verdict::Evidenced,
        (Status::Baselined | Status::Implemented, false) => Verdict::Orphan,
        (Status::Verified, true) => Verdict::Verified,
        (Status::Verified, false) => Verdict::ClaimWithoutEvidence,
    }
}

/// Renders the matrix as Markdown for the CI artifact.
#[must_use]
pub fn to_markdown(matrix: &Matrix, generated_at: &str) -> String {
    let s = &matrix.summary;
    let mut out = String::new();
    let _ = writeln!(out, "# Traceability matrix\n");
    let _ = writeln!(
        out,
        "Generated {generated_at} by `cargo xtask trace`. Gate: **{}**.\n",
        if matrix.ok { "pass" } else { "FAIL" }
    );
    let _ = writeln!(
        out,
        "- requirements: {} total, {} counted, {} withdrawn, {} baselined, {} verified\n- failures: {} orphan(s), {} claim(s) without evidence, {} unknown marker(s)\n",
        s.total,
        s.counted,
        s.withdrawn,
        s.baselined,
        s.verified,
        s.orphans,
        s.claims_without_evidence,
        s.unknown
    );
    let _ = writeln!(
        out,
        "## Forward\n\n| ID | Title | Priority | Method | Status | Milestone | Implements | Verifies | Verdict |\n|---|---|---|---|---|---|---|---|---|"
    );
    for row in &matrix.requirements {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:?} |",
            row.id,
            row.title,
            row.priority,
            row.verification,
            row.status,
            row.milestone,
            locations(&row.implements),
            locations(&row.verifies),
            row.verdict
        );
    }
    let _ = writeln!(
        out,
        "\n## Backward\n\n| Location | Kind | Identifiers |\n|---|---|---|"
    );
    for marker in &matrix.markers {
        let _ = writeln!(
            out,
            "| {}:{} | {:?} | {} |",
            marker.path,
            marker.line,
            marker.kind,
            marker.ids.join(", ")
        );
    }
    if !matrix.unknown_markers.is_empty() {
        let _ = writeln!(out, "\n## Unknown markers\n");
        for unknown in &matrix.unknown_markers {
            let _ = writeln!(
                out,
                "- `{}` at {}:{} — {}",
                unknown.id, unknown.path, unknown.line, unknown.reason
            );
        }
    }
    out
}

fn locations(list: &[Location]) -> String {
    if list.is_empty() {
        return "—".to_owned();
    }
    list.iter()
        .map(|l| format!("{}:{}", l.path, l.line))
        .collect::<Vec<_>>()
        .join("<br>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::requirements::model::{Group, Meta, Need, Priority, Requirement, Verification};

    fn req(id: &str, status: Status) -> Requirement {
        Requirement {
            id: id.into(),
            group: "g".into(),
            milestone: "M1".into(),
            title: "t".into(),
            text: "The x shall y.".into(),
            rationale: "r".into(),
            source: vec!["FRN-NEED-001".into()],
            priority: Priority::Mandatory,
            verification: Verification::Test,
            status,
            category: None,
            notes: None,
        }
    }

    fn set(reqs: Vec<Requirement>) -> RequirementSet {
        RequirementSet {
            meta: Meta {
                project: "P".into(),
                prefix: "FRN".into(),
                standard: "2.12".into(),
                category: "B".into(),
                updated: "2026-10-06".into(),
                needs_preamble: String::new(),
                requirements_preamble: String::new(),
            },
            group: vec![Group {
                id: "g".into(),
                node: "G".into(),
                title: "G".into(),
                intro: String::new(),
            }],
            need: vec![Need {
                id: "FRN-NEED-001".into(),
                title: "n".into(),
                text: "n.".into(),
                who: String::new(),
            }],
            requirement: reqs,
        }
    }

    fn marker(kind: MarkerKind, id: &str) -> Marker {
        Marker {
            kind,
            ids: vec![id.into()],
            path: "a.rs".into(),
            line: 1,
        }
    }

    #[test]
    fn draft_set_with_no_markers_passes() {
        let m = build(&set(vec![req("FRN-SRS-001", Status::Draft)]), &[]);
        assert!(m.ok);
        assert_eq!(m.summary.counted, 1);
        assert_eq!(m.summary.verified, 0);
        assert_eq!(m.requirements[0].verdict, Verdict::Draft);
    }

    #[test]
    fn unknown_and_need_markers_fail() {
        let markers = [
            marker(MarkerKind::Verifies, "FRN-SRS-999"),
            marker(MarkerKind::Verifies, "FRN-NEED-001"),
        ];
        let m = build(&set(vec![req("FRN-SRS-001", Status::Draft)]), &markers);
        assert!(!m.ok);
        assert_eq!(m.summary.unknown, 2);
    }

    #[test]
    fn orphan_and_claim_without_evidence_fail_but_evidence_passes() {
        let reqs = vec![
            req("FRN-SRS-001", Status::Baselined),
            req("FRN-SRS-002", Status::Verified),
            req("FRN-SRS-003", Status::Verified),
            req("FRN-SRS-004", Status::Withdrawn),
        ];
        let markers = [
            marker(MarkerKind::Verifies, "FRN-SRS-003"),
            marker(MarkerKind::Verifies, "FRN-SRS-004"),
        ];
        let m = build(&set(reqs), &markers);
        assert!(!m.ok);
        assert_eq!(m.requirements[0].verdict, Verdict::Orphan);
        assert_eq!(m.requirements[1].verdict, Verdict::ClaimWithoutEvidence);
        assert_eq!(m.requirements[2].verdict, Verdict::Verified);
        assert_eq!(m.requirements[3].verdict, Verdict::Withdrawn);
        assert_eq!(m.summary.unknown, 1, "withdrawn target is rejected");
        assert_eq!(m.summary.verified, 1);
        assert_eq!(m.summary.by_milestone["M1"].counted, 3);
        assert_eq!(m.summary.by_milestone["M1"].baselined, 3);
        assert_eq!(m.summary.by_milestone["M1"].verified, 1);
    }

    #[test]
    fn markdown_lists_rows_and_unknowns() {
        let m = build(
            &set(vec![req("FRN-SRS-001", Status::Draft)]),
            &[marker(MarkerKind::Implements, "X-SRS-1")],
        );
        let md = to_markdown(&m, "2026-10-06T00:00:00Z");
        assert!(md.contains("| FRN-SRS-001 |"));
        assert!(md.contains("## Unknown markers"));
    }
}
