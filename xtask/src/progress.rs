// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask progress`: the Steelbore Standard §17.1 progress block.
//!
//! Reads `PLAN.md` (and `TODO.md`, `PRD.md` when present) in the §17.5 shape —
//! `# <Project> — <Document>`, `MVP: M0–M3 — <deliverable>`, one `## Mn — <desc>`
//! heading per milestone and `- [ ] P-001 …` task-list items — and renders the
//! 20-cell `█`/`░` bars with the §17.2 column geometry. When a traceability
//! matrix is supplied and a milestone has baselined requirements, that
//! milestone's figure is the verified fraction of its baselined mandatory and
//! expected requirements instead of its ticked items (§17.1).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use regex::Regex;
use serde::Serialize;

use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure};
use crate::trace::matrix::{Matrix, MilestoneSummary};

/// Longest description a row may carry (§17.1).
pub const MAX_DESCRIPTION: usize = 40;

/// One task-list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Permanent identifier, e.g. `P-003`.
    pub id: String,
    /// Ticked.
    pub done: bool,
    /// Ends in `(optional)`; never counted.
    pub optional: bool,
    /// Struck through and marked `(dropped)`; never counted.
    pub dropped: bool,
}

impl Item {
    /// Whether the item enters a denominator.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        !self.optional && !self.dropped
    }
}

/// A `## Mn — <description>` section and its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// `n` in `Mn`.
    pub number: u32,
    /// Text after the em dash, verbatim.
    pub description: String,
    /// Items under the heading.
    pub items: Vec<Item>,
}

/// The `MVP:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mvp {
    /// First milestone in the range.
    pub from: u32,
    /// Last milestone in the range.
    pub to: u32,
    /// The deliverable, verbatim.
    pub description: String,
}

/// A parsed tracking document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracking {
    /// Project name from the title (before the em dash).
    pub project: String,
    /// The MVP line, if present.
    pub mvp: Option<Mvp>,
    /// Milestones in file order.
    pub milestones: Vec<Milestone>,
    /// Items outside any milestone heading (a flat `TODO.md`, a `## Gates` section).
    pub loose: Vec<Item>,
}

/// Parses a §17.5 document; `name` is used in error messages.
///
/// # Errors
///
/// Returns a gate failure when the title is missing or a milestone heading is
/// malformed or over-long.
pub fn parse(text: &str, name: &str) -> Result<Tracking, Failure> {
    let heading_re = regex("^## M([0-9]+)\\s+—\\s+(.+?)\\s*$");
    let other_heading_re = regex("^##\\s");
    let mvp_re = regex("^MVP:\\s*M([0-9]+)\\s*[–-]\\s*M([0-9]+)\\s*—\\s*(.+?)\\s*$");
    let item_re = regex("^\\s*- \\[([ xX])\\]\\s+(.+?)\\s*$");
    let dropped_re = regex("^~~(.+?)~~\\s*\\(dropped\\)$");

    let mut project = None;
    let mut mvp = None;
    let mut milestones: Vec<Milestone> = Vec::new();
    let mut loose = Vec::new();
    let mut in_milestone = false;

    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        if project.is_none()
            && let Some(title) = line.strip_prefix("# ")
        {
            project = Some(title.split(" — ").next().unwrap_or(title).trim().to_owned());
            continue;
        }
        if let Some(caps) = mvp_re.captures(line) {
            mvp = Some(Mvp {
                from: parse_number(&caps[1]),
                to: parse_number(&caps[2]),
                description: caps[3].to_owned(),
            });
            continue;
        }
        if let Some(caps) = heading_re.captures(line) {
            let description = caps[2].to_owned();
            if description.chars().count() > MAX_DESCRIPTION {
                return Err(Failure::gate(
                    format!(
                        "{name}:{line_no}: milestone description exceeds {MAX_DESCRIPTION} characters"
                    ),
                    "cargo xtask progress",
                ));
            }
            milestones.push(Milestone {
                number: parse_number(&caps[1]),
                description,
                items: Vec::new(),
            });
            in_milestone = true;
            continue;
        }
        if other_heading_re.is_match(line) {
            in_milestone = false;
            continue;
        }
        if let Some(caps) = item_re.captures(line) {
            let done = &caps[1] != " ";
            let body = caps[2].trim();
            let (dropped, body) = match dropped_re.captures(body) {
                Some(d) => (true, d.get(1).map_or("", |m| m.as_str()).to_owned()),
                None => (false, body.to_owned()),
            };
            let optional = body.ends_with("(optional)");
            let id = body.split_whitespace().next().unwrap_or("").to_owned();
            let item = Item {
                id,
                done,
                optional,
                dropped,
            };
            match (in_milestone, milestones.last_mut()) {
                (true, Some(m)) => m.items.push(item),
                _ => loose.push(item),
            }
        }
    }

    let project = project.ok_or_else(|| {
        Failure::gate(
            format!("{name}: missing `# <Project> — <Document>` title"),
            "cargo xtask progress",
        )
    })?;
    milestones.sort_by_key(|m| m.number);
    Ok(Tracking {
        project,
        mvp,
        milestones,
        loose,
    })
}

