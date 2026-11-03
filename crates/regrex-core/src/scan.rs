//! Dataset scanning: find which columns are computed from other columns.
//!
//! For every numeric column, the scanner searches for a formula over the
//! other columns that reproduces it. Real exports often contain a few rows
//! that do not follow the rule (manual edits, special cases, typos), so the
//! scan is robust: it searches on several subsets of rows, keeps the formula
//! that matches the most rows, and reports the rows that break it.

use crate::expr::Expr;
use crate::search::{search, SearchConfig};
use crate::units::Unit;

/// A table of numeric columns. Missing or non-numeric cells are `NaN`.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// Column names, used in formulas.
    pub names: Vec<String>,
    /// Column values; all columns have the same length.
    pub columns: Vec<Vec<f64>>,
    /// Number of decimals each column is written with, when known (for
    /// example from a CSV file). Used to tolerate rounding.
    pub decimals: Vec<Option<u32>>,
    /// Unit of each column, when known (for example from a header like
    /// `speed (m/s)`). May be empty or shorter than `columns`.
    pub units: Vec<Option<Unit>>,
}

impl Table {
    /// Number of rows.
    pub fn n_rows(&self) -> usize {
        self.columns.first().map_or(0, Vec::len)
    }

    /// Unit of column `i`, if known.
    pub fn unit(&self, i: usize) -> Option<Unit> {
        self.units.get(i).copied().flatten()
    }

    /// Removes column `i`.
    pub fn remove_column(&mut self, i: usize) {
        self.names.remove(i);
        self.columns.remove(i);
        if i < self.decimals.len() {
            self.decimals.remove(i);
        }
        if i < self.units.len() {
            self.units.remove(i);
        }
    }
}

/// Settings for [`scan`] and [`explain_column`].
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Settings for each search. Scanning uses smaller formulas than a
    /// single search by default, because it runs one search per column.
    pub search: SearchConfig,
    /// A column is reported as derived when its formula matches at least
    /// this fraction of rows.
    pub min_match: f64,
    /// Relative tolerance for a row to match when the column's decimals are
    /// unknown.
    pub rel_tolerance: f64,
    /// At most this many other columns are considered as inputs (the most
    /// correlated ones).
    pub max_inputs: usize,
    /// Rows per subset when searching robustly.
    pub subset_rows: usize,
    /// When no formula fits every row, the search is also run on this many
    /// small random samples of rows, which are likely to miss a few wrong
    /// rows that spoil the larger subsets. 0 turns it off (faster).
    pub small_samples: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            search: SearchConfig {
                max_size: 5,
                max_params: 1,
                ..SearchConfig::default()
            },
            min_match: 0.9,
            rel_tolerance: 1e-6,
            max_inputs: 5,
            subset_rows: 40,
            small_samples: 4,
        }
    }
}

/// A column explained by a formula over other columns.
#[derive(Debug, Clone)]
pub struct DerivedColumn {
    /// Index of the derived column.
    pub target: usize,
    /// The formula; `Expr::Var(i)` refers to column `i` of the table.
    pub formula: Expr,
    /// The formula written with column names.
    pub formula_text: String,
    /// Columns used by the formula, in increasing order.
    pub inputs: Vec<usize>,
    /// Weighted complexity of the formula.
    pub complexity: usize,
    /// Rows where the formula reproduces the value.
    pub matched: usize,
    /// Rows that could be checked (no missing values).
    pub checked: usize,
    /// Rows that do not follow the formula, in increasing order.
    pub exceptions: Vec<usize>,
}

impl DerivedColumn {
    /// Fraction of checked rows that follow the formula.
    pub fn match_ratio(&self) -> f64 {
        if self.checked == 0 {
            0.0
        } else {
            self.matched as f64 / self.checked as f64
        }
    }
}

/// Scans every column and returns those that can be computed from others.
///
/// Columns are scanned one by one; the result is ordered by column index.
pub fn scan(table: &Table, config: &ScanConfig) -> Vec<DerivedColumn> {
    (0..table.columns.len())
        .filter_map(|t| explain_column(table, t, config))
        .collect()
}

