//! Formula expressions: parsing, evaluation and dimensional analysis.
//!
//! The syntax is the usual calculator notation:
//!
//! - numbers: `2`, `0.5`, `6.022e23`;
//! - names: variables of the formula (`m`, `v_0`, `ω`) or library constants
//!   by id (`pi`, `g`, `k_B`); a variable hides a constant with the same name;
//! - operators: `+ - * / ^` and unary `-`, with the usual precedence (`^`
//!   binds tightest and is right-associative, so `-x^2` is `-(x^2)`);
//! - functions: see [`Func`].

use std::fmt;

use crate::units::Dimension;

/// Built-in functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Func {
    /// Square root.
    Sqrt,
    /// Cube root.
    Cbrt,
    /// Exponential.
    Exp,
    /// Natural logarithm (`ln` or `log`).
    Ln,
    /// Base-10 logarithm.
    Log10,
    /// Base-2 logarithm.
    Log2,
    /// Sine (radians).
    Sin,
    /// Cosine (radians).
    Cos,
    /// Tangent (radians).
    Tan,
    /// Arcsine.
    Asin,
    /// Arccosine.
    Acos,
    /// Arctangent.
    Atan,
    /// Two-argument arctangent `atan2(y, x)`.
    Atan2,
    /// Hyperbolic sine.
    Sinh,
    /// Hyperbolic cosine.
    Cosh,
    /// Hyperbolic tangent.
    Tanh,
    /// Absolute value.
    Abs,
    /// Round down.
    Floor,
    /// Round up.
    Ceil,
}

impl Func {
    /// Every function.
    pub const ALL: [Func; 19] = [
        Func::Sqrt,
        Func::Cbrt,
        Func::Exp,
        Func::Ln,
        Func::Log10,
        Func::Log2,
        Func::Sin,
        Func::Cos,
        Func::Tan,
        Func::Asin,
        Func::Acos,
        Func::Atan,
        Func::Atan2,
        Func::Sinh,
        Func::Cosh,
        Func::Tanh,
        Func::Abs,
        Func::Floor,
        Func::Ceil,
    ];

    /// Name used in formulas.
    pub fn name(self) -> &'static str {
        match self {
            Func::Sqrt => "sqrt",
            Func::Cbrt => "cbrt",
            Func::Exp => "exp",
            Func::Ln => "ln",
            Func::Log10 => "log10",
            Func::Log2 => "log2",
            Func::Sin => "sin",
            Func::Cos => "cos",
            Func::Tan => "tan",
            Func::Asin => "asin",
            Func::Acos => "acos",
            Func::Atan => "atan",
            Func::Atan2 => "atan2",
            Func::Sinh => "sinh",
            Func::Cosh => "cosh",
            Func::Tanh => "tanh",
            Func::Abs => "abs",
            Func::Floor => "floor",
            Func::Ceil => "ceil",
        }
    }

    /// Looks a function up by name (`log` is the natural logarithm).
    pub fn from_name(name: &str) -> Option<Func> {
        if name == "log" {
            return Some(Func::Ln);
        }
        Func::ALL.into_iter().find(|f| f.name() == name)
    }

    /// Number of arguments.
    pub fn arity(self) -> usize {
        if self == Func::Atan2 {
            2
        } else {
            1
        }
    }

    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Func::Sqrt => a.sqrt(),
            Func::Cbrt => a.cbrt(),
            Func::Exp => a.exp(),
            Func::Ln => a.ln(),
            Func::Log10 => a.log10(),
            Func::Log2 => a.log2(),
            Func::Sin => a.sin(),
            Func::Cos => a.cos(),
            Func::Tan => a.tan(),
            Func::Asin => a.asin(),
            Func::Acos => a.acos(),
            Func::Atan => a.atan(),
            Func::Atan2 => a.atan2(b),
            Func::Sinh => a.sinh(),
            Func::Cosh => a.cosh(),
            Func::Tanh => a.tanh(),
            Func::Abs => a.abs(),
            Func::Floor => a.floor(),
            Func::Ceil => a.ceil(),
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Op {
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

impl Op {
    /// Symbol used in formulas.
    pub fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Pow => "^",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Op::Add | Op::Sub => 1,
            Op::Mul | Op::Div => 2,
            Op::Pow => 4,
        }
    }
}

