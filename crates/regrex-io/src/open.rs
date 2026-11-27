//! Opening a file for a scan, the same way in the command line tool, the
//! MCP server and the Python package: read it, match its rows with a second
//! file when one is given, and drop the columns to ignore.

use crate::{join, load_path, Joined, Loaded};
use serde::Serialize;
use std::path::Path;

/// A file read by [`open()`].
#[derive(Debug, Clone)]
pub struct Opened {
    /// The table, with the columns of the second file when there is one.
    pub loaded: Loaded,
    /// How the rows of the second file were matched, when there is one.
    pub joined: Option<JoinedJson>,
}

/// How the rows of a second file were matched with those of the first (see
/// [`crate::join()`]), as the reports give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinedJson {
    /// The second file.
    pub file: String,
    /// Its columns are named `<name>.<column>` when the first file has a
    /// column with the same name.
    pub name: String,
    /// The columns both files have, matched on.
    pub keys: Vec<String>,
    /// Rows of the first file with a match in the second.
    pub matched_rows: usize,
    /// Rows of the first file.
    pub rows: usize,
    /// Rows of the second file left out because their keys repeat an
    /// earlier row.
    pub repeated_rows: usize,
}

impl JoinedJson {
    /// Builds it from what [`crate::join()`] tells.
    pub fn new(file: &str, name: &str, j: &Joined) -> Self {
        JoinedJson {
            file: file.to_string(),
            name: name.to_string(),
            keys: j.keys.clone(),
            matched_rows: j.matched,
            rows: j.rows,
            repeated_rows: j.repeated,
        }
    }

    /// One sentence, such as `Matched 90 of 100 rows with b on line, pk.`
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Matched {} of {} rows with {} on {}.",
            self.matched_rows,
            self.rows,
            self.name,
            self.keys.join(", ")
        );
        if self.repeated_rows > 0 {
            s.pop();
            s.push_str(&format!(
                " ({} rows of {} repeat an earlier one and were left out).",
                self.repeated_rows, self.name
            ));
        }
        s
    }
}

/// Finds a numeric column by name, or by name ignoring case.
pub fn column_index(table: &regrex_core::Table, name: &str) -> Result<usize, String> {
    table
        .names
        .iter()
        .position(|n| n == name)
        .or_else(|| {
            table
                .names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| {
            format!(
                "no numeric column named {name:?} (columns: {})",
                table.names.join(", ")
            )
        })
}

/// Reads `path` (its `sheet`, or table of a database), matches its rows
/// with those of `with` on the columns both files have when it is given,
/// and drops the `ignore`d columns.
pub fn open(
    path: &Path,
    sheet: Option<&str>,
    with: Option<&Path>,
    ignore: &[String],
) -> Result<Opened, String> {
    let mut loaded = load_path(path, sheet).map_err(|e| e.to_string())?;
    let mut joined = None;
    if let Some(other) = with {
        let b = load_path(other, None).map_err(|e| e.to_string())?;
        let name = other.file_stem().map_or_else(
            || "second file".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        let (both, how) = join(&loaded, &b, &name).map_err(|e| e.to_string())?;
        joined = Some(JoinedJson::new(&other.display().to_string(), &name, &how));
        loaded = both;
    }
    for name in ignore {
        let index = column_index(&loaded.table, name)?;
        loaded.table.remove_column(index);
    }
    if loaded.table.columns.is_empty() {
        let others: Vec<&str> = loaded
            .sheets
            .iter()
            .map(String::as_str)
            .filter(|s| Some(*s) != sheet)
            .collect();
        if sheet.is_none() && others.len() > 1 {
            return Err(format!(
                "no numeric columns found in {:?}; choose another sheet or table: {}",
                others[0],
                others[1..].join(", ")
            ));
        }
        return Err("no numeric columns found".into());
    }
    Ok(Opened { loaded, joined })
}

/// Rows in the sample of a quick scan when none is given: 5% of the rows,
/// and at least 2000.
pub fn default_sample(rows: usize) -> usize {
    (rows / 20).max(2000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn opens_two_files_matched_on_shared_columns() {
        let dir = std::env::temp_dir().join(format!("regrex-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = write(&dir, "a.csv", "id,x\n1,10\n2,20\n3,30\n");
        let b = write(&dir, "b.csv", "id,y\n1,5\n3,15\n");
        let o = open(&a, None, Some(&b), &[]).unwrap();
        let j = o.joined.unwrap();
        assert_eq!(j.keys, ["id"]);
        assert_eq!((j.matched_rows, j.rows), (2, 3));
        assert_eq!(j.summary(), "Matched 2 of 3 rows with b on id.");
        assert!(o.loaded.table.names.iter().any(|n| n == "y"));
        let o = open(&a, None, None, &["X".to_string()]).unwrap();
        assert!(o.joined.is_none());
        assert!(!o.loaded.table.names.iter().any(|n| n == "x"));
        assert!(open(&a, None, None, &["nope".to_string()]).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
