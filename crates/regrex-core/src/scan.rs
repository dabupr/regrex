//! Dataset scanning: find which columns are computed from other columns.
//!
//! For every numeric column, the scanner searches for a formula over the
//! other columns that reproduces it. Real exports often contain a few rows
//! that do not follow the rule (manual edits, special cases, typos), so the
//! scan is robust: it searches on several subsets of rows, keeps the formula
//! that matches the most rows, and reports the rows that break it.

use crate::expr::{BinaryOp, Expr};
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
    /// Whether each column holds dates or times of day, which are positions
    /// on a scale with an arbitrary zero (26 million minutes since 1970).
    /// Such a column only tolerates its own rounding, never a relative
    /// error, which would let `dropoff = pickup + fare` match within half an
    /// hour. May be empty or shorter than `columns`.
    pub timestamps: Vec<bool>,
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

    /// Whether column `i` holds dates or times (see [`Table::timestamps`]).
    pub fn is_timestamp(&self, i: usize) -> bool {
        self.timestamps.get(i).copied().unwrap_or(false)
    }

    /// Fills in [`Table::decimals`] for columns where it is unknown, from
    /// their values: the fewest decimals (up to 10) that write every value
    /// exactly, as for a column read from a CSV file. Columns that need more
    /// (results of a computation, like `0.1 * pi`) stay unknown.
    pub fn infer_decimals(&mut self) {
        self.decimals.resize(self.columns.len(), None);
        for (d, column) in self.decimals.iter_mut().zip(&self.columns) {
            if d.is_none() {
                *d = infer_decimals(column);
            }
        }
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
        if i < self.timestamps.len() {
            self.timestamps.remove(i);
        }
    }
}

/// The fewest decimals (up to 10) that write every finite value of
/// `values` exactly, or `None` when there are no such values or they need
/// more (or are too large for the decimals to be told apart).
pub fn infer_decimals(values: &[f64]) -> Option<u32> {
    let present: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if present.is_empty() {
        return None;
    }
    (0..=10u32).find(|&d| {
        let scale = 10f64.powi(d as i32);
        present.iter().all(|&v| {
            let scaled = v * scale;
            // Beyond 2^50 every float is a whole number: no evidence.
            scaled.abs() < 1e15 && (scaled - scaled.round()).abs() <= 1e-6
        })
    })
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
    /// About this many other columns are considered as inputs: the ones
    /// that predict the column best, and sometimes one more (see
    /// [`inputs_tried`]).
    pub max_inputs: usize,
    /// Rows per subset when searching robustly.
    pub subset_rows: usize,
    /// When no formula fits every row, the search is also run on this many
    /// small random samples of rows, which are likely to miss a few wrong
    /// rows that spoil the larger subsets. 0 turns it off (faster).
    pub small_samples: usize,
    /// Integer operators tried for columns of whole numbers, as in
    /// `a + b * ((x + c) div d)`. Empty turns them off.
    pub integer_ops: Vec<BinaryOp>,
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
            integer_ops: BinaryOp::INTEGER.to_vec(),
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
    /// A second formula followed by many of the exceptions, when the column
    /// mixes two kinds of rows (two tax rates, two price lists).
    pub other: Option<OtherFormula>,
    /// Whether the column is a copy of another one (see [`find_copies`]):
    /// the formula is that column, maybe shifted or scaled, as in `id`,
    /// `serial + 1` or `1000 * km`.
    pub copy: bool,
}

/// A second formula for some rows of a derived column.
#[derive(Debug, Clone)]
pub struct OtherFormula {
    /// The formula; `Expr::Var(i)` refers to column `i` of the table.
    pub formula: Expr,
    /// The formula written with column names.
    pub formula_text: String,
    /// Rows that follow it (all of them exceptions of the main formula).
    pub rows: Vec<usize>,
    /// A rule on another column that tells these rows apart, if one exists.
    pub split: Option<Split>,
}

/// A rule on one column that separates the rows of the second formula from
/// those of the main one, such as `year >= 2024` or `region = 3`.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// The column the rule is about.
    pub column: usize,
    /// The condition the rows of the second formula meet.
    pub condition: Condition,
    /// Fraction of the rows of both formulas the rule classifies correctly.
    pub accuracy: f64,
}

/// A condition on the values of one column.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// The value is at most this.
    AtMost(f64),
    /// The value is at least this.
    AtLeast(f64),
    /// The value is one of these (for columns with few different values).
    OneOf(Vec<f64>),
}

impl Split {
    /// The rule written with column names, such as `year >= 2024`.
    pub fn text(&self, names: &[String]) -> String {
        let name = &names[self.column];
        match &self.condition {
            Condition::AtMost(v) => format!("{name} <= {}", crate::expr::format_number(*v)),
            Condition::AtLeast(v) => format!("{name} >= {}", crate::expr::format_number(*v)),
            Condition::OneOf(values) if values.len() == 1 => {
                format!("{name} = {}", crate::expr::format_number(values[0]))
            }
            Condition::OneOf(values) => {
                let list: Vec<String> = values
                    .iter()
                    .map(|v| crate::expr::format_number(*v))
                    .collect();
                format!("{name} in {{{}}}", list.join(", "))
            }
        }
    }
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
/// Columns are scanned in parallel when the `parallel` feature is enabled;
/// the result is ordered by column index.
pub fn scan(table: &Table, config: &ScanConfig) -> Vec<DerivedColumn> {
    // Copies of other columns are found first, cheaply: they need no search
    // and are left out as inputs, so they do not take the place of others.
    let copies = find_copies(table, config);
    let explain = |t: usize| explain_column_with_copies(table, t, config, &copies);
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        (0..table.columns.len())
            .into_par_iter()
            .filter_map(explain)
            .collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        (0..table.columns.len()).filter_map(explain).collect()
    }
}

/// Like [`explain_column`], with the copies found by [`find_copies`] on the
/// same table: a copy is returned as is, and copies are not used as inputs
/// (the column they copy is).
pub fn explain_column_with_copies(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    copies: &[Option<DerivedColumn>],
) -> Option<DerivedColumn> {
    if let Some(copy) = copies.get(target).and_then(Option::as_ref) {
        return Some(copy.clone());
    }
    let skip: Vec<bool> = copies.iter().map(Option::is_some).collect();
    explain_column_skipping(table, target, config, &skip)
}

/// The columns [`explain_column_with_copies`] tries as inputs of `target`,
/// in increasing order: empty when the column is not searched at all (a
/// constant, a flag or a copy). Lets a report answer "why was no formula
/// found for X": maybe the right inputs were not among these.
pub fn inputs_tried(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    copies: &[Option<DerivedColumn>],
) -> Vec<usize> {
    let searched = table
        .columns
        .get(target)
        .is_some_and(|y| distinct_values(y, 3) >= 3)
        && copies.get(target).is_none_or(Option::is_none);
    if !searched {
        return Vec::new();
    }
    let skip: Vec<bool> = copies.iter().map(Option::is_some).collect();
    select_inputs(table, target, config.max_inputs, &skip)
}

/// Scales tried between a column and its copy, besides 1: signs, powers of
/// ten (units such as km and m, euros and cents) and minutes and hours.
const COPY_SCALES: [f64; 18] = [
    1.0,
    -1.0,
    10.0,
    100.0,
    1e3,
    1e4,
    1e5,
    1e6,
    0.1,
    0.01,
    1e-3,
    1e-4,
    1e-5,
    1e-6,
    60.0,
    3600.0,
    1.0 / 60.0,
    1.0 / 3600.0,
];

