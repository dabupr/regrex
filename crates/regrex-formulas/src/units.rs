//! Units and physical dimensions.
//!
//! A unit such as `J/(mol K)` is parsed into a [`Unit`]: its [`Dimension`]
//! (exponents of the base quantities) and the factor that converts it to the
//! coherent SI unit of that dimension (`km` → 1000).
//!
//! Besides the seven SI base quantities there is an extra one that the
//! library needs: `currency` (money, for finance). Information units (`bit`,
//! `B`) are dimensionless, as in IEC 80000-13, with a byte worth 8 bits.

use std::fmt;

/// Number of base quantities.
pub const BASES: usize = 8;

/// Symbols of the base units, in the order used by [`Dimension`].
pub const BASE_SYMBOLS: [&str; BASES] = ["m", "kg", "s", "A", "K", "mol", "cd", "currency"];

/// Exponents of the base quantities: length, mass, time, electric current,
/// temperature, amount of substance, luminous intensity and currency.
///
/// Exponents can be fractional (`sqrt(m)` has length^0.5).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dimension(pub [f64; BASES]);

impl Dimension {
    /// A dimensionless quantity.
    pub const NONE: Dimension = Dimension([0.0; BASES]);

    /// The dimension of base quantity `i` (see [`BASE_SYMBOLS`]).
    pub fn base(i: usize) -> Dimension {
        let mut d = [0.0; BASES];
        d[i] = 1.0;
        Dimension(d)
    }

    /// Whether every exponent is zero.
    pub fn is_dimensionless(&self) -> bool {
        self.same_as(&Dimension::NONE)
    }

    /// Equality up to rounding of the exponents.
    pub fn same_as(&self, other: &Dimension) -> bool {
        self.0
            .iter()
            .zip(&other.0)
            .all(|(a, b)| (a - b).abs() < 1e-9)
    }

    /// Product of two quantities.
    pub fn mul(&self, other: &Dimension) -> Dimension {
        let mut d = self.0;
        for (x, y) in d.iter_mut().zip(&other.0) {
            *x += y;
        }
        Dimension(d)
    }

    /// Quotient of two quantities.
    pub fn div(&self, other: &Dimension) -> Dimension {
        self.mul(&other.powf(-1.0))
    }

    /// Quantity raised to a power.
    pub fn powf(&self, p: f64) -> Dimension {
        let mut d = self.0;
        for x in d.iter_mut() {
            *x *= p;
        }
        Dimension(d)
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (sym, &e) in BASE_SYMBOLS.iter().zip(&self.0) {
            if e.abs() < 1e-9 {
                continue;
            }
            if !first {
                f.write_str(" ")?;
            }
            first = false;
            if (e - 1.0).abs() < 1e-9 {
                write!(f, "{sym}")?;
            } else {
                write!(f, "{sym}^{}", round_exponent(e))?;
            }
        }
        if first {
            f.write_str("1")?;
        }
        Ok(())
    }
}

fn round_exponent(e: f64) -> f64 {
    (e * 1e6).round() / 1e6
}

/// A parsed unit: dimension and scale factor to coherent SI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// Physical dimension.
    pub dimension: Dimension,
    /// Multiply a value in this unit by `factor` to get coherent SI units.
    /// Offsets (°C, °F) are ignored: only the size of the degree counts.
    pub factor: f64,
}

impl Unit {
    const ONE: Unit = Unit {
        dimension: Dimension::NONE,
        factor: 1.0,
    };

    fn mul(self, other: Unit) -> Unit {
        Unit {
            dimension: self.dimension.mul(&other.dimension),
            factor: self.factor * other.factor,
        }
    }

    fn powf(self, p: f64) -> Unit {
        Unit {
            dimension: self.dimension.powf(p),
            factor: self.factor.powf(p),
        }
    }
}

// Exponents: [m, kg, s, A, K, mol, cd, currency]
const fn dim(e: [i8; BASES]) -> [f64; BASES] {
    let mut d = [0.0; BASES];
    let mut i = 0;
    while i < BASES {
        d[i] = e[i] as f64;
        i += 1;
    }
    d
}

struct Named {
    symbol: &'static str,
    dim: [f64; BASES],
    factor: f64,
    prefixable: bool,
}

const fn named(symbol: &'static str, e: [i8; BASES], factor: f64, prefixable: bool) -> Named {
    Named {
        symbol,
        dim: dim(e),
        factor,
        prefixable,
    }
}

const PI: f64 = std::f64::consts::PI;

