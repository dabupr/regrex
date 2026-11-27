//! A quick scan: search a sample of rows first, then confirm on all rows.
//!
//! On large tables most of a scan's time goes into columns that have no
//! formula: every candidate is checked on every row before giving up. A
//! quick scan searches every column on a sample of rows (blocks of
//! consecutive rows spread over the file, so runs and steps along an order
//! are kept), reports each formula found there at once as provisional, and
//! then confirms it on all rows. Columns without a formula on the sample
//! are not searched again.
//!
//! When `n` sampled rows all match, the share of rows of the whole table
//! that break the formula is below about `3 / n` with 95% confidence (the
//! rule of three); [`QuickScan::sample_bound`] gives that bound.

use crate::scan::{
    check_formula, explain_column_in, find_copies, scan, DerivedColumn, ScanConfig, Table,
};

/// Blocks of consecutive rows the sample is made of.
const BLOCKS: usize = 20;

/// What a quick scan found.
#[derive(Debug, Clone)]
pub struct QuickScan {
    /// Formulas confirmed on all rows.
    pub derived: Vec<DerivedColumn>,
    /// Rows of the sample searched (all rows when the table is small).
    pub sample_rows: usize,
    /// Columns without a formula on the sample, not searched on all rows.
    pub not_searched: Vec<usize>,
    /// Formulas found on the sample that did not hold on all rows.
    pub dropped: Vec<DerivedColumn>,
}

impl QuickScan {
    /// Upper bound, with 95% confidence, on the share of rows breaking a
    /// formula that matched every sampled row (the rule of three).
    pub fn sample_bound(&self) -> f64 {
        (3.0 / self.sample_rows.max(1) as f64).min(1.0)
    }
}

/// The rows of a sample of about `size` rows: 20 blocks of
/// consecutive rows, evenly spread. All rows when the table is not much
/// larger than that.
pub fn sample_rows(n_rows: usize, size: usize) -> Vec<usize> {
    if n_rows <= 2 * size || size == 0 {
        return (0..n_rows).collect();
    }
    let block = size.div_ceil(BLOCKS);
    let gap = n_rows / BLOCKS;
    (0..BLOCKS)
        .flat_map(|b| {
            let start = b * gap;
            start..(start + block).min(n_rows)
        })
        .collect()
}

/// The rows `rows` of `table`, as a table of their own.
pub fn sub_table(table: &Table, rows: &[usize]) -> Table {
    Table {
        names: table.names.clone(),
        columns: table
            .columns
            .iter()
            .map(|c| rows.iter().map(|&r| c[r]).collect())
            .collect(),
        decimals: table.decimals.clone(),
        units: table.units.clone(),
        timestamps: table.timestamps.clone(),
        labels: table.labels.clone(),
    }
}

