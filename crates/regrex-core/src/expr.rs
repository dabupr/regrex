//! Expression trees: construction, evaluation and display.

use std::fmt;

/// Operators that take one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnaryOp {
    /// `-x`
    Neg,
    /// `x^2`
    Square,
    /// `sqrt(x)`
    Sqrt,
    /// `exp(x)`
    Exp,
    /// Natural logarithm, `log(x)`
    Log,
    /// `sin(x)`
    Sin,
    /// `cos(x)`
    Cos,
}

impl UnaryOp {
    /// Every unary operator.
    pub const ALL: [UnaryOp; 7] = [
        UnaryOp::Neg,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ];

    /// Applies the operator to a value.
    #[inline]
    pub fn apply(self, x: f64) -> f64 {
        match self {
            UnaryOp::Neg => -x,
            UnaryOp::Square => x * x,
            UnaryOp::Sqrt => x.sqrt(),
            UnaryOp::Exp => x.exp(),
            UnaryOp::Log => x.ln(),
            UnaryOp::Sin => x.sin(),
            UnaryOp::Cos => x.cos(),
        }
    }

    /// Name used when the operator is displayed as a function.
    pub fn name(self) -> &'static str {
        match self {
            UnaryOp::Neg => "neg",
            UnaryOp::Square => "square",
            UnaryOp::Sqrt => "sqrt",
            UnaryOp::Exp => "exp",
            UnaryOp::Log => "log",
            UnaryOp::Sin => "sin",
            UnaryOp::Cos => "cos",
        }
    }
}

/// Operators that take two arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BinaryOp {
    /// `a + b`
    Add,
    /// `a - b`
    Sub,
    /// `a * b`
    Mul,
    /// `a / b`
    Div,
    /// `a ^ b`
    Pow,
}

impl BinaryOp {
    /// Every binary operator.
    pub const ALL: [BinaryOp; 5] = [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Pow,
    ];

    /// Applies the operator to two values.
    #[inline]
    pub fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            BinaryOp::Add => a + b,
            BinaryOp::Sub => a - b,
            BinaryOp::Mul => a * b,
            BinaryOp::Div => a / b,
            BinaryOp::Pow => a.powf(b),
        }
    }

    /// Symbol used for display.
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
        }
    }

    /// Whether `a op b == b op a`.
    pub fn is_commutative(self) -> bool {
        matches!(self, BinaryOp::Add | BinaryOp::Mul)
    }

    fn precedence(self) -> u8 {
        match self {
            BinaryOp::Add | BinaryOp::Sub => 1,
            BinaryOp::Mul | BinaryOp::Div => 2,
            BinaryOp::Pow => 3,
        }
    }
}

