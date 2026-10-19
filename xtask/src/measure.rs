// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask measure`: the reference-machine figures of the time budgets
//! (Steelbore Standard §22.4, T-089).
//!
//! Each splash ends with a JSON report in the journal of its unit. On the
//! reference machine this verb reads the reports of the last boots
//! (`journalctl -b <n> -u fairing-initrd.service -u fairing.service`) and
//! takes, per boot, the longest of each wait the splash timed:
//!
//! | Budget | Report field |
//! |---|---|
//! | `sigterm-release` | `release_ms` of the stage-2 report that ended in `handoff` |
//! | `reacquire` | `reacquire_ms` |
//! | `ask-detect` | `ask_detect_ms` |
//! | `grab-release` | `grab_release_ms` |
//! | `overlay-entry` | `overlay_entry_ms` |
//!
//! The three that need a free VT rather than a boot come from the JSON the
//! commands wrote: `first-frame` is `first_frame_ms` of `fairing preview
//! --backend drm --json`, `fbdev-fallback` the same field of a preview that
//! fell back to fbdev with the DRM devices withheld, and `artefact-load`
//! `load_ms` of `fairing theme compile --json`. Over several boots the figure
//! is the median and the spread is the range. `--record` appends one dated
//! entry per figure to `doc/measurements.toml`.
//!
//! Each command file is read from `target/measure/<flag>.json` when its flag
//! is not given.
//!
//! `cargo xtask measure [--boots <n>] [--journal <file>] [--preview-drm <json>]
//! [--preview-fbdev <json>] [--theme-compile <json>] [--record --machine <text>]`

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::args::Args;
use crate::budgets::{self, MEASUREMENTS_PATH};
use crate::dist;
use crate::paths;
use crate::report::{Envelope, Failure};

/// The units whose reports the journal holds.
pub const UNITS: [&str; 2] = ["fairing-initrd.service", "fairing.service"];

/// Where RELEASING.md has the command output written; each file there is
/// read when its flag is not given.
pub const DEFAULT_DIR: &str = "target/measure";

/// Boots read when `--boots` is not given.
pub const DEFAULT_BOOTS: usize = 3;

/// Budgets taken from the splash reports of a boot, with the field each is.
pub const BOOT_FIELDS: [(&str, &str); 5] = [
    ("sigterm-release", "release_ms"),
    ("reacquire", "reacquire_ms"),
    ("ask-detect", "ask_detect_ms"),
    ("grab-release", "grab_release_ms"),
    ("overlay-entry", "overlay_entry_ms"),
];

/// One budget's figure over the runs that produced it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Figure {
    /// The budget.
    pub budget: String,
    /// The median, in ms.
    pub value: f64,
    /// The range of the runs, when more than one produced a value.
    pub spread: Option<f64>,
    /// Runs that produced a value.
    pub runs: usize,
    /// What produced it, for the measurements file.
    pub source: String,
}

/// The figures of one boot's reports: for each budget, the longest wait.
#[must_use]
pub fn boot_figures(journal: &str) -> BTreeMap<&'static str, f64> {
    let mut figures = BTreeMap::new();
    for data in journal
        .lines()
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| value.get("data").cloned())
        .filter(|data| data.get("stage").is_some())
    {
        let stage = data.get("stage").and_then(Value::as_str);
        let reason = data.get("reason").and_then(Value::as_str);
        for (budget, field) in BOOT_FIELDS {
            // The handoff's release only: any other SIGTERM is not greetd's.
            if budget == "sigterm-release" && (stage != Some("system") || reason != Some("handoff"))
            {
                continue;
            }
            if let Some(ms) = data.get(field).and_then(Value::as_f64) {
                let worst = figures.entry(budget).or_insert(ms);
                *worst = worst.max(ms);
            }
        }
    }
    figures
}

/// The median and range of `values`, which must not be empty.
fn median(values: &mut [f64]) -> (f64, Option<f64>) {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let mid = if n % 2 == 1 {
        values[n / 2]
    } else {
        f64::midpoint(values[n / 2 - 1], values[n / 2])
    };
    let spread = (n > 1).then(|| values[n - 1] - values[0]);
    (mid, spread)
}

