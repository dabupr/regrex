//! Human-readable and JSON reports.
//!
//! The JSON output is versioned with `schema_version` and is meant to be
//! stable: fields may be added, but existing ones keep their meaning.

use crate::Loaded;
use regrex_core::expr::format_number;
use regrex_core::{diagnose_exception, Candidate, DerivedColumn, ScanConfig, Table};
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
    /// The well-known formula this is, if it is one.
    pub known: Option<KnownJson>,
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
    /// The most likely mistake that gives the value in the file, if a simple
    /// one does.
    pub diagnosis: Option<DiagnosisJson>,
}

/// A well-known formula that a result matches, from the built-in library
/// (`regrex-formulas`).
#[derive(Debug, Clone, Serialize)]
pub struct KnownJson {
    /// Library id, such as `kinetic_energy`.
    pub id: String,
    /// Name, such as `Kinetic energy`.
    pub name: String,
    /// Domain id, such as `physics`.
    pub domain: String,
    /// What it computes, when the library says.
    pub description: Option<String>,
    /// The library's expression, with its own symbols (`0.5 * m * v^2`).
    pub expression: String,
    /// What the formula computes, with its symbol and unit.
    pub output: KnownQuantityJson,
    /// Each variable of the formula and the column or value that gives it.
    pub variables: Vec<KnownQuantityJson>,
    /// A library constant with another value in the data, such as `g = 10`.
    pub changed_constant: Option<ChangedConstantJson>,
    /// Whether the result is exactly the library formula (every variable is a
    /// column and constants have their usual values).
    pub exact: bool,
    /// Where the formula comes from.
    pub source_title: String,
    /// Link to read more about it.
    pub source_url: Option<String>,
}

/// A quantity of a [`KnownJson`] formula.
#[derive(Debug, Clone, Serialize)]
pub struct KnownQuantityJson {
    /// Symbol in the library expression.
    pub symbol: String,
    /// What it is.
    pub name: String,
    /// Its unit in the library.
    pub unit: String,
    /// The column that gives it, if any.
    pub column: Option<String>,
    /// The fixed value it takes in the data, if it is not a column.
    pub value: Option<f64>,
}

/// A library constant with another value, in a [`KnownJson`].
#[derive(Debug, Clone, Serialize)]
pub struct ChangedConstantJson {
    /// Constant id, such as `g`.
    pub id: String,
    /// Its usual value.
    pub library_value: f64,
    /// The value the data uses.
    pub value: f64,
}

impl KnownJson {
    /// Looks up a function of some named columns in the built-in library.
    pub fn identify(
        candidate: &dyn Fn(&[f64]) -> f64,
        input_names: &[String],
        output_name: &str,
    ) -> Option<KnownJson> {
        use regrex_formulas::{Binding, Library};
        let names: Vec<&str> = input_names.iter().map(String::as_str).collect();
        let found = Library::builtin().identify(candidate, &names, Some(output_name));
        let m = found.into_iter().next()?;
        let f = m.formula;
        let quantity =
            |q: &regrex_formulas::Quantity, column: Option<String>, value| KnownQuantityJson {
                symbol: q.symbol.clone(),
                name: q.name.clone(),
                unit: q.unit.clone(),
                column,
                value,
            };
        Some(KnownJson {
            id: f.id.clone(),
            name: f.name.clone(),
            domain: f.domain.clone(),
            description: f.description.clone(),
            expression: f.expr_text.clone(),
            output: quantity(&f.output, Some(output_name.to_string()), None),
            variables: f
                .variables
                .iter()
                .zip(&m.variables)
                .map(|(q, b)| match b {
                    Binding::Input(i) => quantity(q, Some(input_names[*i].clone()), None),
                    Binding::Value(v) => quantity(q, None, Some(*v)),
                })
                .collect(),
            exact: m.is_exact(),
            changed_constant: m.changed_constant.map(|c| ChangedConstantJson {
                id: c.id,
                library_value: c.library_value,
                value: c.value,
            }),
            source_title: f.source.title.clone(),
            source_url: f.source.url.clone(),
        })
    }

    /// One line for text output: `kinetic energy (physics)`, with what
    /// differs from the library.
    pub fn summary(&self) -> String {
        let mut s = format!("known formula: {}", self.name);
        let fixed: Vec<String> = self
            .variables
            .iter()
            .filter_map(|v| {
                v.value
                    .map(|x| format!("{} = {}", v.symbol, format_number(x)))
            })
            .collect();
        if !fixed.is_empty() {
            s += &format!(" with {}", fixed.join(", "));
        }
        if let Some(c) = &self.changed_constant {
            s += &format!(
                " but {} = {} instead of {}",
                c.id,
                format_number(c.value),
                format_number(c.library_value)
            );
        }
        s
    }
}