/// A symbolic expression.
///
/// Variables refer to input columns by index. [`Expr::Param`] is a free
/// constant whose value is found by fitting; [`Expr::Const`] is a fixed number.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Input column `i`.
    Var(usize),
    /// A fixed number.
    Const(f64),
    /// Free constant number `i`, fitted to the data.
    Param(usize),
    /// A unary operation.
    Unary(UnaryOp, Box<Expr>),
    /// A binary operation.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Builds a unary node.
    pub fn unary(op: UnaryOp, a: Expr) -> Expr {
        Expr::Unary(op, Box::new(a))
    }

    /// Builds a binary node.
    pub fn binary(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    /// Number of nodes. This is the complexity measure used for ranking.
    pub fn size(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) | Expr::Param(_) => 1,
            Expr::Unary(_, a) => 1 + a.size(),
            Expr::Binary(_, a, b) => 1 + a.size() + b.size(),
        }
    }

    /// Weighted size used to rank formulas.
    ///
    /// Variables, `+`, `-`, `*` and `-x` cost 1; `/`, `^`, `x^2`, `sqrt` and
    /// the transcendental functions cost 2. This prefers `0.5 * m * v^2`
    /// over the equivalent `m / (2 / v^2)`.
    ///
    /// Numbers cost more the more digits they need (see
    /// [`constant_complexity`]), so `2 * x + 1` beats `2.001 * x + 0.995`.
    pub fn complexity(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Param(_) => 1,
            Expr::Const(c) => constant_complexity(*c),
            Expr::Unary(op, a) => {
                let w = if *op == UnaryOp::Neg { 1 } else { 2 };
                w + a.complexity()
            }
            Expr::Binary(op, a, b) => {
                let w = match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => 1,
                    BinaryOp::Div | BinaryOp::Pow => 2,
                };
                w + a.complexity() + b.complexity()
            }
        }
    }

    /// Number of [`Expr::Param`] nodes.
    pub fn num_params(&self) -> usize {
        match self {
            Expr::Param(_) => 1,
            Expr::Var(_) | Expr::Const(_) => 0,
            Expr::Unary(_, a) => a.num_params(),
            Expr::Binary(_, a, b) => a.num_params() + b.num_params(),
        }
    }

    /// Whether the expression uses at least one input column.
    pub fn has_vars(&self) -> bool {
        match self {
            Expr::Var(_) => true,
            Expr::Const(_) | Expr::Param(_) => false,
            Expr::Unary(_, a) => a.has_vars(),
            Expr::Binary(_, a, b) => a.has_vars() || b.has_vars(),
        }
    }

    /// Renumbers parameters as `0, 1, 2, ...` in depth-first order.
    pub fn number_params(&self) -> Expr {
        fn go(e: &Expr, next: &mut usize) -> Expr {
            match e {
                Expr::Param(_) => {
                    let i = *next;
                    *next += 1;
                    Expr::Param(i)
                }
                Expr::Var(_) | Expr::Const(_) => e.clone(),
                Expr::Unary(op, a) => Expr::unary(*op, go(a, next)),
                Expr::Binary(op, a, b) => {
                    let a = go(a, next);
                    let b = go(b, next);
                    Expr::binary(*op, a, b)
                }
            }
        }
        go(self, &mut 0)
    }

    /// Replaces every parameter by its value.
    pub fn substitute_params(&self, params: &[f64]) -> Expr {
        match self {
            Expr::Param(i) => Expr::Const(params[*i]),
            Expr::Var(_) | Expr::Const(_) => self.clone(),
            Expr::Unary(op, a) => Expr::unary(*op, a.substitute_params(params)),
            Expr::Binary(op, a, b) => Expr::binary(
                *op,
                a.substitute_params(params),
                b.substitute_params(params),
            ),
        }
    }

    /// Evaluates the expression for a single row of inputs.
    pub fn eval_row(&self, row: &[f64], params: &[f64]) -> f64 {
        match self {
            Expr::Var(i) => row[*i],
            Expr::Const(c) => *c,
            Expr::Param(i) => params[*i],
            Expr::Unary(op, a) => op.apply(a.eval_row(row, params)),
            Expr::Binary(op, a, b) => op.apply(a.eval_row(row, params), b.eval_row(row, params)),
        }
    }

    /// Evaluates the expression over whole columns.
    ///
    /// `columns[i]` holds the values of input `i`; all columns must have
    /// `n_rows` values.
    pub fn eval(&self, columns: &[Vec<f64>], params: &[f64], n_rows: usize) -> Vec<f64> {
        match self {
            Expr::Var(i) => columns[*i].clone(),
            Expr::Const(c) => vec![*c; n_rows],
            Expr::Param(i) => vec![params[*i]; n_rows],
            Expr::Unary(op, a) => {
                let mut v = a.eval(columns, params, n_rows);
                for x in &mut v {
                    *x = op.apply(*x);
                }
                v
            }
            Expr::Binary(op, a, b) => {
                let mut va = a.eval(columns, params, n_rows);
                let vb = b.eval(columns, params, n_rows);
                for (x, y) in va.iter_mut().zip(&vb) {
                    *x = op.apply(*x, *y);
                }
                va
            }
        }
    }

    /// Displays the expression using the given variable names.
    ///
    /// Variables without a name are shown as `x0`, `x1`, ...
    pub fn display_with<'a>(&'a self, names: &'a [String]) -> ExprDisplay<'a> {
        ExprDisplay {
            expr: self,
            names: Some(names),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_expr(f, self, None)
    }
}

/// Helper returned by [`Expr::display_with`].
pub struct ExprDisplay<'a> {
    expr: &'a Expr,
    names: Option<&'a [String]>,
}

