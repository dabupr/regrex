//! Human-readable and JSON reports.
//!
//! The JSON output is versioned with `schema_version` and is meant to be
//! stable: fields may be added, but existing ones keep their meaning.

use crate::Loaded;
use regrex_core::expr::format_number;
use regrex_core::{Candidate, DerivedColumn, Table};
use serde::Serialize;
use std::fmt::Write;

/// Version of the JSON output format.
pub const SCHEMA_VERSION: u32 = 1;

/// Result of scanning a file, see [`regrex_core::scan()`].
#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    /// Version of this format, see [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The file that was scanned.
    pub file: String,
    /// Number of data rows.
    pub rows: usize,
    /// Numeric columns.
    pub columns: Vec<String>,
    /// Columns that were not numeric.
    pub skipped_columns: Vec<String>,
    /// Columns computed from others.
    pub derived: Vec<DerivedJson>,
}

/// One derived column in a [`ScanReport`].
#[derive(Debug, Clone, Serialize)]
pub struct DerivedJson {
    /// The derived column.
    pub column: String,
    /// Its formula, using column names.
    pub formula: String,
    /// Columns the formula uses.
    pub inputs: Vec<String>,
    /// Weighted complexity of the formula.
    pub complexity: usize,
    /// Rows that follow the formula.
    pub matched_rows: usize,
    /// Rows that could be checked (no missing values).
    pub checked_rows: usize,
    /// `matched_rows / checked_rows`.
    pub match_ratio: f64,
    /// Data rows (1 = first row after the header) that break the formula.
    pub exception_rows: Vec<usize>,
    /// Details of the first [`MAX_EXCEPTION_DETAILS`] exceptions.
    pub exceptions: Vec<ExceptionJson>,
}

/// A row that does not follow the formula of its column.
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionJson {
    /// Data row (1 = first row after the header).
    pub row: usize,
    /// The value in the file.
    pub actual: f64,
    /// The value the formula gives.
    pub expected: f64,
}

/// At most this many exceptions are detailed in a report.
pub const MAX_EXCEPTION_DETAILS: usize = 100;

impl DerivedJson {
    /// Converts a [`DerivedColumn`] found in `table`.
    pub fn new(table: &Table, d: &DerivedColumn) -> Self {
        let names = &table.names;
        let exceptions = d
            .exceptions
            .iter()
            .take(MAX_EXCEPTION_DETAILS)
            .map(|&r| {
                let row: Vec<f64> = table.columns.iter().map(|c| c[r]).collect();
                ExceptionJson {
                    row: r + 1,
                    actual: table.columns[d.target][r],
                    expected: d.formula.eval_row(&row, &[]),
                }
            })
            .collect();
        DerivedJson {
            column: names[d.target].clone(),
            formula: d.formula_text.clone(),
            inputs: d.inputs.iter().map(|&i| names[i].clone()).collect(),
            complexity: d.complexity,
            matched_rows: d.matched,
            checked_rows: d.checked,
            match_ratio: d.match_ratio(),
            exception_rows: d.exceptions.iter().map(|r| r + 1).collect(),
            exceptions,
        }
    }
}

impl ScanReport {
    /// Builds the report for `derived` columns found in `loaded`.
    pub fn new(file: &str, loaded: &Loaded, derived: &[DerivedColumn]) -> Self {
        let names = &loaded.table.names;
        ScanReport {
            schema_version: SCHEMA_VERSION,
            file: file.to_string(),
            rows: loaded.table.n_rows(),
            columns: names.clone(),
            skipped_columns: loaded.skipped.clone(),
            derived: derived
                .iter()
                .map(|d| DerivedJson::new(&loaded.table, d))
                .collect(),
        }
    }

    /// Pretty-printed JSON.
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

    /// Plain-text table for terminals.
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
            if let [only] = d.exceptions.as_slice() {
                let _ = writeln!(
                    s,
                    "   exception: row {} is {}, formula gives {}",
                    only.row,
                    format_number(only.actual),
                    format_number(only.expected)
                );
                continue;
            }
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

/// Candidate formulas for one column, see [`regrex_core::search()`].
#[derive(Debug, Clone, Serialize)]
pub struct FitReport {
    /// Version of this format, see [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The column explained.
    pub target: String,
    /// Columns used as inputs.
    pub inputs: Vec<String>,
    /// Rows used (rows with missing values are left out).
    pub rows: usize,
    /// Formulas, best first.
    pub candidates: Vec<CandidateJson>,
}

/// One formula in a [`FitReport`].
#[derive(Debug, Clone, Serialize)]
pub struct CandidateJson {
    /// The formula, using column names.
    pub formula: String,
    /// Weighted complexity.
    pub complexity: usize,
    /// Mean squared error.
    pub mse: f64,
    /// Coefficient of determination.
    pub r2: f64,
    /// Largest absolute error.
    pub max_abs_error: f64,
    /// Whether the formula is exact (up to floating point error).
    pub exact: bool,
}

impl FitReport {
    /// Builds the report for candidate formulas of `target`.
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

    /// Pretty-printed JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }

    /// Plain-text table for terminals.
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
