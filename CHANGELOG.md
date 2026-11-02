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

### Changed

- Minimum supported Rust version is now 1.88 (needed by the spreadsheet reader).
