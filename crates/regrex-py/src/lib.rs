//! Native part of the `regrex` Python package. The friendly API lives in
//! `python/regrex/__init__.py`; these functions exchange JSON strings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use regrex_core::units::parse_unit;
use regrex_core::{scan, search, ScanConfig, SearchConfig, Table};
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

/// scan(names, columns, units, ignore, min_match, max_size) -> JSON
#[pyfunction]
#[pyo3(signature = (names, columns, units=None, min_match=0.9, max_size=5, source="<data>".to_string()))]
fn scan_json(
    py: Python<'_>,
    names: Vec<String>,
    columns: Vec<Vec<f64>>,
    units: Option<Vec<Option<String>>>,
    min_match: f64,
    max_size: usize,
    source: String,
) -> PyResult<String> {
    if names.len() != columns.len() {
        return Err(value_error("names and columns must have the same length"));
    }
    let table = Table {
        units: self::units(units, names.len())?,
        decimals: vec![None; names.len()],
        names,
        columns,
    };
    let loaded = regrex_io::Loaded {
        header: table.names.clone(),
        table,
        skipped: Vec::new(),
        rows: Vec::new(),
        sheets: Vec::new(),
        converted: Vec::new(),
    };
    Ok(run_scan(py, &loaded, &source, min_match, max_size))
}

/// scan_file(path, sheet, ignore, min_match, max_size) -> JSON
#[pyfunction]
#[pyo3(signature = (path, sheet=None, ignore=Vec::new(), min_match=0.9, max_size=5))]
fn scan_file_json(
    py: Python<'_>,
    path: std::path::PathBuf,
    sheet: Option<String>,
    ignore: Vec<String>,
    min_match: f64,
    max_size: usize,
) -> PyResult<String> {
    let mut loaded = regrex_io::load_path(&path, sheet.as_deref()).map_err(value_error)?;
    for name in &ignore {
        let i = loaded
            .table
            .names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| value_error(format!("no numeric column named {name:?}")))?;
        loaded.table.remove_column(i);
    }
    Ok(run_scan(
        py,
        &loaded,
        &path.display().to_string(),
        min_match,
        max_size,
    ))
}

fn run_scan(
    py: Python<'_>,
    loaded: &regrex_io::Loaded,
    source: &str,
    min_match: f64,
    max_size: usize,
) -> String {
    let mut config = ScanConfig {
        min_match,
        ..ScanConfig::default()
    };
    config.search.max_size = max_size;
    let derived = py.detach(|| scan(&loaded.table, &config));
    ScanReport::new(source, loaded, &derived).to_json()
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(search_json, m)?)?;
    m.add_function(wrap_pyfunction!(scan_json, m)?)?;
    m.add_function(wrap_pyfunction!(scan_file_json, m)?)?;
    Ok(())
}
