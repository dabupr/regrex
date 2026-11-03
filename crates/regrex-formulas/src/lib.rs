//! # regrex-formulas
//!
//! A library of well-known formulas and constants, grouped by domain
//! (physics, mathematics, chemistry, electronics, finance, computing...),
//! for [Regrex](https://github.com/dabupr/regrex).
//!
//! Each formula has its variables with SI units, the constants it uses and a
//! reference. The data lives in one TOML file per domain under
//! [`library/`](https://github.com/dabupr/regrex/tree/main/crates/regrex-formulas/library),
//! so adding a formula needs no Rust. Every file is checked when it is
//! loaded: names resolve, units parse, both sides of each formula have the
//! same dimension, and the worked examples evaluate to the stated result.
//!
//! Regrex uses the library for the "known formula" badge (a candidate found
//! in the data is equivalent to a formula of the library) and to build the
//! prior of each domain (which operators and constants are common there).
//!
//! ```
//! use regrex_formulas::Library;
//!
//! let lib = Library::builtin();
//! let f = lib.formula("kinetic_energy").unwrap();
//! assert_eq!(f.expr_text, "0.5 * m * v^2");
//! assert_eq!(f.output.unit, "J");
//! assert_eq!(f.eval(&[2.0, 3.0]), 9.0);
//!
//! // A candidate found by the search, with the columns in the same order.
//! assert!(f.equivalent_to(|x| x[0] * x[1] * x[1] / 2.0, 1e-9));
//!
//! let g = lib.constant("g").unwrap();
//! assert_eq!(g.value, 9.80665);
//! ```

#![warn(missing_docs)]

pub mod expr;
pub mod identify;
pub mod units;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

pub use expr::{Expr, Func, Op};
pub use identify::{Binding, ChangedConstant, Match};
pub use units::{parse_unit, Dimension, Unit};

/// The TOML files shipped with the crate, as `(file name, contents)`.
pub const BUILTIN_FILES: &[(&str, &str)] = &[
    (
        "mathematics.toml",
        include_str!("../library/mathematics.toml"),
    ),
    ("physics.toml", include_str!("../library/physics.toml")),
    ("chemistry.toml", include_str!("../library/chemistry.toml")),
    (
        "electronics.toml",
        include_str!("../library/electronics.toml"),
    ),
    ("finance.toml", include_str!("../library/finance.toml")),
    ("computing.toml", include_str!("../library/computing.toml")),
    ("health.toml", include_str!("../library/health.toml")),
    ("business.toml", include_str!("../library/business.toml")),
    ("energy.toml", include_str!("../library/energy.toml")),
    (
        "engineering.toml",
        include_str!("../library/engineering.toml"),
    ),
    ("astronomy.toml", include_str!("../library/astronomy.toml")),
    ("earth.toml", include_str!("../library/earth.toml")),
    (
        "conversions.toml",
        include_str!("../library/conversions.toml"),
    ),
];

/// Every formula of the built-in library (shorthand for
/// `Library::builtin().formulas`).
pub fn all() -> &'static [Formula] {
    &Library::builtin().formulas
}

/// A domain: one TOML file of the library.
#[derive(Debug, Clone)]
pub struct Domain {
    /// Short id, such as `physics`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// What the domain covers.
    pub description: String,
}

/// Where a formula or constant comes from.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Title of the reference (book, standard, article or web page).
    pub title: String,
    /// Link to the reference, if there is one.
    #[serde(default)]
    pub url: Option<String>,
}

/// A named constant, such as `g` or `c`.
#[derive(Debug, Clone)]
pub struct Constant {
    /// Id used in formulas (`k_B`).
    pub id: String,
    /// Usual symbol (`k_B`, `π`).
    pub symbol: String,
    /// What the constant is.
    pub name: String,
    /// Value in SI units.
    pub value: f64,
    /// Unit of the value, as written in the TOML file.
    pub unit: String,
    /// Parsed dimension of `unit`.
    pub dimension: Dimension,
    /// Whether the value is exact by definition.
    pub exact: bool,
    /// Standard uncertainty, for measured constants.
    pub uncertainty: Option<f64>,
    /// Domain id.
    pub domain: String,
    /// Reference.
    pub source: Source,
}