/// A parsed formula expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A number written in the formula.
    Num(f64),
    /// Variable `i` of the formula (index into its variable list).
    Var(usize),
    /// A library constant.
    Const {
        /// Constant id, as written in the formula.
        id: String,
        /// Its value in SI units.
        value: f64,
        /// Its dimension.
        dimension: Dimension,
    },
    /// Unary minus.
    Neg(Box<Expr>),
    /// Binary operation.
    Binary(Op, Box<Expr>, Box<Expr>),
    /// Function call.
    Call(Func, Vec<Expr>),
}

/// What a name in a formula refers to.
#[derive(Debug, Clone, PartialEq)]
pub enum Name {
    /// Variable number `i`.
    Var(usize),
    /// Library constant with the given SI value and dimension.
    Const(f64, Dimension),
}

impl Expr {
    /// Parses `text`, resolving each name with `resolve`.
    pub fn parse(text: &str, resolve: impl Fn(&str) -> Option<Name>) -> Result<Expr, String> {
        let tokens = tokenize(text)?;
        let mut p = Parser {
            tokens,
            pos: 0,
            resolve: &resolve,
        };
        let e = p.expr(0)?;
        if let Some(t) = p.tokens.get(p.pos) {
            return Err(format!("unexpected {t}"));
        }
        Ok(e)
    }

    /// Evaluates the expression with the given variable values (SI units).
    pub fn eval(&self, vars: &[f64]) -> f64 {
        match self {
            Expr::Num(x) => *x,
            Expr::Var(i) => vars[*i],
            Expr::Const { value, .. } => *value,
            Expr::Neg(a) => -a.eval(vars),
            Expr::Binary(op, a, b) => {
                let (a, b) = (a.eval(vars), b.eval(vars));
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Pow => a.powf(b),
                }
            }
            Expr::Call(f, args) => {
                let a = args[0].eval(vars);
                let b = args.get(1).map_or(0.0, |b| b.eval(vars));
                f.apply(a, b)
            }
        }
    }

    /// Indices of the variables used, in order of first use.
    pub fn variables(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.visit(&mut |e| {
            if let Expr::Var(i) = e {
                if !out.contains(i) {
                    out.push(*i);
                }
            }
        });
        out
    }

    /// Ids of the library constants used, in order of first use.
    pub fn constants(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        self.visit_ref(&mut |e| {
            if let Expr::Const { id, .. } = e {
                if !out.contains(&id.as_str()) {
                    out.push(id);
                }
            }
        });
        out
    }

    /// Names of the operators and functions used, once per occurrence
    /// (`"+"`, `"neg"`, `"sqrt"`, ...). Useful to build per-domain priors.
    pub fn operators(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        self.visit(&mut |e| match e {
            Expr::Neg(_) => out.push("neg"),
            Expr::Binary(op, _, _) => out.push(op.symbol()),
            Expr::Call(f, _) => out.push(f.name()),
            _ => {}
        });
        out
    }

    fn visit(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Neg(a) => a.visit(f),
            Expr::Binary(_, a, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit(f)),
            _ => {}
        }
    }

    fn visit_ref<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::Neg(a) => a.visit_ref(f),
            Expr::Binary(_, a, b) => {
                a.visit_ref(f);
                b.visit_ref(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit_ref(f)),
            _ => {}
        }
    }

    /// Dimension of the expression given the dimensions of the variables.
    ///
    /// Plain numbers in a sum take the dimension of the other term, so unit
    /// conversions such as `1.8 * T + 32` pass. Everything else must be
    /// consistent: both sides of `+` agree, arguments of `exp`, `ln` and the
    /// trigonometric functions are dimensionless, and a quantity with a
    /// dimension is only raised to a fixed numeric power.
    pub fn dimension(&self, vars: &[Dimension]) -> Result<Dimension, String> {
        self.dim(vars).map(|d| d.dim)
    }

    fn dim(&self, vars: &[Dimension]) -> Result<DimInfo, String> {
        Ok(match self {
            Expr::Num(x) => DimInfo {
                dim: Dimension::NONE,
                number: Some(*x),
            },
            Expr::Var(i) => DimInfo::of(vars[*i]),
            Expr::Const { dimension, .. } => DimInfo::of(*dimension),
            Expr::Neg(a) => {
                let a = a.dim(vars)?;
                DimInfo {
                    dim: a.dim,
                    number: a.number.map(|x| -x),
                }
            }
            Expr::Binary(op, a, b) => {
                let (da, db) = (a.dim(vars)?, b.dim(vars)?);
                let number = match (da.number, db.number) {
                    (Some(x), Some(y)) => Some(match op {
                        Op::Add => x + y,
                        Op::Sub => x - y,
                        Op::Mul => x * y,
                        Op::Div => x / y,
                        Op::Pow => x.powf(y),
                    }),
                    _ => None,
                };
                let dim = match op {
                    Op::Add | Op::Sub => {
                        if da.number.is_some() {
                            db.dim
                        } else if db.number.is_some() || da.dim.same_as(&db.dim) {
                            da.dim
                        } else {
                            return Err(format!(
                                "cannot {} [{}] and [{}]",
                                if *op == Op::Add { "add" } else { "subtract" },
                                da.dim,
                                db.dim
                            ));
                        }
                    }
                    Op::Mul => da.dim.mul(&db.dim),
                    Op::Div => da.dim.div(&db.dim),
                    Op::Pow => {
                        if !db.dim.is_dimensionless() {
                            return Err(format!("exponent has dimension [{}]", db.dim));
                        }
                        if da.dim.is_dimensionless() {
                            Dimension::NONE
                        } else {
                            match db.number {
                                Some(p) => da.dim.powf(p),
                                None => {
                                    return Err(format!("[{}] raised to a variable power", da.dim))
                                }
                            }
                        }
                    }
                };
                DimInfo { dim, number }
            }
            Expr::Call(f, args) => {
                let ds = args
                    .iter()
                    .map(|a| a.dim(vars))
                    .collect::<Result<Vec<_>, _>>()?;
                let a = ds[0].dim;
                let number = if ds.iter().all(|d| d.number.is_some()) {
                    let x = ds[0].number.unwrap_or(0.0);
                    let y = ds.get(1).and_then(|d| d.number).unwrap_or(0.0);
                    Some(f.apply(x, y))
                } else {
                    None
                };
                let dim = match f {
                    Func::Sqrt => a.powf(0.5),
                    Func::Cbrt => a.powf(1.0 / 3.0),
                    Func::Abs | Func::Floor | Func::Ceil => a,
                    Func::Atan2 => {
                        if !a.same_as(&ds[1].dim) {
                            return Err(format!(
                                "atan2 arguments differ: [{}] and [{}]",
                                a, ds[1].dim
                            ));
                        }
                        Dimension::NONE
                    }
                    _ => {
                        if !a.is_dimensionless() {
                            return Err(format!(
                                "{}() of a quantity with dimension [{a}]",
                                f.name()
                            ));
                        }
                        Dimension::NONE
                    }
                };
                DimInfo { dim, number }
            }
        })
    }
}

