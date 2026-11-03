//! The search: enumerate, fit, score and rank candidate formulas.

use crate::enumerate::{enumerate, Grammar};
use crate::error::Error;
use crate::expr::{constant_complexity, BinaryOp, Expr, UnaryOp};
use crate::fit::{affine_fit, fit_with, levenberg_marquardt, nice_values, FitOptions};
use crate::simplify::canonicalize;
use crate::units::{dimension_of, Dim, Dimension};

/// Search settings. [`SearchConfig::default`] is a good starting point.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Maximum formula size (number of nodes).
    pub max_size: usize,
    /// Maximum number of fitted constants inside a formula, such as `k` in
    /// `exp(k * x)`. Outer scale factors and offsets are always fitted and
    /// do not count.
    pub max_params: usize,
    /// Allowed unary operators.
    pub unary_ops: Vec<UnaryOp>,
    /// Allowed binary operators.
    pub binary_ops: Vec<BinaryOp>,
    /// Maximum number of candidates returned.
    pub max_results: usize,
    /// Cost of one extra node, in units of `ln(error)`. Higher values prefer
    /// simpler formulas.
    pub parsimony: f64,
    /// Normalised error below which a formula counts as exact.
    pub exact_tolerance: f64,
    /// Constants are fitted on at most this many rows, then refined on all.
    pub fit_rows: usize,
    /// Upper bound on the number of formulas enumerated.
    pub max_expressions: usize,
    /// Dimension of each input, if known (see [`crate::units`]). Formulas
    /// that mix incompatible units are skipped.
    pub input_dims: Option<Vec<Dim>>,
    /// Dimension of the target, if known. Formulas whose dimension differs
    /// need an outer constant with units (like `g` in `g * t^2`), which costs
    /// [`SearchConfig::unit_constant_cost`] extra complexity.
    pub target_dim: Option<Dim>,
    /// Extra complexity for an outer constant that must carry units.
    pub unit_constant_cost: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            max_size: 7,
            max_params: 1,
            unary_ops: UnaryOp::ALL.to_vec(),
            binary_ops: BinaryOp::ALL.to_vec(),
            max_results: 10,
            parsimony: 0.5,
            exact_tolerance: 1e-12,
            fit_rows: 64,
            max_expressions: 500_000,
            input_dims: None,
            target_dim: None,
            unit_constant_cost: 2,
        }
    }
}

/// A formula found by the search, with its quality measures.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The formula, with fitted constants substituted.
    pub expr: Expr,
    /// Weighted size of `expr`, see [`Expr::complexity`].
    pub complexity: usize,
    /// Mean squared error.
    pub mse: f64,
    /// Mean squared error divided by the variance of the target
    /// (0 = perfect, 1 = no better than predicting the mean).
    pub nmse: f64,
    /// Coefficient of determination, `1 - nmse`.
    pub r2: f64,
    /// Largest absolute error over all rows.
    pub max_abs_error: f64,
    /// Ranking score, lower is better: `ln(max(nmse, exact_tolerance)) + parsimony * complexity`.
    pub score: f64,
    /// Whether `nmse` is below the exact tolerance.
    pub exact: bool,
}

