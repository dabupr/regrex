//! Bottom-up enumeration of distinct candidate expressions.

use crate::expr::{BinaryOp, Expr, UnaryOp};
use crate::simplify::{canonicalize, structural_key};
use crate::units::{dimension_of, Dim, Dimension};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// What the enumerator may build.
#[derive(Debug, Clone)]
pub struct Grammar {
    /// Number of input variables.
    pub n_vars: usize,
    /// Allowed unary operators.
    pub unary: Vec<UnaryOp>,
    /// Allowed binary operators.
    pub binary: Vec<BinaryOp>,
    /// Maximum expression size (number of nodes).
    pub max_size: usize,
    /// Maximum number of free parameters per expression.
    pub max_params: usize,
    /// Stop after producing this many expressions.
    pub max_expressions: usize,
    /// Dimension of each variable, if known. Expressions that mix
    /// incompatible dimensions are skipped.
    pub var_dims: Option<Vec<Dim>>,
}

/// Values given to free parameters when fingerprinting. They are arbitrary
/// irrational-looking numbers so that different families rarely collide.
const PROBE: [f64; 4] = [1.234_567_8, 0.765_432_1, 2.345_678_9, 0.456_789_1];

/// Enumerates distinct expressions in order of increasing size.
///
/// Two expressions are considered the same when their canonical forms match
/// (see [`canonicalize`]) or when they produce the same values on `sample`,
/// a few rows of the real inputs.
pub fn enumerate(grammar: &Grammar, sample: &[Vec<f64>]) -> Vec<Expr> {
    let n_rows = sample.first().map_or(0, Vec::len);
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut seen_values: HashSet<u64> = HashSet::new();
    // levels[s] = expressions of size s
    let mut levels: Vec<Vec<Expr>> = vec![Vec::new(); grammar.max_size + 1];
    let mut total = 0usize;

    let mut accept = |e: Expr, size: usize, levels: &mut Vec<Vec<Expr>>, total: &mut usize| {
        if *total >= grammar.max_expressions {
            return;
        }
        let e = canonicalize(&e);
        if e.size() != size || e.num_params() > grammar.max_params || !e.has_vars() && size > 1 {
            return;
        }
        if let Some(dims) = &grammar.var_dims {
            if dimension_of(&e, dims) == Dimension::Invalid {
                return;
            }
        }
        if !seen_keys.insert(structural_key(&e)) {
            return;
        }
        if let Some(fp) = fingerprint(&e, sample, n_rows) {
            if !seen_values.insert(fp) {
                return;
            }
        } else if e.num_params() == 0 {
            // Not computable on the data (for example log of a negative column).
            return;
        }
        levels[size].push(e);
        *total += 1;
    };

    if grammar.max_size >= 1 {
        for i in 0..grammar.n_vars {
            accept(Expr::Var(i), 1, &mut levels, &mut total);
        }
        if grammar.max_params > 0 {
            accept(Expr::Param(0), 1, &mut levels, &mut total);
        }
    }

    for size in 2..=grammar.max_size {
        for &op in &grammar.unary {
            for i in 0..levels[size - 1].len() {
                let child = &levels[size - 1][i];
                if redundant_unary(op, child) {
                    continue;
                }
                let e = Expr::unary(op, child.clone());
                accept(e, size, &mut levels, &mut total);
            }
        }
        for left_size in 1..size - 1 {
            let right_size = size - 1 - left_size;
            for &op in &grammar.binary {
                if op.is_commutative() && left_size > right_size {
                    continue;
                }
                for i in 0..levels[left_size].len() {
                    for j in 0..levels[right_size].len() {
                        let (a, b) = (&levels[left_size][i], &levels[right_size][j]);
                        if op == BinaryOp::Pow && !matches!(b, Expr::Param(_) | Expr::Const(_)) {
                            continue;
                        }
                        if a.num_params() + b.num_params() > grammar.max_params {
                            continue;
                        }
                        let e = Expr::binary(op, a.clone(), b.clone());
                        accept(e, size, &mut levels, &mut total);
                    }
                }
            }
        }
        if total >= grammar.max_expressions {
            break;
        }
    }

    levels.into_iter().flatten().collect()
}

