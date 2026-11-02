//! Validates the built-in library and the checks of the loader.
//!
//! `builtin_library_is_valid` is what CI runs on every change to the TOML
//! files: when it fails it lists every problem found.

use regrex_formulas::{all, Library, BUILTIN_FILES};

#[test]
fn builtin_library_is_valid() {
    if let Err(e) = Library::from_sources(BUILTIN_FILES) {
        panic!("{e}");
    }
}

#[test]
fn every_domain_has_formulas_with_sources() {
    let lib = Library::builtin();
    assert_eq!(lib.domains.len(), BUILTIN_FILES.len());
    for (file, _) in BUILTIN_FILES {
        let id = file.trim_end_matches(".toml");
        assert!(
            lib.domain(id).is_some(),
            "{file} must declare id = \"{id}\""
        );
        assert!(lib.formulas_in(id).count() >= 10, "{id} has few formulas");
    }
    assert!(all().len() >= 200);
    for f in all() {
        assert!(f.source.url.is_some(), "{} has no source url", f.id);
    }
}

#[test]
fn known_formulas_are_recognised() {
    let lib = Library::builtin();
    let pendulum = lib.formula("pendulum_period").unwrap();
    assert_eq!(pendulum.constants, ["pi", "g"]);
    assert!(pendulum.equivalent_to(|x| 2.006409292589 * x[0].sqrt(), 1e-9));
    // Using g = 9.81 instead of 9.80665 is close, but not the same formula.
    let g981 = |x: &[f64]| 2.0 * std::f64::consts::PI * (x[0] / 9.81).sqrt();
    assert!(!pendulum.equivalent_to(g981, 1e-6));
    assert!(pendulum.equivalent_to(g981, 1e-3));

    let vat = lib.formula("line_total_with_tax").unwrap();
    assert!(vat.equivalent_to(|x| 1.21 * x[0] * x[1] * (1.0 + x[2]) / 1.21, 1e-12));
    assert!(!vat.equivalent_to(|x| x[0] * x[1] * x[2], 1e-6));
}

#[test]
fn domain_priors() {
    let lib = Library::builtin();
    let physics = lib.operator_counts(Some("physics"));
    let finance = lib.operator_counts(Some("finance"));
    assert!(physics["*"] > physics["+"]);
    assert!(physics.contains_key("sqrt"));
    assert!(!finance.contains_key("sin"));
    assert!(lib.constants_used_in("physics")["g"] >= 5);
    assert!(lib.constants_in("physics").any(|c| c.id == "c"));
}

// --- The loader rejects broken files ----------------------------------------

const HEADER: &str = r#"
id = "test"
name = "Test"
description = "Test domain."

[[constant]]
id = "g"
name = "gravity"
value = 9.80665
unit = "m/s^2"
exact = true
source = { title = "CODATA" }
"#;

fn problems(formula: &str) -> Vec<String> {
    let text = format!("{HEADER}\n{formula}");
    match Library::from_sources(&[("test.toml", text.as_str())]) {
        Ok(_) => Vec::new(),
        Err(e) => e.0.into_iter().map(|p| p.message).collect(),
    }
}

fn assert_rejected(formula: &str, needle: &str) {
    let found = problems(formula);
    assert!(
        found.iter().any(|m| m.contains(needle)),
        "expected a problem containing '{needle}', got {found:?}"
    );
}

const GOOD: &str = r#"
[[formula]]
id = "weight"
name = "Weight"
expr = "m * g"
output = { symbol = "W", name = "weight", unit = "N" }
variables = [{ symbol = "m", name = "mass", unit = "kg" }]
source = { title = "Weight", url = "https://en.wikipedia.org/wiki/Weight" }
checks = [{ inputs = [10], output = 98.0665 }]
"#;

#[test]
fn a_good_formula_loads() {
    assert_eq!(problems(GOOD), Vec::<String>::new());
}

#[test]
fn wrong_dimensions_are_rejected() {
    assert_rejected(
        &GOOD.replace("unit = \"N\"", "unit = \"J\""),
        "dimensions do not match",
    );
}

#[test]
fn wrong_checks_are_rejected() {
    assert_rejected(&GOOD.replace("98.0665", "98.1"), "check [10.0] gives 98.06");
    assert_rejected(&GOOD.replace("[10]", "[10, 2]"), "check has 2 inputs");
}

#[test]
fn unknown_names_and_fields_are_rejected() {
    assert_rejected(&GOOD.replace("m * g", "m * gg"), "unknown name 'gg'");
    assert_rejected(
        &GOOD.replace("m * g", "m * foo(g)"),
        "unknown function 'foo'",
    );
    assert_rejected(&GOOD.replace("expr =", "exprr ="), "unknown field");
    assert_rejected(&GOOD.replace("\"kg\"", "\"kgs\""), "unknown unit 'kgs'");
}

#[test]
fn unused_and_duplicate_variables_are_rejected() {
    let unused = GOOD.replace(
        "variables = [{ symbol = \"m\", name = \"mass\", unit = \"kg\" }]",
        "variables = [{ symbol = \"m\", name = \"mass\", unit = \"kg\" }, { symbol = \"x\", name = \"x\", unit = \"1\" }]",
    );
    assert_rejected(&unused, "'x' is declared but not used");
    assert_rejected(&format!("{GOOD}\n{GOOD}"), "duplicate formula id");
}

#[test]
fn constants_need_si_units() {
    assert_rejected(&GOOD.replace("\"kg\"", "\"g\""), "coherent SI units");
}

#[test]
fn sources_must_be_https_links() {
    assert_rejected(&GOOD.replace("https://", "http://"), "https://");
}

#[test]
fn unit_conversions_are_checked_against_the_unit_table() {
    let conversion = r#"
[[formula]]
id = "km_to_miles"
name = "km to miles"
expr = "d / 1.6"
output = { symbol = "d_mi", name = "miles", unit = "mi" }
variables = [{ symbol = "d", name = "km", unit = "km" }]
tags = ["unit_conversion"]
source = { title = "Mile" }
"#;
    assert_rejected(conversion, "but the unit table says 0.621371");
    assert_eq!(
        problems(&conversion.replace("1.6\"", "1.609344\"")),
        Vec::<String>::new()
    );
}

#[test]
fn every_problem_is_reported_at_once() {
    let bad = GOOD
        .replace("unit = \"N\"", "unit = \"J\"")
        .replace("98.0665", "1");
    assert_eq!(problems(&bad).len(), 2);
}