/// Rows of the evenly spread sample compared before checking every row.
const COPY_SAMPLE_ROWS: usize = 16;

/// Finds the columns that are copies of an earlier column: the same values
/// (`idTren = coche0_numeroSerie`), shifted (`serial2 = serial1 + 1`),
/// negated or scaled by a power of ten, 60 or 3600 (`m = 1000 * km`),
/// in at least `config.min_match` of the rows. Returns, for each column,
/// the copy relation to the first column it copies, or `None`.
///
/// Wide exports are full of such columns. Finding them takes one quick
/// comparison per pair of columns, much less than a search, and reporting
/// them apart keeps them from burying the formulas that matter.
pub fn find_copies(table: &Table, config: &ScanConfig) -> Vec<Option<DerivedColumn>> {
    let n = table.columns.len();
    let mut out: Vec<Option<DerivedColumn>> = vec![None; n];
    // Columns that are not copies, in order: the ones copies are made of.
    let mut originals: Vec<usize> = Vec::new();
    for (j, slot) in out.iter_mut().enumerate() {
        // Flags and constants are neither copies nor worth copying.
        if distinct_values(&table.columns[j], 3) < 3 {
            continue;
        }
        *slot = originals
            .iter()
            .find_map(|&i| copy_relation(table, i, j, config));
        if slot.is_none() {
            originals.push(j);
        }
    }
    out
}

/// `y = a * x + b` with `a` in [`COPY_SCALES`], when it holds in enough rows.
fn copy_relation(table: &Table, x: usize, y: usize, config: &ScanConfig) -> Option<DerivedColumn> {
    let xs = &table.columns[x];
    let ys = &table.columns[y];
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| xs[r].is_finite() && ys[r].is_finite())
        .collect();
    let present = ys.iter().filter(|v| v.is_finite()).count();
    if rows.len() < MIN_UNUSUAL_ROWS || rows.len() * 10 < present * 8 {
        return None;
    }
    // A copy gives exactly the same values, up to floating point error: a
    // value within rounding (`0.01 * station` for `station div 100`) is an
    // approximation, left to the search.
    let tolerance = |r: usize| 1e-9 * ys[r].abs().max(1.0);
    let sample = spread(&rows, COPY_SAMPLE_ROWS);
    let needed = (sample.len() * 3).div_ceil(4);
    let (a, b) = COPY_SCALES.iter().find_map(|&a| {
        let mut offsets: Vec<f64> = sample.iter().map(|&r| ys[r] - a * xs[r]).collect();
        offsets.sort_by(f64::total_cmp);
        let mut b = offsets[offsets.len() / 2];
        if b.abs() <= 1e-9 {
            b = 0.0;
        }
        let fits = sample
            .iter()
            .filter(|&&r| (a * xs[r] + b - ys[r]).abs() <= tolerance(r))
            .count()
            >= needed;
        fits.then_some((a, b))
    })?;
    let mut formula = Expr::Var(x);
    if a == -1.0 {
        formula = Expr::unary(crate::expr::UnaryOp::Neg, formula);
    } else if a != 1.0 {
        formula = Expr::binary(BinaryOp::Mul, Expr::Const(a), formula);
    }
    if b != 0.0 {
        formula = Expr::binary(BinaryOp::Add, formula, Expr::Const(b));
    }
    let matched = matching_rows(&formula, table, &rows, ys, &tolerance);
    if (matched.len() as f64) < config.min_match * rows.len() as f64 {
        return None;
    }
    let mut found = derived(table, y, formula, &rows, &matched);
    found.copy = true;
    explains_more_than_the_usual_value(table, y, config, &rows, &found).then_some(found)
}

/// Looks for a formula that computes column `target` from the others.
pub fn explain_column(table: &Table, target: usize, config: &ScanConfig) -> Option<DerivedColumn> {
    explain_column_skipping(table, target, config, &[])
}

/// [`explain_column`] without the columns marked in `skip` as inputs.
fn explain_column_skipping(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    skip: &[bool],
) -> Option<DerivedColumn> {
    let y_all = table.columns.get(target)?;
    // Constant columns and flags (two values) are not computed from others,
    // or not in a way a formula can show; any formula would be a fluke.
    if distinct_values(y_all, 3) < 3 {
        return None;
    }
    let candidates_cols = select_inputs(table, target, config.max_inputs, skip);
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
    let (formula, matched) = best_formula(table, &candidates_cols, target, &rows, config)?;
    let ratio = matched.len() as f64 / rows.len() as f64;
    let enough = |f: &Expr, m: &[usize]| f.has_vars() && m.len() > f.complexity();
    if !enough(&formula, &matched) {
        return None;
    }
    let mut found = derived(table, target, formula, &rows, &matched);
    // Rows that break the formula may follow another one: a second tax rate,
    // another product. Look for it when there are enough of them.
    if found.exceptions.len() >= MIN_OTHER_ROWS {
        if let Some((other, other_rows)) =
            best_formula(table, &candidates_cols, target, &found.exceptions, config)
        {
            let split = split_rule(table, target, &matched, &other_rows);
            // Without a rule that tells the rows apart, only a formula as
            // simple as the main one is believable.
            let plausible = split.is_some() || other.complexity() <= found.complexity;
            if enough(&other, &other_rows)
                && other_rows.len() * 2 >= found.exceptions.len()
                && plausible
            {
                found.other = Some(OtherFormula {
                    formula_text: other.display_with(&table.names).to_string(),
                    formula: other,
                    rows: other_rows,
                    split,
                });
            }
        }
    }
    // A sensor missing on some trains reads 0 there and the real value
    // elsewhere: the main formula then only "explains" the zeros, so say
    // so plainly instead of showing a formula that happens to give 0.
    if found.other.as_ref().is_some_and(|o| o.split.is_some()) {
        if let Some(value) = usual_value(y_all, &matched) {
            let constant = Expr::Const(value);
            let tolerance = row_tolerance(table, target, config);
            let main_rows: Vec<usize> = rows
                .iter()
                .copied()
                .filter(|r| !found.other.as_ref().is_some_and(|o| o.rows.contains(r)))
                .collect();
            let same = matching_rows(&constant, table, &main_rows, y_all, &tolerance);
            let other = found.other.take();
            found = derived(table, target, constant, &rows, &same);
            found.other = other;
        }
    }
    let covered = found.matched + found.other.as_ref().map_or(0, |o| o.rows.len());
    // With few rows, a complicated formula can fit anything: require more
    // matching rows than the formula has parts. Two formulas together must
    // cover as many rows as one would, the main one a fair share of them,
    // and a rule on another column must tell their rows apart.
    let segmented =
        found.other.as_ref().is_some_and(|o| o.split.is_some()) && ratio >= MIN_SEGMENT_SHARE;
    let accepted = ratio >= config.min_match
        || segmented && covered as f64 >= config.min_match * rows.len() as f64;
    if accepted && explains_more_than_the_usual_value(table, target, config, &rows, &found) {
        Some(found)
    } else {
        None
    }
}