/// The figures of several boots, one per budget that any boot produced.
#[must_use]
pub fn combine(boots: &[BTreeMap<&'static str, f64>]) -> Vec<Figure> {
    BOOT_FIELDS
        .iter()
        .filter_map(|&(budget, field)| {
            let mut values: Vec<f64> = boots.iter().filter_map(|b| b.get(budget).copied()).collect();
            if values.is_empty() {
                return None;
            }
            let runs = values.len();
            let (value, spread) = median(&mut values);
            Some(Figure {
                budget: budget.to_owned(),
                value,
                spread,
                runs,
                source: format!(
                    "`cargo xtask measure`: `{field}` of the splash reports, median of {runs} boot(s)"
                ),
            })
        })
        .collect()
}

/// A figure from the JSON `fairing` wrote with `--json`: `field` of `data`,
/// whose `backend` must be `backend` when one is named.
///
/// # Errors
///
/// Output that is not a `fairing` envelope, lacks `field`, or drew with
/// another backend.
pub fn command_figure(
    text: &str,
    budget: &str,
    field: &str,
    backend: Option<&str>,
    source: &str,
) -> Result<Figure, Failure> {
    let value: Value = serde_json::from_str(text.trim())?;
    let data = value
        .get("data")
        .ok_or_else(|| Failure::usage(format!("{source}: no `data`"), "run it with --json"))?;
    if let Some(want) = backend {
        let drew = data
            .get("backend")
            .and_then(Value::as_str)
            .unwrap_or("none");
        if drew != want {
            return Err(Failure::gate(
                format!("{source}: drew with `{drew}`, not `{want}`"),
                "see RELEASING.md, Measuring on the reference machine",
            ));
        }
    }
    let ms = data
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| Failure::gate(format!("{source}: no `{field}`"), "run it on a free VT"))?;
    Ok(Figure {
        budget: budget.to_owned(),
        value: ms,
        spread: None,
        runs: 1,
        source: format!("`cargo xtask measure`: `{field}` of {source}"),
    })
}

/// `[[measurement]]` entries for `figures`, taken on `date` on `machine`.
///
/// # Errors
///
/// None in practice; serialisation of plain strings and numbers.
pub fn entries(figures: &[Figure], date: &str, machine: &str) -> Result<String, Failure> {
    #[derive(Serialize)]
    struct Entry<'a> {
        budget: &'a str,
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        spread: Option<f64>,
        date: &'a str,
        machine: &'a str,
        source: &'a str,
    }
    #[derive(Serialize)]
    struct File<'a> {
        measurement: Vec<Entry<'a>>,
    }
    let round = |ms: f64| (ms * 10.0).round() / 10.0;
    let file = File {
        measurement: figures
            .iter()
            .map(|f| Entry {
                budget: &f.budget,
                value: round(f.value),
                spread: f.spread.map(round),
                date,
                machine,
                source: &f.source,
            })
            .collect(),
    };
    toml::to_string(&file).map_err(|e| Failure::internal(format!("cannot write TOML: {e}")))
}

/// The reports of the boot `offset` back (0 the current one).
fn journal(root: &Path, offset: usize) -> Result<String, Failure> {
    let boot = format!("-{offset}");
    let mut args = vec!["-b", boot.as_str(), "-o", "cat", "--no-pager"];
    for unit in UNITS {
        args.extend(["-u", unit]);
    }
    dist::run(root, "journalctl", &args)
}

/// The commands whose `--json` output gives a figure: flag, budget, field
/// and the backend the run must have drawn with.
const COMMAND_FIGURES: [(&str, &str, &str, Option<&str>); 3] = [
    ("preview-drm", "first-frame", "first_frame_ms", Some("drm")),
    (
        "preview-fbdev",
        "fbdev-fallback",
        "first_frame_ms",
        Some("fbdev"),
    ),
    ("theme-compile", "artefact-load", "load_ms", None),
];

#[derive(Debug, Serialize)]
struct Output {
    boots: usize,
    figures: Vec<Figure>,
    missing: Vec<&'static str>,
    recorded: bool,
}

/// The journal text of each boot read: `--journal`, or the last `--boots`.
fn read_boots(args: &Args, root: &Path) -> Result<Vec<String>, Failure> {
    if let Some(file) = args.value("journal") {
        return Ok(vec![paths::read_text(
            &paths::resolve(root, file),
            "journalctl -o cat",
        )?]);
    }
    let n = match args.value("boots") {
        None => DEFAULT_BOOTS,
        Some(n) => n.parse().ok().filter(|&n| n > 0).ok_or_else(|| {
            Failure::usage(
                format!("`--boots {n}` is not a count"),
                "cargo xtask measure --boots 3",
            )
        })?,
    };
    // A boot the journal no longer holds ends the list.
    Ok((0..n)
        .map_while(|offset| journal(root, offset).ok())
        .collect())
}