#[rustfmt::skip]
const UNITS: &[Named] = &[
    // SI base units (the kilogram is prefixed through the gram).
    named("m",        [1, 0, 0, 0, 0, 0, 0, 0], 1.0, true),
    named("g",        [0, 1, 0, 0, 0, 0, 0, 0], 1e-3, true),
    named("kg",       [0, 1, 0, 0, 0, 0, 0, 0], 1.0, false),
    named("s",        [0, 0, 1, 0, 0, 0, 0, 0], 1.0, true),
    named("A",        [0, 0, 0, 1, 0, 0, 0, 0], 1.0, true),
    named("K",        [0, 0, 0, 0, 1, 0, 0, 0], 1.0, true),
    named("mol",      [0, 0, 0, 0, 0, 1, 0, 0], 1.0, true),
    named("cd",       [0, 0, 0, 0, 0, 0, 1, 0], 1.0, true),
    // Extra base quantity, and information (dimensionless).
    named("currency", [0, 0, 0, 0, 0, 0, 0, 1], 1.0, false),
    named("bit",      [0, 0, 0, 0, 0, 0, 0, 0], 1.0, true),
    // SI derived units with special names.
    named("rad",      [0, 0, 0, 0, 0, 0, 0, 0], 1.0, true),
    named("sr",       [0, 0, 0, 0, 0, 0, 0, 0], 1.0, false),
    named("Hz",       [0, 0, -1, 0, 0, 0, 0, 0], 1.0, true),
    named("N",        [1, 1, -2, 0, 0, 0, 0, 0], 1.0, true),
    named("Pa",       [-1, 1, -2, 0, 0, 0, 0, 0], 1.0, true),
    named("J",        [2, 1, -2, 0, 0, 0, 0, 0], 1.0, true),
    named("W",        [2, 1, -3, 0, 0, 0, 0, 0], 1.0, true),
    named("C",        [0, 0, 1, 1, 0, 0, 0, 0], 1.0, true),
    named("V",        [2, 1, -3, -1, 0, 0, 0, 0], 1.0, true),
    named("F",        [-2, -1, 4, 2, 0, 0, 0, 0], 1.0, true),
    named("ohm",      [2, 1, -3, -2, 0, 0, 0, 0], 1.0, true),
    named("Ω",        [2, 1, -3, -2, 0, 0, 0, 0], 1.0, true),
    named("S",        [-2, -1, 3, 2, 0, 0, 0, 0], 1.0, true),
    named("Wb",       [2, 1, -2, -1, 0, 0, 0, 0], 1.0, true),
    named("T",        [0, 1, -2, -1, 0, 0, 0, 0], 1.0, true),
    named("H",        [2, 1, -2, -2, 0, 0, 0, 0], 1.0, true),
    named("lm",       [0, 0, 0, 0, 0, 0, 1, 0], 1.0, true),
    named("lx",       [-2, 0, 0, 0, 0, 0, 1, 0], 1.0, true),
    named("Bq",       [0, 0, -1, 0, 0, 0, 0, 0], 1.0, true),
    named("Gy",       [2, 0, -2, 0, 0, 0, 0, 0], 1.0, true),
    named("Sv",       [2, 0, -2, 0, 0, 0, 0, 0], 1.0, true),
    named("kat",      [0, 0, -1, 0, 0, 1, 0, 0], 1.0, true),
    // Units accepted for use with the SI.
    named("min",      [0, 0, 1, 0, 0, 0, 0, 0], 60.0, false),
    named("h",        [0, 0, 1, 0, 0, 0, 0, 0], 3600.0, false),
    named("d",        [0, 0, 1, 0, 0, 0, 0, 0], 86400.0, false),
    named("yr",       [0, 0, 1, 0, 0, 0, 0, 0], 31_557_600.0, false),
    named("L",        [3, 0, 0, 0, 0, 0, 0, 0], 1e-3, true),
    named("t",        [0, 1, 0, 0, 0, 0, 0, 0], 1e3, false),
    named("eV",       [2, 1, -2, 0, 0, 0, 0, 0], 1.602_176_634e-19, true),
    named("Da",       [0, 1, 0, 0, 0, 0, 0, 0], 1.660_539_068_92e-27, true),
    named("deg",      [0, 0, 0, 0, 0, 0, 0, 0], PI / 180.0, false),
    named("degC",     [0, 0, 0, 0, 1, 0, 0, 0], 1.0, false),
    named("degF",     [0, 0, 0, 0, 1, 0, 0, 0], 5.0 / 9.0, false),
    named("percent",  [0, 0, 0, 0, 0, 0, 0, 0], 0.01, false),
    named("B",        [0, 0, 0, 0, 0, 0, 0, 0], 8.0, true),
    // Common non-SI units, for conversions.
    named("in",       [1, 0, 0, 0, 0, 0, 0, 0], 0.0254, false),
    named("ft",       [1, 0, 0, 0, 0, 0, 0, 0], 0.3048, false),
    named("yd",       [1, 0, 0, 0, 0, 0, 0, 0], 0.9144, false),
    named("mi",       [1, 0, 0, 0, 0, 0, 0, 0], 1609.344, false),
    named("nmi",      [1, 0, 0, 0, 0, 0, 0, 0], 1852.0, false),
    named("lb",       [0, 1, 0, 0, 0, 0, 0, 0], 0.453_592_37, false),
    named("oz",       [0, 1, 0, 0, 0, 0, 0, 0], 0.028_349_523_125, false),
    named("gal",      [3, 0, 0, 0, 0, 0, 0, 0], 3.785_411_784e-3, false),
    named("cal",      [2, 1, -2, 0, 0, 0, 0, 0], 4.184, true),
    named("bar",      [-1, 1, -2, 0, 0, 0, 0, 0], 1e5, true),
    named("atm",      [-1, 1, -2, 0, 0, 0, 0, 0], 101_325.0, false),
    named("mmHg",     [-1, 1, -2, 0, 0, 0, 0, 0], 133.322_387_415, false),
    named("psi",      [-1, 1, -2, 0, 0, 0, 0, 0], 6_894.757_293_168, false),
    named("hp",       [2, 1, -3, 0, 0, 0, 0, 0], 745.699_871_582_270_2, false),
    named("kn",       [1, 0, -1, 0, 0, 0, 0, 0], 1852.0 / 3600.0, false),
];