impl fmt::Display for ExprDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_expr(f, self.expr, self.names)
    }
}

/// `e` with its leading negative constant made positive, for terms like
/// `-3`, `-2 * x` or `-3 / a` (the minus is then written as a subtraction).
fn negated_leading(e: &Expr) -> Option<Expr> {
    match e {
        Expr::Const(c) if *c < 0.0 => Some(Expr::Const(-c)),
        Expr::Binary(op @ (BinaryOp::Mul | BinaryOp::Div), a, b) => {
            negated_leading(a).map(|na| Expr::binary(*op, na, (**b).clone()))
        }
        _ => None,
    }
}

/// Binding strength used to decide where parentheses are needed.
fn binding(e: &Expr) -> u8 {
    match e {
        Expr::Binary(op, _, _) => op.precedence(),
        Expr::Unary(UnaryOp::Neg, _) => 2,
        Expr::Const(c) if *c < 0.0 => 2,
        _ => 5,
    }
}

fn write_operand(
    f: &mut fmt::Formatter<'_>,
    e: &Expr,
    names: Option<&[String]>,
    min: u8,
) -> fmt::Result {
    if binding(e) < min {
        write!(f, "(")?;
        write_expr(f, e, names)?;
        write!(f, ")")
    } else {
        write_expr(f, e, names)
    }
}

fn write_expr(f: &mut fmt::Formatter<'_>, e: &Expr, names: Option<&[String]>) -> fmt::Result {
    match e {
        Expr::Var(i) => match names.and_then(|n| n.get(*i)) {
            Some(name) => write!(f, "{name}"),
            None => write!(f, "x{i}"),
        },
        Expr::Const(c) => write!(f, "{}", format_number(*c)),
        Expr::Param(i) => write!(f, "c{i}"),
        Expr::Unary(UnaryOp::Neg, a) => {
            write!(f, "-")?;
            write_operand(f, a, names, 3)
        }
        Expr::Unary(UnaryOp::Square, a) => {
            write_operand(f, a, names, 4)?;
            write!(f, "^2")
        }
        Expr::Unary(op, a) => {
            write!(f, "{}(", op.name())?;
            write_expr(f, a, names)?;
            write!(f, ")")
        }
        // Show `a + (-3)` as `a - 3` and `a + -2 * b` as `a - 2 * b`.
        Expr::Binary(BinaryOp::Add, a, b) if negated_leading(b).is_some() => {
            let nb = negated_leading(b).expect("checked");
            write_operand(f, a, names, 1)?;
            write!(f, " - ")?;
            write_operand(f, &nb, names, 2)
        }
        Expr::Binary(BinaryOp::Add, a, b) if negated_leading(a).is_some() => {
            let na = negated_leading(a).expect("checked");
            write_operand(f, b, names, 1)?;
            write!(f, " - ")?;
            write_operand(f, &na, names, 2)
        }
        Expr::Binary(op, a, b) => {
            let p = op.precedence();
            let (left_min, right_min) = match op {
                BinaryOp::Add | BinaryOp::Mul => (p, p),
                BinaryOp::Sub | BinaryOp::Div => (p, p + 1),
                BinaryOp::Pow => (p + 1, p),
            };
            write_operand(f, a, names, left_min)?;
            if *op == BinaryOp::Pow {
                write!(f, "^")?;
            } else {
                write!(f, " {} ", op.symbol())?;
            }
            write_operand(f, b, names, right_min)
        }
    }
}

