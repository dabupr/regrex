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

## Development

```sh
cargo test --workspace
```

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started and the [Code of Conduct](CODE_OF_CONDUCT.md) before taking part. Report security issues privately as described in [SECURITY.md](SECURITY.md).

## License

MIT
