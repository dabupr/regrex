//! A scan report for AI agents.
//!
//! The [`ScanReport`](crate::report::ScanReport) is written for a person:
//! the formulas found and a few exceptions. An agent that reads the result
//! (through MCP or `regrex scan --agent`) can judge more: whether a formula
//! makes sense for what the columns hold, whether two near misses together
//! explain a column, which rows to search again. [`AgentReport`] gives it
//! what it needs to do that without searching again, in compact JSON:
//! every column with what it holds and whether it was searched, each
//! formula with its errors, the formulas the scan came close to accepting,
//! and the columns left unexplained with the inputs tried.
//!
//! Everything in it is computed by Regrex; deciding whether a formula is
//! plausible is left to the reader.

use crate::report::{DerivedJson, NearMissJson};
use crate::Loaded;
use regrex_core::{find_copies, inputs_tried, DerivedColumn, NearMiss, ScanConfig, Table};
use serde::Serialize;

/// Version of the agent report format. Fields may be added; existing ones
/// keep their meaning.
pub const AGENT_SCHEMA_VERSION: u32 = 1;

/// Rows listed in each row list of an [`AgentReport`]; the full count is
/// given beside it.
pub const MAX_ROWS: usize = 10;

/// Different values counted per column, at most.
const MAX_DISTINCT: usize = 1000;

/// A scan result for AI agents, see the [module](self) documentation.
#[derive(Debug, Clone, Serialize)]
pub struct AgentReport {
    /// Version of this format, see [`AGENT_SCHEMA_VERSION`].
    pub agent_schema_version: u32,
    /// The file scanned.
    pub file: String,
    /// Data rows.
    pub rows: usize,
    /// Every column of the file, in order.
    pub columns: Vec<AgentColumn>,
    /// Columns computed from others.
    pub found: Vec<AgentFormula>,
    /// Formulas found but not reported, most rows matched first.
    pub near_misses: Vec<AgentNearMiss>,
    /// Columns searched without a formula, with the inputs tried.
    pub unexplained: Vec<AgentUnexplained>,
    /// How to read this report, in a few sentences.
    pub notes: Vec<&'static str>,
}

/// One column of the file.
#[derive(Debug, Clone, Serialize)]
pub struct AgentColumn {
    /// Its name.
    pub name: String,
    /// `number` (read as numbers, maybe converted, see `read_as`) or `text`.
    pub kind: &'static str,
    /// How its cells were read when they are not plain numbers, such as
    /// `times as minutes`.
    pub read_as: Option<String>,
    /// Different values, counted up to 1000.
    pub distinct: usize,
    /// Empty or missing cells.
    pub missing: usize,
    /// Smallest and largest values, for numbers.
    pub min: Option<f64>,
    /// See `min`.
    pub max: Option<f64>,
    /// Whether every value is larger than the one before (a row number, an
    /// ID or a distance along a line).
    pub increasing: bool,
    /// Whether the scan looked for a formula for it.
    pub searched: bool,
    /// Why not, when it was not: `text`, `constant`, `flag` (two values),
    /// `copy` (the same values as another column, see `found`) or
    /// `not_requested`.
    pub not_searched: Option<&'static str>,
}

/// A formula found for a column.
#[derive(Debug, Clone, Serialize)]
pub struct AgentFormula {
    /// The column.
    pub column: String,
    /// Its formula, with column names.
    pub formula: String,
    /// Columns it uses.
    pub inputs: Vec<String>,
    /// Size of the formula.
    pub complexity: usize,
    /// Rows it matches, within the column's rounding.
    pub matched_rows: usize,
    /// Rows checked (with no missing value).
    pub checked_rows: usize,
    /// Whether it gives the exact values (up to floating point error) on
    /// every matched row, rather than values within rounding.
    pub exact: bool,
    /// Largest absolute difference between the column and the formula on
    /// the matched rows.
    pub max_error: f64,
    /// Median absolute difference on the matched rows.
    pub median_error: f64,
    /// When the column is a copy of another one, that column.
    pub copy_of: Option<String>,
    /// The id of the library formula it is, such as `tax_amount`.
    pub known: Option<String>,
    /// A second formula for some of the other rows, and the rule that tells
    /// them apart, if any.
    pub other: Option<AgentOther>,
    /// Rows that follow neither formula.
    pub wrong_count: usize,
    /// The first of them (1 = first row after the header), with what is
    /// likely wrong when Regrex could reproduce the mistake.
    pub wrong: Vec<AgentWrongRow>,
    /// The same relation solved for its other columns, as `m = target / (g
    /// * z)`; those columns are not listed apart.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub equivalent: Vec<String>,
}

/// The second formula of a column, see [`AgentFormula::other`].
#[derive(Debug, Clone, Serialize)]
pub struct AgentOther {
    /// The formula, with column names.
    pub formula: String,
    /// Rows that follow it.
    pub rows: usize,
    /// The rule on another column that picks them out, if any.
    pub when: Option<String>,
}

