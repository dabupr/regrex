//! Native part of the `regrex` Python package. The friendly API lives in
//! `python/regrex/__init__.py`; these functions exchange JSON strings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use regrex_core::units::parse_unit;
use regrex_core::{scan_detailed, search, ScanConfig, SearchConfig, Table};
use regrex_io::report::{FitReport, ScanReport};

fn value_error(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn units(
    list: Option<Vec<Option<String>>>,
    n: usize,
) -> PyResult<Vec<Option<regrex_core::units::Unit>>> {
    let Some(list) = list else {
        return Ok(vec![None; n]);
    };
    list.into_iter()
        .map(|u| u.map(|t| parse_unit(&t).map_err(value_error)).transpose())
        .collect()
}

/// search(names, columns, target, max_size, max_params, max_results, units, target_unit) -> JSON
#[pyfunction]
#[pyo3(signature = (names, columns, target, target_name, max_size=7, max_params=1, max_results=10, units=None, target_unit=None))]
#[allow(clippy::too_many_arguments)]
fn search_json(
    py: Python<'_>,
    names: Vec<String>,
    columns: Vec<Vec<f64>>,
    target: Vec<f64>,
    target_name: String,
    max_size: usize,
    max_params: usize,
    max_results: usize,
    units: Option<Vec<Option<String>>>,
    target_unit: Option<String>,
) -> PyResult<String> {
    let input_units = self::units(units, names.len())?;
    let target_unit = target_unit
        .map(|t| parse_unit(&t).map_err(value_error))
        .transpose()?;
    let mut config = SearchConfig {
        max_size,
        max_params,
        max_results,
        ..SearchConfig::default()
    };
    if let (Some(dims), Some(t)) = (
        input_units
            .iter()
            .map(|u| u.map(|u| u.dim))
            .collect::<Option<Vec<_>>>(),
        target_unit,
    ) {
        config.input_dims = Some(dims);
        config.target_dim = Some(t.dim);
    }
    let found = py
        .detach(|| search(&columns, &target, &config))
        .map_err(value_error)?;
    Ok(FitReport::new(&target_name, &names, target.len(), &found).to_json())
}

/// scan(names, columns, units, min_match, max_size, source, decimals) -> JSON
///
/// `decimals` gives the number of decimals each column was rounded to;
/// where it is `None`, it is inferred from the values.
#[pyfunction]
#[pyo3(signature = (names, columns, units=None, min_match=0.9, max_size=5, source="<data>".to_string(), decimals=None))]
#[allow(clippy::too_many_arguments)]
fn scan_json(
    py: Python<'_>,
    names: Vec<String>,
    columns: Vec<Vec<f64>>,
    units: Option<Vec<Option<String>>>,
    min_match: f64,
    max_size: usize,
    source: String,
    decimals: Option<Vec<Option<u32>>>,
) -> PyResult<String> {
    if names.len() != columns.len() {
        return Err(value_error("names and columns must have the same length"));
    }
    let decimals = decimals.unwrap_or_else(|| vec![None; names.len()]);
    if decimals.len() != names.len() {
        return Err(value_error("names and decimals must have the same length"));
    }
    let mut table = Table {
        units: self::units(units, names.len())?,
        decimals,
        names,
        columns,
        timestamps: Vec::new(),
        labels: Vec::new(),
    };
    // Like a CSV file, where the decimals are read from the text.
    table.infer_decimals();
    let loaded = regrex_io::Loaded {
        header: table.names.clone(),
        table,
        skipped: Vec::new(),
        categories: Vec::new(),
        rows: Vec::new(),
        sheets: Vec::new(),
        converted: Vec::new(),
    };
    Ok(run_scan(py, &loaded, &source, min_match, max_size).to_json())
}

/// scan_file(path, sheet, ignore, min_match, max_size, with_path, quick, sample) -> JSON
#[pyfunction]
#[pyo3(signature = (path, sheet=None, ignore=Vec::new(), min_match=0.9, max_size=5, with_path=None, quick=false, sample=None))]
#[allow(clippy::too_many_arguments)]
fn scan_file_json(
    py: Python<'_>,
    path: std::path::PathBuf,
    sheet: Option<String>,
    ignore: Vec<String>,
    min_match: f64,
    max_size: usize,
    with_path: Option<std::path::PathBuf>,
    quick: bool,
    sample: Option<usize>,
) -> PyResult<String> {
    let opened = regrex_io::open(&path, sheet.as_deref(), with_path.as_deref(), &ignore)
        .map_err(value_error)?;
    let loaded = &opened.loaded;
    let source = path.display().to_string();
    if !quick {
        let report = run_scan(py, loaded, &source, min_match, max_size);
        return Ok(report.with_join(opened.joined.as_ref()).to_json());
    }
    let table = &loaded.table;
    let config = scan_config(min_match, max_size);
    let sample = sample.unwrap_or_else(|| regrex_io::open::default_sample(table.n_rows()));
    let q = py.detach(|| regrex_core::scan_quick(table, &config, sample, |_, _| {}));
    Ok(ScanReport::new(&source, loaded, &q.derived)
        .with_quick(table, &q)
        .with_join(opened.joined.as_ref())
        .to_json())
}

/// describe_file(path, sheet, with_path) -> JSON
#[pyfunction]
#[pyo3(signature = (path, sheet=None, with_path=None))]
fn describe_file_json(
    py: Python<'_>,
    path: std::path::PathBuf,
    sheet: Option<String>,
    with_path: Option<std::path::PathBuf>,
) -> PyResult<String> {
    let opened =
        regrex_io::open(&path, sheet.as_deref(), with_path.as_deref(), &[]).map_err(value_error)?;
    let source = path.display().to_string();
    Ok(py.detach(|| {
        regrex_io::describe::Description::new(&source, &opened.loaded, opened.joined.as_ref())
            .to_json()
    }))
}

fn scan_config(min_match: f64, max_size: usize) -> ScanConfig {
    let mut config = ScanConfig {
        min_match,
        ..ScanConfig::default()
    };
    config.search.max_size = max_size;
    config
}

fn run_scan(
    py: Python<'_>,
    loaded: &regrex_io::Loaded,
    source: &str,
    min_match: f64,
    max_size: usize,
) -> ScanReport {
    let config = scan_config(min_match, max_size);
    let found = py.detach(|| scan_detailed(&loaded.table, &config, &|_| {}));
    ScanReport::new(source, loaded, &found.derived)
        .with_near_misses(&loaded.table, &found.near_misses)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(search_json, m)?)?;
    m.add_function(wrap_pyfunction!(scan_json, m)?)?;
    m.add_function(wrap_pyfunction!(scan_file_json, m)?)?;
    m.add_function(wrap_pyfunction!(describe_file_json, m)?)?;
    Ok(())
}
