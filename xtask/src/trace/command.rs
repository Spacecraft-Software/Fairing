// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask trace`: write the matrix artifacts and gate on failures.

use super::{markers, matrix};
use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure, Finding, now_utc, print_findings};
use crate::requirements::{self, validate};

/// Default artifact directory, relative to the repository root.
pub const DEFAULT_OUT: &str = "target/trace";

/// Runs the verb.
///
/// # Errors
///
/// Usage, not-found, or gate failures (invalid set, unknown markers, orphans).
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["requirements", "out"])?;
    let root = args.root();
    let toml_path = paths::resolve(
        &root,
        args.value("requirements")
            .unwrap_or(requirements::DEFAULT_PATH),
    );
    let out_dir = paths::resolve(&root, args.value("out").unwrap_or(DEFAULT_OUT));

    let set = requirements::load(&toml_path)?;
    let errors = validate::validate(&set);
    if !errors.is_empty() {
        print_findings("ERROR", &errors);
        return Err(Failure::gate(
            format!(
                "`{}` has {} validation error(s)",
                paths::relative(&root, &toml_path),
                errors.len()
            ),
            "cargo xtask req-texi",
        ));
    }

    let found = markers::scan(&root)?;
    let matrix = matrix::build(&set, &found);
    std::fs::create_dir_all(&out_dir)?;
    let stamp = now_utc();
    std::fs::write(
        out_dir.join("matrix.json"),
        serde_json::to_string_pretty(&matrix)?,
    )?;
    std::fs::write(
        out_dir.join("matrix.md"),
        matrix::to_markdown(&matrix, &stamp),
    )?;

    let mut findings = Vec::new();
    for unknown in &matrix.unknown_markers {
        findings.push(
            Finding::new("UNKNOWN_MARKER", Some(&unknown.id), unknown.reason.clone())
                .at(&unknown.path, unknown.line),
        );
    }
    for row in matrix.requirements.iter().filter(|r| r.verdict.fails()) {
        let code = match row.verdict {
            matrix::Verdict::Orphan => "ORPHAN",
            _ => "CLAIM_WITHOUT_EVIDENCE",
        };
        findings.push(Finding::new(
            code,
            Some(&row.id),
            format!("status `{}` with no `Verifies:` marker", row.status),
        ));
    }

    if args.json() {
        Envelope::new(&args.command_line(), &matrix).print()?;
    } else {
        let s = &matrix.summary;
        println!(
            "[{}] traceability: {} requirements, {} counted, {} baselined, {} verified; {} unknown, {} orphan(s), {} claim(s) without evidence",
            if matrix.ok { "OK" } else { "ERROR" },
            s.total,
            s.counted,
            s.baselined,
            s.verified,
            s.unknown,
            s.orphans,
            s.claims_without_evidence
        );
        println!(
            "[OK] wrote {}/matrix.json and matrix.md",
            paths::relative(&root, &out_dir)
        );
        print_findings("ERROR", &findings);
    }

    if matrix.ok {
        Ok(())
    } else {
        Err(Failure::gate(
            format!(
                "traceability gate failed with {} finding(s)",
                findings.len()
            ),
            "cargo xtask trace && cat target/trace/matrix.md",
        ))
    }
}