/// Searches formulas `target ≈ f(inputs)`.
///
/// `inputs[i]` is the column of values of variable `i`. Returns the Pareto
/// front of accuracy versus complexity, best score first.
///
/// Every enumerated formula `g` is also tried as `a·g`, `g + b` and
/// `a·g + b` with the best `a` and `b` computed in closed form, so outer
/// scale factors and offsets never need to be enumerated.
///
/// # Errors
///
/// Returns an [`Error`] if the data is empty, the columns have different
/// lengths or contain NaN or infinite values.
pub fn search(
    inputs: &[Vec<f64>],
    target: &[f64],
    config: &SearchConfig,
) -> Result<Vec<Candidate>, Error> {
    validate(inputs, target)?;
    let ctx = Context::new(inputs, target, config);

    let grammar = Grammar {
        n_vars: inputs.len(),
        unary: config.unary_ops.clone(),
        binary: config.binary_ops.clone(),
        max_size: config.max_size,
        max_params: config.max_params,
        max_expressions: config.max_expressions,
        var_dims: config.input_dims.clone(),
    };
    let fingerprint_inputs: Vec<Vec<f64>> = ctx
        .fit_inputs
        .iter()
        .map(|c| c.iter().take(16).copied().collect())
        .collect();
    let exprs = enumerate(&grammar, &fingerprint_inputs);

    let mut candidates = Vec::new();
    let mut front = Front::new(config.exact_tolerance);
    // The constant model, as a baseline.
    let mean = target.iter().sum::<f64>() / target.len() as f64;
    let constant = ctx.snap(mean, |v| ctx.sse_of(&vec![v; target.len()]));
    candidates.extend(ctx.candidate(Expr::Const(constant)));

    // Formulas without inner constants are cheap: try them all first, so that
    // an exact one found among them lets us skip expensive fits below.
    let (plain, with_params): (Vec<&Expr>, Vec<&Expr>) = exprs
        .iter()
        .filter(|e| e.has_vars() && !covered_by_affine(e))
        .partition(|e| e.num_params() == 0);
    for e in plain {
        candidates.extend(ctx.affine_variants(e, &mut front));
    }
    // Once a formula this simple is exact, nothing more complex can beat it.
    let to_fit: Vec<&Expr> = with_params
        .into_iter()
        .filter(|e| !front.is_dominated(e.complexity(), 0.0))
        .collect();
    for g_expr in fit_all(&ctx, &to_fit).into_iter().flatten() {
        candidates.extend(ctx.affine_variants(&g_expr, &mut front));
    }

    let mut front = pareto_front(candidates, config.exact_tolerance);
    front.sort_by(|a, b| {
        a.score
            .total_cmp(&b.score)
            .then(a.complexity.cmp(&b.complexity))
    });
    front.truncate(config.max_results);
    Ok(front)
}

/// Fits the inner constants of every expression, in parallel when the
/// `parallel` feature is enabled.
fn fit_all(ctx: &Context<'_>, exprs: &[&Expr]) -> Vec<Option<Expr>> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        exprs.par_iter().map(|e| ctx.fit_inner(e)).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        exprs.iter().map(|e| ctx.fit_inner(e)).collect()
    }
}

/// `c + g` and `c * g` are already tried as affine variants of `g`.
fn covered_by_affine(e: &Expr) -> bool {
    match e {
        Expr::Binary(op @ (BinaryOp::Add | BinaryOp::Mul), _, _) => {
            let mut terms = Vec::new();
            chain_terms(*op, e, &mut terms);
            terms.iter().any(|t| matches!(t, Expr::Param(_)))
        }
        _ => false,
    }
}

fn chain_terms<'a>(op: BinaryOp, e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::Binary(o, a, b) if *o == op => {
            chain_terms(op, a, out);
            chain_terms(op, b, out);
        }
        other => out.push(other),
    }
}

/// Data shared by every step of one search.
struct Context<'a> {
    inputs: &'a [Vec<f64>],
    target: &'a [f64],
    config: &'a SearchConfig,
    fit_inputs: Vec<Vec<f64>>,
    fit_target: Vec<f64>,
    /// Variance of the target (1 if the target is constant).
    scale: f64,
    /// Sums of squares below this are treated as zero.
    floor: f64,
    fit_opts: FitOptions,
}

impl<'a> Context<'a> {
    fn new(inputs: &'a [Vec<f64>], target: &'a [f64], config: &'a SearchConfig) -> Self {
        let n = target.len();
        let mean = target.iter().sum::<f64>() / n as f64;
        let variance = target.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / n as f64;
        let scale = if variance > 0.0 { variance } else { 1.0 };
        let rows = sample_rows(n, config.fit_rows);
        let fit_inputs = inputs
            .iter()
            .map(|c| rows.iter().map(|&i| c[i]).collect())
            .collect();
        let fit_target: Vec<f64> = rows.iter().map(|&i| target[i]).collect();
        let fit_opts = FitOptions {
            good_enough: config.exact_tolerance * scale * fit_target.len() as f64,
            ..FitOptions::default()
        };
        Context {
            inputs,
            target,
            config,
            fit_inputs,
            fit_target,
            scale,
            floor: config.exact_tolerance * scale * n as f64,
            fit_opts,
        }
    }

    fn n(&self) -> usize {
        self.target.len()
    }

    fn sse_of(&self, pred: &[f64]) -> f64 {
        let mut s = 0.0;
        for (p, y) in pred.iter().zip(self.target) {
            let r = p - y;
            if !r.is_finite() {
                return f64::INFINITY;
            }
            s += r * r;
        }
        s
    }