/// Looks for a formula that computes column `target` from the others.
pub fn explain_column(table: &Table, target: usize, config: &ScanConfig) -> Option<DerivedColumn> {
    let y_all = table.columns.get(target)?;
    let candidates_cols = select_inputs(table, target, config.max_inputs);
    if candidates_cols.is_empty() {
        return None;
    }
    // Rows where the target and every candidate input are present.
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| {
            y_all[r].is_finite()
                && candidates_cols
                    .iter()
                    .all(|&c| table.columns[c][r].is_finite())
        })
        .collect();
    if rows.len() < 3 {
        return None;
    }
    let tolerance = row_tolerance(table, target, config);

    // Search on a few disjoint-ish subsets, so a handful of bad rows cannot
    // spoil every attempt; then once more on the rows the best formula fits.
    let mut best: Option<(Expr, Vec<usize>)> = None;
    let consider = |formula: Expr, best: &mut Option<(Expr, Vec<usize>)>| {
        let matched = matching_rows(&formula, table, &rows, y_all, &tolerance);
        let better = match best {
            None => true,
            Some((f, m)) => {
                matched.len() > m.len()
                    || matched.len() == m.len() && formula.complexity() < f.complexity()
            }
        };
        if better {
            *best = Some((formula, matched));
        }
    };

    for subset in subsets(&rows, config.subset_rows, 3) {
        for formula in search_rows(table, &candidates_cols, target, &subset, config) {
            consider(formula, &mut best);
        }
    }
    // A few wrong rows can spoil every subset of a small table. Unless a
    // formula already fits every row, also try small samples, which are
    // likely to miss the wrong rows.
    if config.small_samples > 0
        && best.as_ref().is_none_or(|(_, m)| m.len() < rows.len())
        && rows.len() >= 2 * SMALL_SUBSET
    {
        for subset in samples(&rows, SMALL_SUBSET, config.small_samples) {
            for formula in search_rows(table, &candidates_cols, target, &subset, config) {
                consider(formula, &mut best);
            }
        }
    }
    let (_, inliers) = best.clone()?;
    if inliers.len() >= 3 && inliers.len() < rows.len() {
        let refined = spread(&inliers, config.subset_rows);
        for formula in search_rows(table, &candidates_cols, target, &refined, config) {
            consider(formula, &mut best);
        }
    }

    let (formula, matched) = best?;
    let ratio = matched.len() as f64 / rows.len() as f64;
    // With few rows, a complicated formula can fit anything: require more
    // matching rows than the formula has parts.
    if ratio < config.min_match || !formula.has_vars() || matched.len() <= formula.complexity() {
        return None;
    }
    Some(derived(table, target, formula, &rows, &matched))
}

/// Checks how well a given formula computes column `target`.
///
/// `formula` refers to table columns with `Expr::Var(i)`, for example as
/// returned by [`crate::parse::parse_expr`] with the table's column names.
/// Rows with a missing value in the target or in a column the formula uses
/// are not checked. Unlike [`explain_column`], the result is returned even
/// when few rows match, so callers can show where a hypothesis fails.
///
/// ```
/// use regrex_core::{check_formula, parse::parse_expr, ScanConfig, Table};
///
/// let table = Table {
///     names: vec!["net".into(), "vat".into()],
///     columns: vec![vec![10.0, 20.0, 30.0], vec![2.1, 4.2, 3.0]],
///     decimals: vec![Some(2), Some(2)],
///     units: vec![None, None],
/// };
/// let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
/// let formula = parse_expr("0.21 * net", &names).unwrap();
/// let check = check_formula(&table, 1, &formula, &ScanConfig::default());
/// assert_eq!((check.matched, check.checked), (2, 3));
/// assert_eq!(check.exceptions, vec![2]);
/// ```
pub fn check_formula(
    table: &Table,
    target: usize,
    formula: &Expr,
    config: &ScanConfig,
) -> DerivedColumn {
    let mut used = Vec::new();
    collect_vars(formula, &mut used);
    let y_all = &table.columns[target];
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| y_all[r].is_finite() && used.iter().all(|&c| table.columns[c][r].is_finite()))
        .collect();
    let tolerance = row_tolerance(table, target, config);
    let matched = matching_rows(formula, table, &rows, y_all, &tolerance);
    derived(table, target, formula.clone(), &rows, &matched)
}

