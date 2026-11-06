//! Recognising library formulas in formulas found from data.
//!
//! [`Library::identify`] compares a candidate (any function of some input
//! columns) with every formula of the library: it tries each way of
//! assigning the candidate's inputs to the formula's variables, and checks
//! numerically that both give the same values. Two kinds of partial match
//! are also found:
//!
//! - one variable of the formula is a fixed value in the data (VAT at
//!   `t = 0.21` in `tax = p * t`);
//! - one library constant takes another value (`g = 10` instead of
//!   `9.80665` in free fall).
//!
//! Column names are compared with each formula's symbols and aliases to rank
//! matches, so `price * quantity` is called a line total, not Ohm's law.

use crate::{Expr, Formula, Library};

/// How a variable of a library formula is given by the candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Binding {
    /// Input `i` of the candidate.
    Input(usize),
    /// A fixed value in the data (for example a tax rate of 0.21).
    Value(f64),
}

/// A library constant that has another value in the data.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedConstant {
    /// Constant id, such as `g`.
    pub id: String,
    /// Its value in the library.
    pub library_value: f64,
    /// The value the data uses.
    pub value: f64,
}

/// A library formula that computes the same values as a candidate.
#[derive(Debug, Clone)]
pub struct Match<'a> {
    /// The formula.
    pub formula: &'a Formula,
    /// For each variable of the formula, in declaration order.
    pub variables: Vec<Binding>,
    /// Library constants with another value (at most one).
    pub changed_constant: Option<ChangedConstant>,
    /// How many of the candidate's column names (and the output name) agree
    /// with the formula's symbols, names or aliases.
    pub name_score: usize,
}

impl Match<'_> {
    /// Whether the candidate is exactly the library formula, with every
    /// variable given by a column and the library's constants.
    pub fn is_exact(&self) -> bool {
        self.changed_constant.is_none()
            && self
                .variables
                .iter()
                .all(|b| matches!(b, Binding::Input(_)))
    }
}

/// Number of test points.
const POINTS: usize = 24;
/// Relative tolerance for two values to be equal.
const REL_TOL: f64 = 1e-6;

impl Library {
    /// Finds the library formulas that compute the same values as
    /// `candidate`, best first.
    ///
    /// `candidate` takes the values of its inputs, named `input_names`;
    /// `output_name` is the name of the computed column, when known. Inputs
    /// are tried with values between 0.5 and 2 (the formulas are compared as
    /// functions, not on the user's data).
    ///
    /// Generic formulas (one operation, no constants) are only returned when
    /// a column name agrees with the formula, so a plain product is not
    /// labelled with every product law of physics.
    ///
    /// ```
    /// use regrex_formulas::Library;
    ///
    /// let lib = Library::builtin();
    /// let found = lib.identify(&|x: &[f64]| 0.5 * x[0] * x[1] * x[1], &["mass", "speed"], Some("energy"));
    /// assert_eq!(found[0].formula.id, "kinetic_energy");
    /// assert!(found[0].is_exact());
    /// ```
    pub fn identify<'a>(
        &'a self,
        candidate: &dyn Fn(&[f64]) -> f64,
        input_names: &[&str],
        output_name: Option<&str>,
    ) -> Vec<Match<'a>> {
        let n = input_names.len();
        if n == 0 {
            return Vec::new();
        }
        let points = test_points(n);
        let wanted: Vec<f64> = points.iter().map(|p| candidate(p)).collect();
        if wanted.iter().filter(|w| w.is_finite()).count() < POINTS / 2 {
            return Vec::new();
        }
        // `a * b` matches "relativistic momentum" too, because `v^2 / c^2`
        // vanishes at the test points: a candidate this plain is only named
        // with hints from column names, whatever formula it matches.
        let plain = is_plain(&points, &wanted);
        let mut out = Vec::new();
        for formula in &self.formulas {
            let nv = formula.variables.len();
            if nv != n && nv != n + 1 {
                continue;
            }
            for assignment in assignments(n, nv) {
                if let Some(m) = match_with(formula, &assignment, &points, &wanted) {
                    let name_score = name_score(formula, &assignment, input_names, output_name);
                    let m = Match { name_score, ..m };
                    if plausible(&m) && (!plain || m.name_score >= 2) {
                        out.push(m);
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            b.name_score
                .cmp(&a.name_score)
                .then(b.is_exact().cmp(&a.is_exact()))
                .then(a.formula.variables.len().cmp(&b.formula.variables.len()))
        });
        out
    }
}

