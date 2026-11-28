//! WebAssembly bindings used by the Regrex web app.
//!
//! The browser loads a file once into a [`Dataset`], then asks for one column
//! at a time so it can show progress. Everything crosses the boundary as
//! JSON strings, using the same report format as the command-line tool.

mod library_search;

use regrex_core::parse::{parse_expr, parse_template};
use regrex_core::{
    check_formula, explain_column_detailed, find_copies, fit_shape, search, DerivedColumn,
    Explanation, NearMiss, ScanConfig, ScanEvent, SearchConfig, UnaryOp,
};
use regrex_io::report::{relations, DerivedJson, EquivalentJson, FitReport, NearMissJson};
use regrex_io::Loaded;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Number of rows shown in the preview of a file.
const PREVIEW_ROWS: usize = 20;

/// A file loaded in memory.
/// A column, the `ignore` list and the sample size of a search on a sample.
type SampleKey = (String, Vec<String>, usize);

#[wasm_bindgen]
pub struct Dataset {
    loaded: Loaded,
    /// Formulas found by [`Dataset::explain`] with the `ignore` list they
    /// were found with, for [`Dataset::review`].
    found: RefCell<(Vec<String>, Vec<DerivedColumn>)>,
    /// Copies of other columns ([`find_copies`]) with the `ignore` list and
    /// `min_match` they were found with: found once, used for every column.
    copies: RefCell<Option<CopiesCache>>,
    /// The table without the last `ignore` list's columns, kept so that
    /// explaining each column of a wide file does not copy the whole table.
    table: RefCell<Option<(Vec<String>, Rc<regrex_core::Table>)>>,
    /// How a second file was matched with this one, if one was given.
    joined: Option<JoinedJson>,
    /// What the search of each column on a sample found, by column, `ignore`
    /// list and sample size: kept by a `sample_only` search so that
    /// confirming it on all rows does not search the sample again.
    sampled: RefCell<std::collections::HashMap<SampleKey, Explanation>>,
}

/// See [`Dataset::copies`].
struct CopiesCache {
    ignore: Vec<String>,
    min_match: f64,
    copies: Vec<Option<DerivedColumn>>,
}

/// Summary of a loaded file, returned by [`Dataset::info`].
#[derive(Serialize)]
struct Info<'a> {
    rows: usize,
    header: &'a [String],
    preview: &'a [Vec<String>],
    sheets: &'a [String],
    columns: Vec<ColumnInfo<'a>>,
    skipped: &'a [String],
    /// Text columns read as categories (not in `columns`, which are the
    /// numbers formulas are searched for).
    categories: &'a [String],
    /// How the table is laid out and what each column is for.
    structure: regrex_io::report::StructureJson,
    /// Relations between text columns (`key = line + "|" + track`).
    text_relations: Vec<regrex_io::report::TextRelationJson>,
    /// How the rows of a second file were matched, when one was given.
    joined: Option<JoinedJson>,
}

/// How the rows of a second file were matched with the first.
#[derive(Serialize, Clone)]
struct JoinedJson {
    /// Name of the second file.
    file: String,
    /// Columns matched on.
    keys: Vec<String>,
    /// Rows of the first file with a match.
    matched: usize,
    /// Rows of the first file.
    rows: usize,
    /// Rows of the second file left out because their keys repeat.
    repeated: usize,
}

#[derive(Serialize)]
struct ColumnInfo<'a> {
    name: &'a str,
    decimals: Option<u32>,
    missing: usize,
    /// How the cells were read when they are not plain numbers, such as
    /// `times as minutes`.
    read_as: Option<&'a str>,
    /// Different values, counting up to [`MAX_DISTINCT`].
    distinct: usize,
    /// Whether every present value is larger than the one before (as in a
    /// row number or an ID).
    increasing: bool,
}

/// Different values counted in [`ColumnInfo::distinct`]: enough to tell
/// constants, flags and codes from measurements.
const MAX_DISTINCT: usize = 1000;

/// Different finite values in `values`, up to `limit`.
fn distinct(values: &[f64], limit: usize) -> usize {
    let mut seen = std::collections::HashSet::new();
    for v in values.iter().filter(|v| v.is_finite()) {
        seen.insert(v.to_bits());
        if seen.len() >= limit {
            break;
        }
    }
    seen.len()
}

/// Whether the present values only go up.
fn increasing(values: &[f64]) -> bool {
    let present: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    present.len() > 1 && present.windows(2).all(|w| w[1] > w[0])
}

