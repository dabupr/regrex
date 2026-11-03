//! A [Model Context Protocol](https://modelcontextprotocol.io) server, so AI
//! assistants can use Regrex as a tool.
//!
//! The server speaks JSON-RPC 2.0 over stdio: one message per line on stdin,
//! one response per line on stdout. It has no state between calls and only
//! reads the files it is asked about.

use crate::{column_index, fit_column, load_file, scan_columns};
use regrex_core::parse::parse_expr;
use regrex_core::units::{dimension_of, parse_unit, Dim, Dimension};
use regrex_core::{check_formula, search, ScanConfig, SearchConfig};
use regrex_io::report::{DerivedJson, FitReport, ScanReport};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Protocol versions this server understands, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Regrex finds the formulas behind tabular data. \
Use scan_dataset to find which columns of a CSV or spreadsheet are computed from \
other columns, and which rows break those formulas. Use fit_formula to find \
candidate formulas for one column, fit_values for numbers you already have, \
check_formula to test a formula you suspect, and check_units to see whether a \
formula is dimensionally consistent. Formulas use + - * / ^, sqrt, exp, log, \
sin, cos, and the constants pi and e.";

/// Reads requests from `input` until it closes and writes responses to `output`.
pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Handles one JSON-RPC message; notifications get no response.
fn handle_line(line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response or an invalid message; there is nothing to answer.
        return message
            .get("id")
            .filter(|_| message.get("result").is_none() && message.get("error").is_none())
            .map(|id| error(id.clone(), -32600, "invalid request"));
    };
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|&&v| v == requested)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            success(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "regrex",
                        "title": "Regrex",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(name, &args) {
                Ok(result) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": result.to_string() }],
                        "structuredContent": result,
                    }),
                ),
                Err(ToolError::Unknown) => error(id, -32602, &format!("unknown tool: {name}")),
                // Tool failures are reported to the model, which can fix its call.
                Err(ToolError::Failed(message)) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true,
                    }),
                ),
            }
        }
        _ => error(id, -32601, &format!("method not found: {method}")),
    })
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    let file = json!({
        "path": { "type": "string", "description": "Path to a CSV file or a spreadsheet (.xlsx, .xls, .ods) with column names in the first row. Units can be written in headers, as in \"time (s)\"." },
        "sheet": { "type": "string", "description": "Sheet to read from a spreadsheet (default: the first one)." },
        "ignore": { "type": "array", "items": { "type": "string" }, "description": "Columns to ignore, for example IDs." },
    });
    let with_file = |extra: Value| {
        let mut props = file.clone();
        props
            .as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("object").clone());
        props
    };
    json!([
        {
            "name": "scan_dataset",
            "title": "Find computed columns",
            "description": "Finds the columns of a table that are computed from other columns, with their formula and the rows that break it (rows are counted from 1 after the header).",
            "inputSchema": {
                "type": "object",
                "properties": with_file(json!({
                    "columns": { "type": "array", "items": { "type": "string" }, "description": "Only check these columns (default: all)." },
                    "min_match": { "type": "number", "minimum": 0, "maximum": 1, "description": "Report a column when its formula matches at least this fraction of rows (default 0.9)." },
                    "max_size": { "type": "integer", "minimum": 1, "maximum": 9, "description": "Maximum formula size (default 5); larger is slower." },
                })),
                "required": ["path"],
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "fit_formula",
            "title": "Fit formulas for a column",
            "description": "Finds candidate formulas for one column of a file from the other columns, from simplest to most accurate, with their error.",
            "inputSchema": {
                "type": "object",
                "properties": with_file(json!({
                    "target": { "type": "string", "description": "The column to explain." },
                    "inputs": { "type": "array", "items": { "type": "string" }, "description": "Columns to use as inputs (default: all others)." },
                    "max_size": { "type": "integer", "minimum": 1, "maximum": 9, "description": "Maximum formula size (default 7)." },
                    "results": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Number of formulas to return (default 10)." },
                })),
                "required": ["path", "target"],
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "fit_values",
            "title": "Fit formulas for numbers",
            "description": "Finds candidate formulas target = f(inputs) for values given directly, for example measurements from a conversation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "inputs": { "type": "object", "additionalProperties": { "type": "array", "items": { "type": "number" } }, "description": "Input variables: name to list of values." },
                    "target": { "type": "array", "items": { "type": "number" }, "description": "Values to explain, one per row." },
                    "units": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Optional units of the inputs, such as {\"t\": \"s\"}; with target_unit, rules out formulas that mix units." },
                    "target_unit": { "type": "string", "description": "Optional unit of the target, such as \"m\"." },
                    "max_size": { "type": "integer", "minimum": 1, "maximum": 9, "description": "Maximum formula size (default 7)." },
                    "results": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Number of formulas to return (default 10)." },
                },
                "required": ["inputs", "target"],
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "check_formula",
            "title": "Check a formula against a file",
            "description": "Checks how many rows of a column follow a given formula over other columns, and lists the rows that do not.",
            "inputSchema": {
                "type": "object",
                "properties": with_file(json!({
                    "target": { "type": "string", "description": "The column the formula should compute." },
                    "formula": { "type": "string", "description": "Formula over column names, such as \"0.21 * net\"." },
                })),
                "required": ["path", "target", "formula"],
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "check_units",
            "title": "Check units of a formula",
            "description": "Computes the unit dimension of a formula from the units of its variables, and whether it matches an expected unit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "formula": { "type": "string", "description": "Formula, such as \"0.5 * m * v^2\"." },
                    "units": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Unit of each variable, such as {\"m\": \"kg\", \"v\": \"m/s\"}." },
                    "expected": { "type": "string", "description": "Optional expected unit of the result, such as \"J\"." },
                },
                "required": ["formula", "units"],
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
    ])
}

