//! WebAssembly bindings used by the Regrex web app.
//!
//! The browser loads a file once into a [`Dataset`], then asks for one column
//! at a time so it can show progress. Everything crosses the boundary as
//! JSON strings, using the same report format as the command-line tool.

use regrex_core::{explain_column, search, ScanConfig, SearchConfig};
use regrex_io::report::{DerivedJson, FitReport};
use regrex_io::Loaded;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// A file loaded in memory.
#[wasm_bindgen]
pub struct Dataset {
    loaded: Loaded,
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
        Ok(Dataset { loaded })
    }

    /// Column names, preview rows, sheets and per-column details, as JSON.
    pub fn info(&self) -> String {
        let t = &self.loaded.table;
        let info = Info {
            rows: t.n_rows(),
            header: &self.loaded.header,
            preview: &self.loaded.preview,
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

    /// Looks for a formula that computes `column` from the other columns.
    /// Returns the derived-column JSON, or `null` when none is found.
    pub fn explain(&self, column: &str, options_json: &str) -> Result<String, JsError> {
        let options: ExplainOptions = parse_options(options_json)?;
        let table = self.table_without(&options.ignore, Some(column));
        let target = index_of(&table, column)?;
        let mut config = ScanConfig::default();
        if let Some(m) = options.min_match {
            config.min_match = m;
        }
        if let Some(s) = options.max_size {
            config.search.max_size = s;
        }
        let found = explain_column(&table, target, &config).map(|d| DerivedJson::new(&table, &d));
        serde_json::to_string(&found).map_err(js_error)
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
    fn fit_lists_candidates() {
        let d = Dataset::new("a.csv", CSV.as_bytes(), None).unwrap();
        let out = d.fit("total", r#"{"inputs":["price","qty"]}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["candidates"][0]["formula"], "price * qty");
    }
}
