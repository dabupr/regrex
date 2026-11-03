//! Fitting free parameters with Levenberg–Marquardt, and snapping fitted
//! values to "nice" constants.

use crate::expr::Expr;

/// Sum of squared residuals of `expr` with `params` against `target`.
///
/// Returns `f64::INFINITY` if any prediction is not finite.
pub fn sse(expr: &Expr, columns: &[Vec<f64>], target: &[f64], params: &[f64]) -> f64 {
    let pred = expr.eval(columns, params, target.len());
    let mut total = 0.0;
    for (p, y) in pred.iter().zip(target) {
        let r = p - y;
        if !r.is_finite() {
            return f64::INFINITY;
        }
        total += r * r;
    }
    total
}

/// Options for [`fit_params`].
#[derive(Debug, Clone)]
pub struct FitOptions {
    /// Maximum Levenberg–Marquardt iterations per starting point.
    pub max_iterations: usize,
    /// Starting values tried for every parameter.
    pub starts: Vec<f64>,
    /// Stop early once the sum of squares falls below this value.
    pub good_enough: f64,
}

impl Default for FitOptions {
    fn default() -> Self {
        FitOptions {
            max_iterations: 30,
            starts: vec![1.0, -1.0, 0.1],
            good_enough: 0.0,
        }
    }
}

/// Finds parameter values that minimise the squared error of `expr`.
///
/// Returns the best parameters and their sum of squared residuals, or `None`
/// if no starting point produced finite predictions.
pub fn fit_params(
    expr: &Expr,
    columns: &[Vec<f64>],
    target: &[f64],
    opts: &FitOptions,
) -> Option<(Vec<f64>, f64)> {
    let n = target.len();
    let residuals = |p: &[f64]| -> Option<Vec<f64>> {
        let pred = expr.eval(columns, p, n);
        let r: Vec<f64> = pred.iter().zip(target).map(|(a, b)| a - b).collect();
        r.iter().all(|x| x.is_finite()).then_some(r)
    };
    fit_with(residuals, expr.num_params(), opts)
}

/// Minimises the sum of squares of `residuals(p)` over `m` parameters, trying
/// every starting value in `opts.starts`.
pub fn fit_with<F>(residuals: F, m: usize, opts: &FitOptions) -> Option<(Vec<f64>, f64)>
where
    F: Fn(&[f64]) -> Option<Vec<f64>>,
{
    if m == 0 {
        let r = residuals(&[])?;
        return Some((Vec::new(), r.iter().map(|x| x * x).sum()));
    }
    let mut best: Option<(Vec<f64>, f64)> = None;
    for &start in &opts.starts {
        if let Some((p, s)) = levenberg_marquardt(&residuals, vec![start; m], opts.max_iterations) {
            if best.as_ref().is_none_or(|(_, b)| s < *b) {
                best = Some((p, s));
            }
            if s <= opts.good_enough {
                break;
            }
        }
    }
    best
}