/// Whether the formula is right on the rows that differ from the column's
/// most common value. Sensor exports are full of columns that are zero
/// almost everywhere: nearly any formula that gives about zero (with a tiny
/// constant, an exponent near zero) matches most of their rows, so they
/// only count when the other rows match too, and there are enough of them.
fn explains_more_than_the_usual_value(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    rows: &[usize],
    found: &DerivedColumn,
) -> bool {
    let y = &table.columns[target];
    let tolerance = row_tolerance(table, target, config);
    let mut counts: Vec<(f64, usize)> = Vec::new();
    for &r in rows {
        match counts
            .iter_mut()
            .find(|(v, _)| (*v - y[r]).abs() <= tolerance(r))
        {
            Some((_, n)) => *n += 1,
            None => counts.push((y[r], 1)),
        }
        if counts.len() > rows.len() / 2 {
            // No value is that common.
            return true;
        }
    }
    let Some(&(usual, _)) = counts.iter().max_by_key(|(_, n)| *n) else {
        return true;
    };
    let unusual: Vec<usize> = rows
        .iter()
        .copied()
        .filter(|&r| (y[r] - usual).abs() > tolerance(r))
        .collect();
    let exceptions: std::collections::HashSet<usize> = found
        .exceptions
        .iter()
        .copied()
        .filter(|r| !found.other.as_ref().is_some_and(|o| o.rows.contains(r)))
        .collect();
    let explained = unusual.iter().filter(|r| !exceptions.contains(r)).count();
    explained > found.complexity.max(MIN_UNUSUAL_ROWS)
        && explained as f64 >= config.min_match * unusual.len() as f64
}

/// Rows that differ from a column's most common value needed to trust a
/// formula for it.
const MIN_UNUSUAL_ROWS: usize = 5;

/// The value `y` takes on nearly all of `rows` (nine in ten), if any.
fn usual_value(y: &[f64], rows: &[usize]) -> Option<f64> {
    let first = y[*rows.first()?];
    let value = rows
        .iter()
        .map(|&r| y[r])
        .find(|&v| rows.iter().filter(|&&r| y[r] == v).count() * 10 >= rows.len() * 9)
        .unwrap_or(first);
    let count = rows.iter().filter(|&&r| y[r] == value).count();
    (count * 10 >= rows.len() * 9).then_some(value)
}

/// Number of different finite values in `x`, counting up to `limit`.
fn distinct_values(x: &[f64], limit: usize) -> usize {
    let mut seen: Vec<f64> = Vec::with_capacity(limit);
    for &v in x.iter().filter(|v| v.is_finite()) {
        if !seen.contains(&v) {
            seen.push(v);
            if seen.len() >= limit {
                break;
            }
        }
    }
    seen.len()
}

/// Whether the constants of a formula look like real ones. Fits on noisy or
/// unrelated columns end in constants like `2.7e-81` or exponents like
/// `x^4.9e-16` (that is, `x^0`) or `x^86`, which no spreadsheet uses.
fn plausible_constants(e: &Expr) -> bool {
    let ok = |c: f64| c == 0.0 || (1e-12..=1e12).contains(&c.abs());
    match e {
        Expr::Var(_) | Expr::Param(_) => true,
        Expr::Const(c) => ok(*c),
        Expr::Unary(_, a) => plausible_constants(a),
        Expr::Binary(BinaryOp::Pow, a, b) => {
            let exponent_ok = match b.as_ref() {
                Expr::Const(p) => (1e-3..=12.0).contains(&p.abs()),
                other => plausible_constants(other),
            };
            exponent_ok && plausible_constants(a)
        }
        Expr::Binary(_, a, b) => plausible_constants(a) && plausible_constants(b),
    }
}

/// Rows needed to look for a second formula among the rows that break the
/// first one.
const MIN_OTHER_ROWS: usize = 4;

/// Share of rows the main formula must match when two formulas share a
/// column.
const MIN_SEGMENT_SHARE: f64 = 0.25;