/// Options for [`Dataset::explain`], as JSON. Every field is optional.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ExplainOptions {
    /// Columns that may not be used as inputs (for example IDs).
    ignore: Vec<String>,
    min_match: Option<f64>,
    max_size: Option<usize>,
    /// Fitted constants allowed inside a formula.
    max_params: Option<usize>,
    /// Small random samples searched when a few rows spoil the fit.
    small_samples: Option<usize>,
    /// Allowed operators: `+ - * / ^ sqrt exp log sin cos div mod`. `-`
    /// also allows negation and `^` squares; `div` and `mod` are only tried
    /// on whole numbers. Missing means all.
    operators: Option<Vec<String>>,
    /// Relative tolerance for a row to match when the column's decimals are
    /// unknown.
    rel_tolerance: Option<f64>,
    /// How many of the most correlated columns are tried as inputs.
    max_inputs: Option<usize>,
    /// Search again with formulas up to this size, but only for columns
    /// that `max_size` does not explain well. Much faster than starting
    /// with the larger size, which most columns do not need.
    deepen_to: Option<usize>,
    /// Only try the formulas of the built-in library instead of searching.
    library_only: bool,
    /// With `library_only`, only the formulas of this domain (such as
    /// `finance`). Missing means every domain.
    domain: Option<String>,
    /// Only search these rows (numbered from 1), as when a near miss holds
    /// on part of the file. Row numbers in the answer are still those of
    /// the file.
    rows: Option<Vec<usize>>,
    /// Only search the rows where this formula for the column (written with
    /// column names, as a near miss gives it) does not hold: what the rest
    /// of the rows follow.
    failing: Option<String>,
    /// Search a sample of about this many rows (blocks spread over the
    /// file), then check on all rows the formula found there, as
    /// `regrex scan --quick`. A column without a formula on the sample is
    /// not searched on all rows. Ignored on small files and with `rows`.
    sample: Option<usize>,
    /// With `sample`, only search the sample and answer with what it
    /// found there, its rows counted on the sample: a quick first look.
    /// The same search with `sample` and without `sample_only` then
    /// checks it on all rows, without searching the sample again.
    sample_only: bool,
}

impl ExplainOptions {
    fn scan_config(&self) -> Result<ScanConfig, JsError> {
        let mut config = ScanConfig::default();
        if let Some(m) = self.min_match {
            config.min_match = m;
        }
        if let Some(s) = self.max_size {
            config.search.max_size = s.clamp(1, 9);
        }
        if let Some(p) = self.max_params {
            config.search.max_params = p.min(3);
        }
        if let Some(t) = self.rel_tolerance {
            config.rel_tolerance = t.clamp(0.0, 0.5);
        }
        if let Some(n) = self.max_inputs {
            config.max_inputs = n.clamp(1, 12);
        }
        if let Some(n) = self.small_samples {
            config.small_samples = n.min(16);
        }
        if let Some(ops) = &self.operators {
            for op in ops {
                if !OPERATORS.contains(&op.as_str()) {
                    return Err(js_error(format!(
                        "unknown operator {op:?}, expected one of {}",
                        OPERATORS.join(" ")
                    )));
                }
            }
            let has = |name: &str| ops.iter().any(|o| o == name);
            config.search.binary_ops.retain(|op| has(op.symbol()));
            config.search.unary_ops.retain(|op| match op {
                UnaryOp::Neg => has("-"),
                UnaryOp::Square => has("^"),
                other => has(other.name()),
            });
            config.integer_ops.retain(|op| has(op.symbol()));
        }
        Ok(config)
    }
}

/// Whether a formula was found and the rows it misses are few enough to be
/// mistakes in the data rather than a sign that a longer formula is needed.
fn explained_well(found: Option<&DerivedColumn>) -> bool {
    found.is_some_and(|d| d.exceptions.len() as f64 <= 0.05 * d.checked as f64)
}

/// Operator names accepted in [`ExplainOptions::operators`].
const OPERATORS: [&str; 12] = [
    "+", "-", "*", "/", "^", "sqrt", "exp", "log", "sin", "cos", "div", "mod",
];

/// Options for [`Dataset::review`], as JSON.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ReviewOptions {
    /// Columns left out of the table, as for [`Dataset::explain`].
    ignore: Vec<String>,
    /// Derived columns to diagnose together, instead of those this worker
    /// found.
    derived: Option<Vec<ReviewItem>>,
}

/// The parts of a derived column (as [`Dataset::explain`] returns it)
/// needed to diagnose its exceptions again.
#[derive(Deserialize)]
struct ReviewItem {
    column: String,
    formula: String,
    matched_rows: usize,
    checked_rows: usize,
    /// Numbered from 1.
    exception_rows: Vec<usize>,
    #[serde(default)]
    other: Option<ReviewOther>,
    #[serde(default)]
    copy_of: Option<String>,
}

#[derive(Deserialize)]
struct ReviewOther {
    formula: String,
    /// Numbered from 1.
    rows: Vec<usize>,
}

impl ReviewItem {
    fn rebuild(&self, table: &regrex_core::Table) -> Option<DerivedColumn> {
        let target = table.names.iter().position(|n| *n == self.column)?;
        let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
        let formula = parse_expr(&self.formula, &names).ok()?;
        let rows = |r: &[usize]| {
            r.iter()
                .filter_map(|r| r.checked_sub(1))
                .collect::<Vec<_>>()
        };
        let other = match &self.other {
            Some(o) => Some(regrex_core::scan::OtherFormula {
                formula_text: o.formula.clone(),
                formula: parse_expr(&o.formula, &names).ok()?,
                rows: rows(&o.rows),
                split: None,
            }),
            None => None,
        };
        let mut inputs = formula.vars();
        inputs.sort_unstable();
        inputs.dedup();
        Some(DerivedColumn {
            target,
            formula_text: self.formula.clone(),
            complexity: formula.complexity(),
            formula,
            inputs,
            matched: self.matched_rows,
            checked: self.checked_rows,
            exceptions: rows(&self.exception_rows),
            other,
            copy: self.copy_of.is_some(),
            regime: None,
            steps: false,
            within: None,
        })
    }
}

/// Options for [`Dataset::fit`], as JSON.
#[derive(Deserialize, Default)]
#[serde(default)]
struct FitOptions {
    inputs: Vec<String>,
    max_size: Option<usize>,
    results: Option<usize>,
}