/// Whether the candidate's values are those of one input, or of a sum,
/// difference, product or quotient of two.
fn is_plain(points: &[Vec<f64>], wanted: &[f64]) -> bool {
    let n = points.first().map_or(0, Vec::len);
    let fits = |f: &dyn Fn(&[f64]) -> f64| {
        points
            .iter()
            .zip(wanted)
            .filter(|(_, w)| w.is_finite())
            .all(|(p, &w)| close(f(p), w))
    };
    (0..n).any(|i| fits(&|p| p[i]))
        || (0..n).any(|i| {
            (0..n).filter(|&j| j != i).any(|j| {
                fits(&|p| p[i] + p[j])
                    || fits(&|p| p[i] - p[j])
                    || fits(&|p| p[i] * p[j])
                    || fits(&|p| p[i] / p[j])
            })
        })
}

/// Pseudo-random points in `[0.5, 2)`, the same every time.
fn test_points(n: usize) -> Vec<Vec<f64>> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..POINTS)
        .map(|_| {
            (0..n)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    0.5 + 1.5 * ((state >> 11) as f64 / (1u64 << 53) as f64)
                })
                .collect()
        })
        .collect()
}

/// Every injective map from `n` inputs to `nv` variables, as the variable of
/// each input.
fn assignments(n: usize, nv: usize) -> Vec<Vec<usize>> {
    fn go(n: usize, nv: usize, current: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if current.len() == n {
            out.push(current.clone());
            return;
        }
        for v in 0..nv {
            if !current.contains(&v) {
                current.push(v);
                go(n, nv, current, out);
                current.pop();
            }
        }
    }
    let mut out = Vec::new();
    if n <= 5 {
        go(n, nv, &mut Vec::new(), &mut out);
    }
    out
}

fn close(got: f64, want: f64) -> bool {
    got.is_finite() && (got - want).abs() <= REL_TOL * want.abs().max(1e-12)
}

/// Checks `formula` against the candidate's values with inputs assigned by
/// `assignment`; a variable left over, or one changed constant, is solved.
fn match_with<'a>(
    formula: &'a Formula,
    assignment: &[usize],
    points: &[Vec<f64>],
    wanted: &[f64],
) -> Option<Match<'a>> {
    let nv = formula.variables.len();
    let leftover = (0..nv).find(|v| !assignment.contains(v));
    let vars_at = |point: &[f64], extra: f64| {
        let mut vars = vec![extra; nv];
        for (input, &v) in assignment.iter().enumerate() {
            vars[v] = point[input];
        }
        vars
    };
    let agrees = |expr: &Expr, extra: f64| {
        let mut compared = 0;
        for (p, &w) in points.iter().zip(wanted) {
            if !w.is_finite() {
                continue;
            }
            if !close(expr.eval(&vars_at(p, extra)), w) {
                return false;
            }
            compared += 1;
        }
        compared >= POINTS / 2
    };
    let bindings = |extra: f64| {
        (0..nv)
            .map(|v| match assignment.iter().position(|&a| a == v) {
                Some(input) => Binding::Input(input),
                None => Binding::Value(extra),
            })
            .collect()
    };
    let first = points
        .iter()
        .zip(wanted)
        .find(|(_, w)| w.is_finite())
        .map(|(p, &w)| (p.clone(), w))?;

    if leftover.is_none() {
        if agrees(&formula.expr, 0.0) {
            return Some(Match {
                formula,
                variables: bindings(0.0),
                changed_constant: None,
                name_score: 0,
            });
        }
        // One library constant with another value (g = 10).
        let mut ids: Vec<&str> = formula.expr.constants();
        ids.dedup();
        for id in ids {
            let library_value = constant_value(&formula.expr, id)?;
            let solved = solve(
                |c| replace_constant(&formula.expr, id, c).eval(&vars_at(&first.0, 0.0)),
                first.1,
                library_value,
            )?;
            let value = round_sig(solved, 6);
            let changed = replace_constant(&formula.expr, id, value);
            if close(value, library_value) || !agrees(&changed, 0.0) {
                continue;
            }
            return Some(Match {
                formula,
                variables: bindings(0.0),
                changed_constant: Some(ChangedConstant {
                    id: id.to_string(),
                    library_value,
                    value,
                }),
                name_score: 0,
            });
        }
        return None;
    }

    // One variable is a fixed value in the data.
    let solved = solve(|x| formula.expr.eval(&vars_at(&first.0, x)), first.1, 1.0)?;
    let value = round_sig(solved, 6);
    agrees(&formula.expr, value).then(|| Match {
        formula,
        variables: bindings(value),
        changed_constant: None,
        name_score: 0,
    })
}