/// One rendered row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// `M0`, `MVP`, `TODO`, `PLAN` or `PRD`.
    pub label: String,
    /// 0–100.
    pub percent: u8,
    /// ≤ 40 characters.
    pub description: String,
    /// Numerator behind the percentage.
    pub done: usize,
    /// Denominator behind the percentage.
    pub total: usize,
    /// Where the figure came from.
    pub source: &'static str,
}

/// Per-milestone requirement figures taken from a traceability matrix.
pub type Figures = BTreeMap<String, MilestoneSummary>;

fn milestone_fraction(
    m: &Milestone,
    figures: Option<&Figures>,
) -> Result<(usize, usize, &'static str), Failure> {
    if let Some(f) = figures.and_then(|f| f.get(&format!("M{}", m.number)))
        && f.baselined > 0
    {
        return Ok((f.verified, f.baselined, "requirements"));
    }
    let required: Vec<&Item> = m.items.iter().filter(|i| i.is_required()).collect();
    if required.is_empty() {
        if m.description.contains(':') {
            return Ok((1, 1, "heading"));
        }
        return Err(Failure::gate(
            format!(
                "milestone M{} has no items and its heading does not say why",
                m.number
            ),
            "cargo xtask progress",
        ));
    }
    Ok((
        required.iter().filter(|i| i.done).count(),
        required.len(),
        "items",
    ))
}

/// Builds the §17.1 rows in normative order.
///
/// # Errors
///
/// Returns a gate failure when `PLAN.md` has no `MVP:` line or an empty
/// milestone whose heading gives no reason.
pub fn rows(
    plan: &Tracking,
    todo: Option<&Tracking>,
    prd: Option<&Tracking>,
    figures: Option<&Figures>,
) -> Result<Vec<Row>, Failure> {
    let mut rows = Vec::new();
    let mut fractions: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for m in &plan.milestones {
        let (done, total, source) = milestone_fraction(m, figures)?;
        fractions.insert(m.number, (done, total));
        rows.push(Row {
            label: format!("M{}", m.number),
            percent: percent(done, total),
            description: m.description.clone(),
            done,
            total,
            source,
        });
    }

    let mvp = plan.mvp.as_ref().ok_or_else(|| {
        Failure::gate(
            "PLAN.md has no `MVP: M<a>–M<b> — <deliverable>` line",
            "cargo xtask progress",
        )
    })?;
    let (mut done, mut total) = (0, 0);
    for m in plan
        .milestones
        .iter()
        .filter(|m| (mvp.from..=mvp.to).contains(&m.number))
    {
        let (d, t) = fractions.get(&m.number).copied().unwrap_or((0, 0));
        if m.items.iter().any(Item::is_required)
            || figures.is_some_and(|f| f.contains_key(&format!("M{}", m.number)))
        {
            done += d;
            total += t;
        }
    }
    rows.push(Row {
        label: "MVP".into(),
        percent: percent(done, total),
        description: mvp.description.clone(),
        done,
        total,
        source: "mvp",
    });

    if let Some(todo) = todo {
        let (done, total, optional) = count(todo);
        let mut description = format!("{done}/{total} tasks");
        if optional > 0 {
            let extended = format!("{description}, +{optional} optional");
            if extended.chars().count() <= MAX_DESCRIPTION {
                description = extended;
            }
        }
        rows.push(Row {
            label: "TODO".into(),
            percent: percent(done, total),
            description,
            done,
            total,
            source: "items",
        });
    }

    let (done, total, _) = count(plan);
    rows.push(Row {
        label: "PLAN".into(),
        percent: percent(done, total),
        description: format!("PLAN.md, {}", range(plan)),
        done,
        total,
        source: "items",
    });

    if let Some(prd) = prd {
        let (done, total, _) = count(prd);
        rows.push(Row {
            label: "PRD".into(),
            percent: percent(done, total),
            description: format!("PRD.md, {}", range(prd)),
            done,
            total,
            source: "items",
        });
    }
    Ok(rows)
}