#[derive(Clone, Copy)]
struct DimInfo {
    dim: Dimension,
    /// Value, if the subexpression is made of plain numbers only.
    number: Option<f64>,
}

impl DimInfo {
    fn of(dim: Dimension) -> DimInfo {
        DimInfo { dim, number: None }
    }
}

impl fmt::Display for Expr {
    /// Shows variables as `x0`, `x1`... (see [`Expr::display_with`]).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, 0, &[])
    }
}

/// Display helper returned by [`Expr::display_with`].
pub struct DisplayWith<'a> {
    expr: &'a Expr,
    names: &'a [String],
}

impl fmt::Display for DisplayWith<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.expr.write(f, 0, self.names)
    }
}

impl Expr {
    /// Displays the expression with variable names.
    pub fn display_with<'a>(&'a self, names: &'a [String]) -> DisplayWith<'a> {
        DisplayWith { expr: self, names }
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, parent: u8, names: &[String]) -> fmt::Result {
        match self {
            Expr::Num(x) if *x < 0.0 && parent > 2 => write!(f, "({x})"),
            Expr::Num(x) => write!(f, "{x}"),
            Expr::Var(i) => match names.get(*i) {
                Some(n) => f.write_str(n),
                None => write!(f, "x{i}"),
            },
            Expr::Const { id, .. } => f.write_str(id),
            Expr::Neg(a) => {
                let paren = parent > 2;
                if paren {
                    f.write_str("(")?;
                }
                f.write_str("-")?;
                a.write(f, 3, names)?;
                if paren {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Expr::Binary(op, a, b) => {
                let p = op.precedence();
                let paren = p < parent;
                if paren {
                    f.write_str("(")?;
                }
                // Left-associative, except for ^ which is right-associative.
                let (pl, pr) = if *op == Op::Pow {
                    (p + 1, p)
                } else {
                    (p, p + 1)
                };
                a.write(f, pl, names)?;
                write!(f, " {} ", op.symbol())?;
                b.write(f, pr, names)?;
                if paren {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Expr::Call(func, args) => {
                write!(f, "{}(", func.name())?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    a.write(f, 0, names)?;
                }
                f.write_str(")")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Op(char),
    Open,
    Close,
    Comma,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Num(x) => write!(f, "number {x}"),
            Token::Name(n) => write!(f, "name '{n}'"),
            Token::Op(c) => write!(f, "'{c}'"),
            Token::Open => f.write_str("'('"),
            Token::Close => f.write_str("')'"),
            Token::Comma => f.write_str("','"),
        }
    }
}

/// Whether `s` is a valid name: a letter or `_`, then letters, digits or `_`.
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = chars[start..i].iter().collect();
            let x: f64 = s.parse().map_err(|_| format!("bad number '{s}'"))?;
            out.push(Token::Num(x));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else {
            out.push(match c {
                '+' | '-' | '*' | '/' | '^' => Token::Op(c),
                '·' | '×' => Token::Op('*'),
                '−' => Token::Op('-'),
                '(' => Token::Open,
                ')' => Token::Close,
                ',' => Token::Comma,
                _ => return Err(format!("unexpected character '{c}'")),
            });
            i += 1;
        }
    }
    Ok(out)
}

struct Parser<'a, R: Fn(&str) -> Option<Name>> {
    tokens: Vec<Token>,
    pos: usize,
    resolve: &'a R,
}

impl<R: Fn(&str) -> Option<Name>> Parser<'_, R> {
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn expect(&mut self, t: Token) -> Result<(), String> {
        match self.next() {
            Some(ref x) if *x == t => Ok(()),
            Some(x) => Err(format!("expected {t}, found {x}")),
            None => Err(format!("expected {t} at the end")),
        }
    }