    /// Whether replacing a fitted value (error `fitted`) by a rounder one
    /// (error `snapped`) is worth it. Uses the Bayesian information
    /// criterion: one fewer free number is worth `ln(n)` of log-likelihood.
    fn snap_ok(&self, snapped: f64, fitted: f64) -> bool {
        let n = self.n() as f64;
        let ratio = (snapped + self.floor) / (fitted + self.floor);
        ratio.is_finite() && n * ratio.ln() <= n.ln().max(1.0)
    }

    /// Returns the simplest nice value close to `x` that keeps the error
    /// (computed by `sse`) acceptable, or `x` itself.
    fn snap(&self, x: f64, sse: impl Fn(f64) -> f64) -> f64 {
        let fitted = sse(x);
        for v in nice_values(x) {
            if self.snap_ok(sse(v), fitted) {
                return v;
            }
        }
        x
    }

    /// Residuals of `y - (a·g + b)` with the best `a`, `b` for given values `g`.
    fn projected_residuals(g: &[f64], y: &[f64]) -> Option<Vec<f64>> {
        if g.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let (a, b) = affine_fit(g, y).unwrap_or((0.0, y.iter().sum::<f64>() / y.len() as f64));
        Some(g.iter().zip(y).map(|(g, y)| a * g + b - y).collect())
    }

    /// Fits the inner parameters of `e` (for example `k` in `exp(k·x)`),
    /// assuming an outer `a·e + b`, and returns `e` with them substituted.
    fn fit_inner(&self, e: &Expr) -> Option<Expr> {
        let m = e.num_params();
        if m == 0 {
            return Some(e.clone());
        }
        let (mut params, sub_sse) = fit_with(
            residuals_for(e, &self.fit_inputs, &self.fit_target),
            m,
            &self.fit_opts,
        )?;
        let mut sse = sub_sse;
        if self.fit_target.len() != self.n() {
            // Refine on all rows only if the formula looks promising.
            if sub_sse / self.fit_target.len() as f64 / self.scale > 0.5 {
                return None;
            }
            let (p, s) = levenberg_marquardt(
                residuals_for(e, self.inputs, self.target),
                params,
                self.fit_opts.max_iterations,
            )?;
            params = p;
            sse = s;
        }
        let full = residuals_for(e, self.inputs, self.target);
        let sse_at = |p: &[f64]| full(p).map_or(f64::INFINITY, |r| r.iter().map(|x| x * x).sum());
        for i in 0..params.len() {
            for v in nice_values(params[i]) {
                let mut trial = params.clone();
                trial[i] = v;
                let s = sse_at(&trial);
                if self.snap_ok(s, sse) {
                    params = trial;
                    sse = s;
                    break;
                }
            }
        }
        Some(canonicalize(&e.substitute_params(&params)))
    }

    /// Candidates `g`, `a·g`, `g + b` and `a·g + b`, with `a` and `b` snapped.
    fn affine_variants(&self, g_expr: &Expr, front: &mut Front) -> Vec<Candidate> {
        let g = g_expr.eval(self.inputs, &[], self.n());
        let Some(stats) = AffineStats::new(&g, self.target) else {
            return Vec::new();
        };
        let mut params = vec![(1.0, 0.0)];
        if let Some(a) = stats.best_scale() {
            params.push((self.snap(a, |a| stats.sse(a, 0.0)), 0.0));
        }
        let b = self.snap(stats.best_offset(1.0), |b| stats.sse(1.0, b));
        params.push((1.0, b));
        if let Some((a, _)) = stats.best_affine() {
            let a = self.snap(a, |a| stats.sse(a, stats.best_offset(a)));
            let b = self.snap(stats.best_offset(a), |b| stats.sse(a, b));
            params.push((a, b));
        }
        let base = g_expr.complexity();
        // With known units, `g` alone or `g + b` must already have the
        // target's dimension; otherwise the scale `a` carries the units.
        let dims_match = match (&self.config.input_dims, self.config.target_dim) {
            (Some(vars), Some(target)) => match dimension_of(g_expr, vars) {
                Dimension::Known(d) => Some(d == target),
                Dimension::Any => None,
                Dimension::Invalid => return Vec::new(),
            },
            _ => None,
        };
        let mut out = Vec::new();
        for (a, b) in params {
            let unit_cost = match dims_match {
                Some(false) if a == 1.0 => continue,
                Some(false) => self.config.unit_constant_cost,
                _ => 0,
            };
            // Cheap upper bound on the complexity of `a·g + b`, used to skip
            // building formulas that a simpler, better one already beats.
            let mut complexity = base + unit_cost;
            if a != 1.0 {
                complexity += 1 + constant_complexity(a);
            }
            if b != 0.0 {
                complexity += 1 + constant_complexity(b);
            }
            let nmse = stats.sse(a, b) / self.n() as f64 / self.scale;
            if front.is_dominated(complexity, nmse) {
                continue;
            }
            if let Some(mut c) = self.score(build(g_expr, a, b), &g, a, b) {
                if unit_cost > 0 {
                    c.complexity += unit_cost;
                    c.score += self.config.parsimony * unit_cost as f64;
                }
                front.insert(c.complexity, c.nmse);
                out.push(c);
            }
        }
        out
    }

