# Roadmap

Regrex is built in the open, one small phase at a time. Each phase ends with something usable.

| Phase | Goal | Deliverable |
|---|---|---|
| 0 ✅ | Foundations | Repo, license, CI green |
| 1 ✅ | Minimal core | Exhaustive search of short formulas, constant fitting, error + complexity score |
| 2 ✅ | Dataset scan (main use case) | `regrex scan data.csv` finds derived columns and rows that break the formula |
| 3 ✅ | Browser app | WASM in a Web Worker, CSV/Excel import, column roles and units, GitHub Pages demo |
| 4 | Domains and units | Dimensional pruning, domain constants and priors, "known formula" badge (e-graphs) |
| 5 ✅ | Python and AI | PyPI package (PyO3), MCP server, stable JSON schema, `llms.txt` |
| 6 ✅ | Error diagnosis | Typical mutations of the correct formula explain each wrong row |
| 7 | Performance | SIMD, multithreading, public benchmarks vs PySR, gplearn, AI Feynman (SRBench) |
| 8 ✅ | First release | 0.1.0 on crates.io and PyPI (`pyregrex`), binaries on GitHub |

## Road to 1.0.0

After 0.1.0 the work goes in three steps: much smarter first, then much faster, then stable. Issue [#81](https://github.com/dabupr/regrex/issues/81) tracks it, and the labels `roadmap: 0.3 smart`, `roadmap: 0.4 fast` and `roadmap: 1.0 stable` group the issues.

| Release | Goal | Main work |
|---|---|---|
| 0.3 "smart" | Find longer formulas, survive noise, report nothing false | Benchmark in CI, held-out validation, one line per relation, sums of terms by sparse regression, separability, known constants (4π), Buckingham π, robust fitting, confidence, formulas per category |
| 0.4 "fast" | 5–8× faster scans, usable in the browser on wide files | Bytecode evaluation on reused buffers, enumerate once per file, reuse subexpression values and reject early, WASM SIMD, live results in the web app |
| 1.0 "stable" | Something people can depend on | Frozen API under semver, full Feynman benchmark against PySR and gplearn, Python parity with pandas/polars and a scikit-learn estimator, npm package, fuzzing, user guide |

1.0.0 ships when these hold:

| Measure | 2026-11-09 | 1.0.0 |
|---|---|---|
| AI Feynman files in the example datasets | 8 of 12 | 12 of 12 |
| Full Feynman set (100 equations, SRBench) | not measured | ≥ 75 % clean, ≥ 60 % at 1 % noise |
| False positives on classic ML datasets | 1 | 0 |
| Wide real export (~550 columns), CLI on 4 cores | ~155 s | < 20 s |
| Same file in the browser | ~377 s | < 60 s |

Principles: one Rust core everywhere; data never leaves the user's machine in the browser; the AI proposes and the engine verifies; every candidate comes with a score and a reason.
