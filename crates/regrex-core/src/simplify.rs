//! Canonical forms, used to avoid evaluating the same formula twice.
//!
//! The rules are deliberately conservative: they only rewrite expressions
//! into others that are equal for every input (or, for free parameters, that
//! describe the same family of functions).

use crate::expr::{BinaryOp, Expr, UnaryOp};
use std::fmt::Write;

/// Returns a canonical version of `e`.
///
/// - constant subexpressions are folded,
/// - any operation whose inputs are all constants or parameters becomes a
///   single parameter (`c0 * 2 + c1` is just one free constant),
/// - `+` and `*` chains are flattened and sorted,
/// - `a - c` becomes `a + c` and `a / c` becomes `a * c` for parameters `c`,
/// - trivial identities (`x + 0`, `x * 1`, `x - x`, `x / x`, `-(-x)`) are removed.
///
/// Parameters are renumbered `c0, c1, ...` in the result.
pub fn canonicalize(e: &Expr) -> Expr {
    canon(e).number_params()
}

/// A string that is equal for two expressions iff their canonical trees are
/// equal (parameter numbers are ignored).
pub fn structural_key(e: &Expr) -> String {
    let mut s = String::new();
    write_key(e, &mut s);
    s
}

fn write_key(e: &Expr, s: &mut String) {
    match e {
        // The leading digit sorts terms in chains: constants, then variables,
        // then functions, then operations (`2 * m * v^2`).
        Expr::Const(c) => {
            let _ = write!(s, "0k{:x}", c.to_bits());
        }
        Expr::Param(_) => s.push_str("0c"),
        Expr::Var(i) => {
            let _ = write!(s, "1v{i}");
        }
        Expr::Unary(op, a) => {
            let _ = write!(s, "2u{}(", *op as u8);
            write_key(a, s);
            s.push(')');
        }
        Expr::Binary(op, a, b) => {
            let _ = write!(s, "3b{}(", *op as u8);
            write_key(a, s);
            s.push(',');
            write_key(b, s);
            s.push(')');
        }
    }
}

fn is_constant_like(e: &Expr) -> bool {
    matches!(e, Expr::Const(_) | Expr::Param(_))
}

fn canon(e: &Expr) -> Expr {
    match e {
        Expr::Var(_) | Expr::Const(_) | Expr::Param(_) => e.clone(),
        Expr::Unary(op, a) => canon_unary(*op, canon(a)),
        Expr::Binary(op, a, b) => canon_binary(*op, canon(a), canon(b)),
    }
}

fn canon_unary(op: UnaryOp, a: Expr) -> Expr {
    match (op, a) {
        (_, Expr::Const(c)) => {
            let v = op.apply(c);
            if v.is_finite() {
                Expr::Const(v)
            } else {
                Expr::unary(op, Expr::Const(c))
            }
        }
        (_, Expr::Param(_)) => Expr::Param(0),
        (UnaryOp::Neg, Expr::Unary(UnaryOp::Neg, inner)) => *inner,
        (op, a) => Expr::unary(op, a),
    }
}

fn canon_binary(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    // Fold or merge constants.
    if let (Expr::Const(x), Expr::Const(y)) = (&a, &b) {
        let v = op.apply(*x, *y);
        if v.is_finite() {
            return Expr::Const(v);
        }
    }
    if is_constant_like(&a)
        && is_constant_like(&b)
        && (matches!(a, Expr::Param(_)) || matches!(b, Expr::Param(_)))
    {
        return Expr::Param(0);
    }
    match op {
        BinaryOp::Add | BinaryOp::Mul => canon_chain(op, a, b),
        BinaryOp::Sub => {
            if structural_key(&a) == structural_key(&b) {
                Expr::Const(0.0)
            } else if matches!(b, Expr::Param(_)) {
                canon_chain(BinaryOp::Add, a, b)
            } else if matches!(b, Expr::Const(0.0)) {
                a
            } else {
                Expr::binary(op, a, b)
            }
        }
        BinaryOp::Div => {
            if structural_key(&a) == structural_key(&b) {
                Expr::Const(1.0)
            } else if matches!(b, Expr::Param(_)) {
                canon_chain(BinaryOp::Mul, a, b)
            } else if matches!(b, Expr::Const(c) if c == 1.0) {
                a
            } else {
                Expr::binary(op, a, b)
            }
        }
        BinaryOp::Pow => match b {
            Expr::Const(1.0) => a,
            Expr::Const(2.0) => Expr::unary(UnaryOp::Square, a),
            Expr::Const(0.5) => Expr::unary(UnaryOp::Sqrt, a),
            b => Expr::binary(op, a, b),
        },
    }
}

/// In a sorted product, replaces `t * t` by `t^2` (`m * v * v` → `m * v^2`).
fn merge_squares(sorted: Vec<(String, Expr)>) -> Vec<(String, Expr)> {
    let mut out: Vec<(String, Expr)> = Vec::with_capacity(sorted.len());
    let mut iter = sorted.into_iter().peekable();
    while let Some((key, term)) = iter.next() {
        if iter.peek().is_some_and(|(next, _)| *next == key) {
            iter.next();
            let sq = Expr::unary(UnaryOp::Square, term);
            out.push((structural_key(&sq), sq));
        } else {
            out.push((key, term));
        }
    }
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

fn flatten(op: BinaryOp, e: Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Binary(o, a, b) if o == op => {
            flatten(op, *a, out);
            flatten(op, *b, out);
        }
        other => out.push(other),
    }
}