    /// Scores `a·g + b` given the values `g` of the inner formula.
    fn score(&self, expr: Expr, g: &[f64], a: f64, b: f64) -> Option<Candidate> {
        let mut sum_sq = 0.0;
        let mut max_abs = 0.0f64;
        for (g, y) in g.iter().zip(self.target) {
            let r = a * g + b - y;
            sum_sq += r * r;
            max_abs = max_abs.max(r.abs());
        }
        if !sum_sq.is_finite() {
            return None;
        }
        Some(self.make_candidate(expr, sum_sq, max_abs))
    }

    fn make_candidate(&self, expr: Expr, sum_sq: f64, max_abs: f64) -> Candidate {
        let mse = sum_sq / self.n() as f64;
        let nmse = mse / self.scale;
        let complexity = expr.complexity();
        let tol = self.config.exact_tolerance;
        Candidate {
            expr,
            complexity,
            mse,
            nmse,
            r2: 1.0 - nmse,
            max_abs_error: max_abs,
            score: nmse.max(tol).ln() + self.config.parsimony * complexity as f64,
            exact: nmse <= tol,
        }
    }

    /// Scores a parameter-free formula on all rows.
    fn candidate(&self, expr: Expr) -> Option<Candidate> {
        let pred = expr.eval(self.inputs, &[], self.n());
        let mut sum_sq = 0.0;
        let mut max_abs = 0.0f64;
        for (p, y) in pred.iter().zip(self.target) {
            let r = p - y;
            sum_sq += r * r;
            max_abs = max_abs.max(r.abs());
        }
        sum_sq
            .is_finite()
            .then(|| self.make_candidate(expr, sum_sq, max_abs))
    }
}

/// Best error seen so far for each complexity, to discard dominated
/// candidates early.
struct Front {
    best: std::collections::BTreeMap<usize, f64>,
    tolerance: f64,
}

impl Front {
    fn new(tolerance: f64) -> Self {
        Front {
            best: std::collections::BTreeMap::new(),
            tolerance,
        }
    }

    /// Whether some formula at most this complex is at least this accurate.
    fn is_dominated(&self, complexity: usize, nmse: f64) -> bool {
        let err = nmse.max(self.tolerance);
        self.best
            .range(..=complexity)
            .any(|(_, &best)| best <= err * (1.0 - 1e-9))
    }

    fn insert(&mut self, complexity: usize, nmse: f64) {
        let err = nmse.max(self.tolerance);
        let entry = self.best.entry(complexity).or_insert(f64::INFINITY);
        if err < *entry {
            *entry = err;
        }
    }
}

/// Sums that give the error of `a·g + b` for any `a`, `b` in constant time.
struct AffineStats {
    n: f64,
    sg: f64,
    sy: f64,
    sgg: f64,
    sgy: f64,
    syy: f64,
}

impl AffineStats {
    fn new(g: &[f64], y: &[f64]) -> Option<Self> {
        let mut s = AffineStats {
            n: g.len() as f64,
            sg: 0.0,
            sy: 0.0,
            sgg: 0.0,
            sgy: 0.0,
            syy: 0.0,
        };
        for (g, y) in g.iter().zip(y) {
            s.sg += g;
            s.sy += y;
            s.sgg += g * g;
            s.sgy += g * y;
            s.syy += y * y;
        }
        [s.sg, s.sgg, s.sgy]
            .iter()
            .all(|v| v.is_finite())
            .then_some(s)
    }

