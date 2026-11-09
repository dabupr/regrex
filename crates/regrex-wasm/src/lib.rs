//! WebAssembly bindings used by the Regrex web app.
//!
//! The browser loads a file once into a [`Dataset`], then asks for one column
//! at a time so it can show progress. Everything crosses the boundary as
//! JSON strings, using the same report format as the command-line tool.

mod library_search;

use regrex_core::parse::{parse_expr, parse_template};
use regrex_core::{
    explain_column, explain_column_with_copies, find_copies, fit_shape, search, DerivedColumn,
    ScanConfig, SearchConfig, UnaryOp,
};
use regrex_io::report::{DerivedJson, FitReport};
use regrex_io::Loaded;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Number of rows shown in the preview of a file.
const PREVIEW_ROWS: usize = 20;

/// A file loaded in memory.
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
}

#[derive(Serialize)]
struct ColumnInfo<'a> {
    name: &'a str,
    decimals: Option<u32>,
    missing: usize,
    /// How the cells were read when they are not plain numbers, such as
    /// `times as minutes`.
    read_as: Option<&'a str>,
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
    /// Loads a CSV file or a spreadsheet from its bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(file_name: &str, bytes: &[u8], sheet: Option<String>) -> Result<Dataset, JsError> {
        let loaded = regrex_io::load_bytes(file_name, bytes, sheet.as_deref()).map_err(js_error)?;
        Ok(Dataset {
            loaded,
            found: RefCell::default(),
            copies: RefCell::default(),
            table: RefCell::default(),
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
                .map(|((name, values), decimals)| ColumnInfo {
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
                })
                .collect(),
            skipped: &self.loaded.skipped,
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
        let options: ExplainOptions = parse_options(options_json)?;
        let table = self.table_keeping(&options.ignore, column);
        let table = table.as_ref();
        let target = index_of(table, column)?;
        let mut config = options.scan_config()?;
        if options.library_only {
            let domain = options.domain.as_deref().filter(|d| !d.is_empty());
            if let Some(d) = domain {
                if regrex_formulas::Library::builtin().domain(d).is_none() {
                    return Err(js_error(format!("unknown domain {d:?}")));
                }
            }
            let found = library_search::best_library_formula(table, target, domain, &config);
            return self.remember(&options.ignore, column, table, target, found);
        }
        // Copies of other columns are found once for the whole table. An
        // ignored column explained on its own is searched as before (its
        // table has one more column, so the copies do not line up).
        let copies = if options.ignore.iter().any(|n| n == column) {
            None
        } else {
            Some(self.copies(&options.ignore, table, &config))
        };
        let explain = |config: &ScanConfig| match &copies {
            Some(copies) => explain_column_with_copies(table, target, config, copies),
            None => explain_column(table, target, config),
        };
        let mut found = explain(&config);
        if found.as_ref().is_some_and(|d| d.copy) {
            return self.remember(&options.ignore, column, table, target, found);
        }
        if let Some(size) = options.deepen_to.map(|s| s.clamp(1, 9)) {
            if size > config.search.max_size && !explained_well(found.as_ref()) {
                config.search.max_size = size;
                let deeper = explain(&config);
                let matched = |d: &Option<DerivedColumn>| d.as_ref().map_or(0, |d| d.matched);
                if matched(&deeper) > matched(&found) {
                    found = deeper;
                }
            }
        }
        self.remember(&options.ignore, column, table, target, found)
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
    /// order (`null` for one whose formula no longer reads).
    pub fn review(&self, options_json: &str) -> Result<String, JsError> {
        let options: ReviewOptions = parse_options(options_json)?;
        let table = self.table_without(&options.ignore, None);
        if let Some(items) = &options.derived {
            let rebuilt: Vec<Option<DerivedColumn>> =
                items.iter().map(|item| item.rebuild(&table)).collect();
            let all: Vec<DerivedColumn> = rebuilt.iter().flatten().cloned().collect();
            let json: Vec<Option<DerivedJson>> = rebuilt
                .iter()
                .map(|d| {
                    d.as_ref()
                        .map(|d| DerivedJson::with_context(&table, d, &all))
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
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
        let info: serde_json::Value = serde_json::from_str(&d.info()).unwrap();
        assert_eq!(info["rows"], 6);
        assert_eq!(info["columns"][1]["name"], "price");
        assert_eq!(info["columns"][1]["decimals"], 2);
    }

    #[test]
    fn explain_finds_the_formula() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
        let out = d.explain("total", r#"{"ignore":["id"]}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["formula"], "price * qty");
        assert_eq!(d.explain("id", r#"{"ignore":["id"]}"#).unwrap(), "null");
    }

    #[test]
    fn review_of_formulas_found_by_other_workers_matches_one_worker() {
        const INVOICES: &str = include_str!("../../regrex-cli/tests/invoices.csv");
        let opts = r#"{"ignore":["invoice"]}"#;
        let columns = ["subtotal", "net", "vat", "total"];
        // One worker finds everything and reviews it.
        let one = Dataset::new("i.csv", INVOICES.as_bytes(), None).unwrap();
        for c in columns {
            one.explain(c, opts).unwrap();
        }
        let expected: serde_json::Value = serde_json::from_str(&one.review(opts).unwrap()).unwrap();
        // Two workers share the columns; a third reviews what they found.
        let a = Dataset::new("i.csv", INVOICES.as_bytes(), None).unwrap();
        let b = Dataset::new("i.csv", INVOICES.as_bytes(), None).unwrap();
        let found: Vec<serde_json::Value> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let worker = if i % 2 == 0 { &a } else { &b };
                serde_json::from_str(&worker.explain(c, opts).unwrap()).unwrap()
            })
            .filter(|v: &serde_json::Value| !v.is_null())
            .collect();
        let main = Dataset::new("i.csv", INVOICES.as_bytes(), None).unwrap();
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
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
        let opts =
            r#"{"ignore":["id"],"max_size":3,"deepen_to":5,"max_inputs":2,"rel_tolerance":0.001}"#;
        let v: serde_json::Value =
            serde_json::from_str(&d.explain("total", opts).unwrap()).unwrap();
        assert_eq!(v["formula"], "price * qty");
        // Without a formula, longer formulas are tried.
        assert!(!explained_well(None));
    }

    #[test]
    fn values_pairs_the_file_with_the_formula() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&d.values("total", "price * qty").unwrap()).unwrap();
        assert_eq!(v["actual"][2], 9.0);
        assert_eq!(v["predicted"][2], 9.0);
        assert_eq!(v["predicted"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn fit_lists_candidates() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
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
