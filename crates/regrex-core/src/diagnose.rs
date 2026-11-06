//! Explaining rows that break a formula.
//!
//! Given the formula a column should follow and a row where it does not,
//! [`diagnose`] looks for a small, human mistake that reproduces the value
//! in the row exactly: a mistyped digit, a wrong constant (`g = 10` instead
//! of `9.81`, VAT at 10% instead of 21%), a unit or percentage factor, a
//! forgotten square, two values swapped, a wrong column, a missing term…
//!
//! Each mistake is checked by recomputing the row with it, so a diagnosis is
//! never a guess about the numbers: it is a formula, close to the expected
//! one, that gives the value actually written.

use crate::expr::{constant_complexity, format_number, BinaryOp, Expr, UnaryOp};

/// What kind of mistake a [`Diagnosis`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MistakeKind {
    /// The value was rounded or truncated to fewer decimals.
    Rounding,
    /// One digit is wrong, or two adjacent digits are swapped.
    Typo,
    /// The sign is wrong.
    Sign,
    /// The value is off by a factor such as 100 (percent), 1000 or 60
    /// (units).
    Factor,
    /// A constant of the formula has another value.
    WrongConstant,
    /// One column was used instead of another.
    WrongVariable,
    /// Two columns were swapped.
    SwappedVariables,
    /// An operation such as a square or a square root was left out.
    MissingOperation,
    /// A term or factor of the formula was left out.
    MissingTerm,
    /// An operation was replaced by another (`-` instead of `+`, `*`
    /// instead of `/`, square instead of square root).
    WrongOperation,
    /// The operands of `-`, `/` or `^` were written in the wrong order.
    SwappedOperands,
    /// The value is right, but another cell of the row it is computed from
    /// is wrong.
    OtherCell,
    /// The value is a whole day off (1440 minutes, 24 hours or 86400
    /// seconds): a time that went past midnight, as in a train arriving at
    /// 00:10 after leaving at 23:50.
    Midnight,
    /// The cell holds 0 where the formula gives another value: usually a
    /// missing value written as 0, or a sensor or switch that was off.
    Zero,
}

impl MistakeKind {
    /// A short, stable identifier, such as `wrong_constant`.
    pub fn id(self) -> &'static str {
        match self {
            MistakeKind::Rounding => "rounding",
            MistakeKind::Typo => "typo",
            MistakeKind::Sign => "sign",
            MistakeKind::Factor => "factor",
            MistakeKind::WrongConstant => "wrong_constant",
            MistakeKind::WrongVariable => "wrong_variable",
            MistakeKind::SwappedVariables => "swapped_variables",
            MistakeKind::MissingOperation => "missing_operation",
            MistakeKind::MissingTerm => "missing_term",
            MistakeKind::WrongOperation => "wrong_operation",
            MistakeKind::SwappedOperands => "swapped_operands",
            MistakeKind::OtherCell => "other_cell",
            MistakeKind::Midnight => "midnight",
            MistakeKind::Zero => "zero",
        }
    }
}

/// A mistake that reproduces the value of a row.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnosis {
    /// The kind of mistake.
    pub kind: MistakeKind,
    /// A short explanation, such as `used 0.1 instead of 0.21`.
    pub description: String,
    /// The formula the row actually follows, when the mistake is in the
    /// formula (not for typos, rounding or factors).
    pub formula: Option<Expr>,
}