    /// Sum of squared residuals of `a·g + b`. Clamped at 0 because the
    /// expanded formula can go slightly negative through rounding.
    fn sse(&self, a: f64, b: f64) -> f64 {
        let v = self.syy - 2.0 * a * self.sgy - 2.0 * b * self.sy
            + a * a * self.sgg
            + 2.0 * a * b * self.sg
            + self.n * b * b;
        v.max(0.0)
    }

    fn best_scale(&self) -> Option<f64> {
        let a = self.sgy / self.sgg;
        (self.sgg > 0.0 && a.is_finite()).then_some(a)
    }

    fn best_offset(&self, a: f64) -> f64 {
        (self.sy - a * self.sg) / self.n
    }

    fn best_affine(&self) -> Option<(f64, f64)> {
        let var_g = self.sgg - self.sg * self.sg / self.n;
        if var_g.is_nan() || var_g <= 1e-12 * self.sgg.abs() {
            return None;
        }
        let a = (self.sgy - self.sg * self.sy / self.n) / var_g;
        a.is_finite().then(|| (a, self.best_offset(a)))
    }
}

/// Residual function for fitting the inner parameters of `e` on `cols`, `y`.
fn residuals_for<'e>(
    e: &'e Expr,
    cols: &'e [Vec<f64>],
    y: &'e [f64],
) -> impl Fn(&[f64]) -> Option<Vec<f64>> + 'e {
    move |p: &[f64]| Context::projected_residuals(&e.eval(cols, p, y.len()), y)
}

/// `a·g + b`, leaving out `a` when it is 1 and `b` when it is 0.
fn build(g: &Expr, a: f64, b: f64) -> Expr {
    let mut e = g.clone();
    if a != 1.0 {
        e = Expr::binary(BinaryOp::Mul, Expr::Const(a), e);
    }
    if b != 0.0 {
        e = Expr::binary(BinaryOp::Add, e, Expr::Const(b));
    }
    canonicalize(&e)
}

fn validate(inputs: &[Vec<f64>], target: &[f64]) -> Result<(), Error> {
    if target.is_empty() {
        return Err(Error::EmptyData);
    }
    if inputs.is_empty() {
        return Err(Error::NoInputs);
    }
    for (column, c) in inputs.iter().enumerate() {
        if c.len() != target.len() {
            return Err(Error::LengthMismatch {
                column,
                expected: target.len(),
                found: c.len(),
            });
        }
        if let Some(row) = c.iter().position(|v| !v.is_finite()) {
            return Err(Error::NonFinite {
                column: Some(column),
                row,
            });
        }
    }
    if let Some(row) = target.iter().position(|v| !v.is_finite()) {
        return Err(Error::NonFinite { column: None, row });
    }
    Ok(())
}

/// Evenly spaced row indices, at most `max` of them.
fn sample_rows(n: usize, max: usize) -> Vec<usize> {
    if n <= max || max == 0 {
        return (0..n).collect();
    }
    (0..max).map(|i| i * n / max).collect()
}

/// Keeps, for each complexity, the most accurate formula, and drops any
/// formula that is not more accurate than a simpler one. Errors below
/// `tolerance` count as equal, so nothing more complex than the simplest
/// exact formula survives.
fn pareto_front(mut candidates: Vec<Candidate>, tolerance: f64) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        a.complexity
            .cmp(&b.complexity)
            .then(a.nmse.total_cmp(&b.nmse))
    });
    let mut front = Vec::new();
    let mut best = f64::INFINITY;
    for c in candidates {
        let err = c.nmse.max(tolerance);
        if err < best * (1.0 - 1e-9) {
            best = err;
            front.push(c);
        }
    }
    front
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_input() {
        let cfg = SearchConfig::default();
        assert_eq!(search(&[vec![]], &[], &cfg).unwrap_err(), Error::EmptyData);
        assert_eq!(search(&[], &[1.0], &cfg).unwrap_err(), Error::NoInputs);
        assert!(matches!(
            search(&[vec![1.0]], &[1.0, 2.0], &cfg).unwrap_err(),
            Error::LengthMismatch { .. }
        ));
        assert!(matches!(
            search(&[vec![f64::NAN]], &[1.0], &cfg).unwrap_err(),
            Error::NonFinite {
                column: Some(0),
                row: 0
            }
        ));
    }

    #[test]
    fn sample_rows_is_spread_out() {
        assert_eq!(sample_rows(5, 10), vec![0, 1, 2, 3, 4]);
        assert_eq!(sample_rows(10, 5), vec![0, 2, 4, 6, 8]);
    }
}