fn count(doc: &Tracking) -> (usize, usize, usize) {
    let items = doc
        .milestones
        .iter()
        .flat_map(|m| m.items.iter())
        .chain(doc.loose.iter());
    let (mut done, mut total, mut optional) = (0, 0, 0);
    for item in items {
        if item.optional {
            optional += 1;
        }
        if item.is_required() {
            total += 1;
            if item.done {
                done += 1;
            }
        }
    }
    (done, total, optional)
}

fn range(doc: &Tracking) -> String {
    match (doc.milestones.first(), doc.milestones.last()) {
        (Some(a), Some(b)) if a.number != b.number => format!("M{}–M{}", a.number, b.number),
        (Some(a), _) => format!("M{}", a.number),
        _ => "flat".to_owned(),
    }
}

/// Percentage rounded half up; `0/0` reads as 100 (nothing left to do).
#[must_use]
pub fn percent(done: usize, total: usize) -> u8 {
    if total == 0 {
        return 100;
    }
    let value = (done * 200 + total) / (2 * total);
    u8::try_from(value).unwrap_or(100)
}

/// The 20-cell bar for `percent` with the §17.2 saturation rules.
#[must_use]
pub fn bar(percent: u8) -> String {
    let filled = match percent {
        0 => 0,
        100 => 20,
        p => (usize::from(p) * 20 + 50) / 100,
    };
    let filled = if matches!(percent, 0 | 100) {
        filled
    } else {
        filled.clamp(1, 19)
    };
    format!("{}{}", "█".repeat(filled), "░".repeat(20 - filled))
}

/// Renders the block: title line, blank line, aligned rows.
#[must_use]
pub fn render(project: &str, rows: &[Row]) -> String {
    let mut out = format!("Project: {project}\n\n");
    for row in rows {
        let label = format!("{}:", row.label);
        let _ = writeln!(
            out,
            "{label:<6}[{}]{:>4}%   {}",
            bar(row.percent),
            row.percent,
            row.description
        );
    }
    out
}

#[derive(Debug, Serialize)]
struct Output<'a> {
    project: &'a str,
    rows: &'a [Row],
    block: String,
}

/// Runs the verb.
///
/// # Errors
///
/// Usage, not-found, or gate failures from parsing.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["plan", "todo", "prd", "matrix", "explain"])?;
    let root = args.root();
    let plan_path = paths::resolve(&root, args.value("plan").unwrap_or("PLAN.md"));
    let todo_path = paths::resolve(&root, args.value("todo").unwrap_or("TODO.md"));
    let prd_path = paths::resolve(&root, args.value("prd").unwrap_or("PRD.md"));

    let plan = parse(
        &paths::read_text(&plan_path, "cargo xtask progress --plan PLAN.md")?,
        "PLAN.md",
    )?;
    let todo = optional_doc(&todo_path, "TODO.md", args.value("todo").is_some())?;
    let prd = optional_doc(&prd_path, "PRD.md", args.value("prd").is_some())?;
    let figures = match args.value("matrix") {
        Some(value) => {
            let text = paths::read_text(&paths::resolve(&root, value), "cargo xtask trace")?;
            let matrix: Matrix = serde_json::from_str(&text)?;
            Some(matrix.summary.by_milestone)
        }
        None => None,
    };

    let rows = rows(&plan, todo.as_ref(), prd.as_ref(), figures.as_ref())?;
    let block = render(&plan.project, &rows);
    if args.json() {
        Envelope::new(
            &args.command_line(),
            Output {
                project: &plan.project,
                rows: &rows,
                block,
            },
        )
        .print()?;
    } else {
        print!("{block}");
        if args.flag("explain") {
            println!();
            for row in &rows {
                println!(
                    "{:<6}{}/{} ({})",
                    format!("{}:", row.label),
                    row.done,
                    row.total,
                    row.source
                );
            }
        }
    }
    Ok(())
}

fn optional_doc(
    path: &std::path::Path,
    name: &str,
    explicit: bool,
) -> Result<Option<Tracking>, Failure> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            parse(&text, name).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !explicit => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Failure::not_found(
            format!("`{}` does not exist", path.display()),
            "cargo xtask progress",
        )),
        Err(error) => Err(error.into()),
    }
}