/// A variable or the result of a formula.
#[derive(Debug, Clone)]
pub struct Quantity {
    /// Symbol used in the formula (`m`, `v_0`, `ω`).
    pub symbol: String,
    /// What it is (`mass`).
    pub name: String,
    /// Unit, as written in the TOML file (`kg`, `m/s`).
    pub unit: String,
    /// Parsed `unit`.
    pub parsed_unit: Unit,
    /// Other names a dataset column may use for it (`price`, `qty`).
    pub aliases: Vec<String>,
}

/// A worked example: inputs in variable order and the expected result.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// Values of the variables, in the order they are declared.
    pub inputs: Vec<f64>,
    /// Expected value of the formula.
    pub output: f64,
}

/// A known formula.
#[derive(Debug, Clone)]
pub struct Formula {
    /// Unique id (`kinetic_energy`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Domain id.
    pub domain: String,
    /// Longer explanation, if any.
    pub description: Option<String>,
    /// The expression as written in the TOML file.
    pub expr_text: String,
    /// Parsed expression. Variables are indices into [`Formula::variables`].
    pub expr: Expr,
    /// What the formula computes.
    pub output: Quantity,
    /// Inputs of the formula.
    pub variables: Vec<Quantity>,
    /// Ids of the library constants it uses.
    pub constants: Vec<String>,
    /// Free-form tags (`mechanics`, `geometry`).
    pub tags: Vec<String>,
    /// Reference.
    pub source: Source,
    /// Worked examples.
    pub checks: Vec<Check>,
}

impl Formula {
    /// Evaluates the formula with the variables in declaration order, in the
    /// units declared for them.
    pub fn eval(&self, inputs: &[f64]) -> f64 {
        self.expr.eval(inputs)
    }

    /// Whether `candidate` computes the same function as this formula.
    ///
    /// Both are evaluated on a fixed set of pseudo-random points (inputs in
    /// `[0.1, 1.0)`, in declaration order) and must agree within `rel_tol`
    /// relative error on every point where the formula is defined. This is
    /// how a candidate found by the search gets the "known formula" badge
    /// once its columns are mapped to the formula's variables.
    pub fn equivalent_to(&self, candidate: impl Fn(&[f64]) -> f64, rel_tol: f64) -> bool {
        let n = self.variables.len();
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut point = vec![0.0; n];
        let mut compared = 0;
        for _ in 0..64 {
            for x in point.iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *x = 0.1 + 0.9 * ((state >> 11) as f64 / (1u64 << 53) as f64);
            }
            let want = self.eval(&point);
            if !want.is_finite() {
                continue;
            }
            let got = candidate(&point);
            let scale = want.abs().max(1e-300);
            if !within(got, want, rel_tol * scale) {
                return false;
            }
            compared += 1;
        }
        compared >= 16
    }

    /// Names of the operators and functions the formula uses, once per
    /// occurrence (see [`Expr::operators`]).
    pub fn operators(&self) -> Vec<&'static str> {
        self.expr.operators()
    }
}