/// Explains why `row` breaks the formula of `derived`: see
/// [`crate::diagnose::diagnose`]. Mistakes are checked with the same
/// tolerance as the scan.
///
/// `all` are the derived columns of the whole scan (it may be empty). When
/// the row breaks several of them because of one wrong cell, the other
/// columns are diagnosed as [`MistakeKind::OtherCell`]: a wrong total also
/// breaks `net = total - vat`, but only the total needs fixing.
///
/// [`MistakeKind::OtherCell`]: crate::diagnose::MistakeKind::OtherCell
pub fn diagnose_exception(
    table: &Table,
    derived: &DerivedColumn,
    all: &[DerivedColumn],
    row: usize,
    config: &ScanConfig,
) -> Vec<crate::diagnose::Diagnosis> {
    use crate::diagnose::{Diagnosis, MistakeKind};
    let values: Vec<f64> = table.columns.iter().map(|c| c[row]).collect();
    let broken: Vec<&DerivedColumn> = all
        .iter()
        .filter(|d| d.exceptions.binary_search(&row).is_ok())
        .collect();
    if broken.len() >= 2 {
        if let Some(culprit) = culprit(table, &broken, all, &values, row, config) {
            if culprit != derived.target {
                return vec![Diagnosis {
                    kind: MistakeKind::OtherCell,
                    description: format!(
                        "the {} in this row looks wrong, it breaks its own formula too",
                        table.names[culprit]
                    ),
                    formula: None,
                }];
            }
        }
    }
    let tolerance = row_tolerance(table, derived.target, config)(row);
    let decimals = table.decimals.get(derived.target).copied().flatten();
    crate::diagnose::diagnose(
        &derived.formula,
        &table.names,
        &values,
        values[derived.target],
        tolerance,
        decimals,
    )
}

/// The one cell of `row` that, set to the value its own formula gives,
/// makes every formula in `all` hold again (or at least those that held, and
/// those in `broken`).
fn culprit(
    table: &Table,
    broken: &[&DerivedColumn],
    all: &[DerivedColumn],
    values: &[f64],
    row: usize,
    config: &ScanConfig,
) -> Option<usize> {
    let mut found = None;
    for candidate in broken {
        let mut fixed = values.to_vec();
        fixed[candidate.target] = candidate.formula.eval_row(values, &[]);
        let holds = |d: &DerivedColumn, row_values: &[f64]| {
            let v = d.formula.eval_row(row_values, &[]);
            (v - row_values[d.target]).abs() <= row_tolerance(table, d.target, config)(row)
        };
        // Formulas with missing values in this row are not checked.
        let all_hold = all
            .iter()
            .filter(|d| holds(d, values) || broken.iter().any(|b| b.target == d.target))
            .all(|d| holds(d, &fixed));
        if all_hold {
            if found.is_some() {
                return None; // ambiguous
            }
            found = Some(candidate.target);
        }
    }
    found
}

/// How far a row of column `target` may be from a formula's value and still
/// match: half a unit in the last written decimal, or a small relative error.
fn row_tolerance<'a>(
    table: &'a Table,
    target: usize,
    config: &'a ScanConfig,
) -> impl Fn(usize) -> f64 + 'a {
    let rounding = table
        .decimals
        .get(target)
        .copied()
        .flatten()
        .map_or(0.0, |d| 0.5 * 10f64.powi(-(d as i32)) * 1.000_001);
    move |r: usize| {
        let y = table.columns[target][r];
        rounding.max(config.rel_tolerance * y.abs()).max(1e-12)
    }
}

