//! Formulas that change with a regime column, and formulas whose constants
//! change from one run of rows to the next.
//!
//! On a railway track, the radius is 0 on straights, constant along each
//! circular curve and, on transition curves (clothoids), the inverse of a
//! line in the chainage, with different constants for every clothoid. No
//! single formula covers the column, but each kind of row has one. Here the
//! rows are split by the values of a regime column (see
//! [`crate::profile::Role::Regime`]), each part is explained on its own, and
//! in ordered tables (see [`crate::profile::Profile::order`]) a part can also
//! be explained run by run: the same shape with its constants refitted on
//! each run of consecutive rows.

use crate::expr::{BinaryOp, Expr};
use crate::profile::{Profile, Role};
use crate::scan::{
    collect_vars, correlation_ratio, explain_column_skipping, matching_rows, parametrize,
    row_tolerance, DerivedColumn, ScanConfig, Table,
};

/// A formula per value of a regime column.
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeFormula {
    /// The regime column.
    pub column: usize,
    /// One piece per value of the regime column with enough rows.
    pub pieces: Vec<Piece>,
}

/// How the rows with one value of the regime column are computed.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// The value of the regime column.
    pub value: f64,
    /// Its label, for text categories (otherwise the number written out).
    pub label: String,
    /// The formula. With `per_run`, a shape whose parameters (`Expr::Param`)
    /// are fitted again on every run of rows.
    pub formula: Expr,
    /// Whether the constants of `formula` change from run to run.
    pub per_run: bool,
    /// The formula for people: `0`, `0.21 * net`, `constant per run`,
    /// `1 / (c0 + c1 * pk), constants per run`.
    pub text: String,
    /// Rows with this value (and the target present).
    pub rows: usize,
    /// Rows the formula reproduces.
    pub matched: usize,
    /// Runs of consecutive rows, for `per_run` pieces.
    pub runs: usize,
    /// Whether the piece links each row to the previous one (a slope per
    /// stretch, see [`crate::steps`]); `formula` is then no formula of one
    /// row.
    pub steps: bool,
    /// Columns the piece uses besides those of `formula`.
    pub inputs: Vec<usize>,
    /// For an approximate piece, its largest error (see
    /// [`DerivedColumn::within`]).
    pub within: Option<f64>,
}

/// Regime columns tried per target, the most related first.
const MAX_REGIMES: usize = 2;
/// How much of the target's variation the runs of a regime (or its values,
/// without an order) must explain to be tried.
const MIN_REGIME_SCORE: f64 = 0.3;
/// Share of the rows that the cheap explanations must cover before the
/// parts of an ordered table are searched.
const MIN_CHEAP_SHARE: f64 = 0.5;
/// Rows a stretch of equal values needs to count as constant.
const MIN_STRETCH: usize = 4;
/// Rows a part needs to be explained.
const MIN_PART_ROWS: usize = 8;
/// Longest runs searched for the shape of a per-run formula.
const SHAPE_RUNS: usize = 2;
/// Rows a run needs to be searched for a shape.
const MIN_SHAPE_RUN: usize = 8;
/// Searched shapes fitted run by run.
const MAX_RUN_SHAPES: usize = 2;
/// Average rows per run for a per-run formula: with shorter runs, the
/// constants fitted on each run explain anything.
const MIN_MEAN_RUN: f64 = 4.0;
/// Values a text column that changes from row to row may have to be tried
/// as a regime (each value is a part to search).
const MAX_CODE_PARTS: usize = 20;
/// Rows of a part searched for one formula (a spread sample of larger parts).
const SEARCH_ROWS: usize = 4000;
/// Share of the explained rows that must follow something more than a
/// constant per run.
const MIN_INFORMATIVE_SHARE: f64 = 0.2;

