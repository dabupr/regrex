//! Runs the `regrex` binary on sample files.

use std::process::Command;

fn regrex(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_regrex"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("binary runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn scan_json(file: &str) -> serde_json::Value {
    let (ok, stdout, stderr) = regrex(&["scan", file, "--ignore", "invoice", "--json"]);
    assert!(ok, "{stderr}");
    serde_json::from_str(&stdout).expect("valid JSON")
}

fn derived<'a>(report: &'a serde_json::Value, column: &str) -> &'a serde_json::Value {
    report["derived"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["column"] == column)
        .unwrap_or_else(|| panic!("{column} not reported"))
}

#[test]
fn scan_csv_finds_formulas_and_exceptions() {
    let report = scan_json("tests/invoices.csv");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["rows"], 60);
    assert_eq!(report["skipped_columns"][0], "product");

    let subtotal = derived(&report, "subtotal");
    assert_eq!(subtotal["formula"], "price * quantity");
    assert_eq!(subtotal["exception_rows"].as_array().unwrap().len(), 0);

    let vat = derived(&report, "vat");
    assert_eq!(vat["formula"], "0.21 * net");
    assert_eq!(vat["exception_rows"], serde_json::json!([42]));

    let total = derived(&report, "total");
    assert_eq!(total["formula"], "net + vat");
    assert_eq!(total["exception_rows"], serde_json::json!([17]));
}

#[test]
fn scan_xlsx_gives_the_same_result() {
    let csv = scan_json("tests/invoices.csv");
    let xlsx = scan_json("tests/invoices.xlsx");
    assert_eq!(csv["derived"], xlsx["derived"]);
}

#[test]
fn scan_text_output_is_readable() {
    let (ok, stdout, _) = regrex(&["scan", "tests/invoices.csv", "--columns", "vat"]);
    assert!(ok);
    assert!(stdout.contains("vat = 0.21 * net"), "{stdout}");
    assert!(stdout.contains("59/60 rows"), "{stdout}");
    assert!(stdout.contains("exceptions: row 42"), "{stdout}");
}

#[test]
fn fit_one_column() {
    let (ok, stdout, stderr) = regrex(&[
        "fit",
        "tests/invoices.csv",
        "--target",
        "subtotal",
        "--inputs",
        "price,quantity",
        "--json",
    ]);
    assert!(ok, "{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["candidates"][0]["formula"], "price * quantity");
}

#[test]
fn helpful_errors() {
    let (ok, _, stderr) = regrex(&["fit", "tests/invoices.csv", "--target", "nope"]);
    assert!(!ok);
    assert!(
        stderr.contains("no numeric column named \"nope\""),
        "{stderr}"
    );
    assert!(stderr.contains("price"), "{stderr}");

    let (ok, _, stderr) = regrex(&["scan", "tests/missing.csv"]);
    assert!(!ok);
    assert!(stderr.contains("cannot read"), "{stderr}");
}

#[test]
fn scan_mermaid_diagram() {
    let (ok, stdout, _) = regrex(&[
        "scan",
        "tests/invoices.csv",
        "--columns",
        "subtotal",
        "--mermaid",
    ]);
    assert!(ok);
    assert!(stdout.starts_with("```mermaid\nflowchart LR\n"), "{stdout}");
    assert!(
        stdout.contains("-->|\"subtotal = price * quantity\"|"),
        "{stdout}"
    );
}