fn derived(
    table: &Table,
    target: usize,
    formula: Expr,
    rows: &[usize],
    matched: &[usize],
) -> DerivedColumn {
    let matched_set: std::collections::HashSet<usize> = matched.iter().copied().collect();
    let exceptions = rows
        .iter()
        .copied()
        .filter(|r| !matched_set.contains(r))
        .collect();
    let mut inputs = Vec::new();
    collect_vars(&formula, &mut inputs);
    inputs.sort_unstable();
    inputs.dedup();
    DerivedColumn {
        target,
        formula_text: formula.display_with(&table.names).to_string(),
        complexity: formula.complexity(),
        formula,
        inputs,
        matched: matched.len(),
        checked: rows.len(),
        exceptions,
    }
}

/// Runs a search for `target` on the given rows. The returned formulas use
/// table column indices.
fn search_rows(
    table: &Table,
    inputs: &[usize],
    target: usize,
    rows: &[usize],
    config: &ScanConfig,
) -> Vec<Expr> {
    let x: Vec<Vec<f64>> = inputs
        .iter()
        .map(|&c| rows.iter().map(|&r| table.columns[c][r]).collect())
        .collect();
    let y: Vec<f64> = rows.iter().map(|&r| table.columns[target][r]).collect();
    // Use units only when every column involved has one.
    let input_units: Option<Vec<_>> = inputs
        .iter()
        .map(|&c| table.unit(c).map(|u| u.dim))
        .collect();
    let mut search_config = config.search.clone();
    if let (Some(dims), Some(target_unit)) = (input_units, table.unit(target)) {
        search_config.input_dims = Some(dims);
        search_config.target_dim = Some(target_unit.dim);
    }
    let Ok(results) = search(&x, &y, &search_config) else {
        return Vec::new();
    };
    results
        .into_iter()
        .map(|c| remap_vars(&c.expr, inputs))
        .collect()
}

/// Rows (from `rows`) where `formula` reproduces `y` within tolerance.
fn matching_rows(
    formula: &Expr,
    table: &Table,
    rows: &[usize],
    y: &[f64],
    tolerance: &dyn Fn(usize) -> f64,
) -> Vec<usize> {
    let mut row_values = vec![0.0; table.columns.len()];
    rows.iter()
        .copied()
        .filter(|&r| {
            for (c, v) in row_values.iter_mut().enumerate() {
                *v = table.columns[c][r];
            }
            let pred = formula.eval_row(&row_values, &[]);
            pred.is_finite() && (pred - y[r]).abs() <= tolerance(r)
        })
        .collect()
}

/// Picks the columns most related to the target: by absolute correlation
/// with the target, its logarithm or its square root, so products and
/// powers are also caught.
fn select_inputs(table: &Table, target: usize, max_inputs: usize) -> Vec<usize> {
    let y = &table.columns[target];
    let mut scored: Vec<(f64, usize)> = (0..table.columns.len())
        .filter(|&c| c != target)
        .filter(|&c| !is_constant(&table.columns[c]))
        .map(|c| {
            let x = &table.columns[c];
            let s = [
                correlation(x, y, |v| v),
                correlation(x, y, |v| v.abs().ln()),
                correlation(x, y, |v| v.abs().sqrt()),
            ]
            .into_iter()
            .map(f64::abs)
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max);
            (s, c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut cols: Vec<usize> = scored
        .into_iter()
        .take(max_inputs)
        .map(|(_, c)| c)
        .collect();
    cols.sort_unstable();
    cols
}

fn is_constant(x: &[f64]) -> bool {
    let mut finite = x.iter().filter(|v| v.is_finite());
    match finite.next() {
        None => true,
        Some(first) => finite.all(|v| v == first),
    }
}

/// Pearson correlation between `x` and `f(y)` over rows where both are finite.
fn correlation(x: &[f64], y: &[f64], f: impl Fn(f64) -> f64) -> f64 {
    let pairs: Vec<(f64, f64)> = x
        .iter()
        .zip(y)
        .map(|(a, b)| (*a, f(*b)))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .collect();
    let n = pairs.len() as f64;
    if n < 3.0 {
        return 0.0;
    }
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in &pairs {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
        syy += (b - my) * (b - my);
    }
    sxy / (sxx * syy).sqrt()
}

/// `k` subsets of about `size` rows each, interleaved so they barely overlap.
/// Rows in each small sample tried when no formula fits every row.
const SMALL_SUBSET: usize = 8;

/// `k` samples of `size` rows, picked with a fixed pseudo-random sequence so
/// results are reproducible.
fn samples(rows: &[usize], size: usize, k: usize) -> Vec<Vec<usize>> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..k)
        .map(|_| {
            let mut picked: Vec<usize> = Vec::with_capacity(size);
            while picked.len() < size.min(rows.len()) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let r = rows[(state % rows.len() as u64) as usize];
                if !picked.contains(&r) {
                    picked.push(r);
                }
            }
            picked.sort_unstable();
            picked
        })
        .collect()
}

