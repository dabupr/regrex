//! Formulas between consecutive rows of an ordered table.
//!
//! In a table with an order (see [`crate::profile`]) a column can be
//! computed from the row before rather than from its own row:
//!
//! - a running total: `balance = previous balance + amount`;
//! - a distance along a path: each step of `pk` is the distance between
//!   consecutive points `(x, y)` (or `(x, y, z)`);
//! - a slope: `gradient = 100 * dz / dpk` on each stretch where the
//!   gradient is constant or changes linearly (a vertical curve).
//!
//! Each check is one pass over the rows per candidate column, so they run
//! before the per-regime search. Steps across the end of a group are not
//! checked.

use crate::profile::{Profile, Role};
use crate::regime::{rounding, Order};
use crate::scan::{DerivedColumn, ScanConfig, Table};
use crate::Expr;

/// Rows a stretch needs before its slope is compared.
const MIN_STRETCH: usize = 8;
/// Share of the rows of a stretch its slope must explain.
const STRETCH_MATCH: f64 = 0.9;
/// Steps tried on a first sample before checking every step.
const SAMPLE_STEPS: usize = 2000;
/// Scales tried for a slope: plain, percent, per mille.
const SLOPE_SCALES: [f64; 3] = [1.0, 100.0, 1000.0];

/// Explains `target` from the previous row: a running total, a distance
/// between consecutive points or a slope per stretch. `None` without an
/// order or when nothing holds on `config.min_match` of the steps.
pub(crate) fn explain_by_steps(
    table: &Table,
    target: usize,
    config: &ScanConfig,
    profile: &Profile,
    skip: &[bool],
) -> Option<DerivedColumn> {
    let order = Order::new(table, profile)?;
    let role = profile.columns.get(target)?.role;
    if table.is_category(target)
        || profile.columns[target].distinct < 3
        || matches!(role, Role::GroupKey | Role::Identifier)
    {
        return None;
    }
    let candidates: Vec<usize> = (0..table.columns.len())
        .filter(|&c| {
            c != target
                && c != order.column
                && !skip.get(c).copied().unwrap_or(false)
                && !table.is_category(c)
                && profile.columns[c].role == Role::Measurement
                && profile.columns[c].distinct >= 3
        })
        .collect();
    let steps = steps(table, &order);
    distance(table, target, &candidates, &steps, config)
        .or_else(|| running_total(table, target, &candidates, &steps, config))
        .or_else(|| {
            (target != order.column)
                .then(|| slope(table, target, &candidates, &order, config))
                .flatten()
        })
}

/// Pairs of consecutive rows `(previous, row)` in one group.
fn steps(table: &Table, order: &Order) -> Vec<(usize, usize)> {
    let x = &table.columns[order.column];
    order
        .sorted
        .windows(2)
        .filter(|w| order.group_of[w[0]] == order.group_of[w[1]] && x[w[1]] > x[w[0]])
        .map(|w| (w[0], w[1]))
        .collect()
}

/// The steps (with every column of `cols` known) where `holds` is true,
/// and those where it is false, if the first are `min_match` of them. A sample is
/// checked first, so most columns are turned down after a few thousand
/// steps.
fn count_steps(
    table: &Table,
    cols: &[usize],
    steps: &[(usize, usize)],
    min_match: f64,
    holds: impl Fn(usize, usize) -> bool,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let known = |&(a, b): &(usize, usize)| {
        cols.iter()
            .all(|&c| table.columns[c][a].is_finite() && table.columns[c][b].is_finite())
    };
    let stride = (steps.len() / SAMPLE_STEPS).max(1);
    let (n, ok) = steps
        .iter()
        .step_by(stride)
        .filter(|s| known(s))
        .fold((0, 0), |(n, ok), &(a, b)| {
            (n + 1, ok + holds(a, b) as usize)
        });
    // A sample can be unlucky; ask a little less of it.
    if n < MIN_STRETCH || (ok as f64) < (min_match - 0.05) * n as f64 {
        return None;
    }
    let (mut rows, mut failed) = (Vec::new(), Vec::new());
    for s in steps.iter().filter(|s| known(s)) {
        if holds(s.0, s.1) {
            rows.push(s.1);
        } else {
            failed.push(s.1);
        }
    }
    let checked = rows.len() + failed.len();
    ((rows.len() as f64) >= min_match * checked as f64).then_some((rows, failed))
}