/// Solves `f(x) = target` by the secant method, starting near `start`.
fn solve(f: impl Fn(f64) -> f64, target: f64, start: f64) -> Option<f64> {
    let g = |x: f64| f(x) - target;
    for s in [start, -start, 0.1 * start, 10.0 * start] {
        let (mut x0, mut x1) = (s, s * 1.1 + 1e-3);
        let (mut g0, mut g1) = (g(x0), g(x1));
        for _ in 0..80 {
            if !g0.is_finite() || !g1.is_finite() || g1 == g0 {
                break;
            }
            if g1.abs() <= 1e-13 * target.abs().max(1e-12) {
                return Some(x1);
            }
            let x2 = x1 - g1 * (x1 - x0) / (g1 - g0);
            (x0, g0) = (x1, g1);
            x1 = x2;
            g1 = g(x1);
        }
        if g1.is_finite() && g1.abs() <= 1e-9 * target.abs().max(1e-12) {
            return Some(x1);
        }
    }
    None
}

fn round_sig(x: f64, digits: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let scale = 10f64.powi(digits - 1 - x.abs().log10().floor() as i32);
    (x * scale).round() / scale
}

fn constant_value(e: &Expr, id: &str) -> Option<f64> {
    match e {
        Expr::Const { id: c, value, .. } if c == id => Some(*value),
        Expr::Neg(a) => constant_value(a, id),
        Expr::Binary(_, a, b) => constant_value(a, id).or_else(|| constant_value(b, id)),
        Expr::Call(_, args) => args.iter().find_map(|a| constant_value(a, id)),
        _ => None,
    }
}