/// Explains `target` with a formula per value of a regime column, when no
/// single formula does. Returns `None` unless the pieces together match
/// `config.min_match` of the rows and say more than "constant along each
/// run".
pub(crate) fn explain_by_regime(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    profile: &Profile,
    skip: &[bool],
) -> Option<DerivedColumn> {
    let y = table.columns.get(target)?;
    let role = profile.columns.get(target)?.role;
    // The order, the group keys and identifiers organise the rows; they
    // are not computed per regime.
    if table.is_category(target)
        || profile.columns[target].distinct < 3
        || matches!(role, Role::Order | Role::GroupKey | Role::Identifier)
    {
        return None;
    }
    let present: Vec<usize> = (0..table.n_rows()).filter(|&r| y[r].is_finite()).collect();
    if present.len() < 2 * MIN_PART_ROWS {
        return None;
    }
    let order = Order::new(table, profile);
    let best = std::cell::OnceCell::new();
    let ranked = std::cell::OnceCell::new();
    let mut regimes: Vec<(f64, usize, bool)> = profile
        .columns
        .iter()
        .enumerate()
        .filter(|&(c, p)| {
            c != target
                // `codi#num` is read out of `codi`: no regime of its own.
                && base_name(&table.names[c]) != base_name(&table.names[target])
                && (p.role == Role::Regime
                    || p.category && matches!(p.role, Role::Flag | Role::Code))
                && !skip.get(c).copied().unwrap_or(false)
        })
        // A regime must be known on most rows of the target.
        .filter(|&(c, _)| {
            let r = &table.columns[c];
            present.iter().filter(|&&i| r[i].is_finite()).count() * 10 >= present.len() * 9
        })
        .filter_map(|(c, p)| {
            // A text column that changes from row to row in an ordered
            // table (a country in invoices sorted by number) has no runs
            // to fit: its values are the parts, as without an order.
            let runs = p.mean_run.unwrap_or(0.0) >= MIN_MEAN_RUN;
            // A code with long runs has too many values for a regime.
            if p.role == Role::Code && (runs || p.distinct > MAX_CODE_PARTS) {
                return None;
            }
            let o = order.as_ref().filter(|_| runs);
            let x = if o.is_none() {
                *best.get_or_init(|| best_input(table, target))
            } else {
                None
            };
            // On ranks too: a few huge values (the radius where a
            // transition curve starts) would hide that the column follows
            // the regime on every other row.
            let score = match o {
                Some(_) => regime_score(table, c, y, o, x).max(regime_score(
                    table,
                    c,
                    ranked.get_or_init(|| ranks(y)),
                    o,
                    x,
                )),
                None => regime_score(table, c, y, o, x),
            };
            Some((score, c, runs))
        })
        .filter(|&(score, _, _)| score >= MIN_REGIME_SCORE)
        .collect();
    regimes.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    regimes.truncate(MAX_REGIMES);

    let mut best: Option<(usize, RegimeFormula, Vec<usize>, bool)> = None;
    for &(_, regime, runs) in &regimes {
        let o = order.as_ref().filter(|_| runs);
        let res = pieces_for(table, target, regime, &present, config, o, profile, skip);
        let Some((formula, matched)) = res else {
            continue;
        };
        if best.as_ref().is_none_or(|b| matched.len() > b.0) {
            best = Some((matched.len(), formula, matched, runs));
        }
        // The next regime could only explain a few rows more.
        if best
            .as_ref()
            .is_some_and(|b| b.0 as f64 >= 0.99 * present.len() as f64)
        {
            break;
        }
    }
    let (_, formula, matched, runs) = best?;
    if (matched.len() as f64) < config.min_match * present.len() as f64 {
        return None;
    }
    Some(derived(
        table,
        target,
        formula,
        &present,
        &matched,
        order.as_ref().filter(|_| runs),
    ))
}

/// How much of `y` the regime explains: the correlation ratio of `y` on the
/// runs of equal regime values along the order (each curve is a run), or
/// on the regime's values when the table has no order. A regime whose
/// changes line up with the changes of `y` scores high.
fn regime_score(
    table: &Table,
    regime: usize,
    y: &[f64],
    order: Option<&Order>,
    x: Option<usize>,
) -> f64 {
    let r = &table.columns[regime];
    match order {
        Some(o) => {
            let mut run_id = vec![f64::NAN; r.len()];
            let mut id = 0.0;
            for w in o.sorted.windows(2) {
                let (a, b) = (w[0], w[1]);
                if run_id[a].is_nan() {
                    run_id[a] = id;
                }
                let same = r[a] == r[b] || r[a].is_nan() && r[b].is_nan();
                if !same || o.group_of[a] != o.group_of[b] {
                    id += 1.0;
                }
                run_id[b] = id;
            }
            // Rows without a value are one group, not a run each.
            for (id, v) in run_id.iter_mut().zip(r) {
                if v.is_nan() {
                    *id = -1.0;
                }
            }
            correlation_ratio_many(&run_id, y)
        }
        // Without an order, the regime may change the slope rather than the
        // level (a VAT rate per country), so a line per regime value in the
        // column most correlated with `y` counts too.
        None => {
            let line = x
                .filter(|&x| x != regime)
                .map_or(0.0, |x| line_score(r, &table.columns[x], y));
            correlation_ratio(r, y).max(line)
        }
    }
}

