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
    assert_eq!(vat["exceptions"][0]["actual"], 29.97);
    let why = &vat["exceptions"][0]["diagnosis"];
    assert_eq!(why["kind"], "wrong_constant");
    assert_eq!(why["description"], "used 0.1 instead of 0.21");
    assert_eq!(why["formula"], "0.1 * net");
    // Row 17 has a wrong total, which also breaks net = total - vat.
    let net = derived(&report, "net");
    assert_eq!(net["exceptions"][0]["diagnosis"]["kind"], "other_cell");

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
    assert!(
        stdout.contains("exception: row 42 is 29.97, formula gives 62.937"),
        "{stdout}"
    );
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

#[test]
fn mcp_server_over_stdio() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_regrex"))
        .arg("mcp")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("binary runs");
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scan_dataset","arguments":{"path":"tests/invoices.csv","ignore":["invoice"],"columns":["vat"]}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"check_formula","arguments":{"path":"tests/invoices.csv","target":"vat","formula":"0.21 * net"}}}"#,
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for r in requests {
            writeln!(stdin, "{r}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("one JSON message per line"))
        .collect();
    assert_eq!(responses.len(), 3, "the notification gets no response");
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");

    let scan = &responses[1]["result"]["structuredContent"];
    assert_eq!(derived(scan, "vat")["formula"], "0.21 * net");

    let check = &responses[2]["result"]["structuredContent"];
    assert_eq!(check["exception_rows"], serde_json::json!([42]));
    assert_eq!(check["checked_rows"], 60);
}

#[test]
fn scan_finds_two_formulas_and_the_rule() {
    let (ok, stdout, stderr) = regrex(&[
        "scan",
        "tests/tax_change.csv",
        "--ignore",
        "invoice",
        "--json",
    ]);
    assert!(ok, "{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let vat = derived(&report, "vat");
    assert_eq!(vat["formula"], "0.21 * net");
    assert_eq!(vat["matched_rows"], 36);
    let other = &vat["other"];
    assert_eq!(other["formula"], "0.1 * net");
    assert_eq!(other["rows"].as_array().unwrap().len(), 24);
    assert_eq!(other["split"]["rule"], "year >= 2023");
    assert_eq!(vat["exceptions"][0]["diagnosis"]["kind"], "other_formula");

    let (_, text, _) = regrex(&["scan", "tests/tax_change.csv", "--ignore", "invoice"]);
    assert!(
        text.contains("other rows: 0.1 * net (24 rows, where year >= 2023)"),
        "{text}"
    );
}

#[test]
fn scan_reads_times_units_flags_and_missing_markers() {
    let (ok, stdout, stderr) = regrex(&["scan", "tests/trains.csv", "--json"]);
    assert!(ok, "{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["skipped_columns"], serde_json::json!(["train"]));
    let converted = report["converted_columns"].as_array().unwrap();
    let read_as = |column: &str| {
        converted
            .iter()
            .find(|c| c["column"] == column)
            .map(|c| c["read_as"].as_str().unwrap().to_string())
    };
    assert_eq!(read_as("departure").as_deref(), Some("times as minutes"));
    assert_eq!(read_as("distance").as_deref(), Some("numbers in km"));
    assert_eq!(read_as("late").as_deref(), Some("true/false as 1/0"));
    let delay = derived(&report, "delay_min");
    assert_eq!(delay["formula"], "actual_arrival - arrival");
    // Two cancelled trains have `-` and `NA` instead of times and delays.
    assert_eq!(delay["checked_rows"], 58);
}

#[test]
fn scan_lists_copies_of_other_columns_apart() {
    let mut csv = String::from("train,serial0,serial1,km,m,price,qty,total\n");
    for i in 1..=30 {
        let serial = 7000 + 4 * ((i * 7) % 23);
        let km = 1.5 + 0.25 * ((i * 11) % 31) as f64;
        let (price, qty) = (2 + (i * 5) % 9, 1 + (i * 3) % 7);
        csv += &format!(
            "{serial},{serial},{},{km},{},{price},{qty},{}\n",
            serial + 1,
            1000.0 * km,
            price * qty
        );
    }
    let path = std::env::temp_dir().join("regrex-copies-test.csv");
    std::fs::write(&path, csv).unwrap();
    let file = path.to_str().unwrap();

    let (ok, stdout, stderr) = regrex(&["scan", file, "--json"]);
    assert!(ok, "{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(derived(&report, "serial0")["copy_of"], "train");
    assert_eq!(derived(&report, "serial1")["formula"], "train + 1");
    assert_eq!(derived(&report, "m")["copy_of"], "km");
    assert_eq!(derived(&report, "total")["formula"], "price * qty");
    assert_eq!(
        derived(&report, "total")["copy_of"],
        serde_json::Value::Null
    );

    let (ok, stdout, stderr) = regrex(&["scan", file]);
    assert!(ok, "{stderr}");
    let copies = stdout
        .split("Copies of other columns:")
        .nth(1)
        .expect(&stdout);
    assert!(
        copies.contains("train = serial0, serial1 = train + 1"),
        "{stdout}"
    );
    assert!(copies.contains("m = 1000 * km"), "{stdout}");
}