const PREFIXES: &[(&str, f64)] = &[
    ("Q", 1e30),
    ("R", 1e27),
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("da", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("µ", 1e-6),
    ("μ", 1e-6),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
    ("z", 1e-21),
    ("y", 1e-24),
    ("r", 1e-27),
    ("q", 1e-30),
];

fn lookup(symbol: &str) -> Option<Unit> {
    let make = |u: &Named, scale: f64| Unit {
        dimension: Dimension(u.dim),
        factor: u.factor * scale,
    };
    if let Some(u) = UNITS.iter().find(|u| u.symbol == symbol) {
        return Some(make(u, 1.0));
    }
    for (p, scale) in PREFIXES {
        if let Some(rest) = symbol.strip_prefix(p) {
            if let Some(u) = UNITS.iter().find(|u| u.prefixable && u.symbol == rest) {
                return Some(make(u, *scale));
            }
        }
    }
    None
}

/// Parses a unit expression.
///
/// Grammar: unit symbols (with optional SI prefix) combined with `*`, `·`,
/// a space (product), `/` (quotient), `^` followed by a number (power) and
/// parentheses. `1` is the dimensionless unit.
///
/// ```
/// use regrex_formulas::units::{parse_unit, Dimension};
///
/// let r = parse_unit("J/(mol K)").unwrap();
/// assert_eq!(r.dimension.to_string(), "m^2 kg s^-2 K^-1 mol^-1");
///
/// let speed = parse_unit("km/h").unwrap();
/// assert!((speed.factor - 1.0 / 3.6).abs() < 1e-12);
/// ```
pub fn parse_unit(text: &str) -> Result<Unit, String> {
    let mut p = UnitParser {
        chars: text.chars().collect(),
        pos: 0,
    };
    let unit = p.quotient()?;
    p.skip_spaces();
    if p.pos < p.chars.len() {
        return Err(format!(
            "unexpected '{}' in unit '{text}'",
            p.chars[p.pos..].iter().collect::<String>()
        ));
    }
    Ok(unit)
}

struct UnitParser {
    chars: Vec<char>,
    pos: usize,
}

impl UnitParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    fn quotient(&mut self) -> Result<Unit, String> {
        let mut u = self.product()?;
        loop {
            self.skip_spaces();
            if self.peek() == Some('/') {
                self.pos += 1;
                let d = self.product()?;
                u = u.mul(d.powf(-1.0));
            } else {
                return Ok(u);
            }
        }
    }

    fn product(&mut self) -> Result<Unit, String> {
        let mut u = self.power()?;
        loop {
            let start = self.pos;
            self.skip_spaces();
            match self.peek() {
                Some('*') | Some('·') => {
                    self.pos += 1;
                    u = u.mul(self.power()?);
                }
                Some(c) if c == '(' || c == '1' || is_unit_char(c) => {
                    // Juxtaposition: "N m", "J/(mol K)"
                    if start == self.pos && c != '(' {
                        return Err("missing space between units".into());
                    }
                    u = u.mul(self.power()?);
                }
                _ => {
                    self.pos = start;
                    return Ok(u);
                }
            }
        }
    }

    fn power(&mut self) -> Result<Unit, String> {
        let base = self.atom()?;
        let before = self.pos;
        self.skip_spaces();
        if self.peek() != Some('^') {
            self.pos = before;
            return Ok(base);
        }
        self.pos += 1;
        self.skip_spaces();
        let mut text = String::new();
        let paren = self.peek() == Some('(');
        if paren {
            self.pos += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '-' || c == '.' || (c == '/' && paren) {
                text.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if paren {
            if self.peek() != Some(')') {
                return Err("expected ')' after exponent".into());
            }
            self.pos += 1;
        }
        let exponent = match text.split_once('/') {
            Some((a, b)) => {
                let a: f64 = a.parse().map_err(|_| format!("bad exponent '{text}'"))?;
                let b: f64 = b.parse().map_err(|_| format!("bad exponent '{text}'"))?;
                a / b
            }
            None => text.parse().map_err(|_| format!("bad exponent '{text}'"))?,
        };
        Ok(base.powf(exponent))
    }

    fn atom(&mut self) -> Result<Unit, String> {
        self.skip_spaces();
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let u = self.quotient()?;
                self.skip_spaces();
                if self.peek() != Some(')') {
                    return Err("expected ')' in unit".into());
                }
                self.pos += 1;
                Ok(u)
            }
            Some('1') => {
                self.pos += 1;
                Ok(Unit::ONE)
            }
            Some(c) if is_unit_char(c) => {
                let start = self.pos;
                while self.peek().is_some_and(is_unit_char) {
                    self.pos += 1;
                }
                let symbol: String = self.chars[start..self.pos].iter().collect();
                lookup(&symbol).ok_or_else(|| format!("unknown unit '{symbol}'"))
            }
            Some(c) => Err(format!("unexpected '{c}' in unit")),
            None => Err("empty unit".into()),
        }
    }
}