    /// Precedence climbing. Levels: 1 `+ -`, 2 `* /`, 3 unary `-`, 4 `^`.
    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = if self.peek() == Some(&Token::Op('-')) && min <= 3 {
            self.pos += 1;
            let a = self.expr(3)?;
            match a {
                Expr::Num(x) => Expr::Num(-x),
                a => Expr::Neg(Box::new(a)),
            }
        } else if self.peek() == Some(&Token::Op('+')) && min <= 3 {
            self.pos += 1;
            self.expr(3)?
        } else {
            self.primary()?
        };
        loop {
            let op = match self.peek() {
                Some(Token::Op('+')) => Op::Add,
                Some(Token::Op('-')) => Op::Sub,
                Some(Token::Op('*')) => Op::Mul,
                Some(Token::Op('/')) => Op::Div,
                Some(Token::Op('^')) => Op::Pow,
                _ => return Ok(lhs),
            };
            let p = op.precedence();
            if p < min {
                return Ok(lhs);
            }
            self.pos += 1;
            // ^ is right-associative and its right side may start with '-'.
            let rhs = if op == Op::Pow {
                self.expr(3)?
            } else {
                self.expr(p + 1)?
            };
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Num(x)) => Ok(Expr::Num(x)),
            Some(Token::Open) => {
                let e = self.expr(0)?;
                self.expect(Token::Close)?;
                Ok(e)
            }
            Some(Token::Name(n)) => {
                if self.peek() == Some(&Token::Open) {
                    let f = Func::from_name(&n).ok_or_else(|| format!("unknown function '{n}'"))?;
                    self.pos += 1;
                    let mut args = vec![self.expr(0)?];
                    while self.peek() == Some(&Token::Comma) {
                        self.pos += 1;
                        args.push(self.expr(0)?);
                    }
                    self.expect(Token::Close)?;
                    if args.len() != f.arity() {
                        return Err(format!(
                            "{n}() takes {} argument(s), got {}",
                            f.arity(),
                            args.len()
                        ));
                    }
                    return Ok(Expr::Call(f, args));
                }
                match (self.resolve)(&n) {
                    Some(Name::Var(i)) => Ok(Expr::Var(i)),
                    Some(Name::Const(value, dimension)) => Ok(Expr::Const {
                        id: n,
                        value,
                        dimension,
                    }),
                    None => Err(format!(
                        "unknown name '{n}' (not a variable of the formula nor a library constant)"
                    )),
                }
            }
            Some(t) => Err(format!("unexpected {t}")),
            None => Err("unexpected end of formula".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::parse_unit;

    fn resolver(names: &'static [&'static str]) -> impl Fn(&str) -> Option<Name> {
        move |n| match n {
            "pi" => Some(Name::Const(std::f64::consts::PI, Dimension::NONE)),
            "g" => Some(Name::Const(9.80665, parse_unit("m/s^2").unwrap().dimension)),
            _ => names.iter().position(|x| *x == n).map(Name::Var),
        }
    }