/// Explains why `actual` differs from what `formula` gives for `row`.
///
/// `row` holds the value of every variable of the formula (`Expr::Var(i)`
/// reads `row[i]`), `names` their names for the descriptions, `tolerance`
/// how close a recomputed value must be to `actual`, and `decimals` the
/// number of decimals `actual` was written with, when known.
///
/// Returns the mistakes that reproduce `actual`, most likely first; it is
/// empty when no simple mistake explains the value.
///
/// ```
/// use regrex_core::diagnose::{diagnose, MistakeKind};
/// use regrex_core::parse::parse_expr;
///
/// let formula = parse_expr("0.21 * net", &["net"]).unwrap();
/// let names = ["net".to_string()];
/// let found = diagnose(&formula, &names, &[299.7], 29.97, 0.005, Some(2));
/// assert_eq!(found[0].kind, MistakeKind::WrongConstant);
/// assert_eq!(found[0].description, "used 0.1 instead of 0.21");
/// ```
pub fn diagnose(
    formula: &Expr,
    names: &[String],
    row: &[f64],
    actual: f64,
    tolerance: f64,
    decimals: Option<u32>,
) -> Vec<Diagnosis> {
    let expected = formula.eval_row(row, &[]);
    if !expected.is_finite() || !actual.is_finite() || (expected - actual).abs() <= tolerance {
        return Vec::new();
    }
    let checker = Checker {
        names,
        row,
        actual,
        tolerance,
    };
    // Value mistakes first, except typos in short numbers, which match by
    // chance too often to beat a mistake in the formula.
    let (short_typos, value): (Vec<_>, Vec<_>) =
        value_mistakes(expected, actual, tolerance, decimals)
            .into_iter()
            .partition(|d| d.kind == MistakeKind::Typo && significant_digits(actual) < 3);
    let mut found = value;
    found.extend(checker.formula_mistakes(formula));
    found.extend(short_typos);
    // Several mutations can describe the same mistake (for example a
    // forgotten factor of 2 and a factor error); keep the first of each text.
    let mut seen = std::collections::HashSet::new();
    found.retain(|d| seen.insert(d.description.clone()));
    found
}

/// Mistakes found by comparing the two numbers alone.
fn value_mistakes(
    expected: f64,
    actual: f64,
    tolerance: f64,
    decimals: Option<u32>,
) -> Vec<Diagnosis> {
    let mut out = Vec::new();
    let mut push = |kind, description: String| {
        out.push(Diagnosis {
            kind,
            description,
            formula: None,
        })
    };
    // A 0 is not a rounded, mistyped or scaled value: with an absolute
    // tolerance, `0` would "match" a hundredth or a thousandth of any small
    // expected value. Say what it most likely is instead.
    if actual == 0.0 {
        push(
            MistakeKind::Zero,
            "the cell is 0: a missing value, or a reading that was off".to_string(),
        );
        return out;
    }
    let decimals = decimals.unwrap_or_else(|| written_decimals(actual));

    // Rounded or truncated to fewer decimals.
    let plural = |d: u32| {
        if d == 1 {
            "1 decimal".to_string()
        } else {
            format!("{d} decimals")
        }
    };
    for d in (0..=decimals).rev() {
        let scale = 10f64.powi(d as i32);
        let close = |v: f64| (v - actual).abs() <= tolerance.max(1e-9 * actual.abs());
        if close((expected * scale).round() / scale) {
            push(MistakeKind::Rounding, format!("rounded to {}", plural(d)));
            break;
        }
        if close((expected * scale).trunc() / scale) {
            push(MistakeKind::Rounding, format!("truncated to {}", plural(d)));
            break;
        }
    }

    // A mistyped digit, or two swapped digits.
    let written = format!("{:.*}", decimals as usize, actual);
    let should = format!("{:.*}", decimals as usize, expected);
    if let Some(what) = typo(&should, &written) {
        push(
            MistakeKind::Typo,
            format!("{what}: {written} instead of {should}"),
        );
    }

    // A time past midnight: a day off, in minutes, hours or seconds.
    for (day, unit) in [(1440.0, "minutes"), (24.0, "hours"), (86400.0, "seconds")] {
        let off = actual - expected;
        if ((off.abs() - day).abs() <= tolerance.max(1e-9 * actual.abs())) && expected != 0.0 {
            let way = if off < 0.0 { "less" } else { "more" };
            push(
                MistakeKind::Midnight,
                format!(
                    "{} {unit} {way}: a time past midnight, a day apart",
                    format_number(day)
                ),
            );
        }
    }

    // Sign and factors (units, percentages, minutes). Only between values
    // that are clearly not zero: a value within rounding of 0 is close to
    // any fraction of a small number.
    let close = |k: f64| {
        actual.abs() > tolerance
            && expected.abs() > tolerance
            && (actual - k * expected).abs() <= tolerance.max(1e-9 * actual.abs())
    };
    if close(-1.0) {
        push(MistakeKind::Sign, "wrong sign".to_string());
    }
    for (k, text) in FACTORS {
        if close(k) {
            push(
                MistakeKind::Factor,
                format!("{} times the expected value ({text})", format_number(k)),
            );
        } else if close(1.0 / k) {
            push(
                MistakeKind::Factor,
                format!("1/{} of the expected value ({text})", format_number(k)),
            );
        }
    }
    out
}

