//! Physical units and dimensional analysis.
//!
//! When the units of the columns are known, formulas that add metres to
//! seconds or take the logarithm of a mass are impossible and are skipped,
//! which prunes most of the search space for physics data.

use crate::expr::{BinaryOp, Expr, UnaryOp};
use std::fmt;

/// Number of base dimensions: length, mass, time, current, temperature,
/// amount, luminosity and currency.
pub const BASE_COUNT: usize = 8;

const BASE_SYMBOLS: [&str; BASE_COUNT] = ["m", "kg", "s", "A", "K", "mol", "cd", "¤"];

/// A physical dimension, as exponents of the base dimensions.
///
/// Exponents are stored doubled so that square roots (`m^(1/2)`) are exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dim([i16; BASE_COUNT]);

impl Dim {
    /// No dimension (a pure number).
    pub const NONE: Dim = Dim([0; BASE_COUNT]);

    /// The base dimension number `i` (see [`BASE_COUNT`]).
    pub fn base(i: usize) -> Dim {
        let mut d = [0; BASE_COUNT];
        d[i] = 2;
        Dim(d)
    }

    /// Whether this is [`Dim::NONE`].
    pub fn is_none(&self) -> bool {
        *self == Dim::NONE
    }

    /// Integer power.
    pub fn powi(self, n: i16) -> Dim {
        Dim(self.0.map(|e| e * n))
    }

    /// Square root, if every exponent stays a multiple of 1/2.
    pub fn sqrt(self) -> Option<Dim> {
        self.0
            .iter()
            .all(|e| e % 2 == 0)
            .then(|| Dim(self.0.map(|e| e / 2)))
    }
}

impl std::ops::Mul for Dim {
    type Output = Dim;

    /// Product of two dimensions: exponents add up.
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn mul(self, other: Dim) -> Dim {
        let mut d = self.0;
        for (a, b) in d.iter_mut().zip(other.0) {
            *a += b;
        }
        Dim(d)
    }
}

impl std::ops::Div for Dim {
    type Output = Dim;

    /// Quotient of two dimensions.
    fn div(self, other: Dim) -> Dim {
        self * other.powi(-1)
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return write!(f, "1");
        }
        let mut first = true;
        for (e, sym) in self.0.iter().zip(BASE_SYMBOLS) {
            if *e == 0 {
                continue;
            }
            if !first {
                write!(f, "·")?;
            }
            first = false;
            match *e {
                2 => write!(f, "{sym}")?,
                e if e % 2 == 0 => write!(f, "{sym}^{}", e / 2)?,
                e => write!(f, "{sym}^({e}/2)")?,
            }
        }
        Ok(())
    }
}

/// A unit: a dimension and the factor that converts it to SI
/// (`cm` is length with factor 0.01).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// Dimension of the unit.
    pub dim: Dim,
    /// Value of one of this unit in SI units.
    pub factor: f64,
}

/// A unit string that could not be understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitError(pub String);

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown unit {:?}", self.0)
    }
}

impl std::error::Error for UnitError {}