fn is_unit_char(c: char) -> bool {
    c.is_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim_of(text: &str) -> String {
        parse_unit(text).unwrap().dimension.to_string()
    }

    #[test]
    fn derived_units_reduce_to_base_units() {
        assert_eq!(dim_of("J"), "m^2 kg s^-2");
        assert_eq!(dim_of("N m"), "m^2 kg s^-2");
        assert_eq!(dim_of("V/A"), dim_of("ohm"));
        assert_eq!(dim_of("Ω"), dim_of("ohm"));
        assert_eq!(dim_of("W s"), dim_of("J"));
        assert_eq!(dim_of("m/s^2"), "m s^-2");
        assert_eq!(dim_of("m s^-2"), "m s^-2");
        assert_eq!(dim_of("J/mol/K"), dim_of("J/(mol K)"));
        assert_eq!(dim_of("1"), "1");
        assert_eq!(dim_of("1/s"), "s^-1");
        assert_eq!(dim_of("currency/yr"), "s^-1 currency");
        assert_eq!(dim_of("bit/s"), dim_of("Hz"));
        assert_eq!(dim_of("m^(1/2)"), "m^0.5");
        assert_eq!(dim_of("C^2/(N m^2)"), dim_of("F/m"));
    }

    #[test]
    fn prefixes_and_factors() {
        let close = |text: &str, f: f64| {
            let u = parse_unit(text).unwrap();
            assert!((u.factor / f - 1.0).abs() < 1e-12, "{text}: {}", u.factor);
        };
        close("km", 1e3);
        close("mg", 1e-6);
        close("kg", 1.0);
        close("µs", 1e-6);
        close("kW h", 3.6e6);
        close("mL", 1e-6);
        close("GB", 8e9);
        close("cm^2", 1e-4);
        close("min", 60.0);
        close("mi", 1609.344);
        close("cd", 1.0);
        close("Pa", 1.0);
        close("mmHg", 133.322387415);
    }

    #[test]
    fn kwh_is_rejected() {
        // "kWh" is common but not valid SI notation; we ask for "kW h".
        assert!(parse_unit("kWh").is_err());
    }

    #[test]
    fn errors() {
        assert!(parse_unit("").is_err());
        assert!(parse_unit("furlong").is_err());
        assert!(parse_unit("kkg").is_err());
        assert!(parse_unit("m/").is_err());
        assert!(parse_unit("(m").is_err());
    }
}