/// Scans a sample of about `size` rows, then confirms on all rows each
/// formula found. `progress` is called with each provisional formula
/// (`false`, found on the sample) and then with each confirmed one
/// (`true`).
pub fn scan_quick(
    table: &Table,
    config: &ScanConfig,
    size: usize,
    mut progress: impl FnMut(&DerivedColumn, bool),
) -> QuickScan {
    let rows = sample_rows(table.n_rows(), size);
    if rows.len() == table.n_rows() {
        let derived = scan(table, config);
        for d in &derived {
            progress(d, true);
        }
        return QuickScan {
            derived,
            sample_rows: rows.len(),
            not_searched: Vec::new(),
            dropped: Vec::new(),
        };
    }
    let sample = sub_table(table, &rows);
    let provisional = scan(&sample, config);
    for d in &provisional {
        progress(d, false);
    }

    // Copies are cheap to find on all rows.
    let copies = find_copies(table, config);
    let profile = crate::profile::profile(table);
    let mut derived = Vec::new();
    let mut dropped = Vec::new();
    let mut found = vec![false; table.columns.len()];
    for (t, copy) in copies.iter().enumerate() {
        if let Some(c) = copy {
            found[t] = true;
            progress(c, true);
            derived.push(c.clone());
        }
    }
    let skip: Vec<bool> = copies.iter().map(Option::is_some).collect();
    let confirm = |d: &DerivedColumn| confirm(table, d, config, &copies, &profile, &skip);
    let todo: Vec<&DerivedColumn> = provisional.iter().filter(|d| !found[d.target]).collect();
    #[cfg(feature = "parallel")]
    let confirmed: Vec<Option<DerivedColumn>> = {
        use rayon::prelude::*;
        todo.par_iter().map(|d| confirm(d)).collect()
    };
    #[cfg(not(feature = "parallel"))]
    let confirmed: Vec<Option<DerivedColumn>> = todo.iter().map(|d| confirm(d)).collect();
    for (d, c) in todo.into_iter().zip(confirmed) {
        found[d.target] = true;
        match c {
            Some(c) => {
                progress(&c, true);
                derived.push(c);
            }
            None => dropped.push(d.clone()),
        }
    }
    derived.sort_by_key(|d| d.target);
    let not_searched = (0..table.columns.len()).filter(|&t| !found[t]).collect();
    QuickScan {
        derived,
        sample_rows: rows.len(),
        not_searched,
        dropped,
    }
}

/// A formula found on a sample of rows, checked on all rows of `table`:
/// a plain formula is checked as it is; one between rows or per regime is
/// looked for again the same way (cheap), and anything else is searched
/// again on all rows. `None` when it does not hold on enough rows.
pub fn confirm(
    table: &Table,
    d: &DerivedColumn,
    config: &ScanConfig,
    copies: &[Option<DerivedColumn>],
    profile: &crate::profile::Profile,
    skip: &[bool],
) -> Option<DerivedColumn> {
    let plain = !d.copy && !d.steps && d.regime.is_none() && d.other.is_none();
    if plain {
        let checked = check_formula(table, d.target, &d.formula, config);
        if checked.match_ratio() >= config.min_match {
            return Some(checked);
        }
    }
    if d.steps {
        return crate::steps::explain_by_steps(table, d.target, config, profile, skip);
    }
    if d.regime.is_some() {
        return crate::regime::explain_by_regime(table, d.target, config, profile, skip);
    }
    explain_column_in(table, d.target, config, copies, profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_is_spread_blocks() {
        let rows = sample_rows(100_000, 2000);
        assert_eq!(rows.len(), 2000);
        assert_eq!(rows[0], 0);
        assert_eq!(rows[100], 5000);
        assert_eq!(sample_rows(3000, 2000).len(), 3000);
    }

    #[test]
    fn finds_on_the_sample_and_confirms_on_all_rows() {
        let n = 20_000;
        let a: Vec<f64> = (0..n).map(|i| ((i * 7919) % 1000) as f64).collect();
        let b: Vec<f64> = (0..n).map(|i| ((i * 104_729) % 997) as f64).collect();
        let sum: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let noise: Vec<f64> = (0..n).map(|i| ((i * 31) % 101) as f64 / 3.0).collect();
        let table = Table {
            names: ["a", "b", "sum", "noise"].map(String::from).to_vec(),
            columns: vec![a, b, sum, noise],
            decimals: vec![Some(0), Some(0), Some(0), Some(2)],
            ..Table::default()
        };
        let mut seen = Vec::new();
        let q = scan_quick(&table, &ScanConfig::default(), 2000, |d, confirmed| {
            seen.push((d.target, confirmed));
        });
        assert_eq!(q.sample_rows, 2000);
        assert!(q.derived.iter().any(|d| d.target == 2), "{q:?}");
        assert!(seen.contains(&(2, false)) && seen.contains(&(2, true)));
        assert!(q.not_searched.contains(&3));
        assert!((q.sample_bound() - 0.0015).abs() < 1e-12);
    }
}