/// Each step of `target` is the distance between consecutive points of
/// two (or three) other columns.
fn distance(
    table: &Table,
    target: usize,
    candidates: &[usize],
    steps: &[(usize, usize)],
    config: &ScanConfig,
) -> Option<DerivedColumn> {
    let mut best: Option<(Vec<usize>, Vec<usize>, Vec<usize>)> = None;
    // More rows win; on a tie, fewer coordinates.
    let mut consider = |cols: Vec<usize>, (rows, checked): (Vec<usize>, Vec<usize>)| {
        let better = best.as_ref().is_none_or(|b| {
            rows.len() > b.1.len() || rows.len() == b.1.len() && cols.len() < b.0.len()
        });
        if better {
            best = Some((cols, rows, checked));
        }
    };
    for (i, &a) in candidates.iter().enumerate() {
        for &b in &candidates[i + 1..] {
            if let Some(found) = distance_rows(table, target, &[a, b], steps, config) {
                consider(vec![a, b], found);
                continue;
            }
            // A third coordinate (a height) may be needed.
            for &c in &candidates[i + 1..] {
                if c == b {
                    continue;
                }
                if let Some(found) = distance_rows(table, target, &[a, b, c], steps, config) {
                    let mut cols = vec![a, b, c];
                    cols.sort_unstable();
                    consider(cols, found);
                    break;
                }
            }
        }
    }
    let (cols, rows, checked) = best?;
    let names = &table.names;
    let points = cols
        .iter()
        .map(|&c| names[c].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Some(derived(
        target,
        format!(
            "previous {} + distance between consecutive ({points})",
            names[target]
        ),
        cols,
        rows,
        checked,
    ))
}

/// Steps where `target` grows by the distance between consecutive points
/// of `cols`, if they are `min_match` of the steps.
fn distance_rows(
    table: &Table,
    target: usize,
    cols: &[usize],
    steps: &[(usize, usize)],
    config: &ScanConfig,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut all = vec![target];
    all.extend_from_slice(cols);
    let t = &table.columns[target];
    // Each coordinate is off by up to its rounding, at both ends of a
    // step. A chainage in whole units is taken as exact: rounding it to
    // the unit would let any column that moves by about one unit pass.
    let t_round = if table.decimals.get(target).copied().flatten() == Some(0) {
        0.0
    } else {
        rounding(table, target)
    };
    let tol = 2.0 * (t_round + cols.iter().map(|&c| rounding(table, c)).sum::<f64>());
    let (rows, checked) = count_steps(table, &all, steps, config.min_match, |a, b| {
        let d = cols
            .iter()
            .map(|&c| {
                let x = &table.columns[c];
                (x[b] - x[a]).powi(2)
            })
            .sum::<f64>()
            .sqrt();
        let dt = (t[b] - t[a]).abs();
        // Points that stay put prove nothing.
        dt > tol && (dt - d).abs() <= tol.max(1e-9 * dt)
    })?;
    Some((rows, checked))
}

/// `target = previous target + other` (or the other's previous value).
fn running_total(
    table: &Table,
    target: usize,
    candidates: &[usize],
    steps: &[(usize, usize)],
    config: &ScanConfig,
) -> Option<DerivedColumn> {
    let t = &table.columns[target];
    let mut best: Option<(usize, Vec<usize>, Vec<usize>)> = None;
    for &c in candidates {
        let x = &table.columns[c];
        let tol = 2.0 * rounding(table, target) + rounding(table, c);
        let holds = |a: usize, b: usize| {
            let dt = t[b] - t[a];
            // A step within the rounding says nothing about the column
            // added.
            x[b].abs() > tol && (dt - x[b]).abs() <= tol.max(1e-9 * dt.abs())
        };
        let Some((rows, checked)) =
            count_steps(table, &[target, c], steps, config.min_match, holds)
        else {
            continue;
        };
        if best.as_ref().is_none_or(|b| rows.len() > b.1.len()) {
            best = Some((c, rows, checked));
        }
    }
    let (c, rows, checked) = best?;
    let names = &table.names;
    Some(derived(
        target,
        format!("previous {} + {}", names[target], names[c]),
        vec![c],
        rows,
        checked,
    ))
}

/// `target = k * d(other)/d(order)` on stretches where the target is
/// constant or linear in the order: a gradient from heights along a
/// chainage, a speed from positions over time.
fn slope(
    table: &Table,
    target: usize,
    candidates: &[usize],
    order: &Order,
    config: &ScanConfig,
) -> Option<DerivedColumn> {
    let g = &table.columns[target];
    let o = &table.columns[order.column];
    let rows: Vec<usize> = order
        .sorted
        .iter()
        .copied()
        .filter(|&r| g[r].is_finite())
        .collect();
    let stretches = linear_stretches(table, target, order, &rows);
    if stretches.iter().map(Vec::len).sum::<usize>() < MIN_STRETCH {
        return None;
    }
    let g_round = rounding(table, target);
    let mut best: Option<(usize, f64, Vec<usize>)> = None;
    for &c in candidates {
        let z = &table.columns[c];
        let z_round = rounding(table, c).max(1e-12);
        // Derivative of z at each row of each long stretch.
        let fitted: Vec<(&Vec<usize>, Vec<f64>, f64, f64)> = stretches
            .iter()
            .filter(|s| s.len() >= MIN_STRETCH && s.iter().all(|&r| z[r].is_finite()))
            .filter_map(|s| {
                let curved = s.iter().any(|&r| (g[r] - g[s[0]]).abs() > g_round);
                derivative(o, z, s, curved).map(|d| {
                    let span = o[*s.last().unwrap()] - o[s[0]];
                    // How far a fitted slope can be off from rounding alone.
                    let off = (if curved { 8.0 } else { 4.0 }) * z_round / span;
                    // A slope given per step belongs half a step away.
                    let half = (g[*s.last().unwrap()] - g[s[0]]).abs() / (s.len() - 1) as f64 / 2.0;
                    (s, d, off, half)
                })
            })
            .collect();
        let Some(k) = scale(g, &fitted) else {
            continue;
        };
        let mut matched = Vec::new();
        for (s, d, off, half) in &fitted {
            let tol = 2.0 * g_round + k.abs() * off + half;
            let ok = s
                .iter()
                .zip(d)
                .filter(|&(&r, &d)| (k * d - g[r]).abs() <= tol.max(1e-9))
                .count();
            if ok as f64 >= STRETCH_MATCH * s.len() as f64 {
                matched.extend_from_slice(s);
            }
        }
        if (matched.len() as f64) < config.min_match * rows.len() as f64 {
            continue;
        }
        if best.as_ref().is_none_or(|b| matched.len() > b.2.len()) {
            best = Some((c, k, matched));
        }
    }
    let (c, k, mut matched) = best?;
    matched.sort_unstable();
    let failed: Vec<usize> = rows
        .iter()
        .copied()
        .filter(|r| matched.binary_search(r).is_err())
        .collect();
    let names = &table.names;
    let scale = if k == 1.0 {
        String::new()
    } else {
        format!("{} * ", crate::expr::format_number(k))
    };
    Some(derived(
        target,
        format!(
            "{scale}slope of {} over {} on each stretch",
            names[c], names[order.column]
        ),
        vec![c, order.column],
        matched,
        failed,
    ))
}

/// Splits `rows` (sorted, with the target known) into stretches of one
/// group, without gaps in the order, along which the target is a line in
/// the order.
fn linear_stretches(
    table: &Table,
    target: usize,
    order: &Order,
    rows: &[usize],
) -> Vec<Vec<usize>> {
    let g = &table.columns[target];
    let o = &table.columns[order.column];
    let tol = 2.0 * rounding(table, target) + 1e-9;
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for &r in rows {
        let joins = cur.last().is_some_and(|&p| {
            order.group_of[p] == order.group_of[r]
                && order.position[r] == order.position[p] + 1
                && o[r] > o[p]
        }) && match cur.as_slice() {
            [.., p2, p1] => {
                // The new point stays on the line of the last two.
                let slope = (g[*p1] - g[*p2]) / (o[*p1] - o[*p2]);
                (g[*p1] + slope * (o[r] - o[*p1]) - g[r]).abs() <= 2.0 * tol
            }
            _ => true,
        };
        if !joins && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(r);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Least-squares derivative of `z` along `o` at each row of `s`: a line,
/// or a parabola when the target changes along the stretch.
fn derivative(o: &[f64], z: &[f64], s: &[usize], curved: bool) -> Option<Vec<f64>> {
    let o0 = o[s[0]];
    let xs: Vec<f64> = s.iter().map(|&r| o[r] - o0).collect();
    let ys: Vec<f64> = s.iter().map(|&r| z[r]).collect();
    let degree = if curved { 2 } else { 1 };
    let c = polyfit(&xs, &ys, degree)?;
    Some(
        xs.iter()
            .map(|&x| if curved { c[1] + 2.0 * c[2] * x } else { c[1] })
            .collect(),
    )
}

/// Coefficients `c0 + c1 x (+ c2 x²)` of the least-squares fit.
fn polyfit(xs: &[f64], ys: &[f64], degree: usize) -> Option<Vec<f64>> {
    let n = degree + 1;
    // Normal equations, centred for stability.
    let mx = xs.iter().sum::<f64>() / xs.len() as f64;
    let mut a = vec![vec![0.0; n + 1]; n];
    for (&x, &y) in xs.iter().zip(ys) {
        let x = x - mx;
        let pw: Vec<f64> = (0..n).map(|k| x.powi(k as i32)).collect();
        for i in 0..n {
            for j in 0..n {
                a[i][j] += pw[i] * pw[j];
            }
            a[i][n] += pw[i] * y;
        }
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        a.swap(col, piv);
        if a[col][col].abs() < 1e-12 {
            return None;
        }
        let pivot = a[col].clone();
        for (i, row) in a.iter_mut().enumerate() {
            if i != col {
                let f = row[col] / pivot[col];
                for (v, p) in row.iter_mut().zip(&pivot).skip(col) {
                    *v -= f * p;
                }
            }
        }
    }
    let c: Vec<f64> = (0..n).map(|i| a[i][n] / a[i][i]).collect();
    // Back from centred x: only c1 and c2 are needed by the callers.
    Some(match degree {
        1 => vec![c[0] - c[1] * mx, c[1]],
        _ => vec![
            c[0] - c[1] * mx + c[2] * mx * mx,
            c[1] - 2.0 * c[2] * mx,
            c[2],
        ],
    })
}

/// The scale `k` with `target ≈ k * slope`, from the median ratio over
/// the stretches, if it is one of [`SLOPE_SCALES`] (with either sign).
fn scale(g: &[f64], fitted: &[(&Vec<usize>, Vec<f64>, f64, f64)]) -> Option<f64> {
    let mut ratios: Vec<f64> = fitted
        .iter()
        .filter_map(|(s, d, _, _)| {
            let (gm, dm) = (g[s[s.len() / 2]], d[s.len() / 2]);
            (gm != 0.0 && dm.abs() > 1e-12).then_some(gm / dm)
        })
        .collect();
    if ratios.len() < 2 {
        return None;
    }
    ratios.sort_by(f64::total_cmp);
    let m = ratios[ratios.len() / 2];
    SLOPE_SCALES
        .iter()
        .flat_map(|&k| [k, -k])
        .find(|&k| (m / k - 1.0).abs() < 0.02)
}

fn derived(
    target: usize,
    text: String,
    mut inputs: Vec<usize>,
    matched: Vec<usize>,
    mut failed: Vec<usize>,
) -> DerivedColumn {
    inputs.sort_unstable();
    inputs.dedup();
    failed.sort_unstable();
    DerivedColumn {
        target,
        formula: Expr::Const(f64::NAN),
        formula_text: text,
        complexity: inputs.len() + 2,
        inputs,
        matched: matched.len(),
        checked: matched.len() + failed.len(),
        exceptions: failed,
        other: None,
        copy: false,
        regime: None,
        steps: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::profile;

    fn table(names: &[&str], columns: Vec<Vec<f64>>, decimals: Vec<Option<u32>>) -> Table {
        Table {
            names: names.iter().map(|s| s.to_string()).collect(),
            columns,
            decimals,
            ..Table::default()
        }
    }

    fn explain(t: &Table, target: &str) -> Option<DerivedColumn> {
        let i = t.names.iter().position(|n| n == target).unwrap();
        let p = profile(t);
        explain_by_steps(t, i, &ScanConfig::default(), &p, &[])
    }

    fn round(v: f64, d: i32) -> f64 {
        let f = 10f64.powi(d);
        (v * f).round() / f
    }

    #[test]
    fn a_chainage_is_the_distance_along_the_points() {
        let (mut pk, mut x, mut y, mut track, mut noise) = (vec![], vec![], vec![], vec![], vec![]);
        for t in 0..2 {
            let (mut px, mut py, mut s) = (1000.0 * t as f64, 50.0, 0.0);
            for i in 0..300 {
                let heading = 0.3 + 0.01 * i as f64 + t as f64;
                if i > 0 {
                    px += heading.cos();
                    py += heading.sin();
                    s += 1.0;
                }
                track.push(t as f64);
                pk.push(round(s, 3));
                x.push(round(px, 3));
                y.push(round(py, 3));
                noise.push(((i * 37 + t * 11) % 53) as f64 * 0.37);
            }
        }
        let t = table(
            &["track", "pk", "x", "y", "noise"],
            vec![track, pk, x, y, noise],
            vec![Some(0), Some(3), Some(3), Some(3), Some(2)],
        );
        let d = explain(&t, "pk").expect("pk from the points");
        assert_eq!(
            d.formula_text,
            "previous pk + distance between consecutive (x, y)"
        );
        assert!(d.steps);
        assert!(explain(&t, "noise").is_none());
    }

    #[test]
    fn a_running_total() {
        let mut amount = vec![];
        let mut balance = vec![];
        let mut day = vec![];
        let mut b = 100.0;
        for i in 0..200 {
            let a = round(((i * 7919) % 211) as f64 * 1.37 - 120.0, 2);
            b = round(b + a, 2);
            day.push(i as f64);
            amount.push(a);
            balance.push(b);
        }
        let t = table(
            &["day", "amount", "balance"],
            vec![day, amount, balance],
            vec![Some(0), Some(2), Some(2)],
        );
        let d = explain(&t, "balance").expect("a running total");
        assert_eq!(d.formula_text, "previous balance + amount");
        assert!(explain(&t, "amount").is_none());
    }

    #[test]
    fn a_gradient_per_stretch_from_heights() {
        // Constant gradients joined by vertical curves (linear gradient).
        let (mut pk, mut z, mut grad, mut kind) = (vec![], vec![], vec![], vec![]);
        let mut h = 10.0;
        let sections = [1.5, -0.8, 2.2, 0.0, -1.7, 0.9];
        let mut s = 0.0;
        for w in sections.windows(2) {
            for (len, curve) in [(60, false), (20, true)] {
                for i in 0..len {
                    let gr = if curve {
                        w[0] + (w[1] - w[0]) * (i as f64 + 0.5) / len as f64
                    } else {
                        w[0]
                    };
                    h += gr / 100.0;
                    s += 1.0;
                    pk.push(s);
                    z.push(round(h, 3));
                    grad.push(round(gr, 4));
                    kind.push(curve as u8 as f64);
                }
            }
        }
        let t = table(
            &["pk", "z", "gradient", "kind"],
            vec![pk, z, grad, kind],
            vec![Some(0), Some(3), Some(4), Some(0)],
        );
        let d = explain(&t, "gradient").expect("gradient from heights");
        assert_eq!(d.formula_text, "100 * slope of z over pk on each stretch");
    }
}