enum ToolError {
    Unknown,
    Failed(String),
}

impl From<String> for ToolError {
    fn from(message: String) -> Self {
        ToolError::Failed(message)
    }
}

fn call_tool(name: &str, args: &Value) -> Result<Value, ToolError> {
    match name {
        "scan_dataset" => {
            let (path, loaded) = load(args)?;
            let derived = scan_columns(
                &loaded,
                &strings(args, "columns")?,
                number(args, "min_match")?.unwrap_or(0.9),
                size(args, "max_size", 5, 9)?,
            )?;
            to_value(&ScanReport::new(&path, &loaded, &derived))
        }
        "fit_formula" => {
            let (_, loaded) = load(args)?;
            let report = fit_column(
                &loaded,
                &string(args, "target")?,
                &strings(args, "inputs")?,
                size(args, "max_size", 7, 9)?,
                size(args, "results", 10, 50)?,
            )?;
            to_value(&report)
        }
        "fit_values" => fit_values(args),
        "check_formula" => {
            let (_, loaded) = load(args)?;
            let table = &loaded.table;
            let target = column_index(table, &string(args, "target")?)?;
            let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
            let formula =
                parse_expr(&string(args, "formula")?, &names).map_err(|e| e.to_string())?;
            let check = check_formula(table, target, &formula, &ScanConfig::default());
            to_value(&DerivedJson::new(table, &check))
        }
        "check_units" => check_units(args),
        _ => Err(ToolError::Unknown),
    }
}

fn fit_values(args: &Value) -> Result<Value, ToolError> {
    let inputs = args
        .get("inputs")
        .and_then(Value::as_object)
        .ok_or_else(|| "\"inputs\" must be an object of name to values".to_string())?;
    let names: Vec<String> = inputs.keys().cloned().collect();
    let columns = inputs
        .iter()
        .map(|(name, values)| {
            numbers(values).ok_or_else(|| format!("input {name:?} must be a list of numbers"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let target = args
        .get("target")
        .and_then(numbers)
        .ok_or_else(|| "\"target\" must be a list of numbers".to_string())?;
    if columns.iter().any(|c| c.len() != target.len()) {
        return Err("every input needs as many values as the target"
            .to_string()
            .into());
    }
    let mut config = SearchConfig {
        max_size: size(args, "max_size", 7, 9)?,
        max_results: size(args, "results", 10, 50)?,
        ..SearchConfig::default()
    };
    if let Some(target_unit) = args.get("target_unit").and_then(Value::as_str) {
        let units = unit_map(args)?;
        let dims = names
            .iter()
            .map(|n| units.iter().find(|(k, _)| k == n).map(|(_, d)| *d))
            .collect::<Option<Vec<Dim>>>()
            .ok_or_else(|| "with target_unit, every input needs a unit".to_string())?;
        config.input_dims = Some(dims);
        config.target_dim = Some(parse_unit(target_unit).map_err(|e| e.to_string())?.dim);
    }
    let found = search(&columns, &target, &config).map_err(|e| e.to_string())?;
    to_value(&FitReport::new("y", &names, target.len(), &found))
}

fn check_units(args: &Value) -> Result<Value, ToolError> {
    let units = unit_map(args)?;
    let names: Vec<&str> = units.iter().map(|(n, _)| n.as_str()).collect();
    let dims: Vec<Dim> = units.iter().map(|(_, d)| *d).collect();
    let formula = parse_expr(&string(args, "formula")?, &names).map_err(|e| e.to_string())?;
    let dimension = dimension_of(&formula, &dims);
    let mut result = match dimension {
        Dimension::Known(d) => json!({ "consistent": true, "dimension": d.to_string() }),
        Dimension::Any => json!({ "consistent": true, "dimension": null }),
        Dimension::Invalid => json!({
            "consistent": false,
            "dimension": null,
            "message": "the formula adds or compares quantities with different units",
        }),
    };
    if let Some(expected) = args.get("expected").and_then(Value::as_str) {
        let want = parse_unit(expected).map_err(|e| e.to_string())?.dim;
        let matches = match dimension {
            Dimension::Known(d) => d == want,
            Dimension::Any => true,
            Dimension::Invalid => false,
        };
        result["expected"] = json!(want.to_string());
        result["matches_expected"] = json!(matches);
    }
    Ok(result)
}

fn load(args: &Value) -> Result<(String, regrex_io::Loaded), ToolError> {
    let path = PathBuf::from(string(args, "path")?);
    let sheet = args.get("sheet").and_then(Value::as_str);
    let loaded = load_file(&path, sheet, &strings(args, "ignore")?)?;
    Ok((path.display().to_string(), loaded))
}

fn to_value(report: &impl serde::Serialize) -> Result<Value, ToolError> {
    serde_json::to_value(report).map_err(|e| ToolError::Failed(e.to_string()))
}

fn string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::Failed(format!("missing string argument {key:?}")))
}

fn strings(args: &Value, key: &str) -> Result<Vec<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| ToolError::Failed(format!("{key:?} must be a list of strings"))),
        Some(_) => Err(ToolError::Failed(format!(
            "{key:?} must be a list of strings"
        ))),
    }
}