fn js_error(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
impl Dataset {
    /// Loads a CSV file or a spreadsheet from its bytes. With a second
    /// file, its rows are matched with those of the first on the columns
    /// both have (see [`regrex_io::join()`]).
    #[wasm_bindgen(constructor)]
    pub fn new(
        file_name: &str,
        bytes: &[u8],
        sheet: Option<String>,
        with_name: Option<String>,
        with_bytes: Option<Vec<u8>>,
    ) -> Result<Dataset, JsError> {
        let mut loaded =
            regrex_io::load_bytes(file_name, bytes, sheet.as_deref()).map_err(js_error)?;
        let mut joined = None;
        if let (Some(name), Some(other)) = (with_name, with_bytes) {
            let b = regrex_io::load_bytes(&name, &other, None).map_err(js_error)?;
            let stem = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(s, _)| s)
                .to_string();
            let (j, how) = regrex_io::join(&loaded, &b, &stem).map_err(js_error)?;
            loaded = j;
            joined = Some(JoinedJson {
                file: name,
                keys: how.keys,
                matched: how.matched,
                rows: how.rows,
                repeated: how.repeated,
            });
        }
        Ok(Dataset {
            loaded,
            found: RefCell::default(),
            copies: RefCell::default(),
            table: RefCell::default(),
            joined,
            sampled: RefCell::default(),
        })
    }

    /// Column names, preview rows, sheets and per-column details, as JSON.
    pub fn info(&self) -> String {
        let t = &self.loaded.table;
        let info = Info {
            rows: t.n_rows(),
            header: &self.loaded.header,
            preview: &self.loaded.rows[..self.loaded.rows.len().min(PREVIEW_ROWS)],
            sheets: &self.loaded.sheets,
            columns: t
                .names
                .iter()
                .zip(&t.columns)
                .zip(&t.decimals)
                .enumerate()
                .filter(|(i, _)| !t.is_category(*i))
                .map(|(_, ((name, values), decimals))| ColumnInfo {
                    name,
                    decimals: *decimals,
                    missing: values.iter().filter(|v| v.is_nan()).count(),
                    read_as: self
                        .loaded
                        .converted
                        .iter()
                        .find(|(header, _)| {
                            regrex_core::units::split_header(header).0 == *name || header == name
                        })
                        .map(|(_, how)| how.as_str()),
                    distinct: distinct(values, MAX_DISTINCT),
                    increasing: increasing(values),
                })
                .collect(),
            skipped: &self.loaded.skipped,
            categories: &self.loaded.categories,
            structure: regrex_io::report::StructureJson::new(t),
            text_relations: regrex_io::text_relations(&self.loaded)
                .into_iter()
                .map(|r| regrex_io::report::TextRelationJson {
                    column: r.column,
                    formula: r.formula,
                    inputs: r.inputs,
                    matched_rows: r.matched,
                    checked_rows: r.checked,
                })
                .collect(),
            joined: self.joined.clone(),
        };
        serde_json::to_string(&info).expect("serializable")
    }

    /// The given rows (numbered from 1) as text, in the order given, as a
    /// JSON array of arrays of strings. Unknown rows are left out.
    pub fn rows(&self, rows_json: &str) -> Result<String, JsError> {
        let wanted: Vec<usize> = parse_options(rows_json)?;
        let rows: Vec<&Vec<String>> = wanted
            .iter()
            .filter_map(|&r| r.checked_sub(1).and_then(|i| self.loaded.rows.get(i)))
            .collect();
        serde_json::to_string(&rows).map_err(js_error)
    }

    /// Looks for a formula that computes `column` from the other columns.
    /// Returns the derived-column JSON, or `null` when none is found.
    pub fn explain(&self, column: &str, options_json: &str) -> Result<String, JsError> {
        let (json, _) = self.explain_with(column, options_json, &|_| {})?;
        Ok(json)
    }

    /// Like [`Dataset::explain`], telling `on_event` what the search is
    /// doing (one JSON object per call, such as `{"kind":"best", "column":…}`) and returning
    /// `{"found": derived column or null, "near_misses": [...]}`: the
    /// formulas it found but did not report, as in the command-line JSON.
    pub fn explain_detailed(
        &self,
        column: &str,
        options_json: &str,
        on_event: &js_sys::Function,
    ) -> Result<String, JsError> {
        let send = |json: String| {
            let _ = on_event.call1(&JsValue::NULL, &JsValue::from_str(&json));
        };
        let (found, near_misses) = self.explain_with(column, options_json, &send)?;
        Ok(format!(
            "{{\"found\":{found},\"near_misses\":{near_misses}}}"
        ))
    }

    /// [`Dataset::explain_detailed`] with the events as JSON strings: the
    /// derived column JSON and the near misses JSON.
    fn explain_with(
        &self,
        column: &str,
        options_json: &str,
        on_event: &dyn Fn(String),
    ) -> Result<(String, String), JsError> {
        let options: ExplainOptions = parse_options(options_json)?;
        let full = self.table_keeping(&options.ignore, column);
        // Searching part of the rows: a table of those rows, whose row
        // numbers are mapped back to the file's at the end.
        let mut wanted: Option<Vec<usize>> = options.rows.as_ref().map(|rows| {
            let mut rows: Vec<usize> = rows
                .iter()
                .filter_map(|r| r.checked_sub(1))
                .filter(|&r| r < full.n_rows())
                .collect();
            rows.sort_unstable();
            rows.dedup();
            rows
        });
        if let Some(formula) = &options.failing {
            let names: Vec<&str> = full.names.iter().map(String::as_str).collect();
            let expr = parse_expr(formula, &names).map_err(js_error)?;
            let target = index_of(&full, column)?;
            let failing = check_formula(&full, target, &expr, &options.scan_config()?).exceptions;
            wanted = Some(match wanted {
                Some(rows) => rows
                    .into_iter()
                    .filter(|r| failing.binary_search(r).is_ok())
                    .collect(),
                None => failing,
            });
        }
        let part = wanted.as_ref().map(|rows| rows_of(&full, rows));
        let table = part.as_ref().unwrap_or(&full);
        let target = index_of(table, column)?;
        let names = &table.names;
        let event = |e: ScanEvent| on_event(EventJson::new(names, &e).to_json());
        let mut config = options.scan_config()?;
        let near = |table: &regrex_core::Table, misses: &[NearMiss]| {
            let json: Vec<NearMissJson> = misses
                .iter()
                .map(|m| {
                    let mut m = m.clone();
                    if let Some(rows) = &wanted {
                        m.missed = m.missed.iter().map(|&r| rows[r]).collect();
                    }
                    NearMissJson::new(table, &m)
                })
                .collect();
            serde_json::to_string(&json).map_err(js_error)
        };
        if options.library_only {
            let domain = options.domain.as_deref().filter(|d| !d.is_empty());
            if let Some(d) = domain {
                if regrex_formulas::Library::builtin().domain(d).is_none() {
                    return Err(js_error(format!("unknown domain {d:?}")));
                }
            }
            let found = library_search::best_library_formula(table, target, domain, &config);
            let found = self.answer(&options, column, table, target, found, wanted.as_deref())?;
            return Ok((found, "[]".to_string()));
        }
        // Copies of other columns are found once for the whole table. An
        // ignored column explained on its own is searched as before (its
        // table has one more column, so the copies do not line up), and so
        // is a part of the rows.
        let copies = if options.ignore.iter().any(|n| n == column) || wanted.is_some() {
            Vec::new()
        } else {
            self.copies(&options.ignore, table, &config)
        };
        if let Some(size) = options
            .sample
            .filter(|_| wanted.is_none() && !copies.is_empty())
        {
            let rows = regrex_core::sample_rows(table.n_rows(), size.max(100));
            if rows.len() < table.n_rows() && copies[target].is_none() {
                return self.explain_sample(
                    &options, column, table, target, &config, &copies, &rows, on_event,
                );
            }
        }
        let explain =
            |config: &ScanConfig| explain_column_detailed(table, target, config, &copies, &event);
        let mut found: Explanation = explain(&config);
        if found.found.as_ref().is_some_and(|d| d.copy) {
            let json = self.answer(&options, column, table, target, found.found, None)?;
            return Ok((json, "[]".to_string()));
        }
        if let Some(size) = options.deepen_to.map(|s| s.clamp(1, 9)) {
            if size > config.search.max_size && !explained_well(found.found.as_ref()) {
                config.search.max_size = size;
                let deeper = explain(&config);
                let matched = |e: &Explanation| {
                    e.found.as_ref().map_or(0, |d| d.matched + d.checked)
                        + e.near_misses.first().map_or(0, |m| m.matched)
                };
                if matched(&deeper) > matched(&found) {
                    found = deeper;
                }
            }
        }
        let misses = near(table, &found.near_misses)?;
        let json = self.answer(
            &options,
            column,
            table,
            target,
            found.found,
            wanted.as_deref(),
        )?;
        Ok((json, misses))
    }

    /// [`Dataset::explain_with`] on the sample `rows` of `table`, with the
    /// formula found there checked on all rows.
    #[allow(clippy::too_many_arguments)]
    fn explain_sample(
        &self,
        options: &ExplainOptions,
        column: &str,
        table: &regrex_core::Table,
        target: usize,
        config: &ScanConfig,
        copies: &[Option<DerivedColumn>],
        rows: &[usize],
        on_event: &dyn Fn(String),
    ) -> Result<(String, String), JsError> {
        let sample = rows_of(table, rows);
        let names = &table.names;
        let total = table.n_rows();
        let event = |e: ScanEvent| {
            let mut json = EventJson::new(names, &e);
            if let EventJson::Best { rows_total, .. } = &mut json {
                *rows_total = Some(total);
            }
            on_event(json.to_json());
        };
        let key = (column.to_string(), options.ignore.clone(), rows.len());
        let kept = self.sampled.borrow_mut().remove(&key);
        let found =
            kept.unwrap_or_else(|| explain_column_detailed(&sample, target, config, &[], &event));
        if options.sample_only {
            let misses = self.sample_misses(&sample, &found.near_misses, rows)?;
            let json = self.answer(
                options,
                column,
                &sample,
                target,
                found.found.clone(),
                Some(rows),
            )?;
            if found.found.is_some() {
                self.sampled.borrow_mut().insert(key, found);
            }
            return Ok((json, misses));
        }
        let confirmed = found.found.as_ref().and_then(|d| {
            on_event(
                EventJson::Confirming {
                    column,
                    formula: d.formula_text.clone(),
                    sample: rows.len(),
                    rows: total,
                }
                .to_json(),
            );
            let profile = regrex_core::profile::profile(table);
            let skip: Vec<bool> = copies.iter().map(Option::is_some).collect();
            regrex_core::confirm(table, d, config, copies, &profile, &skip)
        });
        let misses = self.sample_misses(&sample, &found.near_misses, rows)?;
        let json = self.answer(options, column, table, target, confirmed, None)?;
        Ok((json, misses))
    }

    /// What came close on a sample, as JSON, with its rows numbered as in
    /// the file.
    fn sample_misses(
        &self,
        sample: &regrex_core::Table,
        misses: &[NearMiss],
        rows: &[usize],
    ) -> Result<String, JsError> {
        let json: Vec<NearMissJson> = misses
            .iter()
            .map(|m| {
                let mut m = m.clone();
                m.missed = m.missed.iter().map(|&r| rows[r]).collect();
                NearMissJson::new(sample, &m)
            })
            .collect();
        serde_json::to_string(&json).map_err(js_error)
    }

    /// The values of `column` and those of `formula` (written with column
    /// names, as in a derived column's `formula`) for every row, as JSON
    /// `{"actual": [...], "predicted": [...]}`; missing values are `null`.
    /// Used to plot what the file holds against what the formula gives.
    pub fn values(&self, column: &str, formula: &str) -> Result<String, JsError> {
        let table = &self.loaded.table;
        let target = index_of(table, column)?;
        let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
        let expr = parse_expr(formula, &names).map_err(js_error)?;
        let finite = |v: f64| v.is_finite().then_some(v);
        let mut row = vec![0.0; table.columns.len()];
        let predicted: Vec<Option<f64>> = (0..table.n_rows())
            .map(|r| {
                for (c, column) in table.columns.iter().enumerate() {
                    row[c] = column[r];
                }
                finite(expr.eval_row(&row, &[]))
            })
            .collect();
        let actual: Vec<Option<f64>> = table.columns[target].iter().map(|&v| finite(v)).collect();
        let json = serde_json::json!({ "actual": actual, "predicted": predicted });
        Ok(json.to_string())
    }

    /// Checks a formula written by the user, such as `0.21 * net`, against
    /// `column`. Names that are not columns are constants to fit, so
    /// `rate * net` finds the rate. Returns the derived-column JSON even
    /// when few rows match, so the page can show where the formula fails.
    pub fn formula(&self, column: &str, text: &str) -> Result<String, JsError> {
        let table = &self.loaded.table;
        let target = index_of(table, column)?;
        let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
        let (shape, params) = parse_template(text, &names).map_err(js_error)?;
        if params.len() > 4 {
            return Err(js_error(format!(
                "too many constants to fit ({}): {}; are these column names?",
                params.len(),
                params.join(", ")
            )));
        }
        if shape.vars().contains(&target) {
            return Err(js_error(format!("the formula uses {column} itself")));
        }
        let found = fit_shape(table, target, &shape, &ScanConfig::default())
            .ok_or_else(|| js_error("could not fit the constants of this formula"))?;
        serde_json::to_string(&DerivedJson::new(table, &found)).map_err(js_error)
    }

    /// Every formula found so far with the same `ignore` option, as a JSON
    /// array of derived columns, diagnosed together: a wrong cell that breaks
    /// several formulas is blamed once, as in the command-line scan.
    ///
    /// With `derived` in the options (derived columns as [`Dataset::explain`]
    /// returns them, found by any worker), those are diagnosed together
    /// instead, and the result has one entry per given column, in the same
    /// order (`null` for one whose formula no longer reads). The form of a
    /// relation to show lists the others in `equivalent`.
    pub fn review(&self, options_json: &str) -> Result<String, JsError> {
        let options: ReviewOptions = parse_options(options_json)?;
        let table = self.table_without(&options.ignore, None);
        if let Some(items) = &options.derived {
            let rebuilt: Vec<Option<DerivedColumn>> =
                items.iter().map(|item| item.rebuild(&table)).collect();
            let all: Vec<DerivedColumn> = rebuilt.iter().flatten().cloned().collect();
            // The other forms of each relation go with the form shown.
            let mut equivalent: Vec<Vec<EquivalentJson>> = vec![Vec::new(); all.len()];
            for (i, forms) in relations(&table, &all) {
                equivalent[i] = forms;
            }
            let mut index = 0;
            let json: Vec<Option<DerivedJson>> = rebuilt
                .iter()
                .map(|d| {
                    d.as_ref().map(|d| {
                        index += 1;
                        DerivedJson {
                            equivalent: std::mem::take(&mut equivalent[index - 1]),
                            ..DerivedJson::with_context(&table, d, &all)
                        }
                    })
                })
                .collect();
            return serde_json::to_string(&json).map_err(js_error);
        }
        let cache = self.found.borrow();
        if cache.0 != options.ignore {
            return Ok("[]".to_string());
        }
        let json: Vec<DerivedJson> = cache
            .1
            .iter()
            .map(|d| DerivedJson::with_context(&table, d, &cache.1))
            .collect();
        serde_json::to_string(&json).map_err(js_error)
    }

    /// Candidate formulas for `target`, as a fit report in JSON.
    pub fn fit(&self, target: &str, options_json: &str) -> Result<String, JsError> {
        let options: FitOptions = parse_options(options_json)?;
        let table = &self.loaded.table;
        let t = index_of(table, target)?;
        let inputs: Vec<usize> = if options.inputs.is_empty() {
            (0..table.columns.len()).filter(|&c| c != t).collect()
        } else {
            options
                .inputs
                .iter()
                .map(|n| index_of(table, n))
                .collect::<Result<_, _>>()?
        };
        let rows: Vec<usize> = (0..table.n_rows())
            .filter(|&r| {
                table.columns[t][r].is_finite()
                    && inputs.iter().all(|&c| table.columns[c][r].is_finite())
            })
            .collect();
        let x: Vec<Vec<f64>> = inputs
            .iter()
            .map(|&c| rows.iter().map(|&r| table.columns[c][r]).collect())
            .collect();
        let y: Vec<f64> = rows.iter().map(|&r| table.columns[t][r]).collect();
        let config = SearchConfig {
            max_size: options.max_size.unwrap_or(6),
            max_results: options.results.unwrap_or(10),
            ..SearchConfig::default()
        };
        let found = search(&x, &y, &config).map_err(js_error)?;
        let names: Vec<String> = inputs.iter().map(|&c| table.names[c].clone()).collect();
        Ok(FitReport::new(&table.names[t], &names, rows.len(), &found).to_json())
    }
}