/// Cost of writing down a number: 1 for numbers with at most two
/// significant digits (`2`, `0.5`, `12`) and for `π` and `e`, 2 for up to four
/// digits (`1.21`, `8.314`), 3 otherwise.
pub fn constant_complexity(c: f64) -> usize {
    let text = format_number(c);
    let digits = text
        .split(['e', 'E'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    let significant = digits.trim_start_matches('0').trim_end_matches('0').len();
    match significant {
        0..=2 => 1,
        3..=4 => 2,
        _ => 3,
    }
}

/// Formats a number compactly: integers without decimals, others with up to
/// 6 significant digits, and `π` / `e` by name.
pub fn format_number(c: f64) -> String {
    if c == std::f64::consts::PI {
        return "π".to_string();
    }
    if c == std::f64::consts::E {
        return "e".to_string();
    }
    if !c.is_finite() {
        return format!("{c}");
    }
    if c.fract() == 0.0 && c.abs() < 1e15 {
        return format!("{}", c as i64);
    }
    let magnitude = c.abs().log10().floor() as i32;
    if !(-4..=14).contains(&magnitude) {
        return format!("{c:.5e}");
    }
    let decimals = (5 - magnitude).clamp(0, 12) as usize;
    let s = format!("{c:.decimals$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(i: usize) -> Expr {
        Expr::Var(i)
    }

    #[test]
    fn eval_row_and_columns_agree() {
        // (x0 + 2) * sqrt(x1)
        let e = Expr::binary(
            BinaryOp::Mul,
            Expr::binary(BinaryOp::Add, v(0), Expr::Const(2.0)),
            Expr::unary(UnaryOp::Sqrt, v(1)),
        );
        let cols = vec![vec![1.0, 2.0], vec![4.0, 9.0]];
        let out = e.eval(&cols, &[], 2);
        assert_eq!(out, vec![6.0, 12.0]);
        assert_eq!(e.eval_row(&[1.0, 4.0], &[]), 6.0);
        assert_eq!(e.size(), 6);
    }

    #[test]
    fn params_are_numbered_and_substituted() {
        let e = Expr::binary(BinaryOp::Add, Expr::Param(7), Expr::Param(7)).number_params();
        assert_eq!(
            e,
            Expr::binary(BinaryOp::Add, Expr::Param(0), Expr::Param(1))
        );
        assert_eq!(e.num_params(), 2);
        let s = e.substitute_params(&[1.5, 2.0]);
        assert_eq!(s.eval_row(&[], &[]), 3.5);
    }

    #[test]
    fn display_uses_minimal_parentheses() {
        let names = vec!["m".to_string(), "v".to_string()];
        let e = Expr::binary(
            BinaryOp::Mul,
            Expr::binary(BinaryOp::Mul, Expr::Const(0.5), v(0)),
            Expr::unary(UnaryOp::Square, v(1)),
        );
        assert_eq!(e.display_with(&names).to_string(), "0.5 * m * v^2");

        let e = Expr::binary(
            BinaryOp::Sub,
            v(0),
            Expr::binary(BinaryOp::Add, v(1), Expr::Const(-3.0)),
        );
        assert_eq!(e.to_string(), "x0 - (x1 - 3)");

        let e = Expr::binary(
            BinaryOp::Pow,
            Expr::binary(BinaryOp::Add, v(0), v(1)),
            Expr::Const(-1.0),
        );
        assert_eq!(e.to_string(), "(x0 + x1)^(-1)");

        let e = Expr::unary(UnaryOp::Square, Expr::unary(UnaryOp::Neg, v(0)));
        assert_eq!(e.to_string(), "(-x0)^2");
    }

    #[test]
    fn rounder_numbers_are_simpler() {
        assert_eq!(constant_complexity(2.0), 1);
        assert_eq!(constant_complexity(0.5), 1);
        assert_eq!(constant_complexity(200.0), 1);
        assert_eq!(constant_complexity(std::f64::consts::PI), 1);
        assert_eq!(constant_complexity(1.21), 2);
        assert_eq!(constant_complexity(8.314), 2);
        assert_eq!(constant_complexity(2.00607), 3);
        let simple = Expr::binary(BinaryOp::Mul, Expr::Const(2.0), Expr::Var(0));
        let precise = Expr::binary(BinaryOp::Mul, Expr::Const(2.001), Expr::Var(0));
        assert!(simple.complexity() < precise.complexity());
    }

    #[test]
    fn numbers_are_formatted_compactly() {
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(1.21), "1.21");
        assert_eq!(format_number(9.80665), "9.80665");
        assert_eq!(format_number(1.0 / 3.0), "0.333333");
        assert_eq!(format_number(std::f64::consts::PI), "π");
        assert_eq!(format_number(6.02214076e23), "6.02214e23");
    }
}
