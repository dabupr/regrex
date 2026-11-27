//! Fitted constants written as the known numbers they are.
//!
//! A search fits Coulomb's constant as `0.0795775` and the Gaussian's as
//! `0.398942`. Both are known numbers: `1 / (4 * π)` and `1 / sqrt(2 * π)`.
//! [`snap`] replaces a fitted constant by such a number when the formula
//! with it still matches every row it matched before, and the display then
//! writes it that way (see [`symbolic`]).
//!
//! The known numbers are a small fraction `p / q` (both up to 12) times or
//! divided by `π`, `π^2`, `sqrt(π)`, `sqrt(2)`, `sqrt(3)`, `sqrt(2 * π)` or
//! `e`, and the fractions whose decimals never end (`2 / 3`). Plain fractions are left as decimals: `0.5` reads well as it is,
//! and so do constants with up to four significant digits (`0.21`, `9.81`),
//! which is how people write the numbers they mean.

use crate::expr::Expr;
use crate::Table;
use std::f64::consts::{E, PI};
use std::sync::OnceLock;

/// A known number and how it is written.
struct Known {
    value: f64,
    text: String,
    /// How tightly the text binds, as for the formula's operators: 2 for a
    /// product or quotient, 4 for a power, 5 for a single symbol or call.
    binding: u8,
}

fn table() -> &'static [Known] {
    static TABLE: OnceLock<Vec<Known>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let bases: [(f64, &str, u8); 7] = [
            (PI, "π", 5),
            (PI * PI, "π^2", 4),
            (PI.sqrt(), "sqrt(π)", 5),
            (2f64.sqrt(), "sqrt(2)", 5),
            (3f64.sqrt(), "sqrt(3)", 5),
            ((2.0 * PI).sqrt(), "sqrt(2 * π)", 5),
            (E, "e", 5),
        ];
        let gcd = |mut a: u32, mut b: u32| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        let mut out: Vec<Known> = Vec::new();
        for &(base, name, base_binding) in &bases {
            let wrapped = |n: u32| {
                if n == 1 {
                    name.to_string()
                } else {
                    format!("{n} * {name}")
                }
            };
            for p in 1..=12u32 {
                for q in 1..=12u32 {
                    if gcd(p, q) != 1 {
                        continue;
                    }
                    let r = p as f64 / q as f64;
                    // (p / q) * base
                    let (text, binding) = match (p, q) {
                        (1, 1) => (name.to_string(), base_binding),
                        (_, 1) => (wrapped(p), 2),
                        _ => (format!("{} / {q}", wrapped(p)), 2),
                    };
                    out.push(Known {
                        value: r * base,
                        text,
                        binding,
                    });
                    // p / (q * base)
                    let den = if q == 1 && base_binding == 5 {
                        name.to_string()
                    } else {
                        format!("({})", wrapped(q))
                    };
                    out.push(Known {
                        value: r / base,
                        text: format!("{p} / {den}"),
                        binding: 2,
                    });
                }
            }
        }
        // Fractions that never end as decimals: `2 / 3`, not `0.666667`.
        for p in 1..=12u32 {
            for q in 2..=12u32 {
                let mut d = q;
                for f in [2, 5] {
                    while d % f == 0 {
                        d /= f;
                    }
                }
                if gcd(p, q) == 1 && d != 1 {
                    out.push(Known {
                        value: p as f64 / q as f64,
                        text: format!("{p} / {q}"),
                        binding: 2,
                    });
                }
            }
        }
        // The same number written two ways (`2 / sqrt(2)` is `sqrt(2)`)
        // keeps the shortest.
        out.sort_by(|a, b| a.value.total_cmp(&b.value));
        let mut kept: Vec<Known> = Vec::with_capacity(out.len());
        for k in out {
            match kept.last_mut() {
                Some(last) if (k.value - last.value).abs() <= 1e-12 * last.value => {
                    if k.text.len() < last.text.len() {
                        *last = k;
                    }
                }
                _ => kept.push(k),
            }
        }
        kept
    })
}

fn find(c: f64, rel: f64) -> Option<&'static Known> {
    if !c.is_finite() || c == 0.0 {
        return None;
    }
    let a = c.abs();
    let t = table();
    let i = t.partition_point(|k| k.value < a);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter_map(|j| t.get(j))
        .filter(|k| (k.value - a).abs() <= rel * a)
        .min_by(|x, y| (x.value - a).abs().total_cmp(&(y.value - a).abs()))
}

/// How a known number is written, and how tightly that binds (2 for a
/// product or quotient, 4 for a power, 5 for a symbol or a call): `0.0795775`
/// is not one, `1 / (4 * π)` to full precision is. Negative numbers are
/// `None`; their sign is written apart.
pub fn symbolic(c: f64) -> Option<(&'static str, u8)> {
    if c <= 0.0 {
        return None;
    }
    find(c, 1e-12).map(|k| (k.text.as_str(), k.binding))
}

