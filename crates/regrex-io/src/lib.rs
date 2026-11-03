//! # regrex-io
//!
//! Reads CSV files and spreadsheets (`.xlsx`, `.xls`, `.ods`) into a
//! [`regrex_core::Table`], working out number formats on the way: decimal
//! points or commas, thousands separators, percentages and currency symbols.
//!
//! Works on native targets and in WebAssembly ([`load_bytes`] needs no file
//! system).

#![warn(missing_docs)]

pub mod report;

use calamine::{open_workbook_auto_from_rs, Data, Reader, Sheets};
use regrex_core::Table;
use std::fmt;
use std::io::Cursor;

/// A loaded file.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The numeric columns.
    pub table: Table,
    /// Names of the columns that were not numeric.
    pub skipped: Vec<String>,
    /// Every column name, numeric or not, in file order.
    pub header: Vec<String>,
    /// Every data row as text, in file order (row 1 is `rows[0]`), for
    /// previews and for showing the rows that break a formula.
    pub rows: Vec<Vec<String>>,
    /// Sheet names, for spreadsheets.
    pub sheets: Vec<String>,
}

/// Why a file could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

fn err(message: impl Into<String>) -> LoadError {
    LoadError(message.into())
}

/// Whether a file name looks like a spreadsheet rather than CSV text.
pub fn is_spreadsheet(file_name: &str) -> bool {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "xlsx" | "xlsm" | "xls" | "xlsb" | "ods")
}

/// Loads a file from disk. The first row must hold the column names.
pub fn load_path(path: &std::path::Path, sheet: Option<&str>) -> Result<Loaded, LoadError> {
    let bytes =
        std::fs::read(path).map_err(|e| err(format!("cannot read {}: {e}", path.display())))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    load_bytes(name, &bytes, sheet)
}

/// Loads a file from its bytes; `file_name` is only used to tell CSV from
/// spreadsheets. The first row must hold the column names.
pub fn load_bytes(file_name: &str, bytes: &[u8], sheet: Option<&str>) -> Result<Loaded, LoadError> {
    let (header, rows, sheets) = if is_spreadsheet(file_name) {
        read_spreadsheet(bytes, sheet)?
    } else {
        let text = String::from_utf8_lossy(bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let (header, rows) = parse_csv(text)?;
        (header, rows, Vec::new())
    };
    if header.is_empty() {
        return Err(err("the file is empty"));
    }
    Ok(build_table(header, rows, sheets))
}

/// Parses CSV text, guessing the delimiter (`,`, `;` or tab) from the
/// header line. Returns the header and the rows as text.
pub fn parse_csv(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), LoadError> {
    let first_line = text.lines().next().unwrap_or("");
    let delimiter = b";\t,"
        .iter()
        .copied()
        .max_by_key(|d| first_line.bytes().filter(|b| b == d).count())
        .unwrap_or(b',');
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let header: Vec<String> = reader
        .headers()
        .map_err(|e| err(format!("invalid CSV header: {e}")))?
        .iter()
        .map(str::to_string)
        .collect();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| err(format!("invalid CSV: {e}")))?;
        rows.push(record.iter().map(str::to_string).collect());
    }
    Ok((header, rows))
}

type Parsed = (Vec<String>, Vec<Vec<String>>, Vec<String>);

fn read_spreadsheet(bytes: &[u8], sheet: Option<&str>) -> Result<Parsed, LoadError> {
    let mut workbook: Sheets<_> = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| err(format!("cannot open the spreadsheet: {e}")))?;
    let sheets = workbook.sheet_names().to_vec();
    let name = match sheet {
        Some(s) => s.to_string(),
        None => sheets
            .first()
            .cloned()
            .ok_or_else(|| err("the workbook has no sheets"))?,
    };
    let range = workbook
        .worksheet_range(&name)
        .map_err(|e| err(format!("cannot read sheet {name:?}: {e}")))?;
    let mut rows = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect::<Vec<String>>());
    let header = rows.next().ok_or_else(|| err("the sheet is empty"))?;
    Ok((header, rows.collect(), sheets))
}

fn cell_text(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        other => other.to_string(),
    }
}

