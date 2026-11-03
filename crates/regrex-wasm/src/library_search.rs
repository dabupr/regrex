//! Searching only among the formulas of the built-in library.
//!
//! Instead of enumerating every formula, each library formula (optionally
//! of one domain) is tried with every way of giving its variables by
//! columns. One variable may instead be a constant to fit, such as the tax
//! rate in `tax = p * t`. This is fast and only finds formulas with a name,
//! which is what someone checking a spreadsheet often wants.

use regrex_core::simplify::canonicalize;
use regrex_core::{
    check_formula, fit_shape, BinaryOp, DerivedColumn, Expr, ScanConfig, Table, UnaryOp,
};
use regrex_formulas::{Expr as LibExpr, Formula, Func, Library, Op};

/// Formulas with more variables than this are skipped: the number of ways
/// to assign columns grows too fast.
const MAX_VARIABLES: usize = 4;

/// How a variable of a library formula is given.
#[derive(Clone, Copy, PartialEq)]
enum Given {
    Column(usize),
    /// A constant fitted to the data.
    Fitted,
}

/// The best library formula for column `target` of `table`, among the
/// formulas of `domain` (every domain when `None`), as a derived column.
/// Returns `None` when no formula matches enough rows.
pub fn best_library_formula(
    table: &Table,
    target: usize,
    domain: Option<&str>,
    config: &ScanConfig,
) -> Option<DerivedColumn> {
    let columns: Vec<usize> = (0..table.columns.len()).filter(|&c| c != target).collect();
    let mut best: Option<(Key, DerivedColumn)> = None;
    for formula in Library::builtin().formulas.iter() {
        if domain.is_some_and(|d| d != formula.domain) || formula.variables.len() > MAX_VARIABLES {
            continue;
        }
        for given in assignments(formula.variables.len(), &columns) {
            let Some(shape) = to_core(&formula.expr, &given) else {
                break; // uses a function the engine does not have
            };
            let found = if given.contains(&Given::Fitted) {
                fit_shape(table, target, &shape, config)
            } else {
                Some(check_formula(table, target, &shape, config))
            };
            let Some(found) = found else { continue };
            let key = Key {
                matched: found.matched,
                names: name_score(formula, &given, table, target),
                columns: given
                    .iter()
                    .filter(|g| matches!(g, Given::Column(_)))
                    .count(),
                complexity: found.complexity,
            };
            if best.as_ref().is_none_or(|(k, _)| key.better_than(k)) {
                best = Some((key, found));
            }
        }
    }
    let (_, found) = best?;
    // Write it the way the search would (`0.21 * net`, not `net * 0.21`).
    let found = check_formula(table, target, &canonicalize(&found.formula), config);
    let enough =
        found.checked > 0 && found.matched as f64 >= config.min_match * found.checked as f64;
    (enough && found.formula.has_vars() && found.matched > found.formula.complexity())
        .then_some(found)
}

/// How good a match is: most rows matched, then the simplest formula, then
/// the most column names agreeing with the formula, then the fewest fitted
/// constants.
struct Key {
    /// Rows the formula matches.
    matched: usize,
    /// Column names that agree with the formula's names and aliases.
    names: usize,
    /// Variables given by columns rather than fitted.
    columns: usize,
    /// Size of the resulting formula.
    complexity: usize,
}

impl Key {
    fn better_than(&self, other: &Key) -> bool {
        let rank = |k: &Key| {
            (
                k.matched,
                std::cmp::Reverse(k.complexity),
                k.names,
                k.columns,
            )
        };
        rank(self) > rank(other)
    }
}

