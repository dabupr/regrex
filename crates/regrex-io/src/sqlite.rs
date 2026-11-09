//! Reading a table of a SQLite database into rows of text cells.
//!
//! Each table (or view) is offered like a sheet of a spreadsheet: the
//! first one is read unless another is named.

use crate::{err, LoadError};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

/// Header, rows and the names of every table.
type Parsed = (Vec<String>, Vec<Vec<String>>, Vec<String>);

/// Reads table `table` (the first one when `None`) of the database in
/// `bytes`.
pub(crate) fn read(bytes: &[u8], table: Option<&str>) -> Result<Parsed, LoadError> {
    let fail = |e: rusqlite::Error| err(format!("cannot read the SQLite database: {e}"));
    let mut conn =
        Connection::open_with_flags(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(fail)?;
    conn.deserialize_read_exact("main", bytes, bytes.len(), true)
        .map_err(fail)?;

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY type = 'view', rowid",
        )
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(fail)?;
    let name = match table {
        Some(t) if tables.iter().any(|n| n == t) => t.to_string(),
        Some(t) => {
            return Err(err(format!(
                "no table named {t:?}; the database has: {}",
                tables.join(", ")
            )))
        }
        None => tables
            .first()
            .cloned()
            .ok_or_else(|| err("the database has no tables"))?,
    };

    let quoted = format!("\"{}\"", name.replace('"', "\"\""));
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {quoted}"))
        .map_err(fail)?;
    let header: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let n = header.len();
    let rows = stmt
        .query_map([], |row| {
            (0..n)
                .map(|i| row.get_ref(i).map(cell_text))
                .collect::<Result<Vec<String>, _>>()
        })
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .map_err(fail)?;
    Ok((header, rows, tables))
}

fn cell_text(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null | ValueRef::Blob(_) => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
    }
}