impl Dataset {
    /// The derived column JSON for a search on `table`: kept for
    /// [`Dataset::review`] when it is on every row, and with the row
    /// numbers of the file when it is on the `rows` of it.
    fn answer(
        &self,
        options: &ExplainOptions,
        column: &str,
        table: &regrex_core::Table,
        target: usize,
        found: Option<DerivedColumn>,
        rows: Option<&[usize]>,
    ) -> Result<String, JsError> {
        let Some(rows) = rows else {
            return self.remember(&options.ignore, column, table, target, found);
        };
        let json = found.map(|d| {
            let mut json = DerivedJson::new(table, &d);
            let back = |r: &mut usize| *r = rows[*r - 1] + 1;
            json.exception_rows.iter_mut().for_each(back);
            json.exceptions.iter_mut().for_each(|x| back(&mut x.row));
            if let Some(o) = &mut json.other {
                o.rows.iter_mut().for_each(back);
            }
            json
        });
        serde_json::to_string(&json).map_err(js_error)
    }

    /// The copies of other columns in `table` (the loaded table without the
    /// `ignore`d columns), found on the first call with these options.
    fn copies(
        &self,
        ignore: &[String],
        table: &regrex_core::Table,
        config: &ScanConfig,
    ) -> Vec<Option<DerivedColumn>> {
        let mut cache = self.copies.borrow_mut();
        let fresh = cache
            .as_ref()
            .is_some_and(|c| c.ignore == ignore && c.min_match == config.min_match);
        if !fresh {
            *cache = Some(CopiesCache {
                ignore: ignore.to_vec(),
                min_match: config.min_match,
                copies: find_copies(table, config),
            });
        }
        cache.as_ref().map(|c| c.copies.clone()).unwrap_or_default()
    }