/// The formula that matches most of `rows`, searched robustly, with the
/// rows (among `rows`) it matches.
fn best_formula(
    table: &Table,
    candidates_cols: &[usize],
    target: usize,
    rows: &[usize],
    config: &ScanConfig,
) -> Option<(Expr, Vec<usize>)> {
    let y_all = &table.columns[target];
    let rows = rows.to_vec();
    let tolerance = row_tolerance(table, target, config);

    // Search on a few disjoint-ish subsets, so a handful of bad rows cannot
    // spoil every attempt; then once more on the rows the best formula fits.
    let mut best: Option<(Expr, Vec<usize>)> = None;
    let shapes = std::cell::RefCell::new(Vec::<Expr>::new());
    // Whether a formula gives exactly the values of the rows it matches.
    // Between formulas that match the same rows, an exact one is the rule
    // and one that only stays within rounding (`0.0097 * service` for
    // `service div 100`) an approximation of it, however short.
    let exact = |formula: &Expr, matched: &[usize]| {
        let zero = |_: usize| 0.0;
        matching_rows(formula, table, matched, y_all, &zero).len() == matched.len()
    };
    let consider = |formula: Expr, best: &mut Option<(Expr, Vec<usize>)>| {
        if !plausible_constants(&formula) {
            return;
        }
        let shape = parametrize(&formula);
        if (1..=MAX_SHAPE_PARAMS).contains(&shape.num_params()) {
            let mut shapes = shapes.borrow_mut();
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        let matched = matching_rows(&formula, table, &rows, y_all, &tolerance);
        let better = match best {
            None => true,
            Some((f, m)) => {
                matched.len() > m.len()
                    || matched.len() == m.len() && {
                        let (new_exact, old_exact) = (exact(&formula, &matched), exact(f, m));
                        new_exact && !old_exact
                            || new_exact == old_exact && formula.complexity() < f.complexity()
                    }
            }
        };
        if better {
            *best = Some((formula, matched));
        }
    };

    for subset in subsets(&rows, config.subset_rows, 3) {
        for formula in search_rows(table, candidates_cols, target, &subset, config) {
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
            for formula in search_rows(table, candidates_cols, target, &subset, config) {
                consider(formula, &mut best);
            }
        }
    }
    // Codes and numbers built with whole-number arithmetic (`line = circuit
    // div 100`, `coach = 7250 + (train + 3) div 4`) are out of reach of the
    // search, whose fitted constants need smooth functions.
    // A formula like `0.25 * train + 7250.33` can stay within rounding of
    // such a column on every row; an integer formula that gives the exact
    // values of as many rows is the real rule, and wins over it.
    if best
        .as_ref()
        .is_none_or(|(f, m)| m.len() < rows.len() || !exact(f, m))
    {
        let ops = &config.integer_ops;
        for formula in integer_formulas(table, candidates_cols, target, &rows, ops) {
            consider(formula, &mut best);
        }
    }
    // Rows of two kinds (two tax rates) spoil every sample of a few rows, but
    // the shapes found still fit: refit their constants on tiny samples.
    if best
        .as_ref()
        .is_none_or(|(_, m)| (m.len() as f64) < config.min_match * rows.len() as f64)
    {
        let mut shapes = shapes.take();
        shapes.sort_by_key(Expr::complexity);
        shapes.truncate(MAX_SHAPES);
        for shape in &shapes {
            let size = shape.num_params() + 1;
            if let Some((formula, _)) =
                robust_constants(table, target, shape, &rows, &tolerance, size, TINY_SAMPLES)
            {
                consider(formula, &mut best);
            }
        }
    }
    let (_, inliers) = best.clone()?;
    if inliers.len() >= 3 && inliers.len() < rows.len() {
        let refined = spread(&inliers, config.subset_rows);
        for formula in search_rows(table, candidates_cols, target, &refined, config) {
            consider(formula, &mut best);
        }
    }

    best
}

/// Shapes (formulas with their constants as parameters) refitted on tiny
/// samples when no formula matches enough rows.
const MAX_SHAPES: usize = 12;
/// Shapes with more constants than this are not refitted.
const MAX_SHAPE_PARAMS: usize = 3;
/// Tiny samples tried for each shape.
const TINY_SAMPLES: usize = 24;

/// The formula with its constants turned into parameters, except exponents
/// and small whole numbers (`x^2`, `2 * pi`), which define the shape.
fn parametrize(e: &Expr) -> Expr {
    fn go(e: &Expr) -> Expr {
        match e {
            Expr::Const(c) if c.fract() != 0.0 || c.abs() > 10.0 => Expr::Param(0),
            Expr::Var(_) | Expr::Const(_) | Expr::Param(_) => e.clone(),
            Expr::Unary(op, a) => Expr::unary(*op, go(a)),
            Expr::Binary(BinaryOp::Pow, a, b) => Expr::binary(BinaryOp::Pow, go(a), (**b).clone()),
            Expr::Binary(op, a, b) => Expr::binary(*op, go(a), go(b)),
        }
    }
    go(e).number_params()
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
///     timestamps: Vec::new(),
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

/// Fits the constants of a formula shape, such as `a * net` or
/// `a * x^b + c` (see [`crate::parse::parse_template`]), to column `target`.
///
/// Like the scan, the fit is robust to a few wrong rows: constants are
/// fitted on all rows and on small samples, the fit that matches most rows
/// wins and is refined on the rows it matches, and constants are rounded to
/// simple values (`0.21`, `0.5`, `π`) when that matches as many rows. The
/// result is returned even when few rows match, like [`check_formula`].
/// Returns `None` when the constants cannot be fitted at all.
///
/// ```
/// use regrex_core::{fit_shape, parse::parse_template, ScanConfig, Table};
///
/// let table = Table {
///     names: vec!["net".into(), "vat".into()],
///     columns: vec![vec![10.0, 20.0, 30.0, 40.0], vec![2.1, 4.2, 6.3, 8.4]],
///     decimals: vec![Some(2), Some(2)],
///     units: vec![None, None],
///     timestamps: Vec::new(),
/// };
/// let names: Vec<&str> = table.names.iter().map(String::as_str).collect();
/// let (shape, _) = parse_template("rate * net", &names).unwrap();
/// let fit = fit_shape(&table, 1, &shape, &ScanConfig::default()).unwrap();
/// assert_eq!(fit.formula.display_with(&table.names).to_string(), "0.21 * net");
/// assert_eq!(fit.matched, 4);
/// ```
pub fn fit_shape(
    table: &Table,
    target: usize,
    shape: &Expr,
    config: &ScanConfig,
) -> Option<DerivedColumn> {
    let n_params = shape.num_params();
    if n_params == 0 {
        return Some(check_formula(table, target, shape, config));
    }
    let mut used = Vec::new();
    collect_vars(shape, &mut used);
    let y_all = &table.columns[target];
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| y_all[r].is_finite() && used.iter().all(|&c| table.columns[c][r].is_finite()))
        .collect();
    if rows.len() < n_params {
        return None;
    }
    let tolerance = row_tolerance(table, target, config);
    let sample_size = SMALL_SUBSET.max(2 * n_params + 2);
    let (formula, matched_rows) = robust_constants(
        table,
        target,
        shape,
        &rows,
        &tolerance,
        sample_size,
        config.small_samples.max(4),
    )?;
    Some(derived(table, target, formula, &rows, &matched_rows))
}

/// Fits the parameters of `shape` to `rows` robustly: on all of them and on
/// `n_samples` random samples of `sample_size` rows; the fit that matches
/// most rows wins, is refined on the rows it matches and has its constants
/// rounded to simple values when that matches as many rows.
fn robust_constants(
    table: &Table,
    target: usize,
    shape: &Expr,
    rows: &[usize],
    tolerance: &dyn Fn(usize) -> f64,
    sample_size: usize,
    n_samples: usize,
) -> Option<(Expr, Vec<usize>)> {
    let n_params = shape.num_params();
    let y_all = &table.columns[target];
    let opts = crate::fit::FitOptions::default();
    let fit_on = |subset: &[usize]| -> Option<Vec<f64>> {
        let columns: Vec<Vec<f64>> = table
            .columns
            .iter()
            .map(|c| subset.iter().map(|&r| c[r]).collect())
            .collect();
        let y: Vec<f64> = subset.iter().map(|&r| y_all[r]).collect();
        crate::fit::fit_params(shape, &columns, &y, &opts).map(|(p, _)| p)
    };
    let score = |params: &[f64]| -> usize {
        matching_rows(
            &shape.substitute_params(params),
            table,
            rows,
            y_all,
            &tolerance,
        )
        .len()
    };

    let mut attempts = vec![spread(rows, 400)];
    if rows.len() >= 2 * sample_size {
        attempts.extend(samples(rows, sample_size, n_samples));
    }
    let mut best: Option<(Vec<f64>, usize)> = None;
    for subset in &attempts {
        if let Some(p) = fit_on(subset) {
            let m = score(&p);
            if best.as_ref().is_none_or(|(_, bm)| m > *bm) {
                best = Some((p, m));
            }
        }
    }
    let (mut params, mut matched) = best?;
    // Refine on the rows the best fit matches, which are free of wrong rows.
    let inliers = matching_rows(
        &shape.substitute_params(&params),
        table,
        rows,
        y_all,
        &tolerance,
    );
    if inliers.len() > n_params && inliers.len() < rows.len() {
        if let Some(p) = fit_on(&spread(&inliers, 400)) {
            let m = score(&p);
            if m >= matched {
                (params, matched) = (p, m);
            }
        }
    }
    // Round each constant to the simplest value that matches as many rows.
    for i in 0..n_params {
        for nice in crate::fit::nice_values(params[i]) {
            let mut p = params.clone();
            p[i] = nice;
            let m = score(&p);
            if m >= matched {
                (params, matched) = (p, m);
                break;
            }
        }
    }
    let formula = shape.substitute_params(&params);
    let matched_rows = matching_rows(&formula, table, rows, y_all, tolerance);
    Some((formula, matched_rows))
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
/// match: half a unit in the last written decimal, or a small relative error
/// (except for dates and times).
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
        .map_or(0.0, |d| {
            // Half a unit of the last digit, a hair more to absorb floating
            // point error. Except for whole numbers, where a formula off by
            // exactly 0.5 (`x + 0.5` "matching" both `x` and `x + 1`) is a
            // fluke, not rounding.
            let slack = if d == 0 { 0.999_999 } else { 1.000_001 };
            0.5 * 10f64.powi(-(d as i32)) * slack
        });
    let relative = if table.is_timestamp(target) {
        0.0
    } else {
        config.rel_tolerance
    };
    move |r: usize| {
        let y = table.columns[target][r];
        rounding.max(relative * y.abs()).max(1e-12)
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
        other: None,
        copy: false,
    }
}

/// Columns with more different values than this are not treated as
/// categories by split rules.
const MAX_CATEGORIES: usize = 12;

/// Minimum share of rows a split rule must classify correctly.
const MIN_SPLIT_ACCURACY: f64 = 0.95;

/// The simplest rule on a column other than `target` that tells the rows of
/// `other` apart from the rows of `main`.
fn split_rule(table: &Table, target: usize, main: &[usize], other: &[usize]) -> Option<Split> {
    let total = (main.len() + other.len()) as f64;
    // With hundreds of columns, some column tells a dozen rows apart from
    // the rest by chance with a mistake or two, so mistakes are also limited
    // to a tenth of the rows the rule picks out.
    let max_errors = (other.len() / 10) as f64;
    let mut best: Option<Split> = None;
    let mut consider = |split: Split| {
        let errors = ((1.0 - split.accuracy) * total).round();
        if split.accuracy >= MIN_SPLIT_ACCURACY
            && errors <= max_errors
            && best.as_ref().is_none_or(|b| split.accuracy > b.accuracy)
        {
            best = Some(split);
        }
    };
    for (c, values) in table.columns.iter().enumerate() {
        if c == target {
            continue;
        }
        let a: Vec<f64> = main
            .iter()
            .map(|&r| values[r])
            .filter(|v| v.is_finite())
            .collect();
        let b: Vec<f64> = other
            .iter()
            .map(|&r| values[r])
            .filter(|v| v.is_finite())
            .collect();
        if a.is_empty() || b.is_empty() {
            continue;
        }
        let correct = |is_other: &dyn Fn(f64) -> bool| {
            let hits = a.iter().filter(|&&v| !is_other(v)).count()
                + b.iter().filter(|&&v| is_other(v)).count();
            hits as f64 / total
        };
        // Thresholds: between the largest and smallest values of the groups.
        let (a_min, a_max) = min_max(&a);
        let (b_min, b_max) = min_max(&b);
        if b_max < a_min {
            consider(Split {
                column: c,
                condition: Condition::AtMost(b_max),
                accuracy: correct(&|v| v <= b_max),
            });
        } else if b_min > a_max {
            consider(Split {
                column: c,
                condition: Condition::AtLeast(b_min),
                accuracy: correct(&|v| v >= b_min),
            });
        } else {
            // Overlapping ranges: the best cut among the values of `other`.
            for &t in &b {
                consider(Split {
                    column: c,
                    condition: Condition::AtMost(t),
                    accuracy: correct(&|v| v <= t),
                });
                consider(Split {
                    column: c,
                    condition: Condition::AtLeast(t),
                    accuracy: correct(&|v| v >= t),
                });
            }
        }
        // Categories: the values of `other`, when there are few of them, in
        // a column of categories (few different values overall), not
        // measurements that happen to repeat.
        let mut cats: Vec<f64> = b.clone();
        cats.sort_by(f64::total_cmp);
        cats.dedup();
        if cats.len() <= 3
            && cats.len() < b.len()
            && distinct_values(values, MAX_CATEGORIES + 1) <= MAX_CATEGORIES
        {
            let accuracy = correct(&|v| cats.contains(&v));
            consider(Split {
                column: c,
                condition: Condition::OneOf(cats),
                accuracy,
            });
        }
    }
    best
}

fn min_max(values: &[f64]) -> (f64, f64) {
    values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        })
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
fn select_inputs(table: &Table, target: usize, max_inputs: usize, skip: &[bool]) -> Vec<usize> {
    let ranked = ranked_inputs(table, target, skip);
    let mut cols: Vec<usize> = ranked.iter().copied().take(max_inputs).collect();
    // One more input at most: the only one left out of a narrow table, or
    // the second factor of a product.
    if ranked.len() == max_inputs + 1 {
        cols.push(ranked[max_inputs]);
    } else if let Some(extra) = second_factor(table, target, &ranked, max_inputs) {
        cols.push(extra);
    }
    cols.sort_unstable();
    cols
}