/// Replaces each fitted constant of `formula` that is close to a known
/// number by that number, when the formula still matches every row in
/// `matched` within `tolerance` of column `target`. The others are kept.
pub fn snap(
    table: &Table,
    target: usize,
    formula: Expr,
    matched: &[usize],
    tolerance: &dyn Fn(usize) -> f64,
) -> Expr {
    let mut values = Vec::new();
    collect(&formula, &mut values);
    let mut formula = formula;
    for c in values {
        // A short decimal (`0.21`, `1.5`, `9.81`) is what the data says;
        // only long ones, which no one writes, are worth naming.
        if symbolic(c).is_some() || crate::expr::constant_complexity(c) < 3 {
            continue;
        }
        let Some(k) = find(c, 1e-4) else { continue };
        let exact = k.value.copysign(c);
        let candidate = replace(&formula, c, exact);
        let ys = &table.columns[target];
        let still = crate::scan::matching_rows(&candidate, table, matched, ys, tolerance);
        if still.len() == matched.len() {
            formula = candidate;
        }
    }
    formula
}

fn collect(e: &Expr, out: &mut Vec<f64>) {
    match e {
        Expr::Const(c) => out.push(*c),
        Expr::Unary(_, a) => collect(a, out),
        Expr::Binary(_, a, b) => {
            collect(a, out);
            collect(b, out);
        }
        Expr::Var(_) | Expr::Param(_) => {}
    }
}

fn replace(e: &Expr, from: f64, to: f64) -> Expr {
    match e {
        Expr::Const(c) if *c == from => Expr::Const(to),
        Expr::Unary(op, a) => Expr::unary(*op, replace(a, from, to)),
        Expr::Binary(op, a, b) => Expr::binary(*op, replace(a, from, to), replace(b, from, to)),
        _ => e.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_known_numbers() {
        assert_eq!(symbolic(1.0 / (4.0 * PI)), Some(("1 / (4 * π)", 2)));
        assert_eq!(
            symbolic(1.0 / (2.0 * PI).sqrt()),
            Some(("1 / sqrt(2 * π)", 2))
        );
        assert_eq!(symbolic(2.0 * PI), Some(("2 * π", 2)));
        assert_eq!(symbolic(PI / 2.0), Some(("π / 2", 2)));
        assert_eq!(symbolic(PI), Some(("π", 5)));
        assert_eq!(symbolic(PI * PI), Some(("π^2", 4)));
        assert_eq!(symbolic(4.0 / 3.0 * PI), Some(("4 * π / 3", 2)));
        assert_eq!(symbolic(2f64.sqrt()), Some(("sqrt(2)", 5)));
        assert_eq!(symbolic(2.0 / 3.0), Some(("2 / 3", 2)));
        assert_eq!(symbolic(0.5), None);
        assert_eq!(symbolic(0.25), None);
        assert_eq!(symbolic(0.0795775), None);
        assert_eq!(symbolic(-PI), None);
    }

    #[test]
    fn snaps_a_fitted_constant_that_still_matches() {
        // y = q / (4π r^2), with the constant fitted to 6 digits.
        let q: Vec<f64> = (1..40).map(|i| i as f64 * 0.37).collect();
        let r: Vec<f64> = (1..40).map(|i| 1.0 + (i % 7) as f64).collect();
        let y: Vec<f64> = q
            .iter()
            .zip(&r)
            .map(|(q, r)| q / (4.0 * PI * r * r))
            .collect();
        let table = Table {
            names: vec!["q".into(), "r".into(), "y".into()],
            columns: vec![q, r, y],
            ..Table::default()
        };
        let fitted = Expr::binary(
            crate::expr::BinaryOp::Mul,
            Expr::Const(0.0795775),
            Expr::binary(
                crate::expr::BinaryOp::Div,
                Expr::Var(0),
                Expr::unary(crate::expr::UnaryOp::Square, Expr::Var(1)),
            ),
        );
        let rows: Vec<usize> = (0..39).collect();
        let tight = |_: usize| 1e-9;
        let snapped = snap(&table, 2, fitted.clone(), &rows, &tight);
        let names = ["q".to_string(), "r".to_string()];
        assert_eq!(
            snapped.display_with(&names).to_string(),
            "1 / (4 * π) * q / r^2"
        );
        // A constant that is only near a known number stays as it is.
        let off = Expr::binary(
            crate::expr::BinaryOp::Mul,
            Expr::Const(0.0796),
            Expr::Var(0),
        );
        assert_eq!(snap(&table, 2, off.clone(), &rows, &tight), off);
    }
}