fn parse_number(digits: &str) -> u32 {
    digits.parse().unwrap_or(0)
}

fn regex(pattern: &str) -> Regex {
    // Constant patterns; a compile failure is a programming error (M-PANIC-ON-BUG).
    #[expect(clippy::expect_used, reason = "pattern is a module constant")]
    Regex::new(pattern).expect("valid regex")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERATOR: &str = "# Operator — Plan

MVP: M0–M2 — Operator v0.1

## M0 — Foundation: not needed

## M1 — Daily-driver shell

- [x] P-001 Prompt renders
- [x] P-002 Line editing
- [ ] P-003 History search
- [ ] P-004 Themes (optional)
- [ ] ~~P-005 Mouse support~~ (dropped)

## M2 — Plugin system

- [ ] P-006 Plugin loader
";

    const EXPECTED: &str = "Project: Operator

M0:   [████████████████████] 100%   Foundation: not needed
M1:   [█████████████░░░░░░░]  67%   Daily-driver shell
M2:   [░░░░░░░░░░░░░░░░░░░░]   0%   Plugin system
MVP:  [██████████░░░░░░░░░░]  50%   Operator v0.1
PLAN: [██████████░░░░░░░░░░]  50%   PLAN.md, M0–M2
";

    #[test]
    fn renders_the_standards_worked_example_byte_for_byte() {
        let plan = parse(OPERATOR, "PLAN.md").unwrap_or_else(|e| panic!("{e}"));
        let rows = rows(&plan, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(render(&plan.project, &rows), EXPECTED);
    }

    #[test]
    fn bar_saturates_only_at_the_ends() {
        assert_eq!(bar(0).chars().filter(|c| *c == '█').count(), 0);
        assert_eq!(bar(1).chars().filter(|c| *c == '█').count(), 1);
        assert_eq!(bar(99).chars().filter(|c| *c == '█').count(), 19);
        assert_eq!(bar(100).chars().filter(|c| *c == '█').count(), 20);
        assert_eq!(bar(67), "█████████████░░░░░░░");
    }

    #[test]
    fn percent_rounds_half_up_and_treats_empty_as_done() {
        assert_eq!(percent(2, 3), 67);
        assert_eq!(percent(1, 8), 13);
        assert_eq!(percent(0, 0), 100);
        assert_eq!(percent(0, 4), 0);
    }

    #[test]
    fn empty_milestone_without_reason_is_an_error() {
        let text = "# P — Plan\n\nMVP: M0–M1 — P v0.1\n\n## M0 — Mystery\n\n## M1 — Work\n\n- [ ] P-001 a\n";
        let plan = parse(text, "PLAN.md").unwrap_or_else(|e| panic!("{e}"));
        assert!(rows(&plan, None, None, None).is_err());
    }

    #[test]
    fn todo_row_counts_loose_items_and_optional_suffix() {
        let plan = parse(OPERATOR, "PLAN.md").unwrap_or_else(|e| panic!("{e}"));
        let todo = parse(
            "# Operator — TODO\n\n- [x] T-001 a\n- [ ] T-002 b\n- [ ] T-003 c (optional)\n",
            "TODO.md",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let rows = rows(&plan, Some(&todo), None, None).unwrap_or_else(|e| panic!("{e}"));
        let todo_row = rows
            .iter()
            .find(|r| r.label == "TODO")
            .unwrap_or_else(|| panic!("todo row"));
        assert_eq!(todo_row.description, "1/2 tasks, +1 optional");
        assert_eq!(todo_row.percent, 50);
    }

    #[test]
    fn matrix_figures_override_ticks_when_baselined() {
        let plan = parse(OPERATOR, "PLAN.md").unwrap_or_else(|e| panic!("{e}"));
        let mut figures = Figures::new();
        figures.insert(
            "M2".into(),
            MilestoneSummary {
                counted: 4,
                baselined: 4,
                verified: 1,
            },
        );
        let rows = rows(&plan, None, None, Some(&figures)).unwrap_or_else(|e| panic!("{e}"));
        let m2 = rows
            .iter()
            .find(|r| r.label == "M2")
            .unwrap_or_else(|| panic!("m2"));
        assert_eq!(
            (m2.done, m2.total, m2.percent, m2.source),
            (1, 4, 25, "requirements")
        );
    }
}