    /// Keeps a formula found by [`Dataset::explain`] for [`Dataset::review`]
    /// and returns it as JSON.
    fn remember(
        &self,
        ignore: &[String],
        column: &str,
        table: &regrex_core::Table,
        target: usize,
        found: Option<DerivedColumn>,
    ) -> Result<String, JsError> {
        if !ignore.iter().any(|n| n == column) {
            let mut cache = self.found.borrow_mut();
            if cache.0 != ignore {
                *cache = (ignore.to_vec(), Vec::new());
            }
            cache.1.retain(|d| d.target != target);
            cache.1.extend(found.clone());
        }
        let json = found.map(|d| DerivedJson::new(table, &d));
        serde_json::to_string(&json).map_err(js_error)
    }

    /// [`Dataset::table_without`] for explaining `column`: the same table
    /// for every column that is not ignored, made once.
    fn table_keeping(&self, ignore: &[String], column: &str) -> Rc<regrex_core::Table> {
        if ignore.iter().any(|n| n == column) {
            return Rc::new(self.table_without(ignore, Some(column)));
        }
        let mut cache = self.table.borrow_mut();
        match cache.as_ref() {
            Some((cached, table)) if cached == ignore => Rc::clone(table),
            _ => {
                let table = Rc::new(self.table_without(ignore, None));
                *cache = Some((ignore.to_vec(), Rc::clone(&table)));
                table
            }
        }
    }