/// Factors that come from mixing units or writing percentages.
const FACTORS: [(f64, &str); 6] = [
    (100.0, "percent vs fraction, or m vs cm"),
    (1000.0, "units, such as kg vs g or m vs mm"),
    (10.0, "a misplaced decimal point"),
    (60.0, "minutes vs seconds, or hours vs minutes"),
    (3600.0, "hours vs seconds"),
    (2.0, "a missing or extra factor of 2"),
];

/// Number of significant digits in the shortest way of writing `x`.
fn significant_digits(x: f64) -> usize {
    let s = format!("{}", x.abs());
    let digits: String = s
        .chars()
        .take_while(|&c| c != 'e')
        .filter(char::is_ascii_digit)
        .collect();
    digits.trim_start_matches('0').len()
}

/// Number of decimals in the shortest way of writing `x`.
fn written_decimals(x: f64) -> u32 {
    let s = format!("{x}");
    if s.contains('e') {
        return 0;
    }
    s.split_once('.').map_or(0, |(_, d)| d.len() as u32)
}

/// Describes how `written` is a typing mistake of `should`, if it is one.
fn typo(should: &str, written: &str) -> Option<&'static str> {
    let a: Vec<char> = should.chars().collect();
    let b: Vec<char> = written.chars().collect();
    if a == b {
        return None;
    }
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        match diff.as_slice() {
            [i] if a[*i].is_ascii_digit() && b[*i].is_ascii_digit() => {
                return Some("one digit is wrong")
            }
            [i, j]
                if *j == i + 1
                    && a[*i] == b[*j]
                    && a[*j] == b[*i]
                    && a[*i].is_ascii_digit()
                    && a[*j].is_ascii_digit() =>
            {
                return Some("two digits are swapped")
            }
            _ => {}
        }
    }
    // A digit typed twice or left out.
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if long.len() == short.len() + 1 {
        for i in 0..long.len() {
            if long[i].is_ascii_digit() && long[..i] == short[..i] && long[i + 1..] == short[i..] {
                return Some(if b.len() > a.len() {
                    "an extra digit"
                } else {
                    "a missing digit"
                });
            }
        }
    }
    None
}

struct Checker<'a> {
    names: &'a [String],
    row: &'a [f64],
    actual: f64,
    tolerance: f64,
}

