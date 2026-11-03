//! Parsing formulas written as text, such as `0.5 * m * v^2`.
//!
//! Grammar (usual precedence, `^` binds tightest and is right-associative):
//!
//! ```text
//! expr  = term (("+" | "-") term)*
//! term  = unary (("*" | "/") unary)*
//! unary = "-" unary | power
//! power = atom ("^" unary)?
//! atom  = number | name | name "(" expr ")" | "(" expr ")"
//! ```
//!
//! Functions: `sqrt`, `exp`, `log` (or `ln`), `sin`, `cos`. Constants: `pi`
//! (or `π`) and `e`, unless a variable has that name. `·` and `×` also
//! multiply and `²`/`³` are powers.

use crate::expr::{BinaryOp, Expr, UnaryOp};
use std::fmt;

/// A formula that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// Character position in the input.
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.position + 1)
    }
}

impl std::error::Error for ParseError {}

/// Parses `text` into an expression. Variable names map to `Expr::Var(i)`
/// by their position in `variables`.
///
/// ```
/// use regrex_core::parse::parse_expr;
/// let e = parse_expr("0.5 * m * v^2", &["m", "v"]).unwrap();
/// assert_eq!(e.eval_row(&[2.0, 3.0], &[]), 9.0);
/// ```
pub fn parse_expr(text: &str, variables: &[&str]) -> Result<Expr, ParseError> {
    parse(text, variables, None)
}

/// Parses a formula shape with constants to fit, such as `a * x^b`: every
/// name that is not a variable, a function, `pi` or `e` becomes a parameter
/// (`Expr::Param`), numbered in order of first appearance. Returns the
/// expression and the parameter names.
///
/// ```
/// use regrex_core::parse::parse_template;
/// let (e, params) = parse_template("a * x^b + a", &["x"]).unwrap();
/// assert_eq!(params, ["a", "b"]);
/// assert_eq!(e.eval_row(&[2.0], &[3.0, 2.0]), 15.0);
/// ```
pub fn parse_template(text: &str, variables: &[&str]) -> Result<(Expr, Vec<String>), ParseError> {
    let mut params = Vec::new();
    let e = parse(text, variables, Some(&mut params))?;
    Ok((e, params))
}

