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

- `crates/regrex-core`: the engine (expressions, search, scoring).
- `docs/ROADMAP.md`: what is planned, phase by phase.

## Versioning and releases

Regrex follows [Semantic Versioning](https://semver.org/). Until 1.0, minor versions may contain breaking changes; they are always listed in the changelog.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