/// Columns with at most this many different values are treated as
/// categories (flags, codes, modes) when scoring inputs.
const MAX_INPUT_CATEGORIES: usize = 16;

/// Weight of the correlation ratio of a category column against the
/// correlation of a measurement.
const CATEGORY_DISCOUNT: f64 = 0.9;

/// How closely a column must follow what the best input leaves unexplained
/// to be added as an extra input.
const SECOND_FACTOR_SCORE: f64 = 0.98;

/// The columns that may be inputs of `target`, best first.
///
/// Each column is scored by how well it predicts the target: the best
/// Pearson correlation with the target, its log or its square root, and for
/// columns with few values (flags, codes) the correlation ratio, which also
/// sees relations that are not straight lines (`speed + 100 * (mode - 1.5)^2`).
/// Columns equal to a better one are left out: they would only take a slot.
fn ranked_inputs(table: &Table, target: usize, skip: &[bool]) -> Vec<usize> {
    let y = &table.columns[target];
    let present = y.iter().filter(|v| v.is_finite()).count();
    let mut scored: Vec<(f64, usize)> = (0..table.columns.len())
        .filter(|&c| c != target && !skip.get(c).copied().unwrap_or(false))
        .filter(|&c| !is_constant(&table.columns[c]))
        // Only rows where every input is present are searched, so a column
        // missing in many of the target's rows would leave too few of them.
        .filter(|&c| {
            let x = &table.columns[c];
            let both = (0..y.len())
                .filter(|&r| y[r].is_finite() && x[r].is_finite())
                .count();
            both * 10 >= present * 8
        })
        .map(|c| (input_score(&table.columns[c], y), c))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ranked: Vec<usize> = Vec::with_capacity(scored.len());
    for (_, c) in scored {
        if !ranked
            .iter()
            .any(|&k| same_values(&table.columns[k], &table.columns[c]))
        {
            ranked.push(c);
        }
    }
    ranked
}

/// A column left out of the first `taken` of `ranked` that explains what
/// the best input leaves: `target / best` (the second factor of a product,
/// such as a rate that varies little) or `target - fit(best)` (a small term
/// added to it). Plain correlation ranks such a column low, below columns
/// that merely look like the best one.
fn second_factor(table: &Table, target: usize, ranked: &[usize], taken: usize) -> Option<usize> {
    let &best = ranked.first()?;
    let y = &table.columns[target];
    let x0 = &table.columns[best];
    let ratio: Vec<f64> = y
        .iter()
        .zip(x0)
        .map(|(a, b)| if *b != 0.0 { a / b } else { f64::NAN })
        .collect();
    let residual = linear_residual(x0, y);
    // Only the next few are looked at: wide files have hundreds of columns,
    // and among so many one would follow the leftover by chance.
    let shortlist = ranked.len().min(taken * 4);
    ranked
        .get(taken..shortlist)?
        .iter()
        .map(|&c| {
            let x = &table.columns[c];
            let score = [
                correlation(x, &ratio, |v| v).abs(),
                residual
                    .as_ref()
                    .map_or(0.0, |r| correlation(x, r, |v| v).abs()),
            ]
            .into_iter()
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max);
            (score, c)
        })
        .filter(|&(score, _)| score >= SECOND_FACTOR_SCORE)
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, c)| c)
}

/// How well column `x` predicts `y`, from 0 to 1.
fn input_score(x: &[f64], y: &[f64]) -> f64 {
    let mut s = [
        correlation(x, y, |v| v),
        correlation(x, y, |v| v.abs().ln()),
        correlation(x, y, |v| v.abs().sqrt()),
    ]
    .into_iter()
    .map(f64::abs)
    .filter(|v| v.is_finite())
    .fold(0.0, f64::max);
    // Discounted: a formula is more likely over a column that follows the
    // target than over a code that only groups its values.
    if distinct_values(x, MAX_INPUT_CATEGORIES + 1) <= MAX_INPUT_CATEGORIES {
        s = s.max(CATEGORY_DISCOUNT * correlation_ratio(x, y));
    }
    s
}

