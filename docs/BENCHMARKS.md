# Benchmarks

## Feynman equations

[`crates/regrex-core/examples/feynman.rs`](../crates/regrex-core/examples/feynman.rs) runs Regrex on 24 equations from the Feynman Lectures on Physics, the same family of problems as the [AI Feynman](https://space.mit.edu/home/tegmark/aifeynman.html) database used by [SRBench](https://github.com/cavalab/srbench). Each equation gets 100 rows sampled from fixed ranges with a fixed seed, so the results are reproducible:

```sh
cargo run --release -p regrex-core --example feynman          # no noise
cargo run --release -p regrex-core --example feynman -- 0.01  # 1% noise
```

An equation counts as **recovered** when one of the returned formulas matches the true one on 200 new points sampled from ranges twice as wide. Formulas that only fit the training rows, or that only approximate the truth, do not count. The noise is Gaussian, with a standard deviation equal to a fraction of the target's root mean square, as in SRBench.

This is a curated subset with up to 5 inputs, not the full 100-equation database, and the numbers are not comparable to published SRBench results. They are here to track Regrex's own progress and to catch regressions.

Results on 4 cores of an Intel Xeon at 2.1 GHz, Rust 1.97, default settings with `max_params: 2` and the size limit shown in the source:

| | Recovered | Total time |
| --- | ---: | ---: |
| No noise | 21 of 24 (88%) | 79.4 s |
| 1% noise | 20 of 24 (83%) | 115.7 s |

### Without noise

| Equation | True formula | Best formula found | Recovered | Time |
| --- | --- | --- | :---: | ---: |
| I.6.2a | `exp(-theta^2 / 2) / sqrt(2 * pi)` | `0.398942 * exp(-0.5 * theta^2)` | ✓ | 2.51 s |
| I.12.1 | `mu * N` | `mu * N` | ✓ | 0.00 s |
| I.12.5 | `q2 * Ef` | `q2 * Ef` | ✓ | 0.00 s |
| I.14.3 | `m * g * z` | `m * g * z` | ✓ | 0.08 s |
| I.14.4 | `k * x^2 / 2` | `0.5 * k * x^2` | ✓ | 0.69 s |
| I.25.13 | `q / C` | `q / C` | ✓ | 0.00 s |
| I.29.4 | `omega / c` | `omega / c` | ✓ | 0.00 s |
| I.39.1 | `3 / 2 * p * V` | `1.5 * p * V` | ✓ | 0.05 s |
| I.39.22 | `n * kb * T / V` | `n * kb * T / V` | ✓ | 7.33 s |
| I.47.23 | `sqrt(gamma * p / rho)` | `sqrt(gamma * p / rho)` | ✓ | 8.24 s |
| I.12.4 | `q1 / (4 * pi * epsilon * r^2)` | `0.0795775 * q1 / (epsilon * r^2)` | ✓ | 8.20 s |
| I.34.27 | `h / (2 * pi) * omega` | `0.159155 * h * omega` | ✓ | 0.05 s |
| I.43.31 | `mob * kb * T` | `mob * kb * T` | ✓ | 0.08 s |
| II.3.24 | `Pwr / (4 * pi * r^2)` | `0.0795775 * Pwr / r^2` | ✓ | 0.56 s |
| II.8.31 | `epsilon * Ef^2 / 2` | `0.5 * epsilon * Ef^2` | ✓ | 0.60 s |
| II.27.18 | `epsilon * Ef^2` | `epsilon * Ef^2` | ✓ | 0.01 s |
| II.38.14 | `Y / (2 * (1 + sigma))` | `0.5 * Y / (sigma + 1)` | ✓ | 6.27 s |
| III.12.43 | `n * h / (2 * pi)` | `0.159155 * n * h` | ✓ | 0.06 s |
| I.18.12 | `r * F * sin(theta)` | `r * F * sin(theta)` | ✓ | 8.25 s |
| I.26.2 | `sqrt(L / g) * 2 * pi` | `6.28319 * sqrt(L / g)` | ✓ | 6.48 s |
| II.11.28 | `1 + n * alpha / (1 - n * alpha / 3)` | `1 - 3 * n / (n - 3 / alpha)` | ✓ | 8.93 s |
| I.13.4 | `m / 2 * (v^2 + u^2 + w^2)` | `0.6 * (log(m) * (v + w))^2 + 19` | ✗ | 7.22 s |
| I.50.26 | `x1 * (cos(omega * t) + alpha * cos(omega * t)^2)` | `4 * (cos(omega * t) + 0.25)^2 - 0.25` | ✗ | 7.22 s |
| I.41.16 | `h / (2 * pi) * omega^3 / (pi^2 * c^2 * (exp(h / (2 * pi) * omega / (kb * T)) - 1))` | `2.25 * omega * kb / exp(c)` | ✗ | 6.60 s |

### What is missing

- **Sums of several squares** (`m / 2 * (v^2 + u^2 + w^2)`) and **nested formulas** (Planck's law) are beyond the size that bottom-up enumeration reaches in a few seconds. Detecting additive and multiplicative separability (as AI Feynman does) would split them into smaller searches.
- **Noise** hides the rational function `1 + n α / (1 - n α / 3)` at 1%.

Products of powers with any number of inputs, such as `n * kb * T / V`, are found directly by a weighted least-squares fit in log space before the enumeration starts, which also makes the rest of the search faster when the result is exact.

## How to compare with other tools

The example writes a Markdown table, so it is easy to rerun after a change. To compare with tools such as [PySR](https://github.com/MilesCranmer/PySR) or [gplearn](https://github.com/trevorstephens/gplearn), use the same rows (the generator is a SplitMix64 seeded per equation) and the same recovery test. Contributions of such comparisons are welcome; please include the versions and settings used.