/// A mistake that explains an exception, see [`regrex_core::diagnose`].
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisJson {
    /// Kind of mistake: `rounding`, `typo`, `sign`, `factor`,
    /// `wrong_constant`, `wrong_variable`, `swapped_variables`,
    /// `missing_operation`, `missing_term`, `wrong_operation` or
    /// `swapped_operands`.
    pub kind: String,
    /// What went wrong, such as `used 0.1 instead of 0.21`.
    pub description: String,
    /// The formula the row follows instead, when the mistake is in the formula.
    pub formula: Option<String>,
}

/// At most this many exceptions are detailed in a report.
pub const MAX_EXCEPTION_DETAILS: usize = 100;

impl DerivedJson {
    /// Converts a [`DerivedColumn`] found in `table`.
    pub fn new(table: &Table, d: &DerivedColumn) -> Self {
        Self::with_context(table, d, &[])
    }

    /// Like [`DerivedJson::new`], with every column found in the same scan,
    /// so a wrong cell that breaks several formulas is blamed only once.
    pub fn with_context(table: &Table, d: &DerivedColumn, all: &[DerivedColumn]) -> Self {
        let names = &table.names;
        let exceptions = d
            .exceptions
            .iter()
            .take(MAX_EXCEPTION_DETAILS)
            .map(|&r| {
                let row: Vec<f64> = table.columns.iter().map(|c| c[r]).collect();
                let diagnosis = diagnose_exception(table, d, all, r, &ScanConfig::default())
                    .into_iter()
                    .next()
                    .map(|m| DiagnosisJson {
                        kind: m.kind.id().to_string(),
                        description: m.description,
                        formula: m.formula.map(|f| f.display_with(names).to_string()),
                    });
                ExceptionJson {
                    row: r + 1,
                    actual: table.columns[d.target][r],
                    expected: d.formula.eval_row(&row, &[]),
                    diagnosis,
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
            known: known_column(table, d),
        }
    }
}

/// Looks up the formula of a derived column in the library.
fn known_column(table: &Table, d: &DerivedColumn) -> Option<KnownJson> {
    let n = table.columns.len();
    let candidate = |values: &[f64]| {
        let mut row = vec![0.0; n];
        for (k, &c) in d.inputs.iter().enumerate() {
            row[c] = values[k];
        }
        d.formula.eval_row(&row, &[])
    };
    let names: Vec<String> = d.inputs.iter().map(|&i| table.names[i].clone()).collect();
    KnownJson::identify(&candidate, &names, &table.names[d.target])
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
                .map(|d| DerivedJson::with_context(&loaded.table, d, derived))
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
            let known = d
                .known
                .as_ref()
                .map(|k| format!("\n  {:width$}   {}", "", k.summary()))
                .unwrap_or_default();
            let why = |x: &ExceptionJson| {
                x.diagnosis
                    .as_ref()
                    .map(|m| format!(" ({})", m.description))
                    .unwrap_or_default()
            };
            if let [only] = d.exceptions.as_slice() {
                let _ = writeln!(
                    s,
                    "   exception: row {} is {}, formula gives {}{}",
                    only.row,
                    format_number(only.actual),
                    format_number(only.expected),
                    why(only)
                );
                if !known.is_empty() {
                    let _ = writeln!(s, "{}", known.trim_start_matches('\n'));
                }
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
                // Explain the first few, when there is something to say.
                for x in d
                    .exceptions
                    .iter()
                    .take(5)
                    .filter(|x| x.diagnosis.is_some())
                {
                    let _ = write!(
                        s,
                        "\n      row {}: {} instead of {}{}",
                        x.row,
                        format_number(x.actual),
                        format_number(x.expected),
                        why(x)
                    );
                }
            }
            let _ = writeln!(s, "{known}");
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
    /// The well-known formula this is, if it is one.
    pub known: Option<KnownJson>,
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
                    known: KnownJson::identify(
                        &|x: &[f64]| c.expr.eval_row(x, &[]),
                        inputs,
                        target,
                    ),
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
            let known = c
                .known
                .as_ref()
                .map(|k| format!("  [{}]", k.summary()))
                .unwrap_or_default();
            let _ = writeln!(
                s,
                "  {:>10}  {:>10.6}  {} = {}{mark}{known}",
                c.complexity, c.r2, self.target, c.formula
            );
        }
        s
    }
}