/// The correlation ratio of `y` on the categories of `x`: the square root
/// of the share of the variance of `y` explained by its mean in each
/// category, less what that many categories explain by chance. 1 when `x`
/// alone fixes `y`, whatever the shape of the relation.
fn correlation_ratio(x: &[f64], y: &[f64]) -> f64 {
    let mut groups: Vec<(f64, f64, usize)> = Vec::new(); // value, sum of y, count
    let (mut sum, mut sum2, mut n) = (0.0, 0.0, 0usize);
    for (&a, &b) in x.iter().zip(y) {
        if !(a.is_finite() && b.is_finite()) {
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == a) {
            Some(g) => {
                g.1 += b;
                g.2 += 1;
            }
            None => groups.push((a, b, 1)),
        }
        sum += b;
        sum2 += b * b;
        n += 1;
    }
    // With few rows per category, the means differ by chance: a category
    // with one row "explains" it perfectly. Ask for several rows each.
    let k = groups.len();
    if k < 2 || k * 4 > n {
        return 0.0;
    }
    let mean = sum / n as f64;
    let total = sum2 - n as f64 * mean * mean;
    if total <= 0.0 {
        return 0.0;
    }
    let between: f64 = groups
        .iter()
        .map(|g| {
            let m = g.1 / g.2 as f64;
            g.2 as f64 * (m - mean) * (m - mean)
        })
        .sum();
    // Adjusted for the number of categories, as the adjusted R² is: k
    // categories explain about (k - 1) / (n - 1) of anything by chance.
    let eta2 = (between / total).clamp(0.0, 1.0);
    let adjusted = 1.0 - (1.0 - eta2) * (n - 1) as f64 / (n - k) as f64;
    adjusted.clamp(0.0, 1.0).sqrt()
}

/// Whether two columns hold the same value (or both nothing) in every row.
fn same_values(a: &[f64], b: &[f64]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| x == y || (!x.is_finite() && !y.is_finite()))
}

/// `y - (a + b * x)` for the least-squares line, or `None` when it cannot
/// be fitted.
fn linear_residual(x: &[f64], y: &[f64]) -> Option<Vec<f64>> {
    let pairs = x
        .iter()
        .zip(y)
        .filter(|(a, b)| a.is_finite() && b.is_finite());
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (a, b) in pairs {
        n += 1.0;
        sx += a;
        sy += b;
        sxx += a * a;
        sxy += a * b;
    }
    let d = n * sxx - sx * sx;
    if n < 3.0 || d == 0.0 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / d;
    let intercept = (sy - slope * sx) / n;
    Some(
        x.iter()
            .zip(y)
            .map(|(a, b)| b - (intercept + slope * a))
            .collect(),
    )
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

/// Divisors tried for integer division and remainder: small numbers, and
/// the bases of clocks, angles, codes and computer sizes.
const DIVISORS: [f64; 27] = [
    2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 12.0, 16.0, 20.0, 24.0, 25.0, 32.0, 50.0, 60.0,
    64.0, 100.0, 128.0, 256.0, 360.0, 1000.0, 1024.0, 3600.0, 10000.0, 86400.0,
];

/// Divisors up to this also get every offset, as in `(x + 3) div 4`.
const MAX_OFFSET_DIVISOR: f64 = 12.0;

/// Rows needed to look for an integer formula: with a few rows, some
/// offset and divisor always fit (when looking for a second formula among
/// the handful of rows that break the first one).
const MIN_INTEGER_ROWS: usize = 12;

/// Rows used to choose the integer formulas, which are then checked on
/// every row by the caller.
const INTEGER_FIT_ROWS: usize = 200;

/// Formulas of the form `a + b * ((x + c) div d)` and `a + b * ((x + c) mod
/// d)` for whole-number columns: the best one for each input, with whole
/// `a` and `b` fitted robustly (least squares for `b`, the median for `a`)
/// so that a few wrong rows do not shift them. Empty unless the target and
/// the input hold whole numbers in `rows`.
fn integer_formulas(
    table: &Table,
    inputs: &[usize],
    target: usize,
    rows: &[usize],
    ops: &[BinaryOp],
) -> Vec<Expr> {
    let whole = |c: usize| {
        rows.iter().all(|&r| {
            let v = table.columns[c][r];
            v.fract() == 0.0 && v.abs() < 1e15
        })
    };
    if ops.is_empty() || rows.len() < MIN_INTEGER_ROWS || !whole(target) {
        return Vec::new();
    }
    let sample = spread(rows, INTEGER_FIT_ROWS);
    let y: Vec<f64> = sample.iter().map(|&r| table.columns[target][r]).collect();
    let mut out = Vec::new();
    for &input in inputs.iter().filter(|&&c| whole(c)) {
        let x: Vec<f64> = sample.iter().map(|&r| table.columns[input][r]).collect();
        // (matched rows of the sample, complexity, formula)
        let mut best: Option<(usize, usize, Expr)> = None;
        for d in DIVISORS {
            let offsets = if d <= MAX_OFFSET_DIVISOR {
                d as usize
            } else {
                1
            };
            for c in (0..offsets).map(|c| c as f64) {
                for &op in ops {
                    let f: Vec<f64> = x.iter().map(|&v| op.apply(v + c, d)).collect();
                    for (a, b) in whole_lines(&f, &y) {
                        let matched = f
                            .iter()
                            .zip(&y)
                            .filter(|(fv, yv)| a + b * **fv == **yv)
                            .count();
                        let formula = integer_formula(input, op, c, d, a, b);
                        let complexity = formula.complexity();
                        let better = best.as_ref().is_none_or(|(m, k, _)| {
                            matched > *m || matched == *m && complexity < *k
                        });
                        if better {
                            best = Some((matched, complexity, formula));
                        }
                    }
                }
            }
        }
        // Worth checking on every row only if it fits most of the sample.
        if let Some((matched, _, formula)) = best {
            if matched * 2 > sample.len() {
                out.push(formula);
            }
        }
    }
    out
}

/// Whole numbers `a` and `b` worth trying for `y ≈ a + b * f`, if `f` takes
/// at least two values: `b` from least squares, and `1` and `-1`, which
/// least squares misses when a few rows are far off (line `91` for station
/// `905`); `a` is the median of `y - b * f`.
fn whole_lines(f: &[f64], y: &[f64]) -> Vec<(f64, f64)> {
    let n = f.len() as f64;
    let mf = f.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sfy, mut sff) = (0.0, 0.0);
    for (fv, yv) in f.iter().zip(y) {
        sfy += (fv - mf) * (yv - my);
        sff += (fv - mf) * (fv - mf);
    }
    if sff.is_nan() || sff <= 0.0 {
        return Vec::new();
    }
    let mut slopes = vec![1.0, -1.0];
    let fitted = (sfy / sff).round();
    if fitted != 0.0 && fitted.is_finite() && !slopes.contains(&fitted) {
        slopes.insert(0, fitted);
    }
    slopes
        .into_iter()
        .map(|b| {
            let mut rest: Vec<f64> = f.iter().zip(y).map(|(fv, yv)| yv - b * fv).collect();
            rest.sort_by(f64::total_cmp);
            (rest[rest.len() / 2], b)
        })
        .collect()
}