/// Named units: symbol, dimension exponents (m, kg, s, A, K, mol, cd, ¤) and
/// factor to SI.
const UNITS: &[(&str, [i16; BASE_COUNT], f64)] = &[
    // Length
    ("m", [1, 0, 0, 0, 0, 0, 0, 0], 1.0),
    ("km", [1, 0, 0, 0, 0, 0, 0, 0], 1e3),
    ("cm", [1, 0, 0, 0, 0, 0, 0, 0], 1e-2),
    ("mm", [1, 0, 0, 0, 0, 0, 0, 0], 1e-3),
    ("in", [1, 0, 0, 0, 0, 0, 0, 0], 0.0254),
    ("ft", [1, 0, 0, 0, 0, 0, 0, 0], 0.3048),
    ("mi", [1, 0, 0, 0, 0, 0, 0, 0], 1609.344),
    // Mass
    ("kg", [0, 1, 0, 0, 0, 0, 0, 0], 1.0),
    ("g", [0, 1, 0, 0, 0, 0, 0, 0], 1e-3),
    ("mg", [0, 1, 0, 0, 0, 0, 0, 0], 1e-6),
    ("t", [0, 1, 0, 0, 0, 0, 0, 0], 1e3),
    ("lb", [0, 1, 0, 0, 0, 0, 0, 0], 0.453_592_37),
    // Time
    ("s", [0, 0, 1, 0, 0, 0, 0, 0], 1.0),
    ("ms", [0, 0, 1, 0, 0, 0, 0, 0], 1e-3),
    ("min", [0, 0, 1, 0, 0, 0, 0, 0], 60.0),
    ("h", [0, 0, 1, 0, 0, 0, 0, 0], 3600.0),
    ("day", [0, 0, 1, 0, 0, 0, 0, 0], 86400.0),
    ("Hz", [0, 0, -1, 0, 0, 0, 0, 0], 1.0),
    // Electricity
    ("A", [0, 0, 0, 1, 0, 0, 0, 0], 1.0),
    ("mA", [0, 0, 0, 1, 0, 0, 0, 0], 1e-3),
    ("V", [2, 1, -3, -1, 0, 0, 0, 0], 1.0),
    ("mV", [2, 1, -3, -1, 0, 0, 0, 0], 1e-3),
    ("ohm", [2, 1, -3, -2, 0, 0, 0, 0], 1.0),
    ("Ω", [2, 1, -3, -2, 0, 0, 0, 0], 1.0),
    ("kohm", [2, 1, -3, -2, 0, 0, 0, 0], 1e3),
    ("kΩ", [2, 1, -3, -2, 0, 0, 0, 0], 1e3),
    ("C", [0, 0, 1, 1, 0, 0, 0, 0], 1.0),
    ("F", [-2, -1, 4, 2, 0, 0, 0, 0], 1.0),
    // Mechanics
    ("N", [1, 1, -2, 0, 0, 0, 0, 0], 1.0),
    ("kN", [1, 1, -2, 0, 0, 0, 0, 0], 1e3),
    ("J", [2, 1, -2, 0, 0, 0, 0, 0], 1.0),
    ("kJ", [2, 1, -2, 0, 0, 0, 0, 0], 1e3),
    ("kWh", [2, 1, -2, 0, 0, 0, 0, 0], 3.6e6),
    ("cal", [2, 1, -2, 0, 0, 0, 0, 0], 4.184),
    ("W", [2, 1, -3, 0, 0, 0, 0, 0], 1.0),
    ("kW", [2, 1, -3, 0, 0, 0, 0, 0], 1e3),
    ("Pa", [-1, 1, -2, 0, 0, 0, 0, 0], 1.0),
    ("kPa", [-1, 1, -2, 0, 0, 0, 0, 0], 1e3),
    ("bar", [-1, 1, -2, 0, 0, 0, 0, 0], 1e5),
    ("atm", [-1, 1, -2, 0, 0, 0, 0, 0], 101_325.0),
    ("L", [3, 0, 0, 0, 0, 0, 0, 0], 1e-3),
    ("l", [3, 0, 0, 0, 0, 0, 0, 0], 1e-3),
    ("mL", [3, 0, 0, 0, 0, 0, 0, 0], 1e-6),
    // Temperature and amount
    ("K", [0, 0, 0, 0, 1, 0, 0, 0], 1.0),
    ("mol", [0, 0, 0, 0, 0, 1, 0, 0], 1.0),
    ("cd", [0, 0, 0, 0, 0, 0, 1, 0], 1.0),
    // Angles and ratios are pure numbers
    ("rad", [0; BASE_COUNT], 1.0),
    ("deg", [0; BASE_COUNT], std::f64::consts::PI / 180.0),
    ("°", [0; BASE_COUNT], std::f64::consts::PI / 180.0),
    ("%", [0; BASE_COUNT], 0.01),
    ("1", [0; BASE_COUNT], 1.0),
    // Money: one dimension for every currency (no exchange rates)
    ("€", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
    ("EUR", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
    ("$", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
    ("USD", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
    ("£", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
    ("GBP", [0, 0, 0, 0, 0, 0, 0, 1], 1.0),
];

fn named(symbol: &str) -> Option<Unit> {
    UNITS
        .iter()
        .find(|(s, _, _)| *s == symbol)
        .map(|(_, e, f)| Unit {
            dim: Dim(e.map(|x| x * 2)),
            factor: *f,
        })
}

/// Parses a unit such as `m`, `m/s^2`, `kg*m^2/s^2`, `km/h`, `kg·m/s²` or `€/h`.
///
/// Products use `*`, `·` or a space; a single `/` divides by everything
/// after it. Exponents use `^2`, `²` or `³`.
pub fn parse_unit(text: &str) -> Result<Unit, UnitError> {
    let fail = || UnitError(text.to_string());
    let cleaned = text
        .trim()
        .replace('²', "^2")
        .replace('³', "^3")
        .replace('·', "*");
    if cleaned.is_empty() {
        return Err(fail());
    }
    let (num, den) = match cleaned.split_once('/') {
        Some((n, d)) => (n.trim().to_string(), Some(d.trim().to_string())),
        None => (cleaned.clone(), None),
    };
    let mut unit = Unit {
        dim: Dim::NONE,
        factor: 1.0,
    };
    let mut apply = |part: &str, sign: i16| -> Result<(), UnitError> {
        for factor in part.split(['*', ' ']).filter(|f| !f.is_empty()) {
            let (symbol, power) = match factor.split_once('^') {
                Some((s, p)) => (
                    s,
                    p.trim_matches(['(', ')'])
                        .parse::<i16>()
                        .map_err(|_| fail())?,
                ),
                None => (factor, 1),
            };
            let u = named(symbol).ok_or_else(fail)?;
            unit.dim = unit.dim * u.dim.powi(power * sign);
            unit.factor *= u.factor.powi(i32::from(power * sign));
        }
        Ok(())
    };
    if num != "1" {
        apply(&num, 1)?;
    }
    if let Some(den) = den {
        apply(&den, -1)?;
    }
    Ok(unit)
}

/// Splits a column header like `speed (m/s)` or `mass [kg]` into its name
/// and unit. Returns the header unchanged and `None` if there is no unit or
/// it is not understood.
pub fn split_header(header: &str) -> (String, Option<Unit>) {
    let h = header.trim();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let (Some(start), true) = (h.rfind(open), h.ends_with(close)) {
            let name = h[..start].trim();
            let unit_text = &h[start + 1..h.len() - 1];
            if !name.is_empty() {
                if let Ok(u) = parse_unit(unit_text) {
                    return (name.to_string(), Some(u));
                }
            }
        }
    }
    (h.to_string(), None)
}

/// Dimension of an expression, or why it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    /// The expression has this dimension.
    Known(Dim),
    /// The expression contains a free parameter that can take any unit.
    Any,
    /// The expression mixes incompatible dimensions.
    Invalid,
}

/// Computes the dimension of `e`, given the dimension of each variable.
/// Fixed numbers are pure numbers; free parameters may carry any unit.
pub fn dimension_of(e: &Expr, vars: &[Dim]) -> Dimension {
    use Dimension::*;
    match e {
        Expr::Var(i) => vars.get(*i).map_or(Any, |d| Known(*d)),
        Expr::Const(_) => Known(Dim::NONE),
        Expr::Param(_) => Any,
        Expr::Unary(op, a) => {
            let a = dimension_of(a, vars);
            match (op, a) {
                (_, Invalid) => Invalid,
                (UnaryOp::Neg, d) => d,
                (UnaryOp::Square, Known(d)) => Known(d.powi(2)),
                (UnaryOp::Sqrt, Known(d)) => d.sqrt().map_or(Invalid, Known),
                (UnaryOp::Square | UnaryOp::Sqrt, Any) => Any,
                // exp, log, sin and cos only take pure numbers.
                (_, Known(d)) if !d.is_none() => Invalid,
                _ => Known(Dim::NONE),
            }
        }
        Expr::Binary(op, a, b) => {
            let (da, db) = (dimension_of(a, vars), dimension_of(b, vars));
            match (op, da, db) {
                (_, Invalid, _) | (_, _, Invalid) => Invalid,
                (BinaryOp::Add | BinaryOp::Sub, Known(x), Known(y)) => {
                    if x == y {
                        Known(x)
                    } else {
                        Invalid
                    }
                }
                (BinaryOp::Add | BinaryOp::Sub, Known(x), Any)
                | (BinaryOp::Add | BinaryOp::Sub, Any, Known(x)) => Known(x),
                (BinaryOp::Mul, Known(x), Known(y)) => Known(x * y),
                (BinaryOp::Div | BinaryOp::IntDiv, Known(x), Known(y)) => Known(x / y),
                (BinaryOp::Mod, Known(x), Known(y)) => {
                    if x == y {
                        Known(x)
                    } else {
                        Invalid
                    }
                }
                (BinaryOp::Mod, Known(x), Any) | (BinaryOp::Mod, Any, Known(x)) => Known(x),
                (BinaryOp::Pow, Known(x), _) => match b.as_ref() {
                    Expr::Const(k) if k.fract() == 0.0 && k.abs() <= 8.0 => {
                        Known(x.powi(*k as i16))
                    }
                    _ if x.is_none() => Known(Dim::NONE),
                    _ => Invalid,
                },
                _ => Any,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(text: &str) -> Dim {
        parse_unit(text).unwrap().dim
    }

    #[test]
    fn parses_units() {
        assert_eq!(dim("m/s^2"), dim("m") / dim("s").powi(2));
        assert_eq!(dim("N"), dim("kg*m/s^2"));
        assert_eq!(dim("J"), dim("kg·m²/s²"));
        assert_eq!(dim("W"), dim("J/s"));
        assert_eq!(dim("V"), dim("W/A"));
        assert_eq!(dim("ohm"), dim("V/A"));
        assert_eq!(dim("Hz"), dim("1/s"));
        assert!(dim("%").is_none());
        assert_eq!(dim("€/h"), dim("$/s"));
        let kmh = parse_unit("km/h").unwrap();
        assert!((kmh.factor - 1.0 / 3.6).abs() < 1e-12);
        assert!(parse_unit("parsecs").is_err());
        assert!(parse_unit("").is_err());
    }

    #[test]
    fn displays_dimensions() {
        assert_eq!(dim("N").to_string(), "m·kg·s^-2");
        assert_eq!(Dim::NONE.to_string(), "1");
        assert_eq!(dim("m").sqrt().unwrap().to_string(), "m^(1/2)");
    }

    #[test]
    fn splits_headers() {
        let (name, unit) = split_header("speed (m/s)");
        assert_eq!(name, "speed");
        assert_eq!(unit.unwrap().dim, dim("m/s"));
        let (name, unit) = split_header("mass [kg]");
        assert_eq!((name.as_str(), unit.unwrap().dim), ("mass", dim("kg")));
        assert_eq!(split_header("total").0, "total");
        assert!(split_header("total").1.is_none());
        // Parentheses that are not a unit stay in the name.
        assert_eq!(
            split_header("price (with VAT)"),
            ("price (with VAT)".to_string(), None)
        );
    }

    #[test]
    fn checks_dimensions_of_expressions() {
        let vars = [dim("kg"), dim("m/s"), dim("s")];
        let v = |i| Expr::Var(i);
        // m * v^2 is an energy.
        let e = Expr::binary(BinaryOp::Mul, v(0), Expr::unary(UnaryOp::Square, v(1)));
        assert_eq!(dimension_of(&e, &vars), Dimension::Known(dim("J")));
        // m + v is impossible.
        let e = Expr::binary(BinaryOp::Add, v(0), v(1));
        assert_eq!(dimension_of(&e, &vars), Dimension::Invalid);
        // exp(t) is impossible, exp(c * t) is fine (c can be 1/s).
        assert_eq!(
            dimension_of(&Expr::unary(UnaryOp::Exp, v(2)), &vars),
            Dimension::Invalid
        );
        let e = Expr::unary(
            UnaryOp::Exp,
            Expr::binary(BinaryOp::Mul, Expr::Param(0), v(2)),
        );
        assert_eq!(dimension_of(&e, &vars), Dimension::Known(Dim::NONE));
        // sqrt(m) has dimension kg^(1/2); x^c needs a pure number x.
        let e = Expr::unary(UnaryOp::Sqrt, v(0));
        assert_eq!(
            dimension_of(&e, &vars),
            Dimension::Known(dim("kg").sqrt().unwrap())
        );
        let e = Expr::binary(BinaryOp::Pow, v(0), Expr::Param(0));
        assert_eq!(dimension_of(&e, &vars), Dimension::Invalid);
    }
}
