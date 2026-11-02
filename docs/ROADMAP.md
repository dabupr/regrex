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
| 8 | v1.0 | Releases on crates.io, PyPI and npm; community |

Principles: one Rust core everywhere; data never leaves the user's machine in the browser; the AI proposes and the engine verifies; every candidate comes with a score and a reason.