/// A row that breaks a formula.
#[derive(Debug, Clone, Serialize)]
pub struct AgentWrongRow {
    /// The row, 1 = first row after the header.
    pub row: usize,
    /// The value in the file.
    pub actual: Option<f64>,
    /// What the formula gives.
    pub expected: Option<f64>,
    /// The mistake that reproduces it exactly, if one does, such as `used
    /// 0.1 instead of 0.21`.
    pub likely: Option<String>,
}

/// A formula found but not reported, see [`NearMissJson`].
#[derive(Debug, Clone, Serialize)]
pub struct AgentNearMiss {
    /// The column.
    pub column: String,
    /// The formula, with column names.
    pub formula: String,
    /// Rows it matches.
    pub matched_rows: usize,
    /// Rows it was checked on.
    pub checked_rows: usize,
    /// Why it was not reported: `coverage`, `usual_value`,
    /// `no_split_rule` or `other_values` (see the JSON report docs).
    pub reason: String,
    /// The reason in words.
    pub reason_text: String,
    /// The rule on another column that picks out the rows where it holds.
    pub holds_when: Option<String>,
    /// The regime it was searched within, if any.
    pub regime: Option<String>,
    /// Rows checked where it fails.
    pub missed_count: usize,
    /// The first of them.
    pub missed_rows: Vec<usize>,
}

/// A column searched without finding a formula.
#[derive(Debug, Clone, Serialize)]
pub struct AgentUnexplained {
    /// The column.
    pub column: String,
    /// The columns tried as its inputs; a formula over others cannot have
    /// been found.
    pub inputs_tried: Vec<String>,
}

impl AgentReport {
    /// The report for a scan of `loaded` that found `derived` and
    /// `near_misses`. `columns` are the columns the scan was limited to,
    /// or empty when it checked them all.
    pub fn new(
        file: &str,
        loaded: &Loaded,
        derived: &[DerivedColumn],
        near_misses: &[NearMiss],
        columns: &[String],
    ) -> Self {
        let table = &loaded.table;
        let config = ScanConfig::default();
        let copies = find_copies(table, &config);
        let requested = |name: &str| columns.is_empty() || columns.iter().any(|c| c == name);
        let mut out_columns = Vec::with_capacity(loaded.header.len());
        for (i, header) in loaded.header.iter().enumerate() {
            // Text read as categories is still text for the agent.
            let numeric = table
                .names
                .iter()
                .position(|n| n == header || regrex_core::units::split_header(header).0 == *n)
                .filter(|&c| !table.is_category(c));
            out_columns.push(match numeric {
                Some(c) => {
                    let values = &table.columns[c];
                    let finite: Vec<f64> =
                        values.iter().copied().filter(|v| v.is_finite()).collect();
                    let distinct = distinct_numbers(&finite);
                    let not_searched = if copies[c].is_some() {
                        Some("copy")
                    } else if distinct <= 1 {
                        Some("constant")
                    } else if distinct == 2 {
                        Some("flag")
                    } else if !requested(&table.names[c]) {
                        Some("not_requested")
                    } else {
                        None
                    };
                    AgentColumn {
                        name: table.names[c].clone(),
                        kind: "number",
                        read_as: loaded
                            .converted
                            .iter()
                            .find(|(h, _)| h == header)
                            .map(|(_, how)| how.clone()),
                        distinct,
                        missing: values.len() - finite.len(),
                        min: finite.iter().copied().reduce(f64::min),
                        max: finite.iter().copied().reduce(f64::max),
                        increasing: finite.len() > 1 && finite.windows(2).all(|w| w[1] > w[0]),
                        searched: not_searched.is_none(),
                        not_searched,
                    }
                }
                None => {
                    let cells: Vec<&str> = loaded
                        .rows
                        .iter()
                        .map(|r| r.get(i).map_or("", |s| s.trim()))
                        .collect();
                    let present: Vec<&str> =
                        cells.iter().copied().filter(|s| !s.is_empty()).collect();
                    let mut seen: Vec<&str> = Vec::new();
                    for s in &present {
                        if seen.len() >= MAX_DISTINCT {
                            break;
                        }
                        if !seen.contains(s) {
                            seen.push(s);
                        }
                    }
                    AgentColumn {
                        name: header.clone(),
                        kind: "text",
                        read_as: None,
                        distinct: seen.len(),
                        missing: cells.len() - present.len(),
                        min: None,
                        max: None,
                        increasing: false,
                        searched: false,
                        not_searched: Some("text"),
                    }
                }
            });
        }
        let found = crate::report::relations(table, derived)
            .into_iter()
            .map(|(i, forms)| AgentFormula {
                equivalent: forms
                    .into_iter()
                    .map(|e| format!("{} = {}", e.column, e.formula))
                    .collect(),
                ..formula(table, &derived[i], derived)
            })
            .collect();
        let mut misses: Vec<AgentNearMiss> = near_misses
            .iter()
            .map(|m| {
                let j = NearMissJson::new(table, m);
                AgentNearMiss {
                    column: j.column,
                    formula: j.formula,
                    matched_rows: j.matched_rows,
                    checked_rows: j.checked_rows,
                    reason: j.reason,
                    reason_text: j.reason_text,
                    holds_when: j.holds_when.map(|s| s.rule),
                    regime: j.regime,
                    missed_count: j.missed_count,
                    missed_rows: j.missed_rows.into_iter().take(MAX_ROWS).collect(),
                }
            })
            .collect();
        misses.sort_by_key(|m| std::cmp::Reverse(m.matched_rows));
        let unexplained = (0..table.columns.len())
            .filter(|&c| requested(&table.names[c]) && !derived.iter().any(|d| d.target == c))
            .filter_map(|c| {
                let inputs = inputs_tried(table, c, &config, &copies);
                (!inputs.is_empty()).then(|| AgentUnexplained {
                    column: table.names[c].clone(),
                    inputs_tried: inputs.iter().map(|&i| table.names[i].clone()).collect(),
                })
            })
            .collect();
        AgentReport {
            agent_schema_version: AGENT_SCHEMA_VERSION,
            file: file.to_string(),
            rows: table.n_rows(),
            columns: out_columns,
            found,
            near_misses: misses,
            unexplained,
            notes: NOTES.to_vec(),
        }
    }

