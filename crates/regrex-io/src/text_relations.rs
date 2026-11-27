//! Relations between text columns, found on the cells as written.
//!
//! - Concatenations: `key = line + "|" + track`.
//! - One-to-one codes: each `profile_code` has one `profile_name` and each
//!   name one code.
//!
//! Numbers hidden in text (`GIS5550` = an id − 4835) are read as columns
//! instead (see [`crate::hidden`]), so the formula search finds those.

use crate::{cells::is_missing, Loaded};
use std::collections::HashMap;

/// A relation between text columns.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRelation {
    /// The column explained (the first of a one-to-one set).
    pub column: String,
    /// What holds, with column names: `line + "|" + track`, or
    /// `one to one with profile_name`.
    pub formula: String,
    /// The other columns of the relation.
    pub inputs: Vec<String>,
    /// Rows where it holds.
    pub matched: usize,
    /// Rows where every column of the relation has a value.
    pub checked: usize,
}

/// Share of the checked rows a relation must hold on.
const MIN_SHARE: f64 = 0.99;
/// Rows a relation must be checked on.
const MIN_ROWS: usize = 10;
/// Rows tried before checking every row.
const SAMPLE: usize = 200;
/// A separator is short and not a letter or digit.
const MAX_SEPARATOR: usize = 3;

/// Every concatenation and one-to-one pair of text columns in `loaded`.
pub fn text_relations(loaded: &Loaded) -> Vec<TextRelation> {
    let header = &loaded.header;
    let n = header.len();
    let rows = &loaded.rows;
    let cell = |r: usize, c: usize| -> &str { rows[r].get(c).map_or("", |s| s.trim()) };
    let text: Vec<usize> = (0..n)
        .filter(|&c| loaded.categories.contains(&header[c]) || loaded.skipped.contains(&header[c]))
        .collect();
    let mut out = Vec::new();
    for &k in &text {
        if let Some(rel) = concatenation(rows.len(), k, n, &cell, header) {
            out.push(rel);
        }
    }
    out.extend(one_to_one(
        rows.len(),
        &text,
        &cell,
        header,
        &loaded.categories,
    ));
    out
}

/// `k = a + sep + b` for two other columns.
fn concatenation<'a>(
    n_rows: usize,
    k: usize,
    n: usize,
    cell: &impl Fn(usize, usize) -> &'a str,
    header: &[String],
) -> Option<TextRelation> {
    let first = (0..n_rows).find(|&r| !is_missing(cell(r, k)))?;
    let key = cell(first, k);
    let mut best: Option<TextRelation> = None;
    for a in (0..n).filter(|&a| a != k) {
        let va = cell(first, a);
        if va.is_empty() || !key.starts_with(va) {
            continue;
        }
        for b in (0..n).filter(|&b| b != k && b != a) {
            let vb = cell(first, b);
            if vb.is_empty() || key.len() < va.len() + vb.len() || !key.ends_with(vb) {
                continue;
            }
            let sep = &key[va.len()..key.len() - vb.len()];
            let separator = (1..=MAX_SEPARATOR).contains(&sep.chars().count())
                && !sep.chars().any(char::is_alphanumeric);
            if !separator {
                continue;
            }
            let holds = |r: usize| {
                let (kv, av, bv) = (cell(r, k), cell(r, a), cell(r, b));
                kv.len() == av.len() + sep.len() + bv.len()
                    && kv.starts_with(av)
                    && kv[av.len()..].starts_with(sep)
                    && kv.ends_with(bv)
            };
            let present = |r: usize| [k, a, b].iter().all(|&c| !is_missing(cell(r, c)));
            let sample = (0..n_rows).filter(|&r| present(r)).take(SAMPLE);
            if !sample.clone().all(holds) {
                continue;
            }
            let checked: Vec<usize> = (0..n_rows).filter(|&r| present(r)).collect();
            let matched = checked.iter().filter(|&&r| holds(r)).count();
            if checked.len() < MIN_ROWS || (matched as f64) < MIN_SHARE * checked.len() as f64 {
                continue;
            }
            if best.as_ref().is_none_or(|b| matched > b.matched) {
                best = Some(TextRelation {
                    column: header[k].clone(),
                    formula: format!("{} + {:?} + {}", header[a], sep, header[b]),
                    inputs: vec![header[a].clone(), header[b].clone()],
                    matched,
                    checked: checked.len(),
                });
            }
        }
    }
    best
}