/// Canonical form of an associative, commutative chain (`+` or `*`).
fn canon_chain(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    let mut terms = Vec::new();
    flatten(op, a, &mut terms);
    flatten(op, b, &mut terms);

    let identity = if op == BinaryOp::Add { 0.0 } else { 1.0 };
    let mut constant = identity;
    let mut has_param = false;
    let mut rest = Vec::with_capacity(terms.len());
    for t in terms {
        match t {
            Expr::Const(c) => constant = op.apply(constant, c),
            Expr::Param(_) => has_param = true,
            other => rest.push(other),
        }
    }
    if op == BinaryOp::Mul && constant == 0.0 && !has_param {
        return Expr::Const(0.0);
    }

    let mut keyed: Vec<(String, Expr)> =
        rest.into_iter().map(|t| (structural_key(&t), t)).collect();
    keyed.sort_by(|x, y| x.0.cmp(&y.0));
    if op == BinaryOp::Mul {
        keyed = merge_squares(keyed);
    }
    // A free parameter absorbs any fixed constant in the same chain.
    let constant_term = if has_param {
        Some(Expr::Param(0))
    } else if constant != identity || keyed.is_empty() {
        Some(Expr::Const(constant))
    } else {
        None
    };
    let mut parts: Vec<Expr> = Vec::with_capacity(keyed.len() + 1);
    // Constants lead products (`2 * x`) and end sums (`x + 1`).
    if op == BinaryOp::Mul {
        parts.extend(constant_term.clone());
    }
    parts.extend(keyed.into_iter().map(|(_, t)| t));
    if op == BinaryOp::Add {
        parts.extend(constant_term);
    }

    let mut iter = parts.into_iter();
    let first = iter.next().expect("a chain has at least one term");
    iter.fold(first, |acc, t| Expr::binary(op, acc, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(i: usize) -> Expr {
        Expr::Var(i)
    }
    fn c() -> Expr {
        Expr::Param(0)
    }
    fn add(a: Expr, b: Expr) -> Expr {
        Expr::binary(BinaryOp::Add, a, b)
    }
    fn mul(a: Expr, b: Expr) -> Expr {
        Expr::binary(BinaryOp::Mul, a, b)
    }

    #[test]
    fn commutative_chains_are_sorted() {
        let a = canonicalize(&add(v(1), v(0)));
        let b = canonicalize(&add(v(0), v(1)));
        assert_eq!(a, b);
        let a = canonicalize(&mul(v(2), mul(v(0), v(1))));
        let b = canonicalize(&mul(mul(v(1), v(2)), v(0)));
        assert_eq!(a, b);
    }

    #[test]
    fn repeated_factors_become_squares() {
        let e = canonicalize(&mul(mul(v(0), v(1)), v(1)));
        assert_eq!(e.to_string(), "x0 * x1^2");
    }

    #[test]
    fn parameters_absorb_constants() {
        let e = canonicalize(&mul(Expr::Const(2.0), mul(c(), v(0))));
        assert_eq!(e, mul(Expr::Param(0), v(0)));
        let e = canonicalize(&add(add(c(), v(0)), c()));
        assert_eq!(e, add(v(0), Expr::Param(0)));
        let e = canonicalize(&Expr::unary(UnaryOp::Exp, c()));
        assert_eq!(e, Expr::Param(0));
    }

    #[test]
    fn identities_are_removed() {
        assert_eq!(
            canonicalize(&Expr::binary(BinaryOp::Sub, v(0), v(0))),
            Expr::Const(0.0)
        );
        assert_eq!(
            canonicalize(&Expr::binary(BinaryOp::Div, v(1), v(1))),
            Expr::Const(1.0)
        );
        assert_eq!(canonicalize(&add(v(0), Expr::Const(0.0))), v(0));
        assert_eq!(canonicalize(&mul(Expr::Const(1.0), v(0))), v(0));
        let neg = |e| Expr::unary(UnaryOp::Neg, e);
        assert_eq!(canonicalize(&neg(neg(v(0)))), v(0));
    }

    #[test]
    fn subtracting_or_dividing_by_a_parameter_is_the_same_family() {
        let sub = canonicalize(&Expr::binary(BinaryOp::Sub, v(0), c()));
        assert_eq!(sub, canonicalize(&add(c(), v(0))));
        let div = canonicalize(&Expr::binary(BinaryOp::Div, v(0), c()));
        assert_eq!(div, canonicalize(&mul(v(0), c())));
    }

    #[test]
    fn constants_are_folded() {
        let e = add(mul(Expr::Const(2.0), Expr::Const(3.0)), v(0));
        assert_eq!(canonicalize(&e), add(v(0), Expr::Const(6.0)));
    }

    #[test]
    fn parameters_are_renumbered() {
        let e = canonicalize(&add(mul(c(), v(0)), mul(c(), v(1))));
        assert_eq!(e.num_params(), 2);
        assert_eq!(e, add(mul(Expr::Param(0), v(0)), mul(Expr::Param(1), v(1))));
    }
}
