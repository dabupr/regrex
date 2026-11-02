# Regrex

[![CI](https://github.com/dabupr/regrex/actions/workflows/ci.yml/badge.svg)](https://github.com/dabupr/regrex/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Find the formula behind your data.** Regrex is an open-source symbolic regression engine written in Rust that runs in your browser (WebAssembly, no server), in Python and as a tool for AI agents.

> Status: early development. The engine is being built in the open; see the [roadmap](docs/ROADMAP.md).

## What it will do

- **Scan a dataset**: upload a CSV or Excel file and Regrex tells you which columns are computed from others, and with which formula (for example `Total = Price × Quantity × 1.21`), plus the rows that do not follow it.
- **Ranked candidate formulas**: ordered by error and complexity, with a prior that depends on the domain (physics, maths, signals, computing, finance).
- **Units and dimensional analysis** to prune impossible formulas.
- **"Known formula" badge** when a candidate matches an established formula (kinetic energy, Ohm's law, VAT, compound interest…).
- **Error diagnosis per row**: given the correct formula, explain what each wrong result did (forgot a square, used g = 10, swapped two parameters, cm vs m).
- **Built for AI**: MCP server and stable JSON output. The AI proposes, the engine verifies.
- **Private by design**: in the browser, your data never leaves your computer.

## Command line

Find the columns of a spreadsheet that are computed from other columns, and the rows that break the rule:

```console
$ regrex scan examples/data/invoices.csv --ignore invoice
examples/data/invoices.csv: 60 rows, 7 numeric columns
Skipped (not numeric): product

  price        = subtotal / quantity           60/60 rows
  discount_pct = -100 * net / subtotal + 100   60/60 rows
  subtotal     = price * quantity              60/60 rows
  net          = total - vat                   59/60 rows   exceptions: row 17
  vat          = 0.21 * net                    59/60 rows   exceptions: row 42
  total        = net + vat                     59/60 rows   exceptions: row 17
```

- Reads CSV (comma, semicolon or tab; decimal point or comma) and spreadsheets (`.xlsx`, `.xls`, `.ods`, choose one with `--sheet`).
- `--json` prints a stable, versioned JSON report for scripts and AI agents.
- `--mermaid` prints the dependencies between columns as a diagram.
- `regrex fit data.csv --target y` lists candidate formulas for a single column.

Install from source with `cargo install --path crates/regrex-cli`.

## Rust library

The engine can already rediscover formulas from data (Rust library, phase 1 of the roadmap):

```rust
use regrex_core::{search, SearchConfig};

let mass = vec![1.0, 2.5, 4.0, 0.8, 7.2, 3.3];
let speed = vec![3.0, 1.2, 7.5, 12.0, 0.6, 4.4];
let energy: Vec<f64> = mass.iter().zip(&speed).map(|(m, v)| 0.5 * m * v * v).collect();

let results = search(&[mass, speed], &energy, &SearchConfig::default()).unwrap();
let names = ["m".to_string(), "v".to_string()];
println!("{}", results[0].expr.display_with(&names)); // 0.5 * m * v^2
```

Or run the example: `cargo run --release --example basic`.

## Development

```sh
cargo test --workspace
```

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started and the [Code of Conduct](CODE_OF_CONDUCT.md) before taking part. Report security issues privately as described in [SECURITY.md](SECURITY.md).

## License

MIT
