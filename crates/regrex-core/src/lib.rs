//! Regrex core: expressions and their evaluation over data columns.
//!
//! This is the starting point of the engine. Search, scoring, units and
//! known-formula detection are planned in `docs/ROADMAP.md`.

/// A symbolic expression over input variables and constants.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Var(usize),
    Const(f64),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Evaluates the expression for one row of input values.
    pub fn eval(&self, row: &[f64]) -> f64 {
        match self {
            Expr::Var(i) => row[*i],
            Expr::Const(c) => *c,
            Expr::Add(a, b) => a.eval(row) + b.eval(row),
            Expr::Sub(a, b) => a.eval(row) - b.eval(row),
            Expr::Mul(a, b) => a.eval(row) * b.eval(row),
            Expr::Div(a, b) => a.eval(row) / b.eval(row),
        }
    }

    /// Number of nodes, used as a first complexity measure.
    pub fn size(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) => 1,
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                1 + a.size() + b.size()
            }
        }
    }
}

/// Mean squared error of `expr` against `target` over `rows`.
pub fn mse(expr: &Expr, rows: &[Vec<f64>], target: &[f64]) -> f64 {
    let n = rows.len().max(1) as f64;
    rows.iter()
        .zip(target)
        .map(|(row, y)| (expr.eval(row) - y).powi(2))
        .sum::<f64>()
        / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_with_vat() {
        // Total = Price * Quantity * 1.21
        let expr = Expr::Mul(
            Box::new(Expr::Mul(Box::new(Expr::Var(0)), Box::new(Expr::Var(1)))),
            Box::new(Expr::Const(1.21)),
        );
        let rows = vec![vec![10.0, 2.0], vec![5.5, 4.0]];
        let target = vec![24.2, 26.62];
        assert!(mse(&expr, &rows, &target) < 1e-9);
        assert_eq!(expr.size(), 5);
    }
}