fn number(args: &Value, key: &str) -> Result<Option<f64>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .map(Some)
            .ok_or_else(|| ToolError::Failed(format!("{key:?} must be a number"))),
    }
}

/// An integer argument between 1 and `max`, or `default` when absent.
fn size(args: &Value, key: &str, default: usize, max: usize) -> Result<usize, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .map(|n| n as usize)
            .filter(|n| (1..=max).contains(n))
            .ok_or_else(|| {
                ToolError::Failed(format!("{key:?} must be an integer from 1 to {max}"))
            }),
    }
}

fn numbers(value: &Value) -> Option<Vec<f64>> {
    value.as_array()?.iter().map(Value::as_f64).collect()
}

fn unit_map(args: &Value) -> Result<Vec<(String, Dim)>, ToolError> {
    let Some(units) = args.get("units").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    units
        .iter()
        .map(|(name, unit)| {
            let text = unit
                .as_str()
                .ok_or_else(|| format!("unit of {name:?} must be a string"))?;
            let unit = parse_unit(text).map_err(|e| format!("{name}: {e}"))?;
            Ok((name.clone(), unit.dim))
        })
        .collect::<Result<_, String>>()
        .map_err(ToolError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: u64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        handle_line(&line.to_string()).expect("a response")
    }

    fn call(name: &str, arguments: Value) -> Value {
        request(
            1,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )["result"]
            .clone()
    }

    #[test]
    fn initialize_negotiates_the_version() {
        let r = request(
            1,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "regrex");
        let r = request(2, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn protocol_errors() {
        assert_eq!(handle_line("{not json").unwrap()["error"]["code"], -32700);
        assert_eq!(
            request(1, "resources/list", json!({}))["error"]["code"],
            -32601
        );
        let r = request(1, "tools/call", json!({ "name": "nope", "arguments": {} }));
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn lists_tools_with_schemas() {
        let tools = request(1, "tools/list", json!({}))["result"]["tools"].clone();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "scan_dataset",
                "fit_formula",
                "fit_values",
                "check_formula",
                "check_units"
            ]
        );
        for tool in tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn fit_values_finds_a_formula() {
        let r = call(
            "fit_values",
            json!({
                "inputs": { "t": [0.5, 1.0, 1.5, 2.0, 2.5, 3.0] },
                "target": [1.22625, 4.905, 11.03625, 19.62, 30.65625, 44.145],
                "units": { "t": "s" },
                "target_unit": "m",
                "max_size": 5,
            }),
        );
        assert_eq!(
            r["structuredContent"]["candidates"][0]["formula"],
            "4.905 * t^2"
        );
    }

    #[test]
    fn tool_errors_are_results() {
        let r = call(
            "fit_values",
            json!({ "inputs": { "x": [1, 2] }, "target": [1] }),
        );
        assert_eq!(r["isError"], true);
        let r = call("scan_dataset", json!({ "path": "does-not-exist.csv" }));
        assert_eq!(r["isError"], true);
    }

    #[test]
    fn check_units_of_energy() {
        let r = call(
            "check_units",
            json!({ "formula": "0.5 * m * v^2", "units": { "m": "kg", "v": "m/s" }, "expected": "J" }),
        );
        assert_eq!(r["structuredContent"]["matches_expected"], true);
        let r = call(
            "check_units",
            json!({ "formula": "m + v", "units": { "m": "kg", "v": "m/s" } }),
        );
        assert_eq!(r["structuredContent"]["consistent"], false);
    }

    #[test]
    fn serve_answers_line_by_line() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":"b","method":"ping"}"#,
            "\n",
        );
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["id"], "b");
    }
}
