//! Reading JSON data into rows of text cells, like a CSV file.
//!
//! Accepted shapes:
//!
//! - an array of objects, one per row: `[{"net": 10, "vat": 2.1}, ...]`;
//! - JSON lines (`.ndjson`, `.jsonl`): one object per line;
//! - an object of columns: `{"net": [10, 20], "vat": [2.1, 4.2]}`;
//! - an array of arrays whose first row holds the column names;
//! - any of these under a single key, such as `{"data": [...]}`.
//!
//! Nested objects become columns named with dots (`price.net`). Columns
//! appear in the order they are first seen.

use crate::{err, LoadError};
use serde_json::{Map, Value};

type Rows = (Vec<String>, Vec<Vec<String>>);

/// Parses JSON or JSON lines into a header and rows of text cells.
pub(crate) fn parse(text: &str) -> Result<Rows, LoadError> {
    let value = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(whole) => {
            // JSON lines: one value per non-empty line.
            let lines: Result<Vec<Value>, _> = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(serde_json::from_str)
                .collect();
            match lines {
                Ok(values) if values.len() > 1 => Value::Array(values),
                _ => return Err(err(format!("invalid JSON: {whole}"))),
            }
        }
    };
    from_value(value)
}

fn from_value(value: Value) -> Result<Rows, LoadError> {
    match value {
        Value::Array(items) => from_array(items),
        Value::Object(map) => {
            if map.values().all(Value::is_array) && map.len() > 1 {
                return from_columns(map);
            }
            // A wrapper such as {"data": [...]}: the only array inside.
            let mut arrays = map.into_iter().filter(|(_, v)| v.is_array());
            match (arrays.next(), arrays.next()) {
                (Some((_, inner)), None) => from_value(inner),
                _ => Err(err(
                    "expected an array of rows, an object of columns or JSON lines",
                )),
            }
        }
        _ => Err(err(
            "expected an array of rows, an object of columns or JSON lines",
        )),
    }
}

fn from_array(items: Vec<Value>) -> Result<Rows, LoadError> {
    if items.first().is_some_and(Value::is_array) {
        let mut rows = items.into_iter().map(|row| match row {
            Value::Array(cells) => cells.iter().map(cell).collect::<Vec<_>>(),
            other => vec![cell(&other)],
        });
        let header = rows.next().unwrap_or_default();
        return Ok((header, rows.collect()));
    }
    let mut header: Vec<String> = Vec::new();
    let mut flat_rows = Vec::with_capacity(items.len());
    for item in &items {
        let Value::Object(map) = item else {
            return Err(err("every row must be a JSON object"));
        };
        let mut flat = Vec::new();
        flatten("", map, &mut flat);
        for (name, _) in &flat {
            if !header.contains(name) {
                header.push(name.clone());
            }
        }
        flat_rows.push(flat);
    }
    let rows = flat_rows
        .into_iter()
        .map(|flat| {
            header
                .iter()
                .map(|h| {
                    flat.iter()
                        .find(|(n, _)| n == h)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    Ok((header, rows))
}

fn from_columns(map: Map<String, Value>) -> Result<Rows, LoadError> {
    let header: Vec<String> = map.keys().cloned().collect();
    let columns: Vec<Vec<Value>> = map
        .into_iter()
        .map(|(_, v)| match v {
            Value::Array(items) => items,
            _ => Vec::new(),
        })
        .collect();
    let n = columns.iter().map(Vec::len).max().unwrap_or(0);
    let rows = (0..n)
        .map(|r| {
            columns
                .iter()
                .map(|c| c.get(r).map(cell).unwrap_or_default())
                .collect()
        })
        .collect();
    Ok((header, rows))
}

/// Object fields as `(name, text)`, with nested objects named `a.b`.
fn flatten(prefix: &str, map: &Map<String, Value>, out: &mut Vec<(String, String)>) {
    for (key, value) in map {
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Object(inner) => flatten(&name, inner, out),
            other => out.push((name, cell(other))),
        }
    }
}

/// A JSON value as the text of a cell.
fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Rows {
        parse(text).unwrap()
    }

    #[test]
    fn array_of_objects() {
        let (header, data) =
            rows(r#"[{"net": 10, "vat": 2.1}, {"net": 20.5, "vat": null, "note": "x"}]"#);
        assert_eq!(header, ["net", "vat", "note"]);
        assert_eq!(data, [vec!["10", "2.1", ""], vec!["20.5", "", "x"]]);
    }

    #[test]
    fn columns_keep_the_order_of_the_file() {
        let (header, _) = rows(r#"[{"zeta": 1, "alpha": 2}]"#);
        assert_eq!(header, ["zeta", "alpha"]);
        let (header, _) = rows(r#"{"zeta": [1], "alpha": [2]}"#);
        assert_eq!(header, ["zeta", "alpha"]);
    }

    #[test]
    fn json_lines_and_nested_objects() {
        let (header, data) = rows(
            "{\"id\": 1, \"price\": {\"net\": 10}}\n\n{\"id\": 2, \"price\": {\"net\": 12}}\n",
        );
        assert_eq!(header, ["id", "price.net"]);
        assert_eq!(data, [vec!["1", "10"], vec!["2", "12"]]);
    }

    #[test]
    fn object_of_columns_and_wrappers() {
        let (header, data) = rows(r#"{"net": [10, 20], "vat": [2.1, 4.2]}"#);
        assert_eq!(header, ["net", "vat"]);
        assert_eq!(data, [vec!["10", "2.1"], vec!["20", "4.2"]]);
        let (header, data) = rows(r#"{"data": [["net", "vat"], [10, 2.1]], "count": 1}"#);
        assert_eq!(header, ["net", "vat"]);
        assert_eq!(data, [vec!["10", "2.1"]]);
    }

    #[test]
    fn helpful_errors() {
        assert!(parse("42").unwrap_err().0.contains("expected an array"));
        assert!(parse("[1, {").unwrap_err().0.contains("invalid JSON"));
        assert!(parse(r#"[{"a": 1}, 2]"#)
            .unwrap_err()
            .0
            .contains("JSON object"));
    }
}
