# Releasing

Releases are made by pushing a version tag. The [release workflow](../.github/workflows/release.yml) then:

1. checks that the tag matches the version in `Cargo.toml` and that `CHANGELOG.md` has a section for it;
2. builds the `regrex` command-line tool for Linux (x86_64, ARM64), macOS (Intel, Apple silicon) and Windows;
3. builds Python wheels (one `abi3` wheel per platform, for Python 3.9 and later) and a source distribution, and tests them;
4. creates the GitHub release with those files, a `SHA256SUMS.txt` file and the changelog section as notes;
5. publishes the wheels to PyPI as [`pyregrex`](https://pypi.org/project/pyregrex/) and the crates to crates.io.

## One-time setup

These need the repository owner and are done once:

- **PyPI**: on pypi.org, add a [trusted publisher](https://docs.pypi.org/trusted-publishers/adding-a-publisher/) for the project `pyregrex` with owner `dabupr`, repository `regrex`, workflow `release.yml` and environment `pypi`. No token is stored in GitHub.
- **crates.io**: create an API token with the `publish-new` and `publish-update` scopes and save it as the secret `CARGO_REGISTRY_TOKEN` of a GitHub environment named `crates-io` (Settings → Environments). Protecting both environments with a required reviewer means every publish waits for a click.

## Making a release

1. Pick the version, following [Semantic Versioning](https://semver.org/) (before 1.0, a minor bump may break things).
2. Update the version in one place, the root `Cargo.toml` (`[workspace.package] version`), and the `version = "…"` of the internal dependencies (`cargo set-version --workspace 0.2.0` from [cargo-edit](https://github.com/killercup/cargo-edit) does both). The Python package takes its version from Cargo.
3. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [0.2.0] - YYYY-MM-DD` and start a new empty `## [Unreleased]` section above it.
4. Open a pull request with these changes and merge it once CI is green.
5. Tag the merge commit and push the tag:

   ```sh
   git switch main && git pull
   git tag -a v0.2.0 -m "Regrex 0.2.0"
   git push origin v0.2.0
   ```

6. Follow the run in the Actions tab. If a step fails before publishing, fix it, delete the tag (`git push --delete origin v0.2.0`) and tag again. Published versions on PyPI and crates.io cannot be replaced: make a new patch release instead.

New crates that should be published (for example `regrex-formulas`) must be added, in dependency order, to the `crates-io` job of the workflow and to the `package` job of CI.
