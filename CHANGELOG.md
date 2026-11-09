# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A quality and speed benchmark for `regrex scan`: `cargo run --release -p regrex-cli --example bench` scans the files in `benchmarks/` (12 AI Feynman equations, the planted example tables and 10 classic datasets with no formulas), compares the result with `benchmarks/expected.toml` and prints which relations were found, the false positives and the time per file. CI runs it on every push and fails when a found formula is lost or a new false positive appears ([#64](https://github.com/dabupr/regrex/issues/64)).
- Cells that are numbers in disguise are read as numbers: `true`/`false` (as 1/0), times such as `08:15` or `1:24:00` (as minutes), dates such as `2026-10-15` or `15/10/2026` (as days), dates with times, including Kibana's `Oct 15, 2026 @ 14:24:47.000` and ISO 8601 with a time zone (as minutes), and numbers followed by a unit such as `24.1 km` (which also gives the column its unit). Missing-value markers (`-`, `NA`, `N/A`, `null`, `NaN`, `#N/A`) count as empty cells, so they no longer turn a column into text. The JSON report lists these columns in `converted_columns`, the text report under "Read as numbers", and the web app shows how each was read.
- A new diagnosis, `midnight`: a value a whole day off (1440 minutes, 24 hours or 86400 seconds), such as a train arriving at 00:10 after leaving at 23:50.
- Integer division and remainder: formulas such as `line = service div 100` (the hundreds digit), `coach = 7250 + (train + 3) div 4` or `car = 1 + train mod 4` are found for columns of whole numbers, which before only got approximations like `0.0098 * service`. Formulas written by hand may use `div` (or `//`, or `floor(a / b)`) and `mod` (or `%`), and the web app's search settings can turn both off ([#28](https://github.com/dabupr/regrex/issues/28)). When two formulas match the same rows, one that gives the exact values wins over one that only stays within rounding of them.
- Copies of other columns are found before the search and listed apart: the same values (`idTren = coche0_numeroSerie`), shifted (`coche1_numeroSerie = coche0_numeroSerie + 1`), negated or scaled by a power of ten, 60 or 3600 (`m = 1000 * km`). They are not used as inputs, so they no longer take the place of other columns, and the text report shows them grouped under "Copies of other columns", one line per original column. The JSON report gives each copy a `copy_of` field (`regrex_core::find_copies`), and the web app shows them in a collapsed section. On the 584-column trains export, the results go from 23 formulas (16 of them copies, each found in both directions) to 5 formulas and 8 lines of copies ([#29](https://github.com/dabupr/regrex/issues/29)).

### Changed

- The web app scans columns on several workers at once (all cores but one, at most 8), handing columns out from a queue so a slow one does not hold up the others; the formulas are then diagnosed together on one worker. On the small trains export (4 cores) a scan takes 99 s instead of 278 s. Each worker no longer copies the whole table for every column. `?workers=1` in the address runs on one worker, to compare ([#30](https://github.com/dabupr/regrex/issues/30)).
- SQLite files are read with `rusqlite` 0.41, which bundles SQLite 3.54.1 and fixes [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459). The minimum supported Rust version is now 1.89.
- `scan` checks columns in parallel (with the default `parallel` feature).
- Fewer made-up formulas on exports with hundreds of sensor columns: columns with one or two values (flags) are not explained, a formula must also explain the rows that differ from a column's most common value (columns that are zero almost everywhere matched nearly any formula), constants like `2.7e-81` and exponents like `x^4.9e-16` or `x^86` are rejected, a column missing in many rows is no longer tried as an input, rules that split two formulas may get wrong at most a tenth of the rows they pick out, and only columns with few values are used as categories.
- When the rows of a column that do not follow its formula are all one value (a sensor reading 0 on the trains that lack it), that value is shown as the main formula: `temperature2 = 0`, other rows `temperature1 (where train >= 9013)`.
- A whole-number column no longer matches a formula that is exactly 0.5 away (`x + 0.5` "matching" both `x` and `x + 1`).
- SQLite files are read with rusqlite 0.41, which bundles SQLite 3.54 (RUSTSEC-2026-0459); the minimum supported Rust version is now 1.89.
- A plain sum, difference, product or quotient is only named as a known formula with hints from column names, even when it matches a formula that reduces to it (relativistic momentum is `m * v` at everyday speeds).

### Fixed

- A 0 in a row that breaks a formula is no longer explained as a factor ("1/100 of the expected value"): such a cell gets the new diagnosis `zero` (a missing value, or a reading that was off), and sign and factor mistakes are only looked for between values that are clearly not zero ([#27](https://github.com/dabupr/regrex/issues/27)).
- Dates and times no longer match formulas that are off by minutes: a relative tolerance of 1e-6 on 26 million minutes since 1970 let `pickup = dropoff - fare` "match" taxi rides. Columns read as dates or times now only tolerate their own rounding (`Table::timestamps`) ([#45](https://github.com/dabupr/regrex/issues/45)).
- Python: `regrex.scan(data)` tolerates rounded values as `scan_file` does. The decimals of each column are inferred from its values (`Table::infer_decimals`), or given by hand with `decimals={"vat": 2}` ([#31](https://github.com/dabupr/regrex/issues/31)).

## [0.1.0] - 2026-11-03

First release.

### Added

- Two views of the results in the web app: a diagram of how the columns depend on each other (columns computed from each other share a two-headed arrow; selecting a column goes to its formula), and under each formula a chart of what the file holds against what the formula gives, with the rows that break it highlighted. The WebAssembly `Dataset` gains `values`.
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
- Suspicious rows in the web app: a summary at the top of the results ("2 suspicious rows"), a table with each row in full and the cell to fix highlighted, and an export to CSV. When more than 5% of the rows break a formula they are described as a group that may follow another rule, not as mistakes.
- The web app diagnoses all the formulas of a file together, so a wrong total that also breaks `net = total - vat` is blamed once, as in the command line.
- Search settings in the web app, saved in the browser: Fast, Normal and Thorough profiles, explaining a single column, and choosing the operations formulas may use.
- Advanced search settings in the web app, each explained with its default: rows that must follow the formula, tolerance, columns tried as inputs and a time limit. A running search can be stopped. Thorough only tries longer formulas for the columns Normal cannot explain, so the example takes 40 seconds instead of 4 minutes.
- Search only among known formulas in the web app, optionally of one domain: every library formula is tried with the file's columns, fitting at most one constant (such as a tax rate).
- Parquet, Arrow IPC (Feather v2) and SQLite input in the CLI, MCP server and Python package. In `regrex-io` they are the optional features `parquet`, `arrow` and `sqlite` (`all-formats` for all three); each SQLite table is chosen like a sheet. The web app reads them too, with JavaScript readers loaded only when such a file is opened, and lists every accepted format where files are dropped.
- "Test your own formula" in the web app, and the `check_formula` MCP tool, accept constants to fit: `vat = rate * net` finds `0.21 * net` and the rows that break it. In Rust: `parse::parse_template` and `fit_shape`.
- Columns that mix two formulas: when many rows break a formula, the scan looks for a second formula among them and for a rule on another column that tells the rows apart (`vat = 0.1 * net` for 24 rows, where `year >= 2023`). A column is reported even when neither formula alone covers 90% of the rows, as long as both do together and a rule splits them. JSON reports gain `other`, exceptions that follow it are diagnosed as `other_formula`, and the CLI and web app show it. Example: `examples/data/tax_change.csv`.
- When no formula matches enough rows, the shapes found are refitted on tiny samples, so rows of two kinds no longer hide both formulas.
- JSON input: `.json` files with an array of objects, an object of columns or an array of arrays, and JSON lines (`.ndjson`, `.jsonl`); nested objects become columns such as `price.net`. Works in the CLI, Python, MCP and the web app.
- The web app reads cells pasted from a spreadsheet (Ctrl+V).
- The library of known formulas grows to 1073 formulas in 13 domains, with three new ones: engineering (strength of materials, beams, machine elements, pipes and pumps, civil works, machining), astronomy (magnitudes, stars, orbits, rockets, telescopes, cosmology) and Earth and weather (humidity, comfort indices, hydrology, earthquakes, geology, maps, climate). Every formula keeps a source and a worked example checked when the library loads.
- The library of known formulas grows from 236 to 459 formulas, with two new domains: business and operations (marketing KPIs, accounting ratios, inventory, payroll, HR, hospitality) and energy and environment (electricity bills, solar, wind, hydro, heating, batteries, fuels and CO₂).
- `ScanConfig::small_samples`: when no formula fits every row, the scan also searches small random samples of rows, so a few wrong rows in a small table no longer hide its formula.

### Changed

- Formulas with a negative leading term are written as subtractions: `100 - 100 * net / subtotal` instead of `-100 * net / subtotal + 100`.

- `regrex_io::Loaded` keeps every row as text in `rows` instead of the first 20 in `preview`.
- Minimum supported Rust version is now 1.88 (needed by the spreadsheet reader).
- Scanning only reports a formula when more rows match it than it has parts, so tiny tables do not produce made-up formulas.
