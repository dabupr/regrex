//! End-to-end tests: the search must rediscover well-known formulas.

use regrex_core::{search, Candidate, SearchConfig};

/// Deterministic pseudo-random numbers in `[lo, hi)` (xorshift), so the tests
/// do not need a random-number crate and are reproducible.
fn uniform(n: usize, lo: f64, hi: f64, seed: u64) -> Vec<f64> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            lo + (hi - lo) * ((s >> 11) as f64 / (1u64 << 53) as f64)
        })
        .collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn best(inputs: &[Vec<f64>], target: &[f64], max_size: usize) -> Candidate {
    let config = SearchConfig {
        max_size,
        ..SearchConfig::default()
    };
    let results = search(inputs, target, &config).expect("valid data");
    results.into_iter().next().expect("at least one candidate")
}

#[test]
fn kinetic_energy() {
    let m = uniform(50, 0.5, 10.0, 1);
    let v = uniform(50, 0.1, 30.0, 2);
    let e: Vec<f64> = m.iter().zip(&v).map(|(m, v)| 0.5 * m * v * v).collect();
    let b = best(&[m, v], &e, 6);
    assert!(b.exact, "{b:?}");
    assert_eq!(
        b.expr.display_with(&names(&["m", "v"])).to_string(),
        "0.5 * m * v^2"
    );
}

#[test]
fn ohms_law() {
    let i = uniform(40, 0.01, 2.0, 3);
    let r = uniform(40, 10.0, 1000.0, 4);
    let v: Vec<f64> = i.iter().zip(&r).map(|(i, r)| i * r).collect();
    let b = best(&[i, r], &v, 5);
    assert!(b.exact);
    assert_eq!(
        b.expr.display_with(&names(&["I", "R"])).to_string(),
        "I * R"
    );
}

#[test]
fn pendulum_period() {
    // T = 2π·sqrt(L / g), with g = 9.81 fixed: T = 2.00607 * sqrt(L)
    let l = uniform(40, 0.1, 5.0, 5);
    let t: Vec<f64> = l
        .iter()
        .map(|l| 2.0 * std::f64::consts::PI * (l / 9.81).sqrt())
        .collect();
    let b = best(&[l], &t, 5);
    assert!(b.exact, "{b:?}");
    assert_eq!(b.expr.to_string(), "2.00607 * sqrt(x0)");
}

#[test]
fn exponential_decay() {
    let t = uniform(40, 0.0, 5.0, 6);
    let y: Vec<f64> = t.iter().map(|t| 3.0 * (-0.5 * t).exp()).collect();
    let b = best(&[t], &y, 6);
    assert!(b.exact, "{b:?}");
    assert_eq!(b.expr.to_string(), "3 * exp(-0.5 * x0)");
}

#[test]
fn ideal_gas_with_three_variables() {
    // P = n R T / V, R = 8.314
    let n = uniform(40, 0.5, 3.0, 7);
    let t = uniform(40, 250.0, 400.0, 8);
    let v = uniform(40, 0.01, 0.1, 9);
    let p: Vec<f64> = (0..40).map(|i| n[i] * 8.314 * t[i] / v[i]).collect();
    let b = best(&[n, t, v], &p, 7);
    assert!(b.exact, "{b:?}");
    assert_eq!(
        b.expr.display_with(&names(&["n", "T", "V"])).to_string(),
        "8.314 * n * T / V"
    );
}

#[test]
fn noisy_data_still_finds_the_law() {
    let x = uniform(200, 0.0, 10.0, 10);
    let noise = uniform(200, -0.05, 0.05, 11);
    let y: Vec<f64> = x
        .iter()
        .zip(&noise)
        .map(|(x, e)| 2.0 * x + 1.0 + e)
        .collect();
    let b = best(&[x], &y, 5);
    assert!(!b.exact);
    assert!(b.r2 > 0.999);
    assert_eq!(b.expr.to_string(), "2 * x0 + 1");
}

#[test]
fn results_are_a_pareto_front() {
    let x = uniform(30, 0.0, 3.0, 12);
    let y: Vec<f64> = x.iter().map(|x| x * x * x + x).collect();
    let config = SearchConfig {
        max_size: 6,
        ..SearchConfig::default()
    };
    let mut results = search(&[x], &y, &config).unwrap();
    results.sort_by_key(|c| c.complexity);
    for pair in results.windows(2) {
        assert!(pair[1].complexity > pair[0].complexity);
        assert!(pair[1].nmse < pair[0].nmse);
    }
}

#[test]
fn units_rule_out_impossible_formulas() {
    use regrex_core::units::parse_unit;
    // Free fall: d = 0.5 * g * t^2 with g = 9.81 m/s².
    let t = uniform(30, 0.1, 4.0, 13);
    let v0 = uniform(30, 0.0, 5.0, 14);
    let d: Vec<f64> = t.iter().map(|t| 0.5 * 9.81 * t * t).collect();
    let config = SearchConfig {
        max_size: 5,
        input_dims: Some(vec![
            parse_unit("s").unwrap().dim,
            parse_unit("m/s").unwrap().dim,
        ]),
        target_dim: Some(parse_unit("m").unwrap().dim),
        ..SearchConfig::default()
    };
    let results = search(&[t, v0], &d, &config).unwrap();
    let names = names(&["t", "v0"]);
    let best = &results[0];
    assert!(best.exact);
    assert_eq!(best.expr.display_with(&names).to_string(), "4.905 * t^2");
    // No formula on the front adds a time to a speed.
    for c in &results {
        let text = c.expr.display_with(&names).to_string();
        assert!(
            !text.contains("t + v0") && !text.contains("v0 + t"),
            "{text}"
        );
    }
}

#[test]
fn power_laws_with_many_inputs() {
    // p = n R T / V with four inputs: beyond plain enumeration at this size,
    // found by the log-space fit.
    let mut cols = vec![Vec::new(); 4];
    let mut y = Vec::new();
    for i in 0..60 {
        let f = |k: usize| 1.0 + ((i * (7 + 2 * k) + 3 * k) % 23) as f64 / 5.0;
        let row = [f(0), f(1), f(2), f(3)];
        for (c, v) in cols.iter_mut().zip(row) {
            c.push(v);
        }
        y.push(2.5 * row[0] * row[1] * row[2] / row[3]);
    }
    let config = SearchConfig {
        max_size: 5,
        ..SearchConfig::default()
    };
    let found = search(&cols, &y, &config).unwrap();
    let names: Vec<String> = ["n", "R", "T", "V"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        found[0].expr.display_with(&names).to_string(),
        "2.5 * n * R * T / V"
    );
    assert!(found[0].exact);
}