/// The figures from the command output files named on the command line.
fn command_figures(args: &Args, root: &Path) -> Result<Vec<Figure>, Failure> {
    let mut figures = Vec::new();
    for (flag, budget, field, backend) in COMMAND_FIGURES {
        let default = root.join(DEFAULT_DIR).join(format!("{flag}.json"));
        let path = match args.value(flag) {
            Some(file) => Some(paths::resolve(root, file)),
            None => default.is_file().then_some(default),
        };
        if let Some(path) = path {
            let text = paths::read_text(&path, "see RELEASING.md")?;
            let source = paths::relative(root, &path);
            figures.push(command_figure(&text, budget, field, backend, &source)?);
        }
    }
    Ok(figures)
}

/// Appends `figures` to the measurements file, dated today.
fn record(args: &Args, root: &Path, figures: &[Figure]) -> Result<(), Failure> {
    let machine = args.value("machine").ok_or_else(|| {
        Failure::usage(
            "--record needs --machine",
            "--machine \"ThinkPad T490s, kernel …\"",
        )
    })?;
    let date = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let text = entries(figures, &date, machine)?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(root.join(MEASUREMENTS_PATH))?;
    write!(file, "\n{text}")?;
    Ok(())
}

/// `cargo xtask measure`.
///
/// # Errors
///
/// A bad flag, an unreadable journal or file, a preview that drew with the
/// wrong backend, a figure for a budget the register does not have, or
/// nothing to measure.
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&[
        "boots",
        "journal",
        "preview-drm",
        "preview-fbdev",
        "theme-compile",
        "record",
        "machine",
    ])?;
    let root = args.root();
    let boots = read_boots(args, &root)?;
    let per_boot: Vec<_> = boots.iter().map(|text| boot_figures(text)).collect();
    let mut figures = combine(&per_boot);
    figures.extend(command_figures(args, &root)?);
    if figures.is_empty() {
        return Err(Failure::not_found(
            "no splash report in the journal and no command output given",
            "boot with the module enabled, or pass --journal <file>",
        ));
    }
    let (register, _) = budgets::load(&root)?;
    if let Some(unknown) = figures
        .iter()
        .find(|f| !register.budget.iter().any(|b| b.id == f.budget))
    {
        return Err(Failure::internal(format!(
            "`{}` is not in doc/budgets.toml",
            unknown.budget
        )));
    }
    let missing: Vec<&'static str> = BOOT_FIELDS
        .iter()
        .map(|&(budget, _)| budget)
        .filter(|budget| !figures.iter().any(|f| f.budget == *budget))
        .collect();
    let recorded = args.flag("record");
    if recorded {
        record(args, &root, &figures)?;
    }
    if args.json() {
        let output = Output {
            boots: boots.len(),
            figures,
            missing,
            recorded,
        };
        return Envelope::new(&args.command_line(), &output).print();
    }
    for figure in &figures {
        let spread = figure
            .spread
            .map_or_else(String::new, |s| format!(", spread {s:.1} ms"));
        println!(
            "[OK] MEASURE: {} {:.1} ms ({} run(s){spread})",
            figure.budget, figure.value, figure.runs
        );
    }
    for budget in missing {
        println!(
            "[WARN] MEASURE: {budget} not in the reports of {} boot(s)",
            boots.len()
        );
    }
    if recorded {
        println!(
            "[OK] appended {} figure(s) to {MEASUREMENTS_PATH}",
            figures.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = r#"Starting fairing-initrd.service...
{"data":{"stage":"initrd","reason":"switch-root","release_ms":null,"reacquire_ms":180.5,"ask_detect_ms":3.5,"grab_release_ms":0.4,"overlay_entry_ms":null}}
[OK] ASK_OPENED a password question is open
{"data":{"stage":"system","reason":"handoff","release_ms":21.0,"reacquire_ms":null,"ask_detect_ms":null,"grab_release_ms":null,"overlay_entry_ms":40.0}}
"#;

    #[test]
    fn a_boot_gives_the_longest_wait_of_each_kind() {
        let figures = boot_figures(BOOT);
        assert_eq!(figures.get("sigterm-release"), Some(&21.0));
        assert_eq!(figures.get("reacquire"), Some(&180.5));
        assert_eq!(figures.get("ask-detect"), Some(&3.5));
        assert_eq!(figures.get("grab-release"), Some(&0.4));
        assert_eq!(figures.get("overlay-entry"), Some(&40.0));
        let twice = BOOT.replace("\"reacquire_ms\":null", "\"reacquire_ms\":400.0");
        assert_eq!(boot_figures(&twice).get("reacquire"), Some(&400.0));
    }

    #[test]
    fn only_the_handoff_release_counts() {
        let stopped = BOOT.replace("\"handoff\"", "\"stopped\"");
        assert_eq!(boot_figures(&stopped).get("sigterm-release"), None);
        let initrd = r#"{"data":{"stage":"initrd","reason":"handoff","release_ms":9.0}}"#;
        assert_eq!(boot_figures(initrd).get("sigterm-release"), None);
    }

    #[test]
    fn boots_combine_to_the_median_and_range() {
        let boot = |ms: f64| BTreeMap::from([("ask-detect", ms)]);
        let figures = combine(&[boot(4.0), boot(1.0), boot(9.0)]);
        assert_eq!(figures.len(), 1);
        assert_eq!(
            (figures[0].value, figures[0].spread, figures[0].runs),
            (4.0, Some(8.0), 3)
        );
        let figures = combine(&[boot(4.0), boot(2.0)]);
        assert_eq!((figures[0].value, figures[0].spread), (3.0, Some(2.0)));
        let figures = combine(&[boot(5.0), BTreeMap::new()]);
        assert_eq!(
            (figures[0].value, figures[0].spread, figures[0].runs),
            (5.0, None, 1)
        );
        assert!(combine(&[BTreeMap::new()]).is_empty());
    }

    #[test]
    fn a_preview_must_have_drawn_with_the_backend_named() {
        let drm = r#"{"data":{"backend":"drm","first_frame_ms":20.5}}"#;
        let figure = command_figure(
            drm,
            "first-frame",
            "first_frame_ms",
            Some("drm"),
            "drm.json",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((figure.value, figure.runs), (20.5, 1));
        assert!(
            command_figure(drm, "fbdev-fallback", "first_frame_ms", Some("fbdev"), "f").is_err()
        );
        assert!(command_figure("{}", "artefact-load", "load_ms", None, "c").is_err());
        assert!(command_figure("not json", "artefact-load", "load_ms", None, "c").is_err());
        let compiled = r#"{"data":{"load_ms":0.04}}"#;
        let figure = command_figure(compiled, "artefact-load", "load_ms", None, "c")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!((figure.value - 0.04).abs() < f64::EPSILON);
    }

    #[test]
    fn recorded_entries_read_back_as_measurements() {
        let figures = vec![
            Figure {
                budget: "ask-detect".to_owned(),
                value: 3.456,
                spread: Some(1.04),
                runs: 3,
                source: "test".to_owned(),
            },
            Figure {
                budget: "artefact-load".to_owned(),
                value: 0.04,
                spread: None,
                runs: 1,
                source: "test".to_owned(),
            },
        ];
        let text =
            entries(&figures, "2026-10-19", "ThinkPad \"T490s\"").unwrap_or_else(|e| panic!("{e}"));
        let back: budgets::Measurements = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.measurement.len(), 2);
        assert!(
            (back.measurement[0].value - 3.5).abs() < 1e-9,
            "rounded to 0.1 ms"
        );
        assert_eq!(back.measurement[0].spread, Some(1.0));
        assert_eq!(back.measurement[1].spread, None);
        assert_eq!(back.measurement[0].machine, "ThinkPad \"T490s\"");
        assert_eq!(back.measurement[0].date, "2026-10-19");
    }

    #[test]
    fn every_boot_budget_is_in_the_register() {
        let (register, _) =
            budgets::load(&crate::paths::default_root()).unwrap_or_else(|e| panic!("{e}"));
        for (budget, _) in BOOT_FIELDS {
            let entry = register
                .budget
                .iter()
                .find(|b| b.id == budget)
                .unwrap_or_else(|| panic!("{budget} not in doc/budgets.toml"));
            assert_eq!(entry.unit, budgets::Unit::Ms, "{budget}");
        }
    }
}