    fn parse(text: &str, names: &'static [&'static str]) -> Expr {
        Expr::parse(text, resolver(names)).unwrap()
    }

    #[test]
    fn precedence_and_associativity() {
        let e = parse("-x^2", &["x"]);
        assert_eq!(e.eval(&[3.0]), -9.0);
        let e = parse("2^3^2", &[]);
        assert_eq!(e.eval(&[]), 512.0);
        let e = parse("a / 2 * b", &["a", "b"]);
        assert_eq!(e.eval(&[4.0, 3.0]), 6.0);
        let e = parse("a - b - c", &["a", "b", "c"]);
        assert_eq!(e.eval(&[10.0, 3.0, 2.0]), 5.0);
        let e = parse("x^-1", &["x"]);
        assert_eq!(e.eval(&[4.0]), 0.25);
        let e = parse("2 * pi * sqrt(L / g)", &["L"]);
        assert!((e.eval(&[1.0]) - 2.006_409_3).abs() < 1e-6);
        let e = parse("6.022e23 * n", &["n"]);
        assert_eq!(e.eval(&[2.0]), 1.2044e24);
        let e = parse("atan2(y, x)", &["x", "y"]);
        assert!((e.eval(&[1.0, 1.0]) - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn display_round_trips() {
        for text in [
            "0.5 * m * v^2",
            "-(a + b)",
            "a - (b - c)",
            "a / (b * c)",
            "(a^b)^c",
            "a^b^c",
            "-a^2",
            "(-a)^2",
            "(-2)^a",
            "a * -b",
            "sqrt(a) + atan2(b, c)",
        ] {
            let names: &'static [&'static str] = &["m", "v", "a", "b", "c"];
            let e = parse(text, names);
            let shown = e.to_string();
            let names_x: &'static [&'static str] = &["x0", "x1", "x2", "x3", "x4"];
            let again = parse(&shown, names_x);
            assert_eq!(e, again, "{text} -> {shown}");
        }
    }

    #[test]
    fn parse_errors() {
        let r = resolver(&["x"]);
        assert!(Expr::parse("x +", &r).is_err());
        assert!(Expr::parse("y", &r).is_err());
        assert!(Expr::parse("foo(x)", &r).is_err());
        assert!(Expr::parse("sqrt(x, x)", &r).is_err());
        assert!(Expr::parse("(x", &r).is_err());
        assert!(Expr::parse("x x", &r).is_err());
        assert!(Expr::parse("x $ 2", &r).is_err());
    }

    #[test]
    fn dimensions() {
        let m = parse_unit("kg").unwrap().dimension;
        let v = parse_unit("m/s").unwrap().dimension;
        let j = parse_unit("J").unwrap().dimension;
        let e = parse("0.5 * m * v^2", &["m", "v"]);
        assert!(e.dimension(&[m, v]).unwrap().same_as(&j));
        // Mismatched sum.
        let e = parse("m + v", &["m", "v"]);
        assert!(e.dimension(&[m, v]).is_err());
        // exp of a quantity with dimension.
        let e = parse("exp(m)", &["m"]);
        assert!(e.dimension(&[m]).is_err());
        // Variable power of a quantity with dimension.
        let e = parse("m^v", &["m", "v"]);
        assert!(e.dimension(&[m, Dimension::NONE]).is_err());
        // Numbers adapt in sums (unit conversions).
        let k = parse_unit("K").unwrap().dimension;
        let e = parse("1.8 * t + 32", &["t"]);
        assert!(e.dimension(&[k]).unwrap().same_as(&k));
        // Fractional powers.
        let e = parse("m^(1/2)", &["m"]);
        assert!(e.dimension(&[m]).unwrap().same_as(&m.powf(0.5)));
    }

    #[test]
    fn constants_take_part_in_dimensions() {
        let l = parse_unit("m").unwrap().dimension;
        let s = parse_unit("s").unwrap().dimension;
        let e = parse("2 * pi * sqrt(L / g)", &["L"]);
        let d = e.dimension(&[l]).unwrap();
        assert!(d.same_as(&s));
        assert_eq!(e.constants(), vec!["pi", "g"]);
        assert_eq!(e.operators(), vec!["*", "*", "sqrt", "/"]);
    }

    #[test]
    fn identifiers() {
        assert!(is_identifier("v_0"));
        assert!(is_identifier("ω"));
        assert!(is_identifier("k_B"));
        assert!(!is_identifier("0v"));
        assert!(!is_identifier("a-b"));
        assert!(!is_identifier(""));
    }
}
