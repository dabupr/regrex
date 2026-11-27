# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The scan works out what each column is for, without AI: group keys (line, track), the column that orders the rows (a chainage, a time), text columns read as categories (types, names, codes), identifiers and measurements, shown in the text report and in `structure` in the JSON report (`regrex_core::profile`). Text columns are no longer skipped: their values can decide which formula applies ([#98](https://github.com/dabupr/regrex/issues/98)).
- Formulas that change with a text column: `cant = by curve_type: Straight 0; Circular constant along stretches; Clothoid c0 + c1 * pk, constants per run`, with constants fitted on each run of rows, and rates per category such as a VAT rate per country, even in shuffled rows (`derived[].regime` in the JSON report) ([#101](https://github.com/dabupr/regrex/issues/101), [#72](https://github.com/dabupr/regrex/issues/72)).
- Formulas between consecutive rows of an ordered table: a running total (`previous balance + amount`), a chainage that grows by the distance between consecutive points `(x, y)` or `(x, y, z)`, and a gradient that is the slope of a height on each stretch (`derived[].between_rows`) ([#100](https://github.com/dabupr/regrex/issues/100)).
- Numbers hidden in text are read as columns of their own: a prefix and a number (`GIS5550` as `code#num`, which then turns out to be an id minus 4835), a fraction (`Crossing 1/8,5`), a WKT point (`geom#x`, `geom#y`) and the length of a list ([#99](https://github.com/dabupr/regrex/issues/99)).
- The web app shows the new results: how the table is laid out (groups, order, types), each formula per type as a table with its rows, badges for formulas between rows and approximate ones, relations between text columns, a second file matched on shared columns, and a "What Regrex looks for" section on the start page. While it searches, every column shows whether it was found, is being searched (and how) or is still to do. Files over 50,000 rows are searched in two passes (Search settings → Advanced → "Search a sample first"): a quick look at every column on 5% of the rows, whose formulas appear at once marked "Preview", then each of them checked on all rows (`sample` and `sample_only` in the wasm `explain` options). Each column has its own colour in the preview, the formulas, the tables and the graph; a partial formula says in words how many rows break it and what that may mean, and "How to read the results" explains every mark. Suspicious rows are shown 100 at a time.
- `regrex scan a.csv --with b.csv` matches the rows of two files on the columns both have, with text or whole numbers (a line, a track, a chainage), and scans them as one table, so relations between files are found (`regrex_io::join`).
- A regime piece that is constant along stretches is also tried as equal to another column that changes over the part: on circular curves the measured radius is `radi_curvatura`, not just constant.
- A regime piece can be a slope over each stretch: a gradient is `100 * slope of z over pk` on the grades and constant along the vertical curves between them.
- Approximate slopes: when a slope per stretch holds only to within 1% of the column's usual size (a gradient given with 4 decimals next to heights computed with 15), it is reported as `about ... (within 0.0086)`, with `within` in the JSON report and `exact: false` in the agent report. The rounding of heights is now read per stretch, since one column can mix values with 3 and with 15 decimals.
- A regime is also scored on the ranks of the column, so a radius that is huge where a transition curve starts no longer hides that it follows the curve type everywhere else (`radius = 1 / (c0 + c1 * pk)` per transition curve).
- Text columns set by another: each value of `circuit` goes with one `interlocking`, reported as `interlocking = set by circuit` under "Text relations". Pairs that follow from two others, or that hold only on most rows, are left out.
- Relations between text columns: concatenations (`key = line + "|" + track`) and codes that match names one to one, under "Text relations" and in `text_relations` ([#103](https://github.com/dabupr/regrex/issues/103)).
- `regrex scan --quick` searches every column on a sample of rows (blocks spread over the file), prints each formula as soon as it is found there and confirms it on all rows; on a 249,000-row table it takes a third of the time with the same formulas (`regrex_core::scan_quick`, `quick` in the JSON report) ([#105](https://github.com/dabupr/regrex/issues/105)).
- A synthetic railway track table in the scan benchmark, with groups, an order, regimes, runs and text relations; all eight of its relations are found ([#104](https://github.com/dabupr/regrex/issues/104)).
- A scan report for AI agents, apart from the one for people: `regrex scan --agent` (and `"format": "agent"` in the MCP `scan_dataset` tool) prints compact JSON with every column of the file (text or number, distinct values, range, whether it was searched and why not), each formula with its largest and median error and whether it is exact, the near misses and the inputs tried for the columns left unexplained, so an agent can judge whether the results make sense without searching again ([#106](https://github.com/dabupr/regrex/issues/106)).
- The scan keeps the formulas it came close to accepting: ones that hold on too few rows (60% of them, say), only where another column has some values, or mostly where the column has its most common value. When a column holds one value (often 0) on most rows, the other rows are searched on their own, so a cant that is 0 on straight track and a formula on curves shows up. Each comes with the reason it was not reported and, when there is one, the rule on another column that picks out the rows where it holds. The text report lists them under "Almost", the JSON report in `near_misses` (also through MCP and Python), and the web app in a collapsed "Almost" section where a button searches the rows where one fails, to find what those rows follow. `regrex scan --verbose` and the web app's progress show what each search is doing: the inputs tried, the best formula so far and how it ended (`regrex_core::scan_detailed`, `explain_column_detailed`) ([#102](https://github.com/dabupr/regrex/issues/102)).
- A quality and speed benchmark for `regrex scan`: `cargo run --release -p regrex-cli --example bench` scans the files in `benchmarks/` (12 AI Feynman equations, the planted example tables and 10 classic datasets with no formulas), compares the result with `benchmarks/expected.toml` and prints which relations were found, the false positives and the time per file. CI runs it on every push and fails when a found formula is lost or a new false positive appears ([#64](https://github.com/dabupr/regrex/issues/64)).
- Cells that are numbers in disguise are read as numbers: `true`/`false` (as 1/0), times such as `08:15` or `1:24:00` (as minutes), dates such as `2026-10-15` or `15/10/2026` (as days), dates with times, including Kibana's `Oct 15, 2026 @ 14:24:47.000` and ISO 8601 with a time zone (as minutes), and numbers followed by a unit such as `24.1 km` (which also gives the column its unit). Missing-value markers (`-`, `NA`, `N/A`, `null`, `NaN`, `#N/A`) count as empty cells, so they no longer turn a column into text. The JSON report lists these columns in `converted_columns`, the text report under "Read as numbers", and the web app shows how each was read.
- A new diagnosis, `midnight`: a value a whole day off (1440 minutes, 24 hours or 86400 seconds), such as a train arriving at 00:10 after leaving at 23:50.
- Integer division and remainder: formulas such as `line = service div 100` (the hundreds digit), `coach = 7250 + (train + 3) div 4` or `car = 1 + train mod 4` are found for columns of whole numbers, which before only got approximations like `0.0098 * service`. Formulas written by hand may use `div` (or `//`, or `floor(a / b)`) and `mod` (or `%`), and the web app's search settings can turn both off ([#28](https://github.com/dabupr/regrex/issues/28)). When two formulas match the same rows, one that gives the exact values wins over one that only stays within rounding of them.
- Copies of other columns are found before the search and listed apart: the same values (`idTren = coche0_numeroSerie`), shifted (`coche1_numeroSerie = coche0_numeroSerie + 1`), negated or scaled by a power of ten, 60 or 3600 (`m = 1000 * km`). They are not used as inputs, so they no longer take the place of other columns, and the text report shows them grouped under "Copies of other columns", one line per original column. The JSON report gives each copy a `copy_of` field (`regrex_core::find_copies`), and the web app shows them in a collapsed section. On the 584-column trains export, the results go from 23 formulas (16 of them copies, each found in both directions) to 5 formulas and 8 lines of copies ([#29](https://github.com/dabupr/regrex/issues/29)).

- The JSON report lists the columns searched without finding a formula in `not_found`, each with the `inputs_tried`, to answer "why was X not found" ([#34](https://github.com/dabupr/regrex/issues/34)).

- A test on a synthetic, wide sensor export (250 columns, 1200 rows: constants, flags, columns that are zero almost everywhere, copies, `-` for missing, Kibana dates and a few planted formulas) checks that every planted formula is found and no other column gets one, and CI runs it in release mode with a time budget ([#38](https://github.com/dabupr/regrex/issues/38)).

### Changed

- Wide files are easier to handle in the web app: the preview shows the first 30 columns (and all of them on demand), and a "Columns to scan" panel filters columns by name, ticks or unticks them all or by prefix group (`coche0_…`), and suggests leaving out search metadata (`_score`, `*.keyword`), constant columns and ID-like ones, which start unticked. The choice is remembered per file name ([#33](https://github.com/dabupr/regrex/issues/33)).
- The web app shows each formula as soon as it is found, in column order, while the scan goes on, and the progress line estimates the time left; the dependency diagram and the suspicious rows still come at the end ([#32](https://github.com/dabupr/regrex/issues/32)).
- The web app scans columns on several workers at once (all cores but one, at most 8), handing columns out from a queue so a slow one does not hold up the others; the formulas are then diagnosed together on one worker. On the small trains export (4 cores) a scan takes 99 s instead of 278 s. Each worker no longer copies the whole table for every column. `?workers=1` in the address runs on one worker, to compare ([#30](https://github.com/dabupr/regrex/issues/30)).
- SQLite files are read with `rusqlite` 0.41, which bundles SQLite 3.54.1 and fixes [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459). The minimum supported Rust version is now 1.89.
- Better choice of the columns tried as inputs of a formula ([#34](https://github.com/dabupr/regrex/issues/34)). Columns with few values (flags, codes, modes) are also scored by how much of the column their categories explain, adjusted for chance, which finds inputs of relations that are not straight lines. Columns with the same values take one slot instead of several. One more input is tried when it is the only column left out, or when it explains what the best input leaves (`gross / net` for a rate that varies little, or a small term added to it) even though plain correlation ranks it low. The diamonds dataset now gets `depth = 200 * z / (x + y)`, the definition of a diamond's depth.
- `scan` checks columns in parallel (with the default `parallel` feature).
- Fewer made-up formulas on exports with hundreds of sensor columns: columns with one or two values (flags) are not explained, a formula must also explain the rows that differ from a column's most common value (columns that are zero almost everywhere matched nearly any formula), constants like `2.7e-81` and exponents like `x^4.9e-16` or `x^86` are rejected, a column missing in many rows is no longer tried as an input, rules that split two formulas may get wrong at most a tenth of the rows they pick out, and only columns with few values are used as categories.
- When the rows of a column that do not follow its formula are all one value (a sensor reading 0 on the trains that lack it), that value is shown as the main formula: `temperature2 = 0`, other rows `temperature1 (where train >= 9013)`.
- A whole-number column no longer matches a formula that is exactly 0.5 away (`x + 0.5` "matching" both `x` and `x + 1`).
- SQLite files are read with rusqlite 0.41, which bundles SQLite 3.54 (RUSTSEC-2026-0459); the minimum supported Rust version is now 1.89.
- A plain sum, difference, product or quotient is only named as a known formula with hints from column names, even when it matches a formula that reduces to it (relativistic momentum is `m * v` at everyday speeds).

### Fixed

- Known-formula badges need column names that agree with the formula, not just its shape: on the Feynman datasets `target = m * g * z` is no longer "Heat to change the temperature of a body" but gravitational potential energy, and `g = target / (m * z)` no longer a "CAC payback period". A formula of only sums, products and quotients of columns needs each column named like the library's; one with a fixed value (`vat = 0.21 * net`) needs every input and the output named like it; any other one at least one name. Two new library formulas: potential energy with `g` given as a column, and a discount in percent (`100 - 100 * net / subtotal`, which was "Speed improvement in percent") ([#46](https://github.com/dabupr/regrex/issues/46)).
- Fewer false positives: a formula that only stays within rounding of a column is dropped when it is a row number (`0, 1, 2…`, which only an exact rule such as `1 + column1 div 3` can follow) or when the column takes too few values for the formula's fitted constants (three values and three constants). Formulas that give the exact values are never dropped. On seaborn's `exercise.csv` this removes `column1 = 3 * (id - time^(-0.25))` and `time = 14.5 * (column1 - 3 * id) + 44.3333`; the scan benchmark now reports no false positive and finds the same relations ([#65](https://github.com/dabupr/regrex/issues/65)).
- A chainage given in whole metres (a point every metre about 0.997 m apart) is again found as `previous PK + distance between consecutive (x, y)`: it is now checked over 50 steps instead of one, since one step is off by up to half a metre once rounded. The web app, which reads Parquet values like `2833.0` as whole numbers, missed it.
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
