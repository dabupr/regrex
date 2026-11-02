# Contributing to Regrex

Thanks for your interest in Regrex! Contributions of all sizes are welcome: bug reports, ideas, docs, known formulas for the library, and code.

## Ground rules

- Be kind. This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- Security problems go through the private channel in [SECURITY.md](SECURITY.md), never a public issue.

## Development setup

You need a recent stable Rust toolchain (see `rust-version` in `Cargo.toml` for the minimum).

```sh
git clone https://github.com/dabupr/regrex
cd regrex
cargo test --workspace
```

### Web app

The web app lives in `web/` (Vite + TypeScript, no framework). It needs Node 22, the `wasm32-unknown-unknown` Rust target and `wasm-bindgen-cli` with the same version as the `wasm-bindgen` crate in `Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
cd web
npm ci
npm run dev        # builds the engine and starts a dev server
npm run build      # production build in web/dist
npx playwright install chromium && npm test   # end-to-end tests
```

## Before opening a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

Then:

1. Add tests for new behaviour and for every bug fix.
2. Document public items (`///` doc comments). Examples in docs are tested.
3. Add a line to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md) if users will notice the change.
4. Keep pull requests focused: one topic per PR.

## Commit messages

Use short, imperative subjects (`Add constant fitting`, `Fix division by zero in eval`). We follow [Conventional Commits](https://www.conventionalcommits.org/) loosely: a prefix such as `feat:`, `fix:`, `docs:`, `test:`, `ci:` or `refactor:` is appreciated but not required.

## Project layout

- `crates/regrex-core`: the engine (expressions, search, scoring, dataset scanning).
- `crates/regrex-io`: reading CSV and spreadsheets, JSON reports.
- `crates/regrex-cli`: the `regrex` command-line tool.
- `crates/regrex-wasm`: WebAssembly bindings for the web app.
- `crates/regrex-formulas`: the library of known formulas and constants, one TOML file per domain in `library/`. Adding a formula needs no Rust: see [its README](crates/regrex-formulas/README.md).
- `web/`: the web app.
- `docs/ROADMAP.md`: what is planned, phase by phase.

## Versioning and releases

Regrex follows [Semantic Versioning](https://semver.org/). Until 1.0, minor versions may contain breaking changes; they are always listed in the changelog. How releases are made is described in [docs/RELEASING.md](docs/RELEASING.md).

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