/// Unary applications that are never useful.
fn redundant_unary(op: UnaryOp, child: &Expr) -> bool {
    use UnaryOp::*;
    match child {
        Expr::Unary(inner, _) => matches!(
            (op, *inner),
            (Neg, Neg)
                | (Sqrt, Square)
                | (Square, Sqrt)
                | (Log, Exp)
                | (Exp, Log)
                | (Exp, Exp)
                | (Log, Log)
        ),
        _ => false,
    }
}

/// Hash of the values `e` takes on the sample rows, with parameters set to
/// fixed probe values. `None` if any value is not finite.
fn fingerprint(e: &Expr, sample: &[Vec<f64>], n_rows: usize) -> Option<u64> {
    let params: Vec<f64> = (0..e.num_params())
        .map(|i| PROBE[i % PROBE.len()])
        .collect();
    let values = e.eval(sample, &params, n_rows);
    let mut h = DefaultHasher::new();
    for v in values {
        if !v.is_finite() {
            return None;
        }
        quantize(v).hash(&mut h);
    }
    Some(h.finish())
}

/// Rounds to about 9 significant digits so that tiny floating point
/// differences (`a*b/c` vs `a/c*b`) do not matter.
fn quantize(v: f64) -> (i32, i64) {
    if v == 0.0 {
        return (0, 0);
    }
    let exp = v.abs().log10().floor() as i32;
    let mantissa = (v / 10f64.powi(exp) * 1e8).round() as i64;
    (exp, mantissa)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grammar(max_size: usize) -> Grammar {
        Grammar {
            n_vars: 2,
            unary: vec![UnaryOp::Square, UnaryOp::Sqrt],
            binary: vec![BinaryOp::Add, BinaryOp::Mul, BinaryOp::Sub, BinaryOp::Div],
            max_size,
            max_params: 1,
            max_expressions: 100_000,
            var_dims: None,
        }
    }

    fn sample() -> Vec<Vec<f64>> {
        vec![vec![1.3, 2.7, 0.4], vec![0.9, 3.1, 5.2]]
    }

    #[test]
    fn produces_no_duplicates() {
        let exprs = enumerate(&grammar(5), &sample());
        let keys: HashSet<String> = exprs.iter().map(structural_key).collect();
        assert_eq!(keys.len(), exprs.len());
        // x0 + x1 and x1 + x0 must not both appear.
        let sums = exprs
            .iter()
            .filter(|e| e.to_string() == "x0 + x1" || e.to_string() == "x1 + x0")
            .count();
        assert_eq!(sums, 1);
    }

    #[test]
    fn respects_size_and_parameter_limits() {
        let exprs = enumerate(&grammar(4), &sample());
        assert!(exprs.iter().all(|e| e.size() <= 4 && e.num_params() <= 1));
        assert!(exprs.iter().any(|e| e.to_string() == "c0 * x0"));
        assert!(exprs.iter().any(|e| e.to_string() == "x0 * x1"));
    }

    #[test]
    fn numerically_equal_expressions_are_merged() {
        // x0 * x0 and x0^2 have the same values: only one survives.
        let exprs = enumerate(&grammar(3), &sample());
        let squares = exprs
            .iter()
            .filter(|e| matches!(e.to_string().as_str(), "x0^2" | "x0 * x0"))
            .count();
        assert_eq!(squares, 1);
    }

    #[test]
    fn units_prune_impossible_expressions() {
        use crate::units::parse_unit;
        let free = enumerate(&grammar(4), &sample());
        let g = Grammar {
            var_dims: Some(vec![
                parse_unit("m").unwrap().dim,
                parse_unit("s").unwrap().dim,
            ]),
            ..grammar(4)
        };
        let typed = enumerate(&g, &sample());
        assert!(typed.len() < free.len());
        assert!(!typed.iter().any(|e| e.to_string() == "x0 + x1"));
        assert!(typed.iter().any(|e| e.to_string() == "x0 / x1"));
    }

    #[test]
    fn invalid_expressions_are_skipped() {
        let sample = vec![vec![-1.0, 2.0]];
        let g = Grammar {
            n_vars: 1,
            ..grammar(2)
        };
        let exprs = enumerate(&g, &sample);
        assert!(!exprs.iter().any(|e| e.to_string() == "sqrt(x0)"));
    }
}
