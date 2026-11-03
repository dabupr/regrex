# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Project skeleton: Cargo workspace, `regrex-core` with an expression tree, evaluation and MSE.
- Community files: contributing guide, code of conduct, security policy, issue and PR templates.
- CI: formatting, clippy, tests on Linux, macOS and Windows, docs, MSRV check and dependency audit.
- Search engine in `regrex-core` (phase 1):
  - expression trees with `+ - * / ^`, `-x`, `x^2`, `sqrt`, `exp`, `log`, `sin`, `cos` and readable display;
  - canonical forms (sorted sums and products, folded constants, `x * x` → `x^2`) to skip duplicates;
  - bottom-up enumeration of distinct formulas, deduplicated by structure and by value;
  - Levenberg–Marquardt fitting of inner constants, closed-form outer scale and offset (`a·f + b`);
  - snapping of fitted constants to round values (`0.5`, `1.21`, `π`) using the BIC;
  - weighted complexity (including the digits of each constant) and a Pareto front of accuracy vs simplicity;
  - optional multithreading with the `parallel` feature (on by default).
- Dataset scanning (phase 2):
  - `regrex_core::scan` finds columns computed from other columns, robust to a few rows that break the rule, and reports those rows;
  - the `regrex` command-line tool (`regrex-cli` crate) with `scan` and `fit`, CSV and spreadsheet input, text, JSON and Mermaid output;
  - example data in `examples/data/invoices.csv`.

- Web app (phase 3) in `web/`, deployed to GitHub Pages:
  - drop a CSV or Excel file, preview it, untick ID columns and find derived columns, all in the browser;
  - rows that break a formula are listed with the value in the file and the value the formula gives;
  - JSON download, light and dark themes, end-to-end tests with Playwright.
- `regrex-io` crate: file reading and JSON reports shared by the CLI and the web app; reports now detail each exception (`exceptions`: row, actual and expected value).
- Library of known formulas (`regrex-formulas`): about 235 formulas and 25 constants in eight domains (mathematics, physics, chemistry, electronics and signals, finance, computing, health, unit conversions), each with SI units and a reference, stored as one TOML file per domain and validated in CI (names, units, dimensions, worked examples, conversion factors).

- Units and dimensional analysis (phase 4):
  - `regrex_core::units` parses units (`m/s^2`, `kg·m²/s²`, `km/h`, `kWh`, `€/h`…) into dimensions and conversion factors;
  - column headers like `speed (m/s)` or `mass [kg]` set the unit of the column;
  - with units, formulas that mix incompatible dimensions (`t + v`, `exp(t)`) are never tried, and outer constants that must carry units (`g` in `4.905 * t^2`) cost a little extra complexity.
- `regrex_core::parse::parse_expr` reads formulas written as text (`0.5 * m * v^2`, `2 * pi * sqrt(L / g)`).

- Python bindings (phase 5): the `regrex` package (`crates/regrex-py`, built with maturin) with `search`, `scan` and `scan_file`, accepting dicts of columns and pandas DataFrames, tested with pytest on Linux, macOS and Windows.
- MCP server (phase 5): `regrex mcp` exposes `scan_dataset`, `fit_formula`, `fit_values`, `check_formula` and `check_units` to AI assistants over stdio.
- `regrex_core::check_formula` checks a given formula against a column and reports the rows that break it.
- Documentation of the JSON output (`docs/json-output.md`), MCP setup (`docs/mcp.md`) and an `llms.txt` file.
- Error diagnosis (phase 6): `regrex_core::diagnose` explains a row that breaks a formula with a mistake that reproduces it exactly (wrong constant, mistyped digit, rounding, sign, unit or percentage factor, wrong or swapped columns, forgotten operation or term), and blames a single wrong cell when it breaks several formulas. Scan reports, the CLI and the web app show the most likely mistake of each exception.
- Benchmark (phase 7): `cargo run --release -p regrex-core --example feynman` measures recovery on 24 Feynman equations, with and without noise; results in `docs/BENCHMARKS.md`.
- Release automation (phase 8): pushing a version tag builds the CLI for Linux, macOS and Windows, Python wheels and a source distribution, creates the GitHub release and publishes to PyPI (as `pyregrex`) and crates.io. See `docs/RELEASING.md`.
- `CITATION.cff` for citing Regrex in research.
- The search finds products of powers with any number of inputs (`n * kb * T / V`) with a weighted fit in log space, and stops enumerating larger formulas once an exact one is known.
- Known formulas: scan and fit results name the library formula a column follows (`vat` → "Tax amount" with `t = 0.21`), including a changed constant such as `g = 10`, in the CLI, JSON reports (`known` field) and the web app, where a "Known formula" badge opens a card with the variables, units and source.
- A library page in the web app (`library.html`) lists every known formula with search and a domain filter.

### Changed

- Formulas with a negative leading term are written as subtractions: `100 - 100 * net / subtotal` instead of `-100 * net / subtotal + 100`.

- Minimum supported Rust version is now 1.88 (needed by the spreadsheet reader).
- Scanning only reports a formula when more rows match it than it has parts, so tiny tables do not produce made-up formulas.
