//! Quality and speed benchmark for `regrex scan`.
//!
//! Scans every file listed in `benchmarks/expected.toml` with the same
//! settings as the command line, and compares what it finds with the
//! relations expected in each file:
//!
//! ```sh
//! cargo run --release -p regrex-cli --example bench
//! cargo run --release -p regrex-cli --example bench -- path/to/expected.toml
//! ```
//!
//! It prints a Markdown table (also appended to `$GITHUB_STEP_SUMMARY` when
//! set) and exits with an error when a relation marked as found is lost or
//! a file has more false positives than allowed. Times are reported but
//! never fail the run.

use regrex_core::{scan, DerivedColumn, ScanConfig};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

struct Relation {
    columns: BTreeSet<String>,
    formula: String,
    expected_found: bool,
}

struct Case {
    path: String,
    ignore: Vec<String>,
    relations: Vec<Relation>,
    false_positives: usize,
}

/// What happened on one file.
struct Outcome {
    found: Vec<bool>,
    false_positives: Vec<String>,
    seconds: f64,
}

fn main() -> ExitCode {
    let expected = std::env::args().nth(1).map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/expected.toml"),
        PathBuf::from,
    );
    let cases = match read_cases(&expected) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let base = expected.parent().unwrap_or(Path::new("."));

    let mut report = String::new();
    let mut failures = Vec::new();
    let mut improvements = Vec::new();
    let (mut relations, mut found, mut fps, mut total_time) = (0, 0, 0, 0.0);
    writeln!(
        report,
        "| File | Found | False positives | Time |\n| --- | --- | --- | ---: |"
    )
    .unwrap();
    for case in &cases {
        let outcome = match run(base, case) {
            Ok(o) => o,
            Err(e) => {
                failures.push(format!("{}: {e}", case.path));
                continue;
            }
        };
        let name = Path::new(&case.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&case.path);
        let mut found_cell = Vec::new();
        for (rel, &ok) in case.relations.iter().zip(&outcome.found) {
            found_cell.push(format!("{} `{}`", if ok { "✓" } else { "✗" }, rel.formula));
            if rel.expected_found && !ok {
                failures.push(format!("{name}: lost `{}`", rel.formula));
            }
            if !rel.expected_found && ok {
                improvements.push(format!("{name}: now finds `{}`", rel.formula));
            }
        }
        relations += case.relations.len();
        found += outcome.found.iter().filter(|&&f| f).count();
        fps += outcome.false_positives.len();
        total_time += outcome.seconds;
        if outcome.false_positives.len() > case.false_positives {
            failures.push(format!(
                "{name}: {} false positives (at most {} allowed): {}",
                outcome.false_positives.len(),
                case.false_positives,
                outcome.false_positives.join("; ")
            ));
        } else if outcome.false_positives.len() < case.false_positives {
            improvements.push(format!(
                "{name}: {} false positives instead of {}",
                outcome.false_positives.len(),
                case.false_positives
            ));
        }
        let fp_cell = if outcome.false_positives.is_empty() {
            "0".to_string()
        } else {
            format!(
                "{}: {}",
                outcome.false_positives.len(),
                outcome
                    .false_positives
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let found_cell = if found_cell.is_empty() {
            "(none expected)".to_string()
        } else {
            found_cell.join("<br>")
        };
        writeln!(
            report,
            "| {name} | {found_cell} | {fp_cell} | {:.2} s |",
            outcome.seconds
        )
        .unwrap();
    }
    writeln!(
        report,
        "\n**{found} of {relations} relations found, {fps} false positives, {total_time:.1} s in total.**"
    )
    .unwrap();
    if !improvements.is_empty() {
        writeln!(
            report,
            "\nBetter than recorded; please update `benchmarks/expected.toml`:\n"
        )
        .unwrap();
        for i in &improvements {
            writeln!(report, "- {i}").unwrap();
        }
    }
    if !failures.is_empty() {
        writeln!(report, "\nWorse than recorded:\n").unwrap();
        for f in &failures {
            writeln!(report, "- {f}").unwrap();
        }
    }

    print!("{report}");
    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(summary) {
            let _ = writeln!(f, "## Scan benchmark\n\n{report}");
        }
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Scans one file and classifies every reported formula.
fn run(base: &Path, case: &Case) -> Result<Outcome, String> {
    let mut loaded =
        regrex_io::load_path(&base.join(&case.path), None).map_err(|e| e.to_string())?;
    for name in &case.ignore {
        let i = loaded
            .table
            .names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| format!("no column {name:?} to ignore"))?;
        loaded.table.remove_column(i);
    }
    let names = loaded.table.names.clone();
    for rel in &case.relations {
        if let Some(c) = rel.columns.iter().find(|c| !names.contains(c)) {
            return Err(format!("no column {c:?} for `{}`", rel.formula));
        }
    }
    let start = Instant::now();
    let derived = scan(&loaded.table, &ScanConfig::default());
    let seconds = start.elapsed().as_secs_f64();

    let mut found = vec![false; case.relations.len()];
    let mut false_positives = Vec::new();
    for d in &derived {
        let used = columns_used(d, &names);
        let mut explained = false;
        for (i, rel) in case.relations.iter().enumerate() {
            if used.is_subset(&rel.columns) {
                found[i] = true;
                explained = true;
            }
        }
        if !explained {
            false_positives.push(format!("{} = {}", names[d.target], d.formula_text));
        }
    }
    Ok(Outcome {
        found,
        false_positives,
        seconds,
    })
}

fn columns_used(d: &DerivedColumn, names: &[String]) -> BTreeSet<String> {
    std::iter::once(d.target)
        .chain(d.inputs.iter().copied())
        .map(|i| names[i].clone())
        .collect()
}

fn read_cases(path: &Path) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let files = doc
        .get("file")
        .and_then(|f| f.as_array())
        .ok_or("expected [[file]] entries")?;
    let strings = |v: Option<&toml::Value>| -> Result<Vec<String>, String> {
        v.map_or(Ok(Vec::new()), |v| {
            v.as_array()
                .ok_or("expected a list of strings")?
                .iter()
                .map(|s| {
                    s.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "expected a string".to_string())
                })
                .collect()
        })
    };
    files
        .iter()
        .map(|f| {
            let path = f
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or("a [[file]] without path")?
                .to_string();
            let relations = f
                .get("relations")
                .and_then(|r| r.as_array())
                .map_or(&[][..], |r| r.as_slice())
                .iter()
                .map(|r| {
                    let columns: BTreeSet<String> =
                        strings(r.get("columns"))?.into_iter().collect();
                    let formula = r
                        .get("formula")
                        .and_then(|s| s.as_str())
                        .ok_or_else(|| format!("{path}: a relation without formula"))?
                        .to_string();
                    let expected_found = r.get("found").and_then(|b| b.as_bool()).unwrap_or(true);
                    Ok(Relation {
                        columns,
                        formula,
                        expected_found,
                    })
                })
                .collect::<Result<_, String>>()?;
            let false_positives = f
                .get("false_positives")
                .and_then(|n| n.as_integer())
                .unwrap_or(0) as usize;
            Ok(Case {
                ignore: strings(f.get("ignore"))?,
                path,
                relations,
                false_positives,
            })
        })
        .collect()
}