    /// The table without the `ignore`d columns (the `keep` column is never
    /// dropped).
    fn table_without(&self, ignore: &[String], keep: Option<&str>) -> regrex_core::Table {
        let mut out = self.loaded.table.clone();
        for i in (0..out.names.len()).rev() {
            let name = &out.names[i];
            if ignore.contains(name) && Some(name.as_str()) != keep {
                out.remove_column(i);
            }
        }
        out
    }
}

/// The given rows (numbered from 0, in increasing order) of `table`.
fn rows_of(table: &regrex_core::Table, rows: &[usize]) -> regrex_core::Table {
    regrex_core::progressive::sub_table(table, rows)
}

/// A [`ScanEvent`] as sent to the page, with column names.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EventJson<'a> {
    Started {
        column: &'a str,
        inputs: Vec<&'a str>,
        rows: usize,
    },
    Best {
        column: &'a str,
        formula: String,
        matched: usize,
        checked: usize,
        /// Rows of the table, when the search only looks at a sample.
        #[serde(skip_serializing_if = "Option::is_none")]
        rows_total: Option<usize>,
    },
    /// A formula found on a sample of rows, now checked on all rows.
    Confirming {
        column: &'a str,
        formula: String,
        sample: usize,
        rows: usize,
    },
    SecondFormula {
        column: &'a str,
        rows: usize,
    },
    Done {
        column: &'a str,
        found: bool,
        near_misses: usize,
    },
}