/// One problem found while loading the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// File where the problem is.
    pub file: String,
    /// Formula or constant id, if the problem is inside one.
    pub item: Option<String>,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.item {
            Some(item) => write!(f, "{}: {}: {}", self.file, item, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// Every problem found while loading the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError(pub Vec<Problem>);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} problem(s) in the formula library:", self.0.len())?;
        for p in &self.0 {
            writeln!(f, "  {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for LoadError {}

/// The whole library: domains, constants and formulas.
#[derive(Debug, Clone)]
pub struct Library {
    /// Domains, in file order.
    pub domains: Vec<Domain>,
    /// Constants of every domain.
    pub constants: Vec<Constant>,
    /// Formulas of every domain.
    pub formulas: Vec<Formula>,
}

impl Library {
    /// The library shipped with the crate. It is parsed once, on first use.
    pub fn builtin() -> &'static Library {
        static LIB: OnceLock<Library> = OnceLock::new();
        LIB.get_or_init(|| {
            // The built-in files are validated by the test suite.
            Library::from_sources(BUILTIN_FILES).unwrap_or_else(|e| panic!("{e}"))
        })
    }

    /// Loads and validates a library from TOML files given as
    /// `(file name, contents)`. Constants of any file can be used by the
    /// formulas of any other file.
    ///
    /// Every problem found is reported, not just the first one.
    pub fn from_sources<N: AsRef<str>, T: AsRef<str>>(
        files: &[(N, T)],
    ) -> Result<Library, LoadError> {
        let mut problems = Vec::new();
        let mut parsed = Vec::new();
        for (name, text) in files {
            match toml::from_str::<RawFile>(text.as_ref()) {
                Ok(raw) => parsed.push((name.as_ref().to_string(), raw)),
                Err(e) => problems.push(Problem {
                    file: name.as_ref().to_string(),
                    item: None,
                    message: e.to_string().trim_end().replace('\n', "\n    "),
                }),
            }
        }
        let lib = build(&parsed, &mut problems);
        if problems.is_empty() {
            Ok(lib)
        } else {
            Err(LoadError(problems))
        }
    }

    /// A formula by id.
    pub fn formula(&self, id: &str) -> Option<&Formula> {
        self.formulas.iter().find(|f| f.id == id)
    }

    /// A constant by id.
    pub fn constant(&self, id: &str) -> Option<&Constant> {
        self.constants.iter().find(|c| c.id == id)
    }

    /// A domain by id.
    pub fn domain(&self, id: &str) -> Option<&Domain> {
        self.domains.iter().find(|d| d.id == id)
    }

    /// Formulas of a domain.
    pub fn formulas_in<'a>(&'a self, domain: &'a str) -> impl Iterator<Item = &'a Formula> + 'a {
        self.formulas.iter().filter(move |f| f.domain == domain)
    }

    /// Constants defined in a domain.
    pub fn constants_in<'a>(&'a self, domain: &'a str) -> impl Iterator<Item = &'a Constant> + 'a {
        self.constants.iter().filter(move |c| c.domain == domain)
    }

    /// Constants used by the formulas of a domain, with how many formulas
    /// use each. Useful as the list of constants to try first in that domain.
    pub fn constants_used_in<'a>(&'a self, domain: &str) -> BTreeMap<&'a str, usize> {
        let mut out = BTreeMap::new();
        for f in self.formulas.iter().filter(|f| f.domain == domain) {
            for c in &f.constants {
                *out.entry(c.as_str()).or_insert(0) += 1;
            }
        }
        out
    }

    /// How often each operator and function appears in the formulas of a
    /// domain (`None` for the whole library). This is the raw material of
    /// the per-domain prior.
    pub fn operator_counts(&self, domain: Option<&str>) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for f in &self.formulas {
            if domain.is_some_and(|d| d != f.domain) {
                continue;
            }
            for op in f.operators() {
                *out.entry(op).or_insert(0) += 1;
            }
        }
        out
    }
}