impl Checker<'_> {
    fn reproduces(&self, e: &Expr, params: &[f64]) -> bool {
        let v = e.eval_row(self.row, params);
        v.is_finite() && (v - self.actual).abs() <= self.tolerance
    }

    fn name(&self, i: usize) -> String {
        self.names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("x{i}"))
    }

    fn text(&self, e: &Expr) -> String {
        e.display_with(self.names).to_string()
    }

    /// Mistakes in the formula, checked against the row.
    fn formula_mistakes(&self, formula: &Expr) -> Vec<Diagnosis> {
        if formula.num_params() > 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (mutated, kind, description) in self.mutations(formula) {
            if kind == MistakeKind::WrongConstant {
                // `mutated` has a parameter in place of one constant.
                if let Some(d) = self.wrong_constant(&mutated, &description) {
                    out.push(d);
                }
            } else if self.reproduces(&mutated, &[]) {
                out.push(Diagnosis {
                    kind,
                    description,
                    formula: Some(mutated),
                });
            }
        }
        // Swapping two columns everywhere in the formula.
        let mut vars = Vec::new();
        collect_vars(formula, &mut vars);
        for (n, &i) in vars.iter().enumerate() {
            for &j in &vars[n + 1..] {
                if self.row[i] == self.row[j] {
                    continue;
                }
                let swapped = map_vars(formula, &|v| {
                    if v == i {
                        j
                    } else if v == j {
                        i
                    } else {
                        v
                    }
                });
                if self.reproduces(&swapped, &[]) {
                    out.push(Diagnosis {
                        kind: MistakeKind::SwappedVariables,
                        description: format!("swapped {} and {}", self.name(i), self.name(j)),
                        formula: Some(swapped),
                    });
                }
            }
        }
        out
    }

    /// Finds a simple value for the parameter of `mutated` that reproduces
    /// the row; `original` is the text of the constant it replaced.
    fn wrong_constant(&self, mutated: &Expr, original: &str) -> Option<Diagnosis> {
        let old: f64 = original.parse().ok()?;
        let solved = solve_param(mutated, self.row, self.actual)?;
        let mut options: Vec<f64> = crate::fit::nice_values(solved);
        options.sort_by_key(|&v| constant_complexity(v));
        let value = options.into_iter().find(|&v| {
            v != old
                && constant_complexity(v) <= constant_complexity(old)
                && self.reproduces(mutated, &[v])
        })?;
        let fixed = mutated.substitute_params(&[value]);
        Some(Diagnosis {
            kind: MistakeKind::WrongConstant,
            description: format!(
                "used {} instead of {}",
                format_number(value),
                format_number(old)
            ),
            formula: Some(fixed),
        })
    }

    /// Every formula one small change away from `e`, with the kind of change
    /// and its description. For [`MistakeKind::WrongConstant`] the formula has
    /// `Expr::Param(0)` in place of the constant and the description is the
    /// old value.
    fn mutations(&self, e: &Expr) -> Vec<(Expr, MistakeKind, String)> {
        let mut out = Vec::new();
        match e {
            Expr::Const(c) => {
                // Keep the full precision of the old value in the text.
                out.push((Expr::Param(0), MistakeKind::WrongConstant, format!("{c:?}")));
            }
            Expr::Var(i) => {
                for j in 0..self.row.len() {
                    if j != *i && self.row[j] != self.row[*i] {
                        out.push((
                            Expr::Var(j),
                            MistakeKind::WrongVariable,
                            format!("used {} instead of {}", self.name(j), self.name(*i)),
                        ));
                    }
                }
            }
            Expr::Param(_) => {}
            Expr::Unary(op, a) => {
                let inner = (**a).clone();
                let what = match op {
                    UnaryOp::Neg => "the minus sign".to_string(),
                    UnaryOp::Square => format!("the square of {}", self.text(a)),
                    other => format!("{} of {}", other.name(), self.text(a)),
                };
                out.push((
                    inner.clone(),
                    MistakeKind::MissingOperation,
                    format!("left out {what}"),
                ));
                let swap = match op {
                    UnaryOp::Square => Some(UnaryOp::Sqrt),
                    UnaryOp::Sqrt => Some(UnaryOp::Square),
                    UnaryOp::Sin => Some(UnaryOp::Cos),
                    UnaryOp::Cos => Some(UnaryOp::Sin),
                    _ => None,
                };
                if let Some(other) = swap {
                    out.push((
                        Expr::unary(other, inner),
                        MistakeKind::WrongOperation,
                        format!("used {} instead of {}", other.name(), op.name()),
                    ));
                }
                for (m, kind, text) in self.mutations(a) {
                    out.push((Expr::unary(*op, m), kind, text));
                }
            }
            Expr::Binary(op, a, b) => {
                let (left, right) = ((**a).clone(), (**b).clone());
                let missing = |kept: &Expr, dropped: &Expr| match op {
                    BinaryOp::Add | BinaryOp::Sub => {
                        format!("left out the term {}", self.text(dropped))
                    }
                    BinaryOp::Mul => format!("left out the factor {}", self.text(dropped)),
                    BinaryOp::Div => format!("did not divide by {}", self.text(dropped)),
                    BinaryOp::Pow => match dropped {
                        Expr::Const(c) if *c == 2.0 => {
                            format!("left out the square of {}", self.text(kept))
                        }
                        Expr::Const(c) if *c == 0.5 => {
                            format!("left out the square root of {}", self.text(kept))
                        }
                        _ => format!(
                            "left out the power ^{} of {}",
                            self.text(dropped),
                            self.text(kept)
                        ),
                    },
                };
                out.push((
                    left.clone(),
                    if *op == BinaryOp::Pow {
                        MistakeKind::MissingOperation
                    } else {
                        MistakeKind::MissingTerm
                    },
                    missing(&left, &right),
                ));
                if *op != BinaryOp::Div && *op != BinaryOp::Pow {
                    out.push((
                        right.clone(),
                        MistakeKind::MissingTerm,
                        missing(&right, &left),
                    ));
                }
                let other = match op {
                    BinaryOp::Add => Some(BinaryOp::Sub),
                    BinaryOp::Sub => Some(BinaryOp::Add),
                    BinaryOp::Mul => Some(BinaryOp::Div),
                    BinaryOp::Div => Some(BinaryOp::Mul),
                    BinaryOp::Pow => None,
                };
                if let Some(other) = other {
                    out.push((
                        Expr::binary(other, left.clone(), right.clone()),
                        MistakeKind::WrongOperation,
                        format!("used {} instead of {}", other.symbol(), op.symbol()),
                    ));
                }
                if !op.is_commutative() {
                    out.push((
                        Expr::binary(*op, right.clone(), left.clone()),
                        MistakeKind::SwappedOperands,
                        format!(
                            "swapped {} and {} around {}",
                            self.text(&left),
                            self.text(&right),
                            op.symbol()
                        ),
                    ));
                }
                for (m, kind, text) in self.mutations(a) {
                    out.push((Expr::binary(*op, m, right.clone()), kind, text));
                }
                for (m, kind, text) in self.mutations(b) {
                    out.push((Expr::binary(*op, left.clone(), m), kind, text));
                }
            }
        }
        out
    }
}

