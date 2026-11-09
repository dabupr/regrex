# Scan benchmark

Files that `regrex scan` is checked against on every push, with what it should find in each one in [`expected.toml`](expected.toml).

```sh
cargo run --release -p regrex-cli --example bench
```

The run prints, for each file, the relations found (✓) or missed (✗), the formulas reported that are not part of any expected relation (false positives) and the scan time. It fails when a relation marked as found is lost or a file has more false positives than recorded. When a result gets better, it says so: then update `expected.toml` in the same pull request (remove `found = false`, lower `false_positives`), so the improvement is kept from then on. Times are only reported, since shared CI machines vary too much to gate on them.

A reported formula counts for a relation when every column it uses belongs to that relation, so any rearrangement of `target = m * g * z` counts for it.

## Files

| Files | What they test | Source |
| --- | --- | --- |
| `feynman_*.csv` | 12 equations from the Feynman Lectures, 1000 random rows each; `target` is the result | [PMLB](https://github.com/EpistasisLab/pmlb) (MIT), from [AI Feynman](https://space.mit.edu/home/tegmark/aifeynman.html) |
| `../examples/data/*.csv` | Invoices and a tax change with formulas planted by hand, wrong cells and two tax rates | this repository |
| `diamonds_2000.csv` | `depth = 200 * z / (x + y)` in real, rounded data; 2000 rows of seaborn's diamonds | [seaborn-data](https://github.com/mwaskom/seaborn-data) |
| `exercise.csv`, `iris.csv`, `tips.csv`, `penguins.csv`, `mtcars.csv`, `car_crashes.csv`, `healthexp.csv`, `mpg.csv`, `planets.csv` | Classic datasets where no column is computed from others: anything reported is a false positive | [seaborn-data](https://github.com/mwaskom/seaborn-data), mtcars from [selva86/datasets](https://github.com/selva86/datasets) |

## Results on 2026-11-09

14 of 19 relations found, 2 false positives (both in `exercise.csv`), 40 s in total on 4 cores. Missing: four Feynman equations with square roots of differences or sums of terms, and the diamonds rule without `--ignore price,table`. The goals for 1.0.0 are in [`docs/ROADMAP.md`](../docs/ROADMAP.md).