/// Sets of text columns whose values match one to one, one relation per
/// set, on the first column of the set.
fn one_to_one<'a>(
    n_rows: usize,
    text: &[usize],
    cell: &impl Fn(usize, usize) -> &'a str,
    header: &[String],
    categories: &[String],
) -> Vec<TextRelation> {
    // Only categories: free text with one value per row matches anything
    // one to one.
    let cats: Vec<usize> = text
        .iter()
        .copied()
        .filter(|&c| categories.contains(&header[c]))
        .collect();
    let distinct = |c: usize| {
        let mut v: Vec<&str> = (0..n_rows)
            .map(|r| cell(r, c))
            .filter(|s| !is_missing(s))
            .collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    let counts: Vec<usize> = cats.iter().map(|&c| distinct(c)).collect();
    let mut set_of: Vec<Option<usize>> = vec![None; cats.len()];
    let mut sets: Vec<(Vec<usize>, usize, usize)> = Vec::new();
    for i in 0..cats.len() {
        if set_of[i].is_some() || counts[i] < 2 {
            continue;
        }
        for j in i + 1..cats.len() {
            if set_of[j].is_some() || counts[j] != counts[i] {
                continue;
            }
            let Some((matched, checked)) = maps_one_to_one(n_rows, cats[i], cats[j], cell) else {
                continue;
            };
            let s = *set_of[i].get_or_insert_with(|| {
                sets.push((vec![cats[i]], matched, checked));
                sets.len() - 1
            });
            sets[s].0.push(cats[j]);
            sets[s].1 = sets[s].1.min(matched);
            sets[s].2 = sets[s].2.min(checked);
            set_of[j] = Some(s);
        }
    }
    sets.into_iter()
        .map(|(cols, matched, checked)| TextRelation {
            column: header[cols[0]].clone(),
            formula: format!(
                "one to one with {}",
                cols[1..]
                    .iter()
                    .map(|&c| header[c].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            inputs: cols[1..].iter().map(|&c| header[c].clone()).collect(),
            matched,
            checked,
        })
        .collect()
}

/// Whether each value of `a` goes with one value of `b` and back, on the
/// rows where both are known; `(rows, rows)` when it does.
fn maps_one_to_one<'a>(
    n_rows: usize,
    a: usize,
    b: usize,
    cell: &impl Fn(usize, usize) -> &'a str,
) -> Option<(usize, usize)> {
    let mut ab: HashMap<&str, &str> = HashMap::new();
    let mut ba: HashMap<&str, &str> = HashMap::new();
    let mut checked = 0;
    for r in 0..n_rows {
        let (x, y) = (cell(r, a), cell(r, b));
        if is_missing(x) || is_missing(y) {
            continue;
        }
        checked += 1;
        if *ab.entry(x).or_insert(y) != y || *ba.entry(y).or_insert(x) != x {
            return None;
        }
    }
    // The same text twice is a copy, not a code and its name.
    let copy = ab.iter().all(|(x, y)| x == y);
    (checked >= MIN_ROWS && !copy).then_some((checked, checked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(header: &[&str], rows: &[&[&str]]) -> Loaded {
        let header: Vec<String> = header.iter().map(|s| s.to_string()).collect();
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect();
        crate::build_table(header, rows, Vec::new())
    }

    #[test]
    fn finds_a_concatenated_key_and_codes_with_names() {
        let mut rows = Vec::new();
        for i in 0..40 {
            let line = ["L1", "L2", "L3"][i % 3];
            let track = ["1", "2"][i % 2];
            let key = format!("{line}|{track}");
            let (code, name) = [
                ("S1", "Circuit A"),
                ("S2", "Circuit B"),
                ("S3", "Circuit C"),
            ][i % 3 * (i % 2) % 3];
            rows.push(vec![
                line.to_string(),
                track.to_string(),
                key,
                code.to_string(),
                name.to_string(),
                (i * 7 % 13).to_string(),
            ]);
        }
        let rows: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect();
        let rows: Vec<&[&str]> = rows.iter().map(Vec::as_slice).collect();
        let l = loaded(&["line", "track", "key", "code", "name", "n"], &rows);
        let rel = text_relations(&l);
        let formulas: Vec<String> = rel
            .iter()
            .map(|r| format!("{} = {}", r.column, r.formula))
            .collect();
        assert!(
            formulas.contains(&"key = line + \"|\" + track".to_string()),
            "{formulas:?}"
        );
        assert!(
            formulas.contains(&"code = one to one with name".to_string()),
            "{formulas:?}"
        );
        assert_eq!(rel.len(), 2, "{formulas:?}");
    }
}