fn replace_constant(e: &Expr, id: &str, value: f64) -> Expr {
    match e {
        Expr::Const { id: c, .. } if c == id => Expr::Num(value),
        Expr::Neg(a) => Expr::Neg(Box::new(replace_constant(a, id, value))),
        Expr::Binary(op, a, b) => Expr::Binary(
            *op,
            Box::new(replace_constant(a, id, value)),
            Box::new(replace_constant(b, id, value)),
        ),
        Expr::Call(f, args) => Expr::Call(
            *f,
            args.iter()
                .map(|a| replace_constant(a, id, value))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether a match is likely to be meaningful rather than a numerical
/// coincidence. Partial matches can turn many formulas into the candidate
/// (a trapezoid's area with `h = 2` is a plain sum), so they need column
/// names that agree with the formula.
fn plausible(m: &Match<'_>) -> bool {
    if let Some(c) = &m.changed_constant {
        let ratio = c.value / c.library_value;
        return m.name_score >= 1 && (0.5..=2.0).contains(&ratio);
    }
    if !m.is_exact() {
        let positive = m.variables.iter().all(|b| match b {
            Binding::Value(v) => *v > 0.0,
            Binding::Input(_) => true,
        });
        return m.name_score >= 2 && positive;
    }
    if is_generic(m.formula) {
        // `a + b` is everywhere: one shared word such as "total" is not
        // enough to call it "capacitors in parallel".
        return m.name_score >= 2;
    }
    m.name_score >= 1 || m.formula.operators().len() >= 2
}

/// A formula with a single operation and no constants or numbers, such as
/// `a * b` or `a / b`: too common to name without a hint from column names.
fn is_generic(formula: &Formula) -> bool {
    formula.operators().len() <= 1
        && formula.expr.constants().is_empty()
        && !has_number(&formula.expr)
}

fn has_number(e: &Expr) -> bool {
    match e {
        Expr::Num(x) => *x != 1.0,
        Expr::Neg(a) => has_number(a),
        Expr::Binary(_, a, b) => has_number(a) || has_number(b),
        Expr::Call(_, args) => args.iter().any(has_number),
        _ => false,
    }
}

/// Lowercase, without a unit suffix such as ` (m/s)`, with spaces and dashes
/// as underscores.
fn normalize(name: &str) -> String {
    let base = name.split(['(', '[']).next().unwrap_or(name);
    base.trim().to_lowercase().replace([' ', '-'], "_")
}

/// Whether a column name refers to a quantity with this symbol, name and
/// aliases: equal after normalizing, or one is a whole word of the other
/// (`vat` and `vat_amount`, `net` and `net amount`).
fn names_agree(column: &str, symbol: &str, name: &str, aliases: &[String]) -> bool {
    let col = normalize(column);
    if col.is_empty() {
        return false;
    }
    if col == normalize(symbol) {
        return true;
    }
    let words = |s: &str| -> Vec<String> {
        s.split('_')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let col_words = words(&col);
    std::iter::once(name)
        .chain(aliases.iter().map(String::as_str))
        .map(normalize)
        .any(|a| {
            a == col
                || (col.len() >= 3 && words(&a).contains(&col))
                || (a.len() >= 3 && col_words.contains(&a))
        })
}

fn name_score(
    formula: &Formula,
    assignment: &[usize],
    input_names: &[&str],
    output_name: Option<&str>,
) -> usize {
    let mut score = 0;
    for (input, &v) in assignment.iter().enumerate() {
        let q = &formula.variables[v];
        if names_agree(input_names[input], &q.symbol, &q.name, &q.aliases) {
            score += 1;
        }
    }
    if let Some(out) = output_name {
        let q = &formula.output;
        if names_agree(out, &q.symbol, &q.name, &q.aliases) {
            score += 1;
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> &'static Library {
        Library::builtin()
    }

    #[test]
    fn exact_match_with_names() {
        let found = lib().identify(&|x| x[0] * x[1], &["price", "quantity"], Some("total"));
        assert_eq!(found[0].formula.id, "line_total");
        assert!(found[0].is_exact());
        assert_eq!(
            found[0].variables,
            vec![Binding::Input(0), Binding::Input(1)]
        );
    }

    #[test]
    fn generic_formulas_need_a_name_hint() {
        let found = lib().identify(&|x| x[0] * x[1], &["foo", "bar"], Some("baz"));
        assert!(
            found.iter().all(|m| !is_generic(m.formula)),
            "{:?}",
            found.iter().map(|m| &m.formula.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_variable_fixed_in_the_data() {
        let found = lib().identify(&|x| 0.21 * x[0], &["net"], Some("vat"));
        let m = found
            .iter()
            .find(|m| m.formula.id == "tax_amount")
            .expect("tax amount");
        assert_eq!(m.variables, vec![Binding::Input(0), Binding::Value(0.21)]);
        assert!(!m.is_exact());
    }

    #[test]
    fn a_changed_constant() {
        // Free fall with g = 10.
        let found = lib().identify(&|x| 5.0 * x[0] * x[0], &["t"], Some("distance"));
        let m = found
            .iter()
            .find(|m| m.changed_constant.as_ref().is_some_and(|c| c.id == "g"))
            .expect("a match with g changed");
        assert_eq!(m.changed_constant.as_ref().unwrap().value, 10.0);
    }

    #[test]
    fn one_shared_word_does_not_name_a_generic_formula() {
        // "total" appears in "total capacitance", but net + vat is not about
        // capacitors.
        let found = lib().identify(&|x| x[0] + x[1], &["net", "vat"], Some("total"));
        let ids: Vec<&str> = found.iter().map(|m| m.formula.id.as_str()).collect();
        assert!(!ids.contains(&"capacitors_in_parallel"), "{ids:?}");
    }

    #[test]
    fn argument_order_does_not_matter() {
        let found = lib().identify(&|x| 0.5 * x[1] * x[0] * x[0], &["v", "m"], None);
        assert_eq!(found[0].formula.id, "kinetic_energy");
        assert_eq!(
            found[0].variables,
            vec![Binding::Input(1), Binding::Input(0)]
        );
    }

    #[test]
    fn a_plain_product_is_not_a_limit_of_a_physics_formula() {
        // m * v / sqrt(1 - v^2 / c^2) is m * v at everyday speeds.
        let found = lib().identify(
            &|x: &[f64]| x[0] * x[1],
            &["capacity", "occupancy_pct"],
            Some("passengers"),
        );
        assert!(
            found
                .iter()
                .all(|m| m.formula.id != "relativistic_momentum"),
            "{:?}",
            found.iter().map(|m| &m.formula.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unknown_formulas_give_nothing() {
        assert!(lib()
            .identify(&|x| x[0].sin() * 7.3 + x[1].powi(5), &["a", "b"], None)
            .is_empty());
    }
}