// --- TOML layout -----------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    id: String,
    name: String,
    description: String,
    #[serde(default)]
    constant: Vec<RawConstant>,
    #[serde(default)]
    formula: Vec<RawFormula>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConstant {
    id: String,
    symbol: Option<String>,
    name: String,
    value: f64,
    unit: String,
    #[serde(default)]
    exact: bool,
    uncertainty: Option<f64>,
    source: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawQuantity {
    symbol: String,
    name: String,
    unit: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFormula {
    id: String,
    name: String,
    description: Option<String>,
    expr: String,
    output: RawQuantity,
    variables: Vec<RawQuantity>,
    #[serde(default)]
    tags: Vec<String>,
    source: Source,
    #[serde(default)]
    checks: Vec<Check>,
}

// --- Validation ------------------------------------------------------------

/// `|a - b| <= tol`, false if either is NaN.
fn within(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn check_source(source: &Source, report: &mut impl FnMut(String)) {
    if source.title.trim().is_empty() {
        report("source title is empty".into());
    }
    if let Some(url) = &source.url {
        if !url.starts_with("https://") || url.contains(char::is_whitespace) {
            report(format!("source url must be an https:// link: '{url}'"));
        }
    }
}

fn build(files: &[(String, RawFile)], problems: &mut Vec<Problem>) -> Library {
    let mut lib = Library {
        domains: Vec::new(),
        constants: Vec::new(),
        formulas: Vec::new(),
    };

    // Domains and constants first, so formulas can use constants of any file.
    let mut domain_ids = HashSet::new();
    let mut constant_ids = HashSet::new();
    for (file, raw) in files {
        let mut report = |item: Option<&str>, message: String| {
            problems.push(Problem {
                file: file.clone(),
                item: item.map(str::to_string),
                message,
            })
        };
        if !is_snake_case(&raw.id) {
            report(None, format!("domain id '{}' must be snake_case", raw.id));
        }
        if !domain_ids.insert(raw.id.clone()) {
            report(None, format!("duplicate domain id '{}'", raw.id));
        }
        if raw.name.trim().is_empty() || raw.description.trim().is_empty() {
            report(None, "domain name and description must not be empty".into());
        }
        lib.domains.push(Domain {
            id: raw.id.clone(),
            name: raw.name.clone(),
            description: raw.description.clone(),
        });

        for c in &raw.constant {
            let mut report_c = |m: String| report(Some(&c.id), m);
            if !expr::is_identifier(&c.id) {
                report_c("constant id must be a valid name (letters, digits, _)".into());
            }
            if Func::from_name(&c.id).is_some() {
                report_c("constant id clashes with a function name".into());
            }
            if !constant_ids.insert(c.id.clone()) {
                report_c("duplicate constant id".into());
            }
            if c.name.trim().is_empty() {
                report_c("name is empty".into());
            }
            if !c.value.is_finite() {
                report_c("value is not finite".into());
            }
            if c.exact && c.uncertainty.is_some() {
                report_c("an exact constant has no uncertainty".into());
            }
            check_source(&c.source, &mut report_c);
            let dimension = match parse_unit(&c.unit) {
                Ok(u) => {
                    if (u.factor - 1.0).abs() > 1e-12 {
                        report_c(format!(
                            "constants must use coherent SI units, not '{}'",
                            c.unit
                        ));
                    }
                    u.dimension
                }
                Err(e) => {
                    report_c(e);
                    Dimension::NONE
                }
            };
            lib.constants.push(Constant {
                id: c.id.clone(),
                symbol: c.symbol.clone().unwrap_or_else(|| c.id.clone()),
                name: c.name.clone(),
                value: c.value,
                unit: c.unit.clone(),
                dimension,
                exact: c.exact,
                uncertainty: c.uncertainty,
                domain: raw.id.clone(),
                source: c.source.clone(),
            });
        }
    }

    let constants: HashMap<&str, &Constant> =
        lib.constants.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut formula_ids = HashSet::new();
    let mut formulas = Vec::new();
    for (file, raw) in files {
        for f in &raw.formula {
            let mut report = |m: String| {
                problems.push(Problem {
                    file: file.clone(),
                    item: Some(f.id.clone()),
                    message: m,
                })
            };
            if let Some(formula) = build_formula(f, &raw.id, &constants, &mut report) {
                if !formula_ids.insert(formula.id.clone()) {
                    report("duplicate formula id".into());
                }
                formulas.push(formula);
            }
        }
    }
    lib.formulas = formulas;
    lib
}

fn quantity(raw: &RawQuantity, report: &mut impl FnMut(String)) -> Quantity {
    if !expr::is_identifier(&raw.symbol) {
        report(format!("symbol '{}' is not a valid name", raw.symbol));
    }
    if raw.name.trim().is_empty() {
        report(format!("'{}' has an empty name", raw.symbol));
    }
    let parsed_unit = parse_unit(&raw.unit).unwrap_or_else(|e| {
        report(format!("'{}': {e}", raw.symbol));
        Unit {
            dimension: Dimension::NONE,
            factor: 1.0,
        }
    });
    Quantity {
        symbol: raw.symbol.clone(),
        name: raw.name.clone(),
        unit: raw.unit.clone(),
        parsed_unit,
        aliases: raw.aliases.clone(),
    }
}

fn build_formula(
    f: &RawFormula,
    domain: &str,
    constants: &HashMap<&str, &Constant>,
    report: &mut impl FnMut(String),
) -> Option<Formula> {
    if !is_snake_case(&f.id) {
        report("formula id must be snake_case".into());
    }
    if f.name.trim().is_empty() {
        report("name is empty".into());
    }
    for t in &f.tags {
        if !is_snake_case(t) {
            report(format!("tag '{t}' must be snake_case"));
        }
    }
    check_source(&f.source, report);
    let output = quantity(&f.output, report);
    let variables: Vec<Quantity> = f.variables.iter().map(|v| quantity(v, report)).collect();
    if variables.is_empty() {
        report("a formula needs at least one variable".into());
    }
    let mut seen = HashSet::new();
    for v in &variables {
        if !seen.insert(v.symbol.as_str()) {
            report(format!("variable '{}' is declared twice", v.symbol));
        }
        if v.symbol == output.symbol {
            report(format!("'{}' is both an input and the output", v.symbol));
        }
        if Func::from_name(&v.symbol).is_some() {
            report(format!(
                "variable '{}' clashes with a function name",
                v.symbol
            ));
        }
    }

    let resolve = |name: &str| {
        if let Some(i) = variables.iter().position(|v| v.symbol == name) {
            return Some(expr::Name::Var(i));
        }
        constants
            .get(name)
            .map(|c| expr::Name::Const(c.value, c.dimension))
    };
    let parsed = match Expr::parse(&f.expr, resolve) {
        Ok(e) => e,
        Err(e) => {
            report(format!("cannot parse '{}': {e}", f.expr));
            return None;
        }
    };

    let uses_constants = parsed.constants().iter().any(|id| {
        constants
            .get(id)
            .is_some_and(|c| !c.dimension.is_dimensionless())
    });
    if uses_constants {
        for q in variables.iter().chain(std::iter::once(&output)) {
            if (q.parsed_unit.factor - 1.0).abs() > 1e-12 {
                report(format!(
                    "'{}' is in {}, but formulas that use library constants with a dimension must \
                     use coherent SI units (constants are stored in SI)",
                    q.symbol, q.unit
                ));
            }
        }
    }

    let used = parsed.variables();
    for (i, v) in variables.iter().enumerate() {
        if !used.contains(&i) {
            report(format!("variable '{}' is declared but not used", v.symbol));
        }
    }

    let dims: Vec<Dimension> = variables.iter().map(|v| v.parsed_unit.dimension).collect();
    match parsed.dimension(&dims) {
        Ok(d) if d.same_as(&output.parsed_unit.dimension) => {}
        Ok(d) => report(format!(
            "dimensions do not match: the formula gives [{d}] but '{}' is in {} [{}]",
            output.symbol, output.unit, output.parsed_unit.dimension
        )),
        Err(e) => report(format!("dimension error in '{}': {e}", f.expr)),
    }

    for check in &f.checks {
        if check.inputs.len() != variables.len() {
            report(format!(
                "check has {} inputs but the formula has {} variables",
                check.inputs.len(),
                variables.len()
            ));
            continue;
        }
        let got = parsed.eval(&check.inputs);
        let tol = 1e-6 * check.output.abs().max(1e-12);
        if !within(got, check.output, tol) {
            report(format!(
                "check {:?} gives {got}, expected {}",
                check.inputs, check.output
            ));
        }
    }

    // Unit conversions must agree with the unit table: 1 of the input unit,
    // converted, is the same physical amount in the output unit.
    let is_tagged = |t: &str| f.tags.iter().any(|x| x == t);
    if is_tagged("unit_conversion") && !is_tagged("affine") && variables.len() == 1 {
        let input = variables[0].parsed_unit.factor;
        let converted = parsed.eval(&[1.0]) * output.parsed_unit.factor;
        if !within(converted, input, 1e-9 * input.abs()) {
            report(format!(
                "1 {} converts to {} {}, but the unit table says {}",
                variables[0].unit,
                parsed.eval(&[1.0]),
                output.unit,
                input / output.parsed_unit.factor
            ));
        }
    }

    Some(Formula {
        id: f.id.clone(),
        name: f.name.clone(),
        domain: domain.to_string(),
        description: f.description.clone(),
        expr_text: f.expr.clone(),
        constants: parsed.constants().into_iter().map(str::to_string).collect(),
        expr: parsed,
        output,
        variables,
        tags: f.tags.clone(),
        source: f.source.clone(),
        checks: f.checks.clone(),
    })
}