    /// Compact JSON, on one line.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("report is serializable")
    }
}

/// [`AgentReport::notes`].
const NOTES: [&str; 4] = [
    "Rows are numbered from 1, the first row after the header. Row lists show the first 10; the *_count fields give how many there are.",
    "A formula is reported when it matches at least 90% of the checked rows within the column's rounding. near_misses are formulas that matched fewer rows, or only rows picked out by a rule on another column: they may be real rules of part of the data, or coincidences. Judge them by what the columns mean.",
    "Text columns are not searched. A text column with few distinct values may be the category that decides which formula a row follows.",
    "unexplained lists the inputs tried for each column: a formula using other columns cannot have been found, so it may exist.",
];

/// Different values among `finite`, counting up to [`MAX_DISTINCT`].
fn distinct_numbers(finite: &[f64]) -> usize {
    let mut seen: Vec<f64> = Vec::new();
    for &v in finite {
        if seen.len() >= MAX_DISTINCT {
            break;
        }
        if !seen.contains(&v) {
            seen.push(v);
        }
    }
    seen.len()
}

/// [`AgentFormula`] for `d`, found in the same scan as `all`.
fn formula(table: &Table, d: &DerivedColumn, all: &[DerivedColumn]) -> AgentFormula {
    let json = DerivedJson::with_context(table, d, all);
    let y = &table.columns[d.target];
    let missed: std::collections::HashSet<usize> = d.exceptions.iter().copied().collect();
    let mut row = vec![0.0; table.columns.len()];
    let mut errors: Vec<f64> = Vec::new();
    let mut exact = true;
    for (r, &actual) in y.iter().enumerate() {
        if missed.contains(&r) || !actual.is_finite() {
            continue;
        }
        for (c, v) in row.iter_mut().enumerate() {
            *v = table.columns[c][r];
        }
        let predicted = d.formula.eval_row(&row, &[]);
        if !predicted.is_finite() {
            continue;
        }
        let error = (predicted - actual).abs();
        exact &= error <= 1e-9 * actual.abs().max(1.0);
        errors.push(error);
    }
    errors.sort_by(f64::total_cmp);
    // An approximate relation (a slope per stretch) has no value per row
    // to compare with, but its largest error is known.
    exact &= d.within.is_none();
    let max_error = errors
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(d.within.unwrap_or(0.0));
    let median_error = errors.get(errors.len() / 2).copied().unwrap_or(0.0);
    let other_rows = json.other.as_ref().map_or(&[][..], |o| o.rows.as_slice());
    let wrong_rows: Vec<usize> = json
        .exception_rows
        .iter()
        .copied()
        .filter(|r| other_rows.binary_search(r).is_err())
        .collect();
    let finite = |v: f64| v.is_finite().then_some(v);
    let wrong = wrong_rows
        .iter()
        .take(MAX_ROWS)
        .map(|&r| {
            let detail = json.exceptions.iter().find(|x| x.row == r);
            AgentWrongRow {
                row: r,
                actual: detail.and_then(|x| finite(x.actual)),
                expected: detail.and_then(|x| finite(x.expected)),
                likely: detail
                    .and_then(|x| x.diagnosis.as_ref())
                    .map(|m| m.description.clone()),
            }
        })
        .collect();
    AgentFormula {
        column: json.column,
        formula: json.formula,
        inputs: json.inputs,
        complexity: json.complexity,
        matched_rows: json.matched_rows,
        checked_rows: json.checked_rows,
        exact,
        max_error,
        median_error,
        copy_of: json.copy_of,
        known: json.known.map(|k| k.id),
        other: json.other.map(|o| AgentOther {
            formula: o.formula,
            rows: o.rows.len(),
            when: o.split.map(|s| s.rule),
        }),
        wrong_count: wrong_rows.len(),
        wrong,
        equivalent: Vec::new(),
    }
}