/// Every way to give `n` variables by distinct `columns`, with at most one
/// of them fitted instead (never all of them).
fn assignments(n: usize, columns: &[usize]) -> Vec<Vec<Given>> {
    fn extend(
        n: usize,
        columns: &[usize],
        fitted: bool,
        prefix: &mut Vec<Given>,
        out: &mut Vec<Vec<Given>>,
    ) {
        if prefix.len() == n {
            out.push(prefix.clone());
            return;
        }
        for &c in columns {
            if !prefix.contains(&Given::Column(c)) {
                prefix.push(Given::Column(c));
                extend(n, columns, fitted, prefix, out);
                prefix.pop();
            }
        }
        if !fitted && n > 1 {
            prefix.push(Given::Fitted);
            extend(n, columns, true, prefix, out);
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    if n > 0 {
        extend(n, columns, false, &mut Vec::new(), &mut out);
    }
    out
}

/// The library expression as an engine expression, with variables given by
/// `given`. `None` if it uses a function the engine does not support.
fn to_core(e: &LibExpr, given: &[Given]) -> Option<Expr> {
    Some(match e {
        LibExpr::Num(v) => Expr::Const(*v),
        LibExpr::Const { value, .. } => Expr::Const(*value),
        LibExpr::Var(i) => match given[*i] {
            Given::Column(c) => Expr::Var(c),
            Given::Fitted => Expr::Param(0),
        },
        LibExpr::Neg(a) => Expr::unary(UnaryOp::Neg, to_core(a, given)?),
        LibExpr::Binary(op, a, b) => {
            let op = match op {
                Op::Add => BinaryOp::Add,
                Op::Sub => BinaryOp::Sub,
                Op::Mul => BinaryOp::Mul,
                Op::Div => BinaryOp::Div,
                Op::Pow => BinaryOp::Pow,
            };
            Expr::binary(op, to_core(a, given)?, to_core(b, given)?)
        }
        LibExpr::Call(f, args) => {
            let [a] = args.as_slice() else { return None };
            let a = to_core(a, given)?;
            let ln = |base: f64| {
                Expr::binary(
                    BinaryOp::Div,
                    Expr::unary(UnaryOp::Log, a.clone()),
                    Expr::Const(base.ln()),
                )
            };
            match f {
                Func::Sqrt => Expr::unary(UnaryOp::Sqrt, a),
                Func::Cbrt => Expr::binary(BinaryOp::Pow, a, Expr::Const(1.0 / 3.0)),
                Func::Exp => Expr::unary(UnaryOp::Exp, a),
                Func::Ln => Expr::unary(UnaryOp::Log, a),
                Func::Log10 => ln(10.0),
                Func::Log2 => ln(2.0),
                Func::Sin => Expr::unary(UnaryOp::Sin, a),
                Func::Cos => Expr::unary(UnaryOp::Cos, a),
                _ => return None,
            }
        }
    })
}

/// How many columns (and the target) are named like the formula's symbol,
/// name or aliases for the quantity they give.
fn name_score(formula: &Formula, given: &[Given], table: &Table, target: usize) -> usize {
    let agrees = |q: &regrex_formulas::Quantity, column: usize| {
        let name = table.names[column].to_lowercase();
        name == q.symbol.to_lowercase()
            || name == q.name.to_lowercase()
            || q.aliases.iter().any(|a| a.to_lowercase() == name)
    };
    let inputs = formula
        .variables
        .iter()
        .zip(given)
        .filter(|(q, g)| matches!(g, Given::Column(c) if agrees(q, *c)))
        .count();
    inputs + usize::from(agrees(&formula.output, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(names: &[&str], columns: Vec<Vec<f64>>) -> Table {
        Table {
            names: names.iter().map(|s| s.to_string()).collect(),
            decimals: vec![Some(2); columns.len()],
            units: vec![None; columns.len()],
            columns,
        }
    }

    #[test]
    fn finds_a_library_formula_with_a_fitted_rate() {
        let net: Vec<f64> = (1..=20).map(|i| 10.0 + 3.0 * i as f64).collect();
        let vat: Vec<f64> = net
            .iter()
            .map(|n| (n * 0.21 * 100.0).round() / 100.0)
            .collect();
        let t = table(&["net", "vat"], vec![net, vat]);
        let found = best_library_formula(&t, 1, Some("finance"), &ScanConfig::default()).unwrap();
        assert_eq!(
            found.formula.display_with(&t.names).to_string(),
            "0.21 * net"
        );
        assert_eq!(found.matched, 20);
    }

    #[test]
    fn respects_the_domain() {
        let m: Vec<f64> = (1..=12).map(|i| i as f64).collect();
        let v: Vec<f64> = (1..=12).map(|i| 0.5 + i as f64 * 0.25).collect();
        let e: Vec<f64> = m
            .iter()
            .zip(&v)
            .map(|(m, v)| (0.5 * m * v * v * 100.0).round() / 100.0)
            .collect();
        let t = table(&["mass", "speed", "energy"], vec![m, v, e]);
        let found = best_library_formula(&t, 2, Some("physics"), &ScanConfig::default()).unwrap();
        assert_eq!(found.matched, 12);
        assert!(best_library_formula(&t, 2, Some("finance"), &ScanConfig::default()).is_none());
    }

    #[test]
    fn assigns_columns_and_one_fitted_constant() {
        // 2 variables over 3 columns: 3 * 2 ordered pairs, plus 2 * 3 with one fitted.
        assert_eq!(assignments(2, &[0, 1, 2]).len(), 12);
        assert!(assignments(1, &[0, 1])
            .iter()
            .all(|g| g != &[Given::Fitted]));
    }
}