impl<'a> EventJson<'a> {
    fn new(names: &'a [String], e: &ScanEvent) -> Self {
        let name = |c: usize| names[c].as_str();
        match e {
            ScanEvent::Started {
                target,
                inputs,
                rows,
            } => EventJson::Started {
                column: name(*target),
                inputs: inputs.iter().map(|&c| name(c)).collect(),
                rows: *rows,
            },
            ScanEvent::Best {
                target,
                formula,
                matched,
                checked,
            } => EventJson::Best {
                column: name(*target),
                formula: formula.clone(),
                matched: *matched,
                checked: *checked,
                rows_total: None,
            },
            ScanEvent::SecondFormula { target, rows } => EventJson::SecondFormula {
                column: name(*target),
                rows: *rows,
            },
            ScanEvent::Done {
                target,
                found,
                near_misses,
            } => EventJson::Done {
                column: name(*target),
                found: *found,
                near_misses: *near_misses,
            },
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("serializable")
    }
}

fn parse_options<T: for<'de> Deserialize<'de> + Default>(json: &str) -> Result<T, JsError> {
    if json.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(json).map_err(|e| js_error(format!("invalid options: {e}")))
}

fn index_of(table: &regrex_core::Table, name: &str) -> Result<usize, JsError> {
    table
        .names
        .iter()
        .position(|n| n == name)
        .ok_or_else(|| js_error(format!("no numeric column named {name:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str =
        "id,price,qty,total\n1,2.5,2,5\n2,1.25,4,5\n3,3,3,9\n4,10,1,10\n5,0.5,8,4\n6,7,2,14\n";

    #[test]
    fn info_lists_columns() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None, None, None).unwrap();
        let info: serde_json::Value = serde_json::from_str(&d.info()).unwrap();
        assert_eq!(info["rows"], 6);
        assert_eq!(info["columns"][1]["name"], "price");
        assert_eq!(info["columns"][1]["decimals"], 2);
    }

    #[test]
    fn explain_finds_the_formula() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None, None, None).unwrap();
        let out = d.explain("total", r#"{"ignore":["id"]}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["formula"], "price * qty");
        assert_eq!(d.explain("id", r#"{"ignore":["id"]}"#).unwrap(), "null");
    }

    /// `vat = 0.21 * net` on rows 1 to 14, `0.1 * net` on rows 15 to 30,
    /// except row 20, typed by hand.
    fn two_rates() -> String {
        let mut csv = String::from("net,vat\n");
        for i in 1..=30 {
            let net = 10.0 * i as f64 + 3.0;
            let vat = match i {
                20 => 99.0,
                1..=14 => net * 0.21,
                _ => net * 0.1,
            };
            csv += &format!("{net},{vat:.2}\n");
        }
        csv
    }

    #[test]
    fn explain_searches_the_rows_a_formula_misses() {
        let d = Dataset::new("a.csv", two_rates().as_bytes(), None, None, None).unwrap();
        let events = RefCell::new(Vec::new());
        let options = r#"{"failing":"0.21 * net"}"#;
        let (found, _) = d
            .explain_with("vat", options, &|e| events.borrow_mut().push(e))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&found).unwrap();
        assert_eq!(v["formula"], "0.1 * net");
        assert_eq!(v["checked_rows"], 16);
        assert_eq!(v["exception_rows"], serde_json::json!([20]));
        let events = events.into_inner();
        assert!(events[0].contains(r#""kind":"started""#), "{events:?}");
        assert!(
            events.last().unwrap().contains(r#""kind":"done""#),
            "{events:?}"
        );
    }

    #[test]
    fn review_of_formulas_found_by_other_workers_matches_one_worker() {
        const INVOICES: &str = include_str!("../../regrex-cli/tests/invoices.csv");
        let opts = r#"{"ignore":["invoice"]}"#;
        let columns = ["subtotal", "net", "vat", "total"];
        // One worker finds everything and reviews it.
        let one = Dataset::new("i.csv", INVOICES.as_bytes(), None, None, None).unwrap();
        for c in columns {
            one.explain(c, opts).unwrap();
        }
        let expected: serde_json::Value = serde_json::from_str(&one.review(opts).unwrap()).unwrap();
        // Two workers share the columns; a third reviews what they found.
        let a = Dataset::new("i.csv", INVOICES.as_bytes(), None, None, None).unwrap();
        let b = Dataset::new("i.csv", INVOICES.as_bytes(), None, None, None).unwrap();
        let found: Vec<serde_json::Value> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let worker = if i % 2 == 0 { &a } else { &b };
                serde_json::from_str(&worker.explain(c, opts).unwrap()).unwrap()
            })
            .filter(|v: &serde_json::Value| !v.is_null())
            .collect();
        let main = Dataset::new("i.csv", INVOICES.as_bytes(), None, None, None).unwrap();
        let review = serde_json::json!({ "ignore": ["invoice"], "derived": found }).to_string();
        let got: serde_json::Value = serde_json::from_str(&main.review(&review).unwrap()).unwrap();
        let exceptions = |v: &serde_json::Value| -> Vec<serde_json::Value> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|d| serde_json::json!([d["column"], d["exceptions"]]))
                .collect()
        };
        assert_eq!(exceptions(&got), exceptions(&expected));
        // Row 17 has a wrong total, which also breaks net = total - vat:
        // only the total is blamed, which needs both formulas together.
        assert!(got
            .to_string()
            .contains("the total in this row looks wrong"));
    }

    #[test]
    fn explain_accepts_advanced_limits() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None, None, None).unwrap();
        let opts =
            r#"{"ignore":["id"],"max_size":3,"deepen_to":5,"max_inputs":2,"rel_tolerance":0.001}"#;
        let v: serde_json::Value =
            serde_json::from_str(&d.explain("total", opts).unwrap()).unwrap();
        assert_eq!(v["formula"], "price * qty");
        // Without a formula, longer formulas are tried.
        assert!(!explained_well(None));
    }

    #[test]
    fn a_sample_is_searched_first_then_checked_on_all_rows() {
        let mut csv = String::from("a,b,sum,noise\n");
        for i in 0..3000u64 {
            let (a, b) = ((i * 7919) % 1000, (i * 104_729) % 997);
            csv += &format!("{a},{b},{},{}\n", a + b, (i * 31) % 101);
        }
        let d = Dataset::new("a.csv", csv.as_bytes(), None, None, None).unwrap();
        let events = RefCell::new(Vec::new());
        let (found, _) = d
            .explain_with("sum", r#"{"sample":200}"#, &|e| events.borrow_mut().push(e))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&found).unwrap();
        assert_eq!(v["formula"], "a + b");
        assert_eq!(v["checked_rows"], 3000);
        let events = events.into_inner();
        assert!(events
            .iter()
            .any(|e| e.contains(r#""kind":"confirming""#) && e.contains(r#""sample":200"#)));
        assert!(events.iter().any(|e| e.contains(r#""rows_total":3000"#)));
        // A first look on the sample alone, then the check on all rows.
        let (quick, _) = d
            .explain_with("sum", r#"{"sample":200,"sample_only":true}"#, &|_| {})
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&quick).unwrap();
        assert_eq!(
            (v["formula"].as_str(), v["checked_rows"].as_u64()),
            (Some("a + b"), Some(200))
        );
        let searched = RefCell::new(false);
        let (all, _) = d
            .explain_with("sum", r#"{"sample":200}"#, &|e| {
                *searched.borrow_mut() |= e.contains(r#""kind":"started""#)
            })
            .unwrap();
        assert!(all.contains(r#""checked_rows":3000"#) && !*searched.borrow());
        // A column without a formula on the sample is not searched on all rows.
        let (none, _) = d
            .explain_with("noise", r#"{"sample":200}"#, &|_| {})
            .unwrap();
        assert_eq!(none, "null");
    }

    #[test]
    fn values_pairs_the_file_with_the_formula() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None, None, None).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&d.values("total", "price * qty").unwrap()).unwrap();
        assert_eq!(v["actual"][2], 9.0);
        assert_eq!(v["predicted"][2], 9.0);
        assert_eq!(v["predicted"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn fit_lists_candidates() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None, None, None).unwrap();
        let out = d.fit("total", r#"{"inputs":["price","qty"]}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["candidates"][0]["formula"], "price * qty");
    }
}

/// One formula of the built-in library, for the library page.
#[derive(Serialize)]
struct LibraryFormula<'a> {
    id: &'a str,
    name: &'a str,
    domain: &'a str,
    description: Option<&'a str>,
    expression: &'a str,
    output: LibraryQuantity<'a>,
    variables: Vec<LibraryQuantity<'a>>,
    tags: &'a [String],
    source_title: &'a str,
    source_url: Option<&'a str>,
}

#[derive(Serialize)]
struct LibraryQuantity<'a> {
    symbol: &'a str,
    name: &'a str,
    unit: &'a str,
}

#[derive(Serialize)]
struct LibraryDomain<'a> {
    id: &'a str,
    name: &'a str,
    description: &'a str,
}

#[derive(Serialize)]
struct LibraryJson<'a> {
    domains: Vec<LibraryDomain<'a>>,
    formulas: Vec<LibraryFormula<'a>>,
}

/// The built-in library of known formulas, as JSON: `{domains, formulas}`.
#[wasm_bindgen]
pub fn library() -> String {
    let lib = regrex_formulas::Library::builtin();
    let quantity = |q: &'static regrex_formulas::Quantity| LibraryQuantity {
        symbol: &q.symbol,
        name: &q.name,
        unit: &q.unit,
    };
    let json = LibraryJson {
        domains: lib
            .domains
            .iter()
            .map(|d| LibraryDomain {
                id: &d.id,
                name: &d.name,
                description: &d.description,
            })
            .collect(),
        formulas: lib
            .formulas
            .iter()
            .map(|f| LibraryFormula {
                id: &f.id,
                name: &f.name,
                domain: &f.domain,
                description: f.description.as_deref(),
                expression: &f.expr_text,
                output: quantity(&f.output),
                variables: f.variables.iter().map(quantity).collect(),
                tags: &f.tags,
                source_title: &f.source.title,
                source_url: f.source.url.as_deref(),
            })
            .collect(),
    };
    serde_json::to_string(&json).expect("serializable")
}
