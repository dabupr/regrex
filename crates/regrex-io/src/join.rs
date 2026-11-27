//! Two files read as one: the rows of both matched on the columns they
//! share (a line, a track and a chainage), so that the scan finds the
//! relations between them, such as a measured radius that equals the
//! designed one on circular curves.

use crate::{build_table, cells::is_missing, err, LoadError, Loaded};
use std::collections::HashMap;

/// How two files were matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    /// The columns of both files with the same name, matched on.
    pub keys: Vec<String>,
    /// Rows of the first file with a match in the second.
    pub matched: usize,
    /// Rows of the first file.
    pub rows: usize,
    /// Rows of the second file whose keys repeat an earlier row (the first
    /// one is used).
    pub repeated: usize,
}

/// The value of a key cell: numbers match when they are equal (`100` and
/// `100.0`), text when it is the same once trimmed.
fn key_cell(s: &str) -> Option<String> {
    let s = s.trim();
    if is_missing(s) {
        return None;
    }
    Some(match crate::parse_number_with(s, false) {
        Some((v, _)) => format!("{v}"),
        None => s.to_string(),
    })
}

/// The rows of `a` that have a row of `b` with the same values in the
/// columns both share, with the other columns of `b` added after those of
/// `a`. Shared columns of text or whole numbers are the keys; a column of
/// `b` whose name is also in `a` and is no key gets
/// ` (b_name)` after its name.
pub fn join(a: &Loaded, b: &Loaded, b_name: &str) -> Result<(Loaded, Joined), LoadError> {
    // Text and whole numbers name rows (a line, a chainage); a measurement
    // in both files is one more thing to compare, not a key.
    let names_rows = |l: &Loaded, n: &str| {
        l.categories.iter().any(|c| c == n)
            || l.skipped.iter().any(|c| c == n)
            || l.table.names.iter().position(|m| m == n).is_some_and(|c| {
                l.table.columns[c]
                    .iter()
                    .all(|v| !v.is_finite() || v.fract() == 0.0)
            })
    };
    let keys: Vec<(usize, usize)> = a
        .header
        .iter()
        .enumerate()
        .filter_map(|(i, n)| b.header.iter().position(|m| m == n).map(|j| (i, j)))
        .filter(|&(i, j)| names_rows(a, &a.header[i]) && names_rows(b, &b.header[j]))
        .collect();
    if keys.is_empty() {
        return Err(err(format!(
            "{b_name} has no column named as one of the first file, to match rows on"
        )));
    }
    let key = |row: &[String], side: fn(&(usize, usize)) -> usize| -> Option<Vec<String>> {
        keys.iter()
            .map(|k| key_cell(row.get(side(k)).map_or("", String::as_str)))
            .collect()
    };
    let mut index: HashMap<Vec<String>, usize> = HashMap::new();
    let mut repeated = 0;
    for (r, row) in b.rows.iter().enumerate() {
        if let Some(k) = key(row, |k| k.1) {
            match index.entry(k) {
                std::collections::hash_map::Entry::Occupied(_) => repeated += 1,
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(r);
                }
            }
        }
    }
    let extra: Vec<usize> = (0..b.header.len())
        .filter(|j| !keys.iter().any(|k| k.1 == *j))
        .collect();
    let mut header = a.header.clone();
    for &j in &extra {
        let name = &b.header[j];
        header.push(if a.header.contains(name) {
            format!("{name} ({b_name})")
        } else {
            name.clone()
        });
    }
    let rows: Vec<Vec<String>> = a
        .rows
        .iter()
        .filter_map(|row| {
            let r = *index.get(&key(row, |k| k.0)?)?;
            let mut out = row.clone();
            out.resize(a.header.len(), String::new());
            out.extend(
                extra
                    .iter()
                    .map(|&j| b.rows[r].get(j).cloned().unwrap_or_default()),
            );
            Some(out)
        })
        .collect();
    if rows.is_empty() {
        return Err(err(format!(
            "no row of {b_name} has the same {} as a row of the first file",
            keys.iter()
                .map(|k| a.header[k.0].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let joined = Joined {
        keys: keys.iter().map(|k| a.header[k.0].clone()).collect(),
        matched: rows.len(),
        rows: a.rows.len(),
        repeated,
    };
    Ok((build_table(header, rows, Vec::new()), joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(header: &[&str], rows: &[&[&str]]) -> Loaded {
        build_table(
            header.iter().map(|s| s.to_string()).collect(),
            rows.iter()
                .map(|r| r.iter().map(|s| s.to_string()).collect())
                .collect(),
            Vec::new(),
        )
    }

    #[test]
    fn matches_rows_on_the_shared_columns() {
        let a = loaded(
            &["line", "pk", "radius"],
            &[
                &["L1", "1.0", "300.5"],
                &["L1", "2.0", "310.2"],
                &["L2", "1.0", "0"],
            ],
        );
        let b = loaded(
            &["pk", "line", "radius", "cant"],
            &[
                &["2", "L1", "311.1", "0.1"],
                &["1", "L2", "0", "0"],
                &["9", "L3", "5", "1"],
            ],
        );
        let (j, how) = join(&a, &b, "measured").unwrap();
        assert_eq!(how.keys, ["line", "pk"]);
        assert_eq!((how.matched, how.rows, how.repeated), (2, 3, 0));
        assert_eq!(
            j.header,
            ["line", "pk", "radius", "radius (measured)", "cant"]
        );
        assert_eq!(j.rows[0], ["L1", "2.0", "310.2", "311.1", "0.1"]);
    }

    #[test]
    fn needs_a_shared_column() {
        let a = loaded(&["x"], &[&["1"]]);
        let b = loaded(&["y"], &[&["1"]]);
        assert!(join(&a, &b, "other").is_err());
    }
}
