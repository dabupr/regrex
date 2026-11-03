//! Recovery benchmark on equations from the Feynman Lectures on Physics, in
//! the style of the AI Feynman / SRBench "Feynman" problems.
//!
//! For each equation, data is sampled from the given ranges (with a fixed
//! seed), Regrex searches for a formula, and the equation counts as
//! recovered when a returned formula matches the true one on fresh points
//! sampled from twice the range (so overfitted formulas that only match the
//! training data do not count).
//!
//! ```sh
//! cargo run --release --example feynman            # noiseless
//! cargo run --release --example feynman -- 0.01    # 1% Gaussian noise
//! ```
//!
//! Prints a Markdown table; see `docs/BENCHMARKS.md`.

use regrex_core::parse::parse_expr;
use regrex_core::{search, SearchConfig};
use std::time::Instant;

/// One equation: id in the Feynman dataset, the formula, its variables with
/// sampling ranges, and the maximum size given to the search.
struct Problem {
    id: &'static str,
    formula: &'static str,
    vars: &'static [(&'static str, f64, f64)],
    max_size: usize,
}

const PROBLEMS: &[Problem] = &[
    Problem {
        id: "I.6.2a",
        formula: "exp(-theta^2 / 2) / sqrt(2 * pi)",
        vars: &[("theta", 1.0, 3.0)],
        max_size: 7,
    },
    Problem {
        id: "I.12.1",
        formula: "mu * N",
        vars: &[("mu", 1.0, 5.0), ("N", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "I.12.5",
        formula: "q2 * Ef",
        vars: &[("q2", 1.0, 5.0), ("Ef", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "I.14.3",
        formula: "m * g * z",
        vars: &[("m", 1.0, 5.0), ("g", 1.0, 5.0), ("z", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "I.14.4",
        formula: "k * x^2 / 2",
        vars: &[("k", 1.0, 5.0), ("x", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.25.13",
        formula: "q / C",
        vars: &[("q", 1.0, 5.0), ("C", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "I.29.4",
        formula: "omega / c",
        vars: &[("omega", 1.0, 10.0), ("c", 1.0, 10.0)],
        max_size: 5,
    },
    Problem {
        id: "I.39.1",
        formula: "3 / 2 * p * V",
        vars: &[("p", 1.0, 5.0), ("V", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.39.22",
        formula: "n * kb * T / V",
        vars: &[
            ("n", 1.0, 5.0),
            ("kb", 1.0, 5.0),
            ("T", 1.0, 5.0),
            ("V", 1.0, 5.0),
        ],
        max_size: 7,
    },
    Problem {
        id: "I.47.23",
        formula: "sqrt(gamma * p / rho)",
        vars: &[("gamma", 1.0, 5.0), ("p", 1.0, 5.0), ("rho", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.12.4",
        formula: "q1 / (4 * pi * epsilon * r^2)",
        vars: &[("q1", 1.0, 5.0), ("epsilon", 1.0, 5.0), ("r", 1.0, 5.0)],
        max_size: 9,
    },
    Problem {
        id: "I.34.27",
        formula: "h / (2 * pi) * omega",
        vars: &[("h", 1.0, 5.0), ("omega", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.43.31",
        formula: "mob * kb * T",
        vars: &[("mob", 1.0, 5.0), ("kb", 1.0, 5.0), ("T", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "II.3.24",
        formula: "Pwr / (4 * pi * r^2)",
        vars: &[("Pwr", 1.0, 5.0), ("r", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "II.8.31",
        formula: "epsilon * Ef^2 / 2",
        vars: &[("epsilon", 1.0, 5.0), ("Ef", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "II.27.18",
        formula: "epsilon * Ef^2",
        vars: &[("epsilon", 1.0, 5.0), ("Ef", 1.0, 5.0)],
        max_size: 5,
    },
    Problem {
        id: "II.38.14",
        formula: "Y / (2 * (1 + sigma))",
        vars: &[("Y", 1.0, 5.0), ("sigma", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "III.12.43",
        formula: "n * h / (2 * pi)",
        vars: &[("n", 1.0, 5.0), ("h", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.18.12",
        formula: "r * F * sin(theta)",
        vars: &[("r", 1.0, 5.0), ("F", 1.0, 5.0), ("theta", 0.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "I.26.2",
        formula: "sqrt(L / g) * 2 * pi",
        vars: &[("L", 1.0, 5.0), ("g", 1.0, 5.0)],
        max_size: 7,
    },
    Problem {
        id: "II.11.28",
        formula: "1 + n * alpha / (1 - n * alpha / 3)",
        vars: &[("n", 0.0, 1.0), ("alpha", 0.0, 1.0)],
        max_size: 9,
    },
    Problem {
        id: "I.13.4",
        formula: "m / 2 * (v^2 + u^2 + w^2)",
        vars: &[
            ("m", 1.0, 5.0),
            ("v", 1.0, 5.0),
            ("u", 1.0, 5.0),
            ("w", 1.0, 5.0),
        ],
        max_size: 9,
    },
    Problem {
        id: "I.50.26",
        formula: "x1 * (cos(omega * t) + alpha * cos(omega * t)^2)",
        vars: &[
            ("x1", 1.0, 3.0),
            ("omega", 1.0, 3.0),
            ("t", 1.0, 3.0),
            ("alpha", 1.0, 3.0),
        ],
        max_size: 9,
    },
    Problem {
        id: "I.41.16",
        formula:
            "h / (2 * pi) * omega^3 / (pi^2 * c^2 * (exp(h / (2 * pi) * omega / (kb * T)) - 1))",
        vars: &[
            ("omega", 1.0, 5.0),
            ("T", 1.0, 5.0),
            ("h", 1.0, 5.0),
            ("kb", 1.0, 5.0),
            ("c", 1.0, 5.0),
        ],
        max_size: 9,
    },
];

/// A small deterministic random number generator (SplitMix64), so results
/// are reproducible without extra dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    fn normal(&mut self) -> f64 {
        let (u, v) = (self.next().max(1e-300), self.next());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

fn sample(p: &Problem, rng: &mut Rng, n: usize, widen: f64) -> Vec<Vec<f64>> {
    p.vars
        .iter()
        .map(|&(_, lo, hi)| {
            let mid = (lo + hi) / 2.0;
            let half = (hi - lo) / 2.0 * widen;
            (0..n)
                .map(|_| rng.uniform(mid - half, mid + half))
                .collect()
        })
        .collect()
}

fn main() {
    let noise: f64 = std::env::args()
        .nth(1)
        .map(|a| a.parse().expect("noise level, such as 0.01"))
        .unwrap_or(0.0);
    let only: Option<String> = std::env::args().nth(2);

    println!("| Equation | True formula | Best formula found | Recovered | Time |");
    println!("| --- | --- | --- | :---: | ---: |");
    let (mut recovered, mut total, mut seconds) = (0, 0, 0.0);
    for p in PROBLEMS {
        if only.as_deref().is_some_and(|o| o != p.id) {
            continue;
        }
        let names: Vec<&str> = p.vars.iter().map(|v| v.0).collect();
        let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let truth = parse_expr(p.formula, &names).expect("valid formula");
        let mut rng = Rng(0x5EED ^ total as u64);

        let x = sample(p, &mut rng, 100, 1.0);
        let clean = truth.eval(&x, &[], 100);
        let scale = (clean.iter().map(|v| v * v).sum::<f64>() / 100.0).sqrt();
        let y: Vec<f64> = clean
            .iter()
            .map(|v| v + noise * scale * rng.normal())
            .collect();

        let config = SearchConfig {
            max_size: p.max_size,
            max_params: 2,
            ..SearchConfig::default()
        };
        let start = Instant::now();
        let found = search(&x, &y, &config).unwrap_or_default();
        let elapsed = start.elapsed().as_secs_f64();

        // Recovered: some candidate matches the truth on wider, unseen points.
        let test = sample(p, &mut rng, 200, 2.0);
        let want = truth.eval(&test, &[], 200);
        let matches = |c: &regrex_core::Candidate| {
            let got = c.expr.eval(&test, &[], 200);
            let (mut err, mut norm) = (0.0, 0.0);
            for (g, w) in got.iter().zip(&want) {
                if !w.is_finite() {
                    continue;
                }
                err += (g - w) * (g - w);
                norm += w * w;
            }
            err.is_finite() && err <= norm * (1e-8 + 10.0 * noise * noise)
        };
        let hit = found.iter().find(|c| matches(c));
        let shown = hit.or(found.last());
        total += 1;
        seconds += elapsed;
        if hit.is_some() {
            recovered += 1;
        }
        println!(
            "| {} | `{}` | `{}` | {} | {:.2} s |",
            p.id,
            p.formula,
            shown.map_or("—".to_string(), |c| c
                .expr
                .display_with(&owned)
                .to_string()),
            if hit.is_some() { "✓" } else { "✗" },
            elapsed
        );
    }
    println!();
    println!(
        "Recovered {recovered} of {total} equations ({:.0}%) in {seconds:.1} s, noise {:.0}%.",
        100.0 * recovered as f64 / total.max(1) as f64,
        noise * 100.0
    );
}