fn subsets(rows: &[usize], size: usize, k: usize) -> Vec<Vec<usize>> {
    if rows.len() <= size {
        return vec![rows.to_vec()];
    }
    (0..k)
        .map(|offset| {
            let step = (rows.len() / size).max(1);
            rows.iter()
                .skip(offset * step / k.max(1) + offset)
                .step_by(step)
                .take(size)
                .copied()
                .collect()
        })
        .collect()
}

/// At most `size` evenly spread rows.
fn spread(rows: &[usize], size: usize) -> Vec<usize> {
    if rows.len() <= size {
        return rows.to_vec();
    }
    (0..size).map(|i| rows[i * rows.len() / size]).collect()
}

fn remap_vars(e: &Expr, map: &[usize]) -> Expr {
    match e {
        Expr::Var(i) => Expr::Var(map[*i]),
        Expr::Const(_) | Expr::Param(_) => e.clone(),
        Expr::Unary(op, a) => Expr::unary(*op, remap_vars(a, map)),
        Expr::Binary(op, a, b) => Expr::binary(*op, remap_vars(a, map), remap_vars(b, map)),
    }
}

fn collect_vars(e: &Expr, out: &mut Vec<usize>) {
    match e {
        Expr::Var(i) => out.push(*i),
        Expr::Const(_) | Expr::Param(_) => {}
        Expr::Unary(_, a) => collect_vars(a, out),
        Expr::Binary(_, a, b) => {
            collect_vars(a, out);
            collect_vars(b, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoices() -> Table {
        // price, quantity, subtotal = price * quantity, total = subtotal * 1.21
        // rounded to 2 decimals; row 7 was edited by hand.
        let price = [
            10.0, 5.5, 3.2, 8.0, 1.25, 99.9, 12.3, 7.0, 0.99, 45.0, 2.5, 18.75,
        ];
        let quantity = [2.0, 4.0, 1.0, 3.0, 10.0, 1.0, 5.0, 6.0, 12.0, 2.0, 8.0, 3.0];
        let subtotal: Vec<f64> = price.iter().zip(&quantity).map(|(p, q)| p * q).collect();
        let mut total: Vec<f64> = subtotal
            .iter()
            .map(|s| (s * 1.21 * 100.0).round() / 100.0)
            .collect();
        total[7] = 50.0;
        Table {
            names: ["price", "quantity", "subtotal", "total"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            columns: vec![price.to_vec(), quantity.to_vec(), subtotal, total],
            decimals: vec![Some(2), Some(0), Some(2), Some(2)],
            ..Table::default()
        }
    }

    #[test]
    fn finds_derived_columns_and_exceptions() {
        let table = invoices();
        let found = scan(&table, &ScanConfig::default());
        let total = found
            .iter()
            .find(|d| d.target == 3)
            .expect("total is derived");
        assert_eq!(total.formula_text, "1.21 * subtotal");
        assert_eq!(total.exceptions, vec![7]);
        assert_eq!(total.matched, 11);

        let subtotal = found
            .iter()
            .find(|d| d.target == 2)
            .expect("subtotal is derived");
        assert_eq!(subtotal.formula_text, "price * quantity");
        assert!(subtotal.exceptions.is_empty());
        assert_eq!(subtotal.inputs, vec![0, 1]);
    }

    #[test]
    fn a_few_wrong_rows_in_a_small_table() {
        // 40 rows, 3 of them with 10% instead of 21% VAT: every row is in the
        // single 40-row subset, so only the small samples find the rule.
        let net: Vec<f64> = (1..=40).map(|i| 10.0 + 7.0 * i as f64).collect();
        let vat: Vec<f64> = net
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let rate = if (i + 1) % 13 == 0 { 0.1 } else { 0.21 };
                (n * rate * 100.0).round() / 100.0
            })
            .collect();
        let table = Table {
            names: vec!["net".into(), "vat".into()],
            columns: vec![net, vat],
            decimals: vec![Some(0), Some(2)],
            units: vec![None, None],
        };
        let found = explain_column(&table, 1, &ScanConfig::default()).expect("vat is derived");
        assert_eq!(
            found.formula.display_with(&table.names).to_string(),
            "0.21 * net"
        );
        assert_eq!(found.exceptions, vec![12, 25, 38]);
    }

    #[test]
    fn independent_columns_are_not_reported() {
        let table = Table {
            names: vec!["a".into(), "b".into()],
            columns: vec![
                vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0],
                vec![2.0, 7.0, 1.0, 8.0, 2.0, 8.0, 1.0, 8.0],
            ],
            decimals: vec![Some(0), Some(0)],
            ..Table::default()
        };
        assert!(scan(&table, &ScanConfig::default()).is_empty());
    }

    #[test]
    fn complicated_formulas_need_enough_rows() {
        // Six unrelated rows: some long formula may fit them, but it is not evidence.
        let table = Table {
            names: vec!["id".into(), "x".into()],
            columns: vec![
                vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                vec![2.5, 1.25, 3.0, 10.0, 0.5, 7.0],
            ],
            decimals: vec![Some(0), Some(2)],
            ..Table::default()
        };
        assert!(explain_column(&table, 0, &ScanConfig::default()).is_none());
    }

    #[test]
    fn units_from_the_table_are_used() {
        use crate::units::parse_unit;
        // Kinetic energy with units: E = 0.5 * m * v^2.
        let m = [1.0, 2.0, 3.5, 0.5, 7.0, 4.0, 2.5, 6.0, 1.5, 9.0];
        let v = [3.0, 1.0, 2.0, 8.0, 0.5, 4.0, 6.0, 1.5, 2.5, 3.0];
        let e: Vec<f64> = m.iter().zip(&v).map(|(m, v)| 0.5 * m * v * v).collect();
        let table = Table {
            names: vec!["m".into(), "v".into(), "E".into()],
            columns: vec![m.to_vec(), v.to_vec(), e],
            units: ["kg", "m/s", "J"]
                .iter()
                .map(|u| parse_unit(u).ok())
                .collect(),
            ..Table::default()
        };
        let d = explain_column(&table, 2, &ScanConfig::default()).unwrap();
        assert_eq!(d.formula_text, "0.5 * m * v^2");
    }

    #[test]
    fn missing_values_are_skipped() {
        let mut table = invoices();
        table.columns[0][2] = f64::NAN;
        let d = explain_column(&table, 2, &ScanConfig::default()).unwrap();
        assert_eq!(d.formula_text, "price * quantity");
        assert_eq!(d.checked, 11);
    }

    #[test]
    fn samples_are_distinct_and_reproducible() {
        let rows: Vec<usize> = (0..40).collect();
        let s = samples(&rows, 8, 4);
        assert_eq!(s, samples(&rows, 8, 4));
        assert!(s
            .iter()
            .all(|x| x.len() == 8 && x.windows(2).all(|w| w[0] < w[1])));
        assert_ne!(s[0], s[1]);
    }

    #[test]
    fn subsets_cover_different_rows() {
        let rows: Vec<usize> = (0..100).collect();
        let s = subsets(&rows, 10, 3);
        assert_eq!(s.len(), 3);
        assert!(s.iter().all(|x| x.len() == 10));
        assert_ne!(s[0], s[1]);
        assert_eq!(subsets(&rows[..5], 10, 3), vec![rows[..5].to_vec()]);
    }
}