/// `a + b * ((x + c) op d)`, leaving out what is 0 or 1.
fn integer_formula(input: usize, op: BinaryOp, c: f64, d: f64, a: f64, b: f64) -> Expr {
    let mut inner = Expr::Var(input);
    if c != 0.0 {
        inner = Expr::binary(BinaryOp::Add, inner, Expr::Const(c));
    }
    let mut e = Expr::binary(op, inner, Expr::Const(d));
    if b == -1.0 {
        e = Expr::unary(crate::expr::UnaryOp::Neg, e);
    } else if b != 1.0 {
        e = Expr::binary(BinaryOp::Mul, Expr::Const(b), e);
    }
    if a != 0.0 {
        e = Expr::binary(BinaryOp::Add, Expr::Const(a), e);
    }
    e
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

    /// Trains numbered 9001 to 9120, with a service number, the line it
    /// runs on (the hundreds digit) and the number of one of its coaches.
    fn trains() -> Table {
        let train: Vec<f64> = (0..120).map(|i| 9001.0 + i as f64).collect();
        let service: Vec<f64> = (0..120)
            .map(|i| 100.0 * (1 + (i * i * 7919 + 3) % 13 % 5) as f64 + ((i * 37) % 100) as f64)
            .collect();
        let line: Vec<f64> = service.iter().map(|s| (s / 100.0).floor()).collect();
        let coach: Vec<f64> = train
            .iter()
            .map(|t| 9500.0 + ((t - 9000.0 + 3.0) / 4.0).floor())
            .collect();
        let car: Vec<f64> = train.iter().map(|t| t % 4.0 + 1.0).collect();
        Table {
            names: ["train", "service", "line", "coach", "car"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            columns: vec![train, service, line, coach, car],
            decimals: vec![Some(0); 5],
            ..Table::default()
        }
    }

    #[test]
    fn finds_integer_division_and_remainder() {
        let table = trains();
        let config = ScanConfig::default();
        let formula = |t: usize| {
            let d = explain_column(&table, t, &config).expect("derived");
            assert!(
                d.exceptions.is_empty(),
                "{}: {:?}",
                d.formula_text,
                d.exceptions
            );
            d.formula_text
        };
        assert_eq!(formula(2), "service div 100");
        assert_eq!(formula(3), "7250 + (train + 3) div 4");
        assert_eq!(formula(4), "1 + train mod 4");
    }

    #[test]
    fn integer_formulas_beat_approximations_and_far_off_rows() {
        // Line = hundreds digit of the station, except a few stations of
        // another numbering (905 is on line 91), which spoil least squares;
        // `0.0097 * station` stays within rounding of most rows too.
        let station: Vec<f64> = (0..60)
            .map(|i| (100 * (1 + i % 5) + (i * 7) % 40) as f64)
            .collect();
        let mut line: Vec<f64> = station.iter().map(|s| (s / 100.0).floor()).collect();
        let mut station = station;
        for r in [7, 23, 41] {
            station[r] = 905.0 + r as f64;
            line[r] = 91.0;
        }
        let table = Table {
            names: vec!["station".into(), "line".into()],
            columns: vec![station, line],
            decimals: vec![Some(0); 2],
            ..Table::default()
        };
        let config = ScanConfig {
            min_match: 0.8,
            ..ScanConfig::default()
        };
        let d = explain_column(&table, 1, &config).expect("derived");
        assert_eq!(d.formula_text, "station div 100");
        assert_eq!(d.exceptions, vec![7, 23, 41]);
    }

    #[test]
    fn integer_formulas_tolerate_a_wrong_row() {
        let mut table = trains();
        table.columns[2][17] = 0.0;
        let d = explain_column(&table, 2, &ScanConfig::default()).expect("derived");
        assert_eq!(d.formula_text, "service div 100");
        assert_eq!(d.exceptions, vec![17]);
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

    /// `vat` is 21% of `net` until 2022 and 10% from 2023 (a made-up tax
    /// change). `year_of` gives the year of each row.
    fn two_rates(year_of: &dyn Fn(usize) -> f64) -> Table {
        let n = 60;
        let net: Vec<f64> = (0..n).map(|i| 20.0 + ((i * 37) % 101) as f64).collect();
        let year: Vec<f64> = (0..n).map(year_of).collect();
        let vat: Vec<f64> = (0..n)
            .map(|i| {
                let rate = if year[i] >= 2023.0 { 0.1 } else { 0.21 };
                (net[i] * rate * 100.0).round() / 100.0
            })
            .collect();
        Table {
            names: vec!["net".into(), "year".into(), "vat".into()],
            columns: vec![net, year, vat],
            decimals: vec![Some(0), Some(0), Some(2)],
            units: vec![None; 3],
            timestamps: Vec::new(),
        }
    }

    #[test]
    fn two_formulas_and_the_rule_that_splits_them() {
        for (name, year_of) in [
            // Sorted by year, 12 rows a year from 2020 to 2024.
            (
                "sorted",
                &(|i: usize| 2020.0 + (i / 12) as f64) as &dyn Fn(usize) -> f64,
            ),
            ("mixed", &|i: usize| 2020.0 + (i % 5) as f64),
        ] {
            let table = two_rates(year_of);
            let found = explain_column(&table, 2, &ScanConfig::default())
                .unwrap_or_else(|| panic!("{name}: vat is derived"));
            assert_eq!(found.formula_text, "0.21 * net", "{name}");
            let other = found
                .other
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: two rates"));
            assert_eq!(other.formula_text, "0.1 * net", "{name}");
            assert_eq!(other.rows.len(), 24, "{name}");
            let split = other.split.as_ref().expect("the year splits them");
            assert_eq!(split.text(&table.names), "year >= 2023", "{name}");
            assert_eq!(split.accuracy, 1.0);
        }
    }

    #[test]
    fn fits_a_shape_despite_wrong_rows() {
        use crate::parse::parse_template;
        // y = 3 * x^2 with two wrong rows.
        let x: Vec<f64> = (1..=30).map(f64::from).collect();
        let mut y: Vec<f64> = x.iter().map(|v| 3.0 * v * v).collect();
        y[4] = 1.0;
        y[17] = 999.0;
        let table = Table {
            names: vec!["x".into(), "y".into()],
            columns: vec![x, y],
            decimals: vec![Some(0), Some(0)],
            units: vec![None, None],
            timestamps: Vec::new(),
        };
        let (shape, params) = parse_template("a * x^b", &["x", "y"]).unwrap();
        assert_eq!(params, ["a", "b"]);
        let fit = fit_shape(&table, 1, &shape, &ScanConfig::default()).unwrap();
        assert_eq!(
            fit.formula.display_with(&table.names).to_string(),
            "3 * x^2"
        );
        assert_eq!(fit.exceptions, vec![4, 17]);
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
            timestamps: Vec::new(),
        };
        let found = explain_column(&table, 1, &ScanConfig::default()).expect("vat is derived");
        assert_eq!(
            found.formula.display_with(&table.names).to_string(),
            "0.21 * net"
        );
        assert_eq!(found.exceptions, vec![12, 25, 38]);
    }

    #[test]
    fn timestamps_only_tolerate_their_own_rounding() {
        // Taxi rides: dropoff and pickup as minutes since 1970 (about 26
        // million), rides of ~10 minutes, fares of ~10 dollars. A relative
        // tolerance of 1e-6 would be 26 minutes, so `dropoff - fare` would
        // "explain" pickup.
        let n = 60;
        let pickup: Vec<f64> = (0..n)
            .map(|i| 25_862_000.0 + 97.0 * i as f64 + ((i * 7) % 60) as f64 / 60.0)
            .collect();
        let ride: Vec<f64> = (0..n).map(|i| 4.0 + ((i * 13) % 17) as f64).collect();
        let fare: Vec<f64> = (0..n).map(|i| 5.0 + ((i * 11) % 19) as f64 * 0.5).collect();
        let dropoff: Vec<f64> = pickup.iter().zip(&ride).map(|(p, r)| p + r).collect();
        let mut table = Table {
            names: vec![
                "pickup".into(),
                "dropoff".into(),
                "fare".into(),
                "ride".into(),
            ],
            columns: vec![pickup, dropoff, fare, ride],
            decimals: vec![Some(2), Some(2), Some(1), Some(0)],
            units: Vec::new(),
            timestamps: vec![true, true, false, false],
        };
        // The real relation is still found.
        let found = explain_column(&table, 1, &ScanConfig::default()).expect("dropoff is derived");
        assert_eq!(found.exceptions, Vec::<usize>::new());
        assert!(found.inputs.contains(&3), "{}", found.formula_text);
        // Without the ride length, the fare must not stand in for it.
        table.remove_column(3);
        assert_eq!(table.timestamps, vec![true, true, false]);
        assert!(explain_column(&table, 0, &ScanConfig::default()).is_none());
        assert!(explain_column(&table, 1, &ScanConfig::default()).is_none());
    }

    /// A table with `noise` columns that follow `main` loosely, so plain
    /// correlation fills the input slots with them.
    fn with_lookalikes(
        mut names: Vec<&str>,
        mut columns: Vec<Vec<f64>>,
        main: usize,
        noise: usize,
    ) -> Table {
        let n = columns[0].len();
        for k in 0..noise {
            let name: &'static str = Box::leak(format!("look{k}").into_boxed_str());
            names.push(name);
            columns.push(
                (0..n)
                    .map(|i| {
                        columns[main][i] * (1.0 + 0.02 * (((i * (k + 3) * 7) % 11) as f64 - 5.0))
                    })
                    .collect(),
            );
        }
        Table {
            names: names.into_iter().map(String::from).collect(),
            decimals: vec![None; columns.len()],
            columns,
            ..Table::default()
        }
    }

    #[test]
    fn the_second_factor_of_a_product_is_an_input_even_if_it_varies_little() {
        let n = 60;
        let net: Vec<f64> = (0..n)
            .map(|i| 100.0 + 37.0 * ((i * 13) % 41) as f64)
            .collect();
        let rate: Vec<f64> = (0..n).map(|i| 1.0 + 0.01 * ((i * 7) % 5) as f64).collect();
        let gross: Vec<f64> = net.iter().zip(&rate).map(|(a, b)| a * b).collect();
        let table = with_lookalikes(vec!["net", "rate", "gross"], vec![net, rate, gross], 0, 8);
        let inputs = select_inputs(&table, 2, 3, &[]);
        assert!(inputs.contains(&1), "rate missing from {inputs:?}");
        let found = explain_column(&table, 2, &ScanConfig::default()).expect("gross");
        assert_eq!(found.formula_text, "net * rate");
    }

    #[test]
    fn a_category_that_changes_a_column_is_an_input() {
        // The mode adds 225, 25, 25 or 225: no straight line in the mode.
        let n = 80;
        let mode: Vec<f64> = (0..n).map(|i| ((i * 5) % 4) as f64).collect();
        let speed: Vec<f64> = (0..n).map(|i| 20.0 + ((i * 17) % 53) as f64).collect();
        let y: Vec<f64> = mode
            .iter()
            .zip(&speed)
            .map(|(m, s)| s + 100.0 * (m - 1.5) * (m - 1.5))
            .collect();
        let table = with_lookalikes(vec!["mode", "speed", "y"], vec![mode, speed, y], 1, 8);
        let inputs = select_inputs(&table, 2, 2, &[]);
        assert!(inputs.contains(&0), "mode missing from {inputs:?}");
    }

    #[test]
    fn equal_columns_take_one_input_slot() {
        let n = 30;
        let a: Vec<f64> = (0..n).map(|i| 3.0 + ((i * 7) % 19) as f64).collect();
        let b: Vec<f64> = (0..n).map(|i| 1.0 + ((i * 5) % 13) as f64).collect();
        let y: Vec<f64> = a.iter().zip(&b).map(|(x, z)| x + z).collect();
        let table = Table {
            names: ["a", "a2", "a3", "b", "y"].map(String::from).to_vec(),
            columns: vec![a.clone(), a.clone(), a, b, y],
            decimals: vec![Some(0); 5],
            ..Table::default()
        };
        assert_eq!(select_inputs(&table, 4, 2, &[]), vec![0, 3]);
    }

    #[test]
    fn correlation_ratio_sees_relations_that_are_not_lines() {
        let x: Vec<f64> = (0..40).map(|i| (i % 4) as f64).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|v| [5.0, -3.0, 9.0, 0.0][*v as usize])
            .collect();
        assert!((correlation_ratio(&x, &y) - 1.0).abs() < 1e-12);
        assert!(correlation(&x, &y, |v| v).abs() < 0.5);
        // One row per category says nothing.
        let ids: Vec<f64> = (0..40).map(f64::from).collect();
        assert_eq!(correlation_ratio(&ids, &y), 0.0);
    }

    #[test]
    fn copies_are_found_before_the_search_and_not_used_as_inputs() {
        let n = 40;
        let serial: Vec<f64> = (0..n)
            .map(|i| 7000.0 + 40.0 * ((i * 7) % 23) as f64)
            .collect();
        let next: Vec<f64> = serial.iter().map(|s| s + 1.0).collect();
        let same: Vec<f64> = serial.clone();
        let km: Vec<f64> = (0..n)
            .map(|i| 1.5 + 0.25 * ((i * 11) % 31) as f64)
            .collect();
        let m: Vec<f64> = km.iter().map(|k| 1000.0 * k).collect();
        // Rounded, not a copy: left to the search.
        let line: Vec<f64> = serial.iter().map(|s| (s / 100.0).floor()).collect();
        let mut doubled: Vec<f64> = m.iter().map(|v| 2.0 * v).collect();
        doubled[5] += 3.0;
        let table = Table {
            names: ["serial", "next", "same", "km", "m", "line", "doubled"]
                .map(String::from)
                .to_vec(),
            columns: vec![serial, next, same, km, m, line, doubled],
            decimals: vec![
                Some(0),
                Some(0),
                Some(0),
                Some(2),
                Some(0),
                Some(0),
                Some(0),
            ],
            ..Table::default()
        };
        let config = ScanConfig::default();
        let copies = find_copies(&table, &config);
        let text = |c: usize| copies[c].as_ref().map(|d| d.formula_text.clone());
        assert_eq!(text(0), None);
        assert_eq!(text(1).as_deref(), Some("serial + 1"));
        assert_eq!(text(2).as_deref(), Some("serial"));
        assert_eq!(text(3), None);
        assert_eq!(text(4).as_deref(), Some("1000 * km"));
        assert_eq!(text(5), None);
        // 2 * m is no copy (2 is not a unit scale); the search finds it from
        // km, as m is a copy and not an input.
        assert_eq!(text(6), None);
        let found = scan(&table, &config);
        let doubled = found.iter().find(|d| d.target == 6).expect("doubled");
        assert!(!doubled.copy);
        assert_eq!(doubled.inputs, vec![3], "{}", doubled.formula_text);
        assert_eq!(doubled.exceptions, vec![5]);
        assert!(found.iter().filter(|d| d.copy).count() == 3);
    }

    #[test]
    fn decimals_are_inferred_from_values() {
        assert_eq!(infer_decimals(&[1.0, 20.0, f64::NAN]), Some(0));
        assert_eq!(infer_decimals(&[0.53, 3.46, 2.1]), Some(2));
        assert_eq!(infer_decimals(&[0.21 * 10.37, 1.5]), Some(4));
        assert_eq!(infer_decimals(&[std::f64::consts::PI]), None);
        assert_eq!(infer_decimals(&[f64::NAN]), None);
        let mut table = Table {
            names: vec!["a".into(), "b".into()],
            columns: vec![vec![1.25, 2.5], vec![1.0, 2.0]],
            decimals: vec![None, Some(3)],
            ..Table::default()
        };
        table.infer_decimals();
        assert_eq!(table.decimals, vec![Some(2), Some(3)]);
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
