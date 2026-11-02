//! Human-readable and JSON reports.
//!
//! The JSON output is versioned with `schema_version` and is meant to be
//! stable: fields may be added, but existing ones keep their meaning.

use crate::input::Loaded;
use regrex_core::{Candidate, DerivedColumn};
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;

/// Version of the JSON output format.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct ScanReport {
    schema_version: u32,
    file: String,
    rows: usize,
    columns: Vec<String>,
    skipped_columns: Vec<String>,
    derived: Vec<DerivedJson>,
}

#[derive(Serialize)]
struct DerivedJson {
    column: String,
    formula: String,
    inputs: Vec<String>,
    complexity: usize,
    matched_rows: usize,
    checked_rows: usize,
    match_ratio: f64,
    /// Data rows (1 = first row after the header) that break the formula.
    exception_rows: Vec<usize>,
}

impl ScanReport {
    pub fn new(path: &Path, loaded: &Loaded, derived: &[DerivedColumn]) -> Self {
        let names = &loaded.table.names;
        ScanReport {
            schema_version: SCHEMA_VERSION,
            file: path.display().to_string(),
            rows: loaded.table.n_rows(),
            columns: names.clone(),
            skipped_columns: loaded.skipped.clone(),
            derived: derived
                .iter()
                .map(|d| DerivedJson {
                    column: names[d.target].clone(),
                    formula: d.formula_text.clone(),
                    inputs: d.inputs.iter().map(|&i| names[i].clone()).collect(),
                    complexity: d.complexity,
                    matched_rows: d.matched,
                    checked_rows: d.checked,
                    match_ratio: d.match_ratio(),
                    exception_rows: d.exceptions.iter().map(|r| r + 1).collect(),
                })
                .collect(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }

    /// A Mermaid flowchart: an arrow from each input to each derived column,
    /// labelled with the formula.
    pub fn to_mermaid(&self) -> String {
        let id = |name: &str| {
            let i = self.columns.iter().position(|c| c == name).unwrap_or(0);
            format!("c{i}")
        };
        let mut s = String::from("```mermaid\nflowchart LR\n");
        for (i, name) in self.columns.iter().enumerate() {
            let _ = writeln!(s, "  c{i}[\"{}\"]", name.replace('"', "'"));
        }
        for d in &self.derived {
            let label = format!("{} = {}", d.column, d.formula).replace('"', "'");
            for input in &d.inputs {
                let _ = writeln!(s, "  {} -->|\"{label}\"| {}", id(input), id(&d.column));
            }
        }
        s.push_str("```\n");
        s
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{}: {} rows, {} numeric columns",
            self.file,
            self.rows,
            self.columns.len()
        );
        if !self.skipped_columns.is_empty() {
            let _ = writeln!(
                s,
                "Skipped (not numeric): {}",
                self.skipped_columns.join(", ")
            );
        }
        let _ = writeln!(s);
        if self.derived.is_empty() {
            let _ = writeln!(s, "No derived columns found.");
            return s;
        }
        let width = self
            .derived
            .iter()
            .map(|d| d.column.chars().count())
            .max()
            .unwrap_or(0);
        let fwidth = self
            .derived
            .iter()
            .map(|d| d.formula.chars().count())
            .max()
            .unwrap_or(0);
        for d in &self.derived {
            let _ = write!(
                s,
                "  {:<width$} = {:<fwidth$}   {}/{} rows",
                d.column, d.formula, d.matched_rows, d.checked_rows
            );
            if !d.exception_rows.is_empty() {
                let shown: Vec<String> = d
                    .exception_rows
                    .iter()
                    .take(10)
                    .map(|r| r.to_string())
                    .collect();
                let more = d.exception_rows.len().saturating_sub(10);
                let _ = write!(s, "   exceptions: row {}", shown.join(", "));
                if more > 0 {
                    let _ = write!(s, " and {more} more");
                }
            }
            let _ = writeln!(s);
        }
        s
    }
}

#[derive(Serialize)]
pub struct FitReport {
    schema_version: u32,
    target: String,
    inputs: Vec<String>,
    rows: usize,
    candidates: Vec<CandidateJson>,
}

#[derive(Serialize)]
struct CandidateJson {
    formula: String,
    complexity: usize,
    mse: f64,
    r2: f64,
    max_abs_error: f64,
    exact: bool,
}

impl FitReport {
    pub fn new(target: &str, inputs: &[String], rows: usize, found: &[Candidate]) -> Self {
        FitReport {
            schema_version: SCHEMA_VERSION,
            target: target.to_string(),
            inputs: inputs.to_vec(),
            rows,
            candidates: found
                .iter()
                .map(|c| CandidateJson {
                    formula: c.expr.display_with(inputs).to_string(),
                    complexity: c.complexity,
                    mse: c.mse,
                    r2: c.r2,
                    max_abs_error: c.max_abs_error,
                    exact: c.exact,
                })
                .collect(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{} from {} ({} rows)\n",
            self.target,
            self.inputs.join(", "),
            self.rows
        );
        let _ = writeln!(s, "  {:>10}  {:>10}  formula", "complexity", "R²");
        for c in &self.candidates {
            let mark = if c.exact { "  (exact)" } else { "" };
            let _ = writeln!(
                s,
                "  {:>10}  {:>10.6}  {} = {}{mark}",
                c.complexity, c.r2, self.target, c.formula
            );
        }
        s
    }
}
