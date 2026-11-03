//! WebAssembly bindings used by the Regrex web app.
//!
//! The browser loads a file once into a [`Dataset`], then asks for one column
//! at a time so it can show progress. Everything crosses the boundary as
//! JSON strings, using the same report format as the command-line tool.

use regrex_core::parse::parse_template;
use regrex_core::{
    explain_column, fit_shape, search, DerivedColumn, ScanConfig, SearchConfig, UnaryOp,
};
use regrex_io::report::{DerivedJson, FitReport};
use regrex_io::Loaded;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
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
    /// Allowed operators: `+ - * / ^ sqrt exp log sin cos`. `-` also
    /// allows negation and `^` squares. Missing means all.
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
const OPERATORS: [&str; 10] = ["+", "-", "*", "/", "^", "sqrt", "exp", "log", "sin", "cos"];

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
        let table = self.table_without(&options.ignore, Some(column));
        let target = index_of(&table, column)?;
        let mut config = options.scan_config()?;
        let mut found = explain_column(&table, target, &config);
        if let Some(size) = options.deepen_to.map(|s| s.clamp(1, 9)) {
            if size > config.search.max_size && !explained_well(found.as_ref()) {
                config.search.max_size = size;
                let deeper = explain_column(&table, target, &config);
                let matched = |d: &Option<DerivedColumn>| d.as_ref().map_or(0, |d| d.matched);
                if matched(&deeper) > matched(&found) {
                    found = deeper;
                }
            }
        }
        if !options.ignore.iter().any(|n| n == column) {
            let mut cache = self.found.borrow_mut();
            if cache.0 != options.ignore {
                *cache = (options.ignore.clone(), Vec::new());
            }
            cache.1.retain(|d| d.target != target);
            cache.1.extend(found.clone());
        }
        let json = found.map(|d| DerivedJson::new(&table, &d));
        serde_json::to_string(&json).map_err(js_error)
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
    pub fn review(&self, options_json: &str) -> Result<String, JsError> {
        let options: ExplainOptions = parse_options(options_json)?;
        let cache = self.found.borrow();
        if cache.0 != options.ignore {
            return Ok("[]".to_string());
        }
        let table = self.table_without(&options.ignore, None);
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