/// Solves `e(row, [p]) = target` for the single parameter `p`, by the secant
/// method from a few starting points.
fn solve_param(e: &Expr, row: &[f64], target: f64) -> Option<f64> {
    let f = |p: f64| e.eval_row(row, &[p]) - target;
    for start in [1.0, 0.1, 10.0, -1.0] {
        let (mut p0, mut p1) = (start, start * 1.1 + 1e-3);
        let (mut f0, mut f1) = (f(p0), f(p1));
        for _ in 0..100 {
            if !f1.is_finite() || !f0.is_finite() {
                break;
            }
            if f1.abs() <= 1e-12 * target.abs().max(1.0) {
                return Some(p1);
            }
            if f1 == f0 {
                break;
            }
            let p2 = p1 - f1 * (p1 - p0) / (f1 - f0);
            (p0, f0) = (p1, f1);
            p1 = p2;
            f1 = f(p1);
        }
    }
    None
}

fn collect_vars(e: &Expr, out: &mut Vec<usize>) {
    match e {
        Expr::Var(i) => {
            if !out.contains(i) {
                out.push(*i);
            }
        }
        Expr::Const(_) | Expr::Param(_) => {}
        Expr::Unary(_, a) => collect_vars(a, out),
        Expr::Binary(_, a, b) => {
            collect_vars(a, out);
            collect_vars(b, out);
        }
    }
}