fn parse(
    text: &str,
    variables: &[&str],
    params: Option<&mut Vec<String>>,
) -> Result<Expr, ParseError> {
    let normalized = text.replace('²', "^2").replace('³', "^3");
    let tokens = tokenize(&normalized)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        variables,
        params,
    };
    let e = p.expr()?;
    match p.peek() {
        None => Ok(e),
        Some((t, at)) => Err(ParseError {
            message: format!("unexpected {t}"),
            position: at,
        }),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Op(char),
    Open,
    Close,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Number(n) => write!(f, "number {n}"),
            Token::Name(n) => write!(f, "name {n:?}"),
            Token::Op(c) => write!(f, "{c:?}"),
            Token::Open => write!(f, "\"(\""),
            Token::Close => write!(f, "\")\""),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, ParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // Scientific notation: 6.02e23, 1e-3.
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '-' || chars[j] == '+') {
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
            let n = s.parse::<f64>().map_err(|_| ParseError {
                message: format!("invalid number {s:?}"),
                position: start,
            })?;
            out.push((Token::Number(n), start));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push((Token::Name(chars[start..i].iter().collect()), start));
            continue;
        }
        let token = match c {
            '+' | '-' | '*' | '/' | '^' => Token::Op(c),
            '·' | '×' => Token::Op('*'),
            '−' => Token::Op('-'),
            '(' => Token::Open,
            ')' => Token::Close,
            _ => {
                return Err(ParseError {
                    message: format!("unexpected character {c:?}"),
                    position: start,
                })
            }
        };
        out.push((token, start));
        i += 1;
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    variables: &'a [&'a str],
    /// When parsing a template, the names of the parameters found so far.
    params: Option<&'a mut Vec<String>>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<(Token, usize)> {
        self.tokens.get(self.pos).cloned()
    }

    fn end_position(&self) -> usize {
        self.tokens.last().map_or(0, |(_, p)| p + 1)
    }

    fn eat_op(&mut self, ops: &[char]) -> Option<char> {
        match self.tokens.get(self.pos) {
            Some((Token::Op(c), _)) if ops.contains(c) => {
                self.pos += 1;
                Some(*c)
            }
            _ => None,
        }
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.term()?;
        while let Some(op) = self.eat_op(&['+', '-']) {
            let rhs = self.term()?;
            let op = if op == '+' {
                BinaryOp::Add
            } else {
                BinaryOp::Sub
            };
            e = Expr::binary(op, e, rhs);
        }
        Ok(e)
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.unary()?;
        while let Some(op) = self.eat_op(&['*', '/']) {
            let rhs = self.unary()?;
            let op = if op == '*' {
                BinaryOp::Mul
            } else {
                BinaryOp::Div
            };
            e = Expr::binary(op, e, rhs);
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat_op(&['-']).is_some() {
            let inner = self.unary()?;
            return Ok(match inner {
                Expr::Const(c) => Expr::Const(-c),
                other => Expr::unary(UnaryOp::Neg, other),
            });
        }
        if self.eat_op(&['+']).is_some() {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.atom()?;
        if self.eat_op(&['^']).is_some() {
            let exponent = self.unary()?;
            return Ok(Expr::binary(BinaryOp::Pow, base, exponent));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, ParseError> {
        let Some((token, at)) = self.peek() else {
            return Err(ParseError {
                message: "unexpected end of formula".into(),
                position: self.end_position(),
            });
        };
        self.pos += 1;
        match token {
            Token::Number(n) => Ok(Expr::Const(n)),
            Token::Open => {
                let e = self.expr()?;
                self.close(at)?;
                Ok(e)
            }
            Token::Name(name) => {
                if matches!(self.tokens.get(self.pos), Some((Token::Open, _))) {
                    let op = match name.as_str() {
                        "sqrt" => UnaryOp::Sqrt,
                        "exp" => UnaryOp::Exp,
                        "log" | "ln" => UnaryOp::Log,
                        "sin" => UnaryOp::Sin,
                        "cos" => UnaryOp::Cos,
                        _ if self.variables.contains(&name.as_str()) => {
                            return Err(ParseError {
                                message: format!(
                                    "{name:?} is a variable, not a function (write {name} * (...))"
                                ),
                                position: at,
                            })
                        }
                        _ => {
                            return Err(ParseError {
                                message: format!("unknown function {name:?}"),
                                position: at,
                            })
                        }
                    };
                    self.pos += 1;
                    let arg = self.expr()?;
                    self.close(at)?;
                    return Ok(Expr::unary(op, arg));
                }
                if let Some(i) = self.variables.iter().position(|v| *v == name) {
                    return Ok(Expr::Var(i));
                }
                match name.as_str() {
                    "pi" | "π" => Ok(Expr::Const(std::f64::consts::PI)),
                    "e" => Ok(Expr::Const(std::f64::consts::E)),
                    _ if self.params.is_some() => {
                        let params = self.params.as_mut().expect("checked");
                        let i = params.iter().position(|p| *p == name).unwrap_or_else(|| {
                            params.push(name.clone());
                            params.len() - 1
                        });
                        Ok(Expr::Param(i))
                    }
                    _ => Err(ParseError {
                        message: format!("unknown variable {name:?}"),
                        position: at,
                    }),
                }
            }
            other => Err(ParseError {
                message: format!("unexpected {other}"),
                position: at,
            }),
        }
    }

    fn close(&mut self, opened_at: usize) -> Result<(), ParseError> {
        match self.tokens.get(self.pos) {
            Some((Token::Close, _)) => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(ParseError {
                message: "missing \")\"".into(),
                position: opened_at,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simplify::canonicalize;

    fn parse(text: &str) -> Expr {
        parse_expr(text, &["m", "v", "t", "e_k"]).unwrap()
    }

    #[test]
    fn precedence_and_associativity() {
        let e = parse("1 + 2 * 3 ^ 2");
        assert_eq!(e.eval_row(&[], &[]), 19.0);
        let e = parse("2 ^ 3 ^ 2");
        assert_eq!(e.eval_row(&[], &[]), 512.0);
        let e = parse("8 / 4 / 2");
        assert_eq!(e.eval_row(&[], &[]), 1.0);
        // As in maths, -2^2 is -(2^2).
        let e = parse("-2 ^ 2");
        assert_eq!(e.eval_row(&[], &[]), -4.0);
        let e = parse("10 - 3 - 2");
        assert_eq!(e.eval_row(&[], &[]), 5.0);
    }

    #[test]
    fn variables_functions_and_constants() {
        let e = parse("0.5 · m × v²");
        assert_eq!(e.eval_row(&[2.0, 3.0, 0.0, 0.0], &[]), 9.0);
        let e = parse("2 * pi * sqrt(t / 9.81)");
        assert!(
            (e.eval_row(&[0.0, 0.0, 9.81, 0.0], &[]) - 2.0 * std::f64::consts::PI).abs() < 1e-12
        );
        let e = parse("exp(-t) + ln(e) + e_k");
        assert!((e.eval_row(&[0.0, 0.0, 0.0, 1.0], &[]) - 3.0).abs() < 1e-12);
        assert_eq!(parse("6.02e23").eval_row(&[], &[]), 6.02e23);
    }

    #[test]
    fn equivalent_formulas_have_the_same_canonical_form() {
        let a = canonicalize(&parse("0.5 * m * v^2"));
        let b = canonicalize(&parse("v * v * m / 2"));
        // Both evaluate the same; the canonical form of the first is the
        // reference display.
        assert_eq!(a.to_string(), "0.5 * x0 * x1^2");
        assert_eq!(a.eval_row(&[3.0, 4.0], &[]), b.eval_row(&[3.0, 4.0], &[]));
    }

    #[test]
    fn helpful_errors() {
        let err = |t: &str| parse_expr(t, &["m"]).unwrap_err().to_string();
        assert_eq!(err("m +"), "unexpected end of formula (at character 4)");
        assert_eq!(err("(m"), "missing \")\" (at character 1)");
        assert_eq!(err("foo(m)"), "unknown function \"foo\" (at character 1)");
        assert_eq!(err("x * 2"), "unknown variable \"x\" (at character 1)");
        assert_eq!(err("m $ 2"), "unexpected character '$' (at character 3)");
        assert_eq!(err("m 2"), "unexpected number 2 (at character 3)");
        assert!(err("m(2)").contains("is a variable, not a function"));
    }
}