/// Parses a number written as `1234.5`, `1,234.5`, `1234,5` or `1.234,5`.
/// Also accepts a trailing `%` (as a fraction) and surrounding currency
/// symbols. Returns the value and the number of decimals written.
///
/// When `comma_decimal` is true a single comma is always the decimal
/// separator (`1,234` is 1.234), as in many European spreadsheets.
pub fn parse_number_with(s: &str, comma_decimal: bool) -> Option<(f64, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (s, percent) = match s.strip_suffix('%') {
        Some(rest) => (rest.trim(), true),
        None => (s, false),
    };
    let s = s.trim_matches(|c: char| "€$£¥ ".contains(c));
    let last_comma = s.rfind(',');
    let last_dot = s.rfind('.');
    // The right-most separator is the decimal one, unless a lone comma is
    // followed by exactly three digits (a thousands separator: 1,234).
    let normalized: String = match (last_comma, last_dot) {
        (Some(c), Some(d)) if c > d => s.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => s.replace(',', ""),
        (Some(c), None) => {
            let after = &s[c + 1..];
            if s.matches(',').count() > 1
                || !comma_decimal && after.len() == 3 && !s.starts_with("0,")
            {
                s.replace(',', "")
            } else {
                s.replace(',', ".")
            }
        }
        _ => s.to_string(),
    };
    let value: f64 = normalized.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let mut decimals = match normalized.find('.') {
        Some(i) => normalized[i + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .count() as u32,
        None => 0,
    };
    let value = if percent {
        decimals += 2;
        value / 100.0
    } else {
        value
    };
    Some((value, decimals))
}

/// Whether a cell can only be read with a decimal comma (`1,5`, `0,25`).
fn clearly_decimal_comma(s: &str) -> bool {
    let s = s.trim();
    s.matches(',').count() == 1
        && !s.contains('.')
        && s.rsplit(',').next().is_some_and(|after| {
            !after.is_empty() && after.len() != 3 && after.chars().all(|c| c.is_ascii_digit())
        })
}

/// Keeps columns where most non-empty cells are numbers.
fn build_table(header: Vec<String>, rows: Vec<Vec<String>>, sheets: Vec<String>) -> Loaded {
    let mut table = Table::default();
    let mut skipped = Vec::new();
    // If any cell in the file clearly uses a decimal comma, they all do.
    let comma_decimal = rows.iter().flatten().any(|s| clearly_decimal_comma(s));
    for (c, name) in header.iter().enumerate() {
        let cells: Vec<&str> = rows
            .iter()
            .map(|r| r.get(c).map_or("", String::as_str))
            .collect();
        let parsed: Vec<Option<(f64, u32)>> = cells
            .iter()
            .map(|s| parse_number_with(s, comma_decimal))
            .collect();
        let non_empty = cells.iter().filter(|s| !s.trim().is_empty()).count();
        let numeric = parsed.iter().filter(|p| p.is_some()).count();
        if non_empty == 0 || numeric * 10 < non_empty * 9 {
            skipped.push(name.clone());
            continue;
        }
        // `speed (m/s)` is the column `speed`, in metres per second.
        let (name, unit) = regrex_core::units::split_header(name);
        let name = if name.is_empty() {
            format!("column{}", c + 1)
        } else {
            name
        };
        table.names.push(name);
        table.units.push(unit);
        table.columns.push(
            parsed
                .iter()
                .map(|p| p.map_or(f64::NAN, |(v, _)| v))
                .collect(),
        );
        table
            .decimals
            .push(parsed.iter().flatten().map(|(_, d)| *d).max());
    }
    Loaded {
        table,
        skipped,
        rows,
        header,
        sheets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_number(s: &str) -> Option<(f64, u32)> {
        parse_number_with(s, false)
    }

    #[test]
    fn numbers_in_many_formats() {
        assert_eq!(parse_number("12"), Some((12.0, 0)));
        assert_eq!(parse_number("1.21"), Some((1.21, 2)));
        assert_eq!(parse_number("1,21"), Some((1.21, 2)));
        assert_eq!(parse_number("1,234"), Some((1234.0, 0)));
        assert_eq!(parse_number("1,234.50"), Some((1234.5, 2)));
        assert_eq!(parse_number("1.234,5"), Some((1234.5, 1)));
        assert_eq!(parse_number("0,125"), Some((0.125, 3)));
        assert_eq!(parse_number("21%"), Some((0.21, 2)));
        assert_eq!(parse_number("€ 9.99"), Some((9.99, 2)));
        assert_eq!(parse_number("-3.5"), Some((-3.5, 1)));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number_with("1,234", true), Some((1.234, 3)));
    }

    #[test]
    fn decimal_comma_is_detected_for_the_whole_file() {
        let (header, rows) = parse_csv("x;y\n1,5;1,234\n2,25;2,000\n").unwrap();
        let loaded = build_table(header, rows, Vec::new());
        assert_eq!(loaded.table.columns[1], vec![1.234, 2.0]);
    }

    #[test]
    fn csv_with_semicolons_and_text_columns() {
        let text = "name;price;qty;total\nA;1,50;2;3,00\nB;2,25;4;9,00\nC;10;1;10\n";
        let (header, rows) = parse_csv(text).unwrap();
        let loaded = build_table(header, rows, Vec::new());
        assert_eq!(loaded.skipped, vec!["name".to_string()]);
        assert_eq!(loaded.table.names, vec!["price", "qty", "total"]);
        assert_eq!(loaded.table.columns[0], vec![1.5, 2.25, 10.0]);
        assert_eq!(loaded.table.decimals[0], Some(2));
    }

    #[test]
    fn units_are_read_from_headers() {
        let (header, rows) = parse_csv("time (s),distance [m],note\n1,4.9,a\n2,19.6,b\n").unwrap();
        let loaded = build_table(header, rows, Vec::new());
        assert_eq!(loaded.table.names, vec!["time", "distance"]);
        let s = regrex_core::units::parse_unit("s").unwrap();
        assert_eq!(loaded.table.units[0], Some(s));
        assert_eq!(loaded.header[0], "time (s)");
    }

    #[test]
    fn missing_cells_become_nan() {
        let (header, rows) = parse_csv("a,b\n1,2\n,4\n5,6\n").unwrap();
        let loaded = build_table(header, rows, Vec::new());
        assert!(loaded.table.columns[0][1].is_nan());
        assert_eq!(loaded.table.columns[1], vec![2.0, 4.0, 6.0]);
    }
}