fn map_vars(e: &Expr, f: &dyn Fn(usize) -> usize) -> Expr {
    match e {
        Expr::Var(i) => Expr::Var(f(*i)),
        Expr::Const(_) | Expr::Param(_) => e.clone(),
        Expr::Unary(op, a) => Expr::unary(*op, map_vars(a, f)),
        Expr::Binary(op, a, b) => Expr::binary(*op, map_vars(a, f), map_vars(b, f)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_expr;

    fn check(
        formula: &str,
        vars: &[(&str, f64)],
        actual: f64,
        decimals: Option<u32>,
    ) -> Vec<Diagnosis> {
        let names: Vec<String> = vars.iter().map(|(n, _)| n.to_string()).collect();
        let refs: Vec<&str> = vars.iter().map(|(n, _)| *n).collect();
        let row: Vec<f64> = vars.iter().map(|(_, v)| *v).collect();
        let e = parse_expr(formula, &refs).unwrap();
        let tol = decimals.map_or(1e-9 * actual.abs(), |d| {
            0.5 * 10f64.powi(-(d as i32)) * 1.000_001
        });
        diagnose(&e, &names, &row, actual, tol, decimals)
    }

    fn first(found: &[Diagnosis]) -> (MistakeKind, &str) {
        let d = found.first().expect("a diagnosis");
        (d.kind, d.description.as_str())
    }

    #[test]
    fn nothing_to_explain() {
        assert!(check("2 * x", &[("x", 3.0)], 6.0, Some(2)).is_empty());
        let found = check("2 * x", &[("x", 3.0)], 7.37, Some(2));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn wrong_constants() {
        // VAT at 10% instead of 21%.
        let found = check("0.21 * net", &[("net", 299.7)], 29.97, Some(2));
        assert_eq!(
            first(&found),
            (MistakeKind::WrongConstant, "used 0.1 instead of 0.21")
        );
        // g = 10 instead of 9.81 in free fall.
        let found = check("0.5 * 9.81 * t^2", &[("t", 2.0)], 20.0, Some(2));
        assert!(
            found
                .iter()
                .any(|d| d.description == "used 10 instead of 9.81"),
            "{found:?}"
        );
    }

    #[test]
    fn typos() {
        assert!(check("net + vat", &[("net", 73.8), ("vat", 15.5)], 94.3, Some(1)).is_empty());
        let found = check("net + vat", &[("net", 73.8), ("vat", 15.5)], 89.8, Some(1));
        assert_eq!(
            first(&found),
            (
                MistakeKind::Typo,
                "one digit is wrong: 89.8 instead of 89.3"
            )
        );
        let found = check("net + vat", &[("net", 73.8), ("vat", 15.5)], 98.3, Some(1));
        assert_eq!(
            first(&found),
            (
                MistakeKind::Typo,
                "two digits are swapped: 98.3 instead of 89.3"
            )
        );
        assert_eq!(typo("12.50", "125.0"), None);
        assert_eq!(typo("1250", "12500"), Some("an extra digit"));
        assert_eq!(typo("1250", "120"), Some("a missing digit"));
    }

    #[test]
    fn rounding_sign_and_factors() {
        let found = check("x / 3", &[("x", 10.0)], 3.3, Some(2));
        assert_eq!(
            first(&found),
            (MistakeKind::Rounding, "rounded to 1 decimal")
        );
        let found = check("x / 3", &[("x", 20.0)], 6.66, Some(2));
        assert_eq!(first(&found).1, "truncated to 2 decimals");
        let found = check("a - b", &[("a", 2.0), ("b", 5.0)], 3.0, Some(0));
        assert!(found.iter().any(|d| d.kind == MistakeKind::Sign));
        assert!(found.iter().any(|d| d.kind == MistakeKind::SwappedOperands));
        let found = check("0.21 * net", &[("net", 50.0)], 1050.0, Some(2));
        assert_eq!(first(&found).0, MistakeKind::Factor);
    }

    #[test]
    fn zero_is_not_a_factor() {
        // A whole-number column holding 0 where the formula gives 2: within
        // half a unit of 2/100, but not a percentage mistake.
        let found = check("x + 1", &[("x", 1.0)], 0.0, Some(0));
        assert_eq!(
            first(&found),
            (
                MistakeKind::Zero,
                "the cell is 0: a missing value, or a reading that was off"
            )
        );
        assert!(found.iter().all(|d| d.kind != MistakeKind::Factor), "{found:?}");
        // Small values near zero are not scaled versions of each other either.
        let found = check("x / 100", &[("x", 30.0)], 0.0, Some(0));
        assert!(found.iter().all(|d| d.kind != MistakeKind::Factor), "{found:?}");
    }

    #[test]
    fn past_midnight() {
        // Left at 23:50 (1430), 20 minutes trip: arrives at 00:10 (10).
        let found = check(
            "dep + trip",
            &[("dep", 1430.0), ("trip", 20.0)],
            10.0,
            Some(0),
        );
        assert_eq!(
            first(&found),
            (
                MistakeKind::Midnight,
                "1440 minutes less: a time past midnight, a day apart"
            )
        );
    }

    #[test]
    fn structural_mistakes() {
        // Forgot the square of v.
        let found = check("0.5 * m * v^2", &[("m", 2.0), ("v", 3.0)], 3.0, Some(2));
        assert!(
            found.iter().any(|d| d.kind == MistakeKind::MissingOperation
                && d.description == "left out the square of v"),
            "{found:?}"
        );
        // Used the wrong column.
        let found = check(
            "price * quantity",
            &[("price", 2.5), ("quantity", 4.0), ("discount", 10.0)],
            25.0,
            Some(2),
        );
        assert!(
            found
                .iter()
                .any(|d| d.description == "used discount instead of quantity"),
            "{found:?}"
        );
        // Swapped two inputs.
        let found = check("a / b", &[("a", 6.0), ("b", 3.0), ("c", 7.0)], 0.5, Some(2));
        assert!(
            found
                .iter()
                .any(|d| d.kind == MistakeKind::SwappedVariables),
            "{found:?}"
        );
        // Missing term.
        let found = check("net + vat", &[("net", 80.0), ("vat", 16.8)], 80.0, Some(2));
        assert!(
            found
                .iter()
                .any(|d| d.description == "left out the term vat"),
            "{found:?}"
        );
        // The returned formula reproduces the value.
        let d = found.iter().find(|d| d.formula.is_some()).unwrap();
        assert_eq!(
            d.formula.as_ref().unwrap().eval_row(&[80.0, 16.8], &[]),
            80.0
        );
    }
}