/// The rank of each value of `y` (ties share their mean rank; missing
/// values stay missing).
fn ranks(y: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..y.len()).filter(|&i| y[i].is_finite()).collect();
    idx.sort_by(|&a, &b| y[a].total_cmp(&y[b]));
    let mut out = vec![f64::NAN; y.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && y[idx[j + 1]] == y[idx[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0;
        for &k in &idx[i..=j] {
            out[k] = rank;
        }
        i = j + 1;
    }
    out
}

/// The numeric column most correlated with `y` (not `y` itself, nor a
/// copy of it), for [`line_score`].
fn best_input(table: &Table, target: usize) -> Option<usize> {
    let y = &table.columns[target];
    let pearson = |x: &[f64]| {
        let (mut n, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for (&a, &b) in x.iter().zip(y) {
            if a.is_finite() && b.is_finite() {
                n += 1.0;
                sx += a;
                sy += b;
                sxx += a * a;
                syy += b * b;
                sxy += a * b;
            }
        }
        let cov = sxy - sx * sy / n;
        let den = ((sxx - sx * sx / n) * (syy - sy * sy / n)).sqrt();
        if n < 3.0 || den <= 0.0 {
            0.0
        } else {
            (cov / den).abs()
        }
    };
    (0..table.columns.len())
        .filter(|&c| c != target && !table.is_category(c))
        .map(|c| (pearson(&table.columns[c]), c))
        .filter(|(p, _)| *p < 1.0 - 1e-12)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// How much better a line in `x` per regime value explains `y` than one
/// line for all rows: the share of the squared residuals removed, from 0
/// to 1.
fn line_score(r: &[f64], x: &[f64], y: &[f64]) -> f64 {
    // Sums for a least-squares line: n, Σx, Σy, Σxx, Σxy, Σyy.
    let mut parts: std::collections::HashMap<u64, [f64; 6]> = Default::default();
    let mut all = [0.0; 6];
    for ((&g, &a), &b) in r.iter().zip(x).zip(y) {
        if !(g.is_finite() && a.is_finite() && b.is_finite()) {
            continue;
        }
        for s in [parts.entry(g.to_bits()).or_insert([0.0; 6]), &mut all] {
            s[0] += 1.0;
            s[1] += a;
            s[2] += b;
            s[3] += a * a;
            s[4] += a * b;
            s[5] += b * b;
        }
    }
    let sse = |s: &[f64; 6]| {
        let [n, sx, sy, sxx, sxy, syy] = *s;
        if n < 3.0 {
            return f64::NAN;
        }
        let vxx = sxx - sx * sx / n;
        let vxy = sxy - sx * sy / n;
        let vyy = syy - sy * sy / n;
        let fit = if vxx > 0.0 { vxy * vxy / vxx } else { 0.0 };
        (vyy - fit).max(0.0)
    };
    let global = sse(&all);
    let within: f64 = parts.values().map(sse).filter(|v| v.is_finite()).sum();
    if global.is_nan() || global <= 1e-12 || parts.len() < 2 {
        return 0.0;
    }
    (1.0 - within / global).clamp(0.0, 1.0)
}

/// The correlation ratio for a grouping with many groups: the share of the
/// variance of `y` between group means, adjusted for the number of groups.
fn correlation_ratio_many(groups: &[f64], y: &[f64]) -> f64 {
    let mut sums: std::collections::HashMap<u64, (f64, usize)> = Default::default();
    let (mut sum, mut sum2, mut n) = (0.0, 0.0, 0usize);
    for (&g, &v) in groups.iter().zip(y) {
        if !(g.is_finite() && v.is_finite()) {
            continue;
        }
        let e = sums.entry(g.to_bits()).or_insert((0.0, 0));
        e.0 += v;
        e.1 += 1;
        sum += v;
        sum2 += v * v;
        n += 1;
    }
    let k = sums.len();
    if n < 2 || k < 2 || k >= n {
        return 0.0;
    }
    let mean = sum / n as f64;
    let total = sum2 - n as f64 * mean * mean;
    if total <= 0.0 {
        return 0.0;
    }
    let between: f64 = sums
        .values()
        .map(|&(s, c)| {
            let m = s / c as f64;
            c as f64 * (m - mean) * (m - mean)
        })
        .sum();
    let r2 = between / total;
    let adjusted = 1.0 - (1.0 - r2) * (n - 1) as f64 / (n - k) as f64;
    adjusted.max(0.0).sqrt()
}

/// The column a number read out of text comes from: `codi` for `codi#num`.
pub(crate) fn base_name(name: &str) -> &str {
    name.split('#').next().unwrap_or(name)
}

/// The rows of an ordered table, sorted, with the group of each row.
pub(crate) struct Order {
    pub(crate) column: usize,
    /// Position of each row in the sorted order (`usize::MAX` if left out).
    pub(crate) position: Vec<usize>,
    pub(crate) sorted: Vec<usize>,
    pub(crate) group_of: Vec<usize>,
}

impl Order {
    pub(crate) fn new(table: &Table, profile: &Profile) -> Option<Order> {
        let column = profile.order?;
        let sorted = profile.sorted_rows(table)?;
        let mut position = vec![usize::MAX; table.n_rows()];
        for (i, &r) in sorted.iter().enumerate() {
            position[r] = i;
        }
        Some(Order {
            column,
            position,
            sorted,
            group_of: profile.group_of_rows(table),
        })
    }

    /// The order of `rows` alone, each run of consecutive rows a group of
    /// its own, so that rows on either side of a gap are not neighbours.
    pub(crate) fn within(&self, rows: &[usize]) -> Order {
        let n = self.position.len();
        let (mut position, mut group_of) = (vec![usize::MAX; n], vec![usize::MAX; n]);
        let mut sorted = Vec::new();
        for (g, run) in self.runs(rows).into_iter().enumerate() {
            for r in run {
                position[r] = sorted.len();
                group_of[r] = g;
                sorted.push(r);
            }
        }
        Order {
            column: self.column,
            position,
            sorted,
            group_of,
        }
    }

    /// Runs of `rows` (sorted, consecutive in the order and in one group).
    pub(crate) fn runs(&self, rows: &[usize]) -> Vec<Vec<usize>> {
        let mut rows: Vec<usize> = rows
            .iter()
            .copied()
            .filter(|&r| self.position[r] != usize::MAX)
            .collect();
        rows.sort_by_key(|&r| self.position[r]);
        let mut runs: Vec<Vec<usize>> = Vec::new();
        for r in rows {
            let joins = runs.last().and_then(|run| run.last()).is_some_and(|&last| {
                self.position[r] == self.position[last] + 1
                    && self.group_of[r] == self.group_of[last]
            });
            if joins {
                runs.last_mut().unwrap().push(r);
            } else {
                runs.push(vec![r]);
            }
        }
        runs
    }
}

/// The best piece for each value of `regime`, and the rows they match.
#[allow(clippy::too_many_arguments)]
fn pieces_for(
    table: &Table,
    target: usize,
    regime: usize,
    present: &[usize],
    config: &ScanConfig,
    order: Option<&Order>,
    profile: &Profile,
    skip: &[bool],
) -> Option<(RegimeFormula, Vec<usize>)> {
    let r = &table.columns[regime];
    let mut values: Vec<f64> = present
        .iter()
        .map(|&i| r[i])
        .filter(|v| v.is_finite())
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    if values.len() < 2 {
        return None;
    }
    let mut pieces = Vec::new();
    let mut matched = Vec::new();
    // The regime column is no input of its own pieces.
    let mut skip = skip.to_vec();
    skip.resize(table.columns.len(), false);
    skip[regime] = true;
    let parts: Vec<(f64, Vec<usize>)> = values
        .iter()
        .map(|&v| (v, present.iter().copied().filter(|&i| r[i] == v).collect()))
        .filter(|(_, part): &(f64, Vec<usize>)| part.len() >= MIN_PART_ROWS)
        .collect();
    // Cheap explanations first (a value, a line per run); searching every
    // part costs as much as scanning the column again, so it is only done
    // when the cheap ones already cover a fair share of the rows of an
    // ordered table.
    let mut found: Vec<Option<(Piece, Vec<usize>)>> = parts
        .iter()
        .map(|(_, part)| explain_part(table, target, part, config, order, &skip, false))
        .collect();
    let cheap: usize = found.iter().flatten().map(|(_, rows)| rows.len()).sum();
    if order.is_some() && (cheap as f64) < MIN_CHEAP_SHARE * present.len() as f64 {
        return None;
    }
    {
        // Largest parts first: without an order, a regime whose largest
        // part has no formula is dropped after one search.
        let mut by_size: Vec<usize> = (0..parts.len()).collect();
        by_size.sort_by_key(|&i| std::cmp::Reverse(parts[i].1.len()));
        // Once more rows are left unexplained than `min_match` allows, the
        // other parts are not searched.
        let allowed = (1.0 - config.min_match) * present.len() as f64;
        let mut missed = present.len() - parts.iter().map(|p| p.1.len()).sum::<usize>();
        for (k, &i) in by_size.iter().enumerate() {
            let (part, slot) = (&parts[i].1, &mut found[i]);
            // A constant along stretches may still be a slope (see
            // `explain_part`).
            let done = slot.as_ref().is_some_and(|(p, rows)| {
                rows.len() as f64 >= 0.98 * part.len() as f64
                    && !(order.is_some() && p.per_run && !p.formula.has_vars())
            });
            if done {
                continue;
            }
            // A value constant along stretches may be the slope of another
            // column over each stretch (a gradient on the grades, but not on
            // the vertical curves between them): that says more, so it wins
            // on nearly as many rows.
            let stretches = slot
                .as_ref()
                .is_some_and(|(p, _)| p.per_run && !p.formula.has_vars());
            if let (true, Some(o)) = (stretches, order) {
                let have = slot.as_ref().map_or(0, |s| s.1.len());
                if let Some(better) = slope_piece(table, target, part, config, o, profile, &skip) {
                    if better.1.len() as f64 >= 0.98 * have as f64 {
                        *slot = Some(better);
                        continue;
                    }
                }
            }
            if let Some(better) = explain_part(table, target, part, config, order, &skip, true) {
                if slot.as_ref().is_none_or(|s| better.1.len() > s.1.len()) {
                    *slot = Some(better);
                }
            }
            let covered = slot.as_ref().map_or(0, |s| s.1.len());
            if k == 0 && order.is_none() && (covered as f64) < config.min_match * part.len() as f64
            {
                return None;
            }
            missed += part.len() - covered.min(part.len());
            if missed as f64 > allowed {
                return None;
            }
        }
    }
    for ((v, _), slot) in parts.iter().zip(found) {
        if let Some((mut piece, rows)) = slot {
            piece.value = *v;
            piece.label = table
                .label(regime, *v)
                .map_or_else(|| crate::expr::format_number(*v), str::to_string);
            matched.extend(rows);
            pieces.push(piece);
        }
    }
    // Saying only that the column keeps its value along runs of the regime
    // is no formula: any column that changes seldom does.
    // The rest must hold on a fair share of the rows, so one small regime
    // with a single value does not carry a column of per-run constants.
    // Pieces with fitted constants per run fit nearly anything, so pieces
    // with one formula for all their rows must hold on a fair share of the
    // rows, and not all be the same formula (a column that is mostly one
    // value is not explained by any regime).
    let fixed: Vec<&Piece> = pieces.iter().filter(|p| !p.per_run).collect();
    let informative: usize = fixed.iter().map(|p| p.matched).sum();
    let all_same =
        pieces.len() == fixed.len() && fixed.windows(2).all(|w| w[0].formula == w[1].formula);
    if pieces.len() < 2
        || all_same
        || (informative as f64) < MIN_INFORMATIVE_SHARE * matched.len() as f64
    {
        return None;
    }
    matched.sort_unstable();
    Some((
        RegimeFormula {
            column: regime,
            pieces,
        },
        matched,
    ))
}

/// The slope of another column over each stretch of `part` (see
/// [`crate::steps`]), as a piece.
fn slope_piece(
    table: &Table,
    target: usize,
    part: &[usize],
    config: &ScanConfig,
    order: &Order,
    profile: &Profile,
    skip: &[bool],
) -> Option<(Piece, Vec<usize>)> {
    let y = &table.columns[target];
    let within = order.within(part);
    let d = crate::steps::explain_by_steps_in(table, target, config, profile, skip, &within)?;
    let rows: Vec<usize> = within
        .sorted
        .iter()
        .copied()
        .filter(|&r| y[r].is_finite() && d.exceptions.binary_search(&r).is_err())
        .collect();
    let piece = Piece {
        value: f64::NAN,
        label: String::new(),
        formula: d.formula,
        per_run: false,
        text: d.formula_text,
        rows: part.len(),
        matched: rows.len(),
        runs: 0,
        steps: true,
        inputs: d.inputs,
        within: d.within,
    };
    Some((piece, rows))
}

/// The best explanation of `target` on the rows of `part`: one value, a
/// shape with constants per run, or one formula; cheap ones first, and the
/// searches only while the rows are not all explained.
fn explain_part(
    table: &Table,
    target: usize,
    part: &[usize],
    config: &ScanConfig,
    order: Option<&Order>,
    skip: &[bool],
    search: bool,
) -> Option<(Piece, Vec<usize>)> {
    let y = &table.columns[target];
    let tolerance = row_tolerance(table, target, config);
    let rounding = rounding(table, target);
    let mut best: Option<(Piece, Vec<usize>)> = None;
    // More rows wins; then the simpler kind, in the order tried.
    let consider = |best: &mut Option<(Piece, Vec<usize>)>, piece: Piece, rows: Vec<usize>| {
        if best.as_ref().is_none_or(|b| rows.len() > b.1.len()) {
            *best = Some((piece, rows));
        }
    };
    let done = |best: &Option<(Piece, Vec<usize>)>| {
        best.as_ref()
            .is_some_and(|b| b.1.len() as f64 >= config.min_match.max(0.98) * part.len() as f64)
    };
    let piece = |formula: Expr, text: String, per_run: bool, matched: usize, runs: usize| Piece {
        value: f64::NAN,
        label: String::new(),
        formula,
        per_run,
        text,
        rows: part.len(),
        matched,
        runs,
        steps: false,
        inputs: Vec::new(),
        within: None,
    };

    // One value: the most common one.
    let usual = most_common(y, part);
    let constant = Expr::Const(usual);
    let rows = matching_rows(&constant, table, part, y, &tolerance);
    let text = crate::expr::format_number(usual);
    let n = rows.len();
    consider(&mut best, piece(constant, text, false, n, 0), rows);

    let runs = order.map(|o| o.runs(part)).unwrap_or_default();
    let mean_run = part.len() as f64 / runs.len().max(1) as f64;
    let by_run = order.is_some() && runs.len() >= 2 && mean_run >= MIN_MEAN_RUN;
    let fit_shape = |best: &mut Option<(Piece, Vec<usize>)>, shape: Expr| {
        let mut stretches = 0;
        let rows = fit_per_run(table, target, &shape, &runs, rounding, &mut stretches);
        let text = if shape.has_vars() {
            format!("{}, constants per run", shape.display_with(&table.names))
        } else if stretches > 0 {
            "constant along stretches".to_string()
        } else {
            "constant per run".to_string()
        };
        let n = rows.len();
        consider(best, piece(shape, text, true, n, runs.len()), rows);
    };
    if !search && by_run && !done(&best) {
        for shape in basic_shapes(order.unwrap().column) {
            fit_shape(&mut best, shape);
            if done(&best) {
                break;
            }
        }
    }

    if !search {
        return best;
    }
    // One formula for the whole part.
    if !done(&best) {
        // A spread sample is searched; the formula is then checked on every
        // row of the part.
        let step = part.len().div_ceil(SEARCH_ROWS).max(1);
        let sample: Vec<usize> = part.iter().copied().step_by(step).collect();
        let sub = crate::progressive::sub_table(table, &sample);
        if let Some(d) = explain_column_skipping(&sub, target, config, skip) {
            if d.other.is_none() {
                let rows = matching_rows(&d.formula, table, part, y, &tolerance);
                let text = d.formula_text.clone();
                let n = rows.len();
                consider(&mut best, piece(d.formula, text, false, n, 0), rows);
            }
        }
    }

    // The shapes of formulas found on the longest runs, refitted per run.
    if by_run && !done(&best) {
        for shape in searched_shapes(table, target, &runs, config, skip) {
            fit_shape(&mut best, shape);
            if done(&best) {
                break;
            }
        }
    }
    best
}

/// A constant, a line and the inverse of a line in the order column (the
/// curvature of a transition curve grows linearly along it).
fn basic_shapes(order: usize) -> Vec<Expr> {
    let line = Expr::binary(
        BinaryOp::Add,
        Expr::Param(0),
        Expr::binary(BinaryOp::Mul, Expr::Param(1), Expr::Var(order)),
    );
    vec![
        Expr::Param(0),
        line.clone(),
        Expr::binary(BinaryOp::Div, Expr::Const(1.0), line),
    ]
}

/// Half a unit of the last decimal of the target, when known.
pub(crate) fn rounding(table: &Table, target: usize) -> f64 {
    table
        .decimals
        .get(target)
        .copied()
        .flatten()
        .map_or(0.0, |d| 0.5 * 10f64.powi(-(d as i32)) * 1.000_001)
}

/// Share of a run's range of values a per-run formula may be off by:
/// constants fitted on rounded values carry some of the rounding. Relative
/// to the range, not to the values, so that a line cannot pass for a short
/// stretch of a curve of large coordinates.
const RUN_TOLERANCE: f64 = 1e-5;

/// The shapes of the formulas found on the longest runs, with one or two
/// constants to refit on each run.
fn searched_shapes(
    table: &Table,
    target: usize,
    runs: &[Vec<usize>],
    config: &ScanConfig,
    skip: &[bool],
) -> Vec<Expr> {
    let mut shapes: Vec<Expr> = Vec::new();
    let mut longest: Vec<&Vec<usize>> = runs.iter().filter(|r| r.len() >= MIN_SHAPE_RUN).collect();
    longest.sort_by_key(|r| std::cmp::Reverse(r.len()));
    for run in longest.into_iter().take(SHAPE_RUNS) {
        let sub = crate::progressive::sub_table(table, run);
        if let Some(d) = explain_column_skipping(&sub, target, config, skip) {
            let shape = parametrize(&d.formula);
            if (1..=2).contains(&shape.num_params()) && !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        if shapes.len() >= MAX_RUN_SHAPES {
            break;
        }
    }
    shapes
}

/// Rows matched when the parameters of `shape` are fitted on each run.
fn fit_per_run(
    table: &Table,
    target: usize,
    shape: &Expr,
    runs: &[Vec<usize>],
    rounding: f64,
    stretches: &mut usize,
) -> Vec<usize> {
    let y = &table.columns[target];
    let n_params = shape.num_params();
    let mut used = Vec::new();
    collect_vars(shape, &mut used);
    let opts = crate::fit::FitOptions::default();
    let mut matched = Vec::new();
    for run in runs {
        let rows: Vec<usize> = run
            .iter()
            .copied()
            .filter(|&r| used.iter().all(|&c| table.columns[c][r].is_finite()))
            .collect();
        // Enough rows to pin the constants down and check them.
        if rows.len() < n_params + 2 {
            continue;
        }
        if n_params == 1 && !shape.has_vars() {
            // A constant per run, or per stretch of a run: two curves in a
            // row are one run of the curve type, each with its own value.
            let mut start = 0;
            let mut pieces: usize = 0;
            for i in 1..=rows.len() {
                let ends =
                    i == rows.len() || (y[rows[i]] - y[rows[start]]).abs() > rounding.max(1e-12);
                if ends {
                    if i - start >= MIN_STRETCH {
                        matched.extend_from_slice(&rows[start..i]);
                        pieces += 1;
                    }
                    start = i;
                }
            }
            *stretches += pieces.saturating_sub(1);
            continue;
        }
        let params = if let Some(p) = closed_form(shape, table, y, &rows) {
            Some(p)
        } else {
            let columns: Vec<Vec<f64>> = table
                .columns
                .iter()
                .map(|c| rows.iter().map(|&r| c[r]).collect())
                .collect();
            let ys: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
            crate::fit::fit_params(shape, &columns, &ys, &opts).map(|(p, _)| p)
        };
        if let Some(p) = params {
            let formula = shape.substitute_params(&p);
            let (lo, hi) = rows
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &r| {
                    (lo.min(y[r]), hi.max(y[r]))
                });
            let tol = rounding.max(RUN_TOLERANCE * (hi - lo)).max(1e-12);
            let ok = matching_rows(&formula, table, &rows, y, &|_| tol);
            // A run counts only when the constants fit most of it.
            if ok.len() * 10 >= rows.len() * 9 {
                matched.extend(ok);
            }
        }
    }
    matched
}

/// Least squares by hand for `c0 + c1 * x` and `1 / (c0 + c1 * x)`
/// (a line fitted to `1 / y`): exact and much faster than an iterative fit.
fn closed_form(shape: &Expr, table: &Table, y: &[f64], rows: &[usize]) -> Option<Vec<f64>> {
    use Expr::{Binary, Const, Param, Var};
    let line_var = |e: &Expr| match e {
        Binary(BinaryOp::Add, a, b) => match (a.as_ref(), b.as_ref()) {
            (Param(0), Binary(BinaryOp::Mul, p, v)) => match (p.as_ref(), v.as_ref()) {
                (Param(1), Var(x)) => Some(*x),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    let (x, inverse) = match shape {
        Binary(BinaryOp::Div, one, line) if matches!(one.as_ref(), Const(c) if *c == 1.0) => {
            (line_var(line)?, true)
        }
        other => (line_var(other)?, false),
    };
    let xs = &table.columns[x];
    let pts: Vec<(f64, f64)> = rows
        .iter()
        .map(|&r| (xs[r], if inverse { 1.0 / y[r] } else { y[r] }))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    if sxx == 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    Some(vec![my - slope * mx, slope])
}

/// The most common value of `y` on `rows`.
fn most_common(y: &[f64], rows: &[usize]) -> f64 {
    let mut counts: std::collections::HashMap<u64, (f64, usize)> = Default::default();
    for &r in rows {
        let v = y[r];
        let e = counts.entry(v.to_bits()).or_insert((v, 0));
        e.1 += 1;
    }
    counts
        .into_values()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.total_cmp(&a.0)))
        .map_or(f64::NAN, |(v, _)| v)
}

/// The derived column for a regime formula.
fn derived(
    table: &Table,
    target: usize,
    formula: RegimeFormula,
    rows: &[usize],
    matched: &[usize],
    order: Option<&Order>,
) -> DerivedColumn {
    let names = &table.names;
    let mut inputs = vec![formula.column];
    for p in &formula.pieces {
        collect_vars(&p.formula, &mut inputs);
        inputs.extend_from_slice(&p.inputs);
        if p.per_run {
            if let Some(o) = order {
                inputs.push(o.column);
            }
        }
    }
    inputs.sort_unstable();
    inputs.dedup();
    let text = formula
        .pieces
        .iter()
        .map(|p| format!("{}: {}", p.label, p.text))
        .collect::<Vec<_>>()
        .join("; ");
    let matched_set: std::collections::HashSet<usize> = matched.iter().copied().collect();
    let exceptions = rows
        .iter()
        .copied()
        .filter(|r| !matched_set.contains(r))
        .collect();
    // The largest piece stands for the formula where one is needed.
    let main = formula
        .pieces
        .iter()
        .filter(|p| !p.per_run && !p.steps)
        .max_by_key(|p| p.rows)
        .map_or(Expr::Const(f64::NAN), |p| p.formula.clone());
    DerivedColumn {
        target,
        formula: main,
        formula_text: format!("by {}: {text}", names[formula.column]),
        inputs,
        complexity: formula
            .pieces
            .iter()
            .map(|p| p.formula.complexity())
            .sum::<usize>()
            + 1,
        matched: matched.len(),
        checked: rows.len(),
        exceptions,
        other: None,
        copy: false,
        within: formula
            .pieces
            .iter()
            .filter_map(|p| p.within)
            .reduce(f64::max),
        regime: Some(formula),
        steps: false,
    }
}

#[cfg(test)]
mod tests {
    use crate::profile::{profile, Role};
    use crate::scan::{scan, ScanConfig, Table};

    /// Three kinds of rows with three formulas, told apart by a text column.
    #[test]
    fn a_formula_per_category() {
        let n = 90;
        let x: Vec<f64> = (0..n)
            .map(|i| 1.0 + (((i * 37) % 53) as f64).sqrt() * 3.0)
            .collect();
        let kind: Vec<f64> = (0..n).map(|i| (i % 3) as f64).collect();
        let y: Vec<f64> = (0..n)
            .map(|i| match i % 3 {
                0 => 2.0 * x[i],
                1 => 3.0 * x[i] + 1.0,
                _ => x[i] * x[i],
            })
            .collect();
        let table = Table {
            names: vec!["x".into(), "kind".into(), "y".into()],
            columns: vec![x, kind, y],
            labels: vec![None, Some(vec!["a".into(), "b".into(), "c".into()]), None],
            ..Table::default()
        };
        // Without an order, a text column with few values is a regime.
        assert_eq!(profile(&table).columns[1].role, Role::Regime);
        let found = scan(&table, &ScanConfig::default());
        let d = found.iter().find(|d| d.target == 2).expect("y is found");
        let g = d.regime.as_ref().expect("a formula per kind");
        let texts: Vec<String> = g
            .pieces
            .iter()
            .map(|p| format!("{}: {}", p.label, p.text))
            .collect();
        assert_eq!(texts, ["a: 2 * x", "b: 3 * x + 1", "c: x^2"], "{d:?}");
        assert_eq!(d.matched, n);
    }

    /// A VAT rate per country, in shuffled rows: the country changes the
    /// slope, not the level, so its correlation ratio alone is low.
    #[test]
    fn a_rate_per_category_in_shuffled_rows() {
        let n = 200;
        let net: Vec<f64> = (0..n)
            .map(|i| (10.0 + ((i * 7919) % 491) as f64 * 1.01).round())
            .collect();
        let country: Vec<f64> = (0..n).map(|i| ((i * 13) % 4) as f64).collect();
        let rates = [0.21, 0.2, 0.19, 0.23];
        let vat: Vec<f64> = (0..n)
            .map(|i| (net[i] * rates[country[i] as usize] * 100.0).round() / 100.0)
            .collect();
        let table = Table {
            names: vec!["net".into(), "country".into(), "vat".into()],
            columns: vec![net, country, vat],
            labels: vec![
                None,
                Some(vec!["ES".into(), "FR".into(), "DE".into(), "PT".into()]),
                None,
            ],
            decimals: vec![Some(0), Some(0), Some(2)],
            ..Table::default()
        };
        let found = scan(&table, &ScanConfig::default());
        let d = found.iter().find(|d| d.target == 2).expect("vat is found");
        assert_eq!(
            d.formula_text,
            "by country: ES: 0.21 * net; FR: 0.2 * net; DE: 0.19 * net; PT: 0.23 * net",
            "{d:?}"
        );
    }

    /// Two tracks along a chainage: 0 on straights, constant on each
    /// curve, a ramp up before it and down after it.
    #[test]
    fn constants_per_run_on_an_ordered_table() {
        let (mut track, mut pk, mut kind, mut cant, mut noise) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for t in 0..2 {
            let mut m = 0;
            for curve in 0..6 {
                let full = 20.0 + 13.0 * curve as f64 + 7.0 * t as f64;
                // (kind, length, cant at row i of len).
                type Cant<'a> = &'a dyn Fn(f64) -> f64;
                let pieces: [(usize, usize, Cant); 4] = [
                    (0, 30, &|_| 0.0),
                    (1, 12, &|s| full * s),
                    (2, 25, &|_| full),
                    (1, 12, &|s| full * (1.0 - s)),
                ];
                for (k, len, f) in pieces {
                    for i in 0..len {
                        track.push(t as f64);
                        pk.push(m as f64);
                        kind.push(k as f64);
                        let v = f((i as f64 + 0.5) / len as f64);
                        cant.push((v * 1e6_f64).round() / 1e6);
                        noise.push(((m * 7919 + t * 13) % 101) as f64 / 7.0);
                        m += 1;
                    }
                }
            }
        }
        let table = Table {
            names: ["track", "pk", "kind", "cant", "noise"]
                .map(String::from)
                .to_vec(),
            columns: vec![track, pk, kind, cant, noise],
            decimals: vec![Some(0), Some(0), Some(0), Some(6), None],
            labels: vec![
                None,
                None,
                Some(vec![
                    "Straight".into(),
                    "Clothoid".into(),
                    "Circular".into(),
                ]),
                None,
                None,
            ],
            ..Table::default()
        };
        let p = profile(&table);
        assert_eq!(p.order, Some(1));
        assert_eq!(p.groups, vec![0]);
        let found = scan(&table, &ScanConfig::default());
        let d = found.iter().find(|d| d.target == 3).expect("cant is found");
        let g = d.regime.as_ref().expect("by kind");
        let texts: Vec<&str> = g.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            ["0", "c0 + c1 * pk, constants per run", "constant per run"]
        );
        assert!(
            found.iter().all(|d| d.target != 4),
            "noise is not explained"
        );
    }

    #[test]
    fn a_radius_that_is_huge_where_a_transition_curve_starts() {
        let (mut pk, mut kind, mut radius) = (Vec::new(), Vec::new(), Vec::new());
        let mut m = 0;
        for curve in 0..12 {
            let r = 300.0 + 61.0 * curve as f64;
            // Curvature grows from almost 0 along the transition curve, so
            // the radius starts near a million.
            type Radius<'a> = &'a dyn Fn(f64) -> f64;
            let pieces: [(usize, usize, Radius); 4] = [
                (0, 30, &|_| 0.0),
                (1, 12, &|s| r / s),
                (2, 25, &|_| r),
                (1, 12, &|s| r / (1.000_000_2 - s)),
            ];
            for (k, len, f) in pieces {
                for i in 0..len {
                    pk.push(m as f64);
                    kind.push(k as f64);
                    let s = (i as f64 + 1e-7) / len as f64;
                    radius.push((f(s) * 1e3).round() / 1e3);
                    m += 1;
                }
            }
        }
        let table = Table {
            names: ["pk", "kind", "radius"].map(String::from).to_vec(),
            columns: vec![pk, kind, radius],
            decimals: vec![Some(0), Some(0), Some(3)],
            labels: vec![
                None,
                Some(vec![
                    "Straight".into(),
                    "Clothoid".into(),
                    "Circular".into(),
                ]),
                None,
            ],
            ..Table::default()
        };
        let found = scan(&table, &ScanConfig::default());
        let d = found
            .iter()
            .find(|d| d.target == 2)
            .expect("radius is found");
        let g = d.regime.as_ref().expect("by kind");
        let texts: Vec<&str> = g.pieces.iter().map(|p| p.text.as_str()).collect();
        assert!(
            texts.contains(&"1 / (c0 + c1 * pk), constants per run"),
            "{texts:?}"
        );
    }
}