/// Refines `p` with Levenberg–Marquardt using a forward-difference Jacobian.
///
/// `residuals` returns `None` when the model cannot be evaluated at `p`.
pub fn levenberg_marquardt<F>(
    residuals: F,
    mut p: Vec<f64>,
    max_iterations: usize,
) -> Option<(Vec<f64>, f64)>
where
    F: Fn(&[f64]) -> Option<Vec<f64>>,
{
    let m = p.len();
    let mut r = residuals(&p)?;
    let n = r.len();
    let mut cost: f64 = r.iter().map(|x| x * x).sum();
    let mut lambda = 1e-3;

    for _ in 0..max_iterations {
        if cost == 0.0 {
            break;
        }
        // Jacobian, column by column.
        let mut jac = vec![vec![0.0; n]; m];
        for j in 0..m {
            let h = 1e-7 * p[j].abs().max(1.0);
            let mut q = p.clone();
            q[j] += h;
            let rq = residuals(&q)?;
            for i in 0..n {
                jac[j][i] = (rq[i] - r[i]) / h;
            }
        }
        // Normal equations: (JᵀJ + λ·diag) δ = -Jᵀr.
        let mut a = vec![vec![0.0; m]; m];
        let mut g = vec![0.0; m];
        for j in 0..m {
            for k in 0..=j {
                let s: f64 = jac[j].iter().zip(&jac[k]).map(|(x, y)| x * y).sum();
                a[j][k] = s;
                a[k][j] = s;
            }
            g[j] = jac[j].iter().zip(&r).map(|(x, y)| x * y).sum();
        }

        let mut improved = false;
        while lambda < 1e12 {
            let mut damped = a.clone();
            for (j, row) in damped.iter_mut().enumerate() {
                row[j] += lambda * a[j][j].max(1e-12);
            }
            let rhs: Vec<f64> = g.iter().map(|x| -x).collect();
            if let Some(delta) = solve(damped, rhs) {
                let q: Vec<f64> = p.iter().zip(&delta).map(|(x, d)| x + d).collect();
                if let Some(rq) = residuals(&q) {
                    let new_cost: f64 = rq.iter().map(|x| x * x).sum();
                    if new_cost < cost {
                        let gain = (cost - new_cost) / cost;
                        p = q;
                        r = rq;
                        cost = new_cost;
                        lambda = (lambda / 10.0).max(1e-12);
                        improved = gain > 1e-14;
                        break;
                    }
                }
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    Some((p, cost))
}

/// Least-squares `a` and `b` for `y ≈ a·g + b`. `None` if `g` is constant.
pub fn affine_fit(g: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let n = g.len() as f64;
    let mg = g.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut sgg = 0.0;
    let mut sgy = 0.0;
    for (a, b) in g.iter().zip(y) {
        sgg += (a - mg) * (a - mg);
        sgy += (a - mg) * (b - my);
    }
    if sgg.is_nan() || sgg <= 1e-300 * n {
        return None;
    }
    let a = sgy / sgg;
    let b = my - a * mg;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

/// Least-squares `a` for `y ≈ a·g`. `None` if `g` is all zeros.
pub fn scale_fit(g: &[f64], y: &[f64]) -> Option<f64> {
    let sgg: f64 = g.iter().map(|x| x * x).sum();
    let sgy: f64 = g.iter().zip(y).map(|(a, b)| a * b).sum();
    let a = sgy / sgg;
    (sgg > 0.0 && a.is_finite()).then_some(a)
}

/// Solves a small dense linear system by Gaussian elimination with partial
/// pivoting. Returns `None` if the matrix is singular.
pub(crate) fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let (top, bottom) = a.split_at_mut(col + 1);
        let pivot_row = &top[col];
        for (offset, row) in bottom.iter_mut().enumerate() {
            let f = row[col] / pivot_row[col];
            for (x, p) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *x -= f * p;
            }
            b[col + 1 + offset] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// "Nice" values close to `x`, simplest first.
///
/// Fitted constants such as `0.49999998` are almost always a rounded `0.5`;
/// trying these values lets the search report exact formulas.
pub fn nice_values(x: f64) -> Vec<f64> {
    use std::f64::consts::{E, PI};
    let mut out = Vec::new();
    let mut push = |v: f64| {
        if v.is_finite() && !out.contains(&v) {
            out.push(v);
        }
    };
    push(x.round());
    for d in [2.0, 4.0, 3.0] {
        push((x * d).round() / d);
    }
    for k in [PI, E, 2.0 * PI, PI / 2.0] {
        let ratio = (x / k).round();
        if ratio != 0.0 && ratio.abs() <= 4.0 {
            push(ratio * k);
        }
    }
    for decimals in 1..=4 {
        let scale = 10f64.powi(decimals);
        push((x * scale).round() / scale);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{BinaryOp, Expr, UnaryOp};

    #[test]
    fn fits_a_linear_model() {
        // y = c0 * x + c1 with c0 = 3, c1 = -2
        let e = Expr::binary(
            BinaryOp::Add,
            Expr::binary(BinaryOp::Mul, Expr::Param(0), Expr::Var(0)),
            Expr::Param(1),
        );
        let x: Vec<f64> = (0..20).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|x| 3.0 * x - 2.0).collect();
        let (p, s) = fit_params(&e, &[x], &y, &FitOptions::default()).unwrap();
        assert!((p[0] - 3.0).abs() < 1e-8, "{p:?}");
        assert!((p[1] + 2.0).abs() < 1e-8, "{p:?}");
        assert!(s < 1e-12);
    }

    #[test]
    fn fits_a_nonlinear_model() {
        // y = exp(c0 * x) with c0 = -0.7
        let e = Expr::unary(
            UnaryOp::Exp,
            Expr::binary(BinaryOp::Mul, Expr::Param(0), Expr::Var(0)),
        );
        let x: Vec<f64> = (0..30).map(|i| f64::from(i) / 10.0).collect();
        let y: Vec<f64> = x.iter().map(|x| (-0.7 * x).exp()).collect();
        let (p, _) = fit_params(&e, &[x], &y, &FitOptions::default()).unwrap();
        assert!((p[0] + 0.7).abs() < 1e-6, "{p:?}");
    }

    #[test]
    fn non_finite_predictions_are_rejected() {
        // log(c0 * x) cannot be evaluated when x has both signs.
        let e = Expr::unary(
            UnaryOp::Log,
            Expr::binary(BinaryOp::Mul, Expr::Param(0), Expr::Var(0)),
        );
        let x = vec![-1.0, 1.0];
        let y = vec![0.0, 0.0];
        assert!(fit_params(&e, &[x], &y, &FitOptions::default()).is_none());
    }

    #[test]
    fn nice_values_start_with_the_simplest() {
        let v = nice_values(0.4999999);
        assert_eq!(v[0], 0.0);
        assert!(v.contains(&0.5));
        assert!(nice_values(1.2100000004).contains(&1.21));
        assert!(nice_values(std::f64::consts::PI + 1e-7).contains(&std::f64::consts::PI));
    }

    #[test]
    fn affine_and_scale_fits() {
        let g = vec![1.0, 2.0, 3.0];
        let y: Vec<f64> = g.iter().map(|x| 4.0 * x - 1.0).collect();
        let (a, b) = affine_fit(&g, &y).unwrap();
        assert!((a - 4.0).abs() < 1e-12 && (b + 1.0).abs() < 1e-12);
        assert!(affine_fit(&[2.0, 2.0], &[1.0, 3.0]).is_none());
        assert_eq!(scale_fit(&[1.0, 2.0], &[3.0, 6.0]), Some(3.0));
        assert_eq!(scale_fit(&[0.0, 0.0], &[3.0, 6.0]), None);
    }

    #[test]
    fn solves_small_systems() {
        let x = solve(vec![vec![2.0, 1.0], vec![1.0, 3.0]], vec![3.0, 5.0]).unwrap();
        assert!((x[0] - 0.8).abs() < 1e-12 && (x[1] - 1.4).abs() < 1e-12);
        assert!(solve(vec![vec![1.0, 2.0], vec![2.0, 4.0]], vec![1.0, 2.0]).is_none());
    }
}
