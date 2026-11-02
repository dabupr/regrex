//! Rediscovers the kinetic energy formula from a small table of values.
//!
//! Run with `cargo run --release --example basic`.

use regrex_core::{search, SearchConfig};

fn main() {
    let mass = vec![1.0, 2.5, 4.0, 0.8, 7.2, 3.3, 5.1, 9.0];
    let speed = vec![3.0, 1.2, 7.5, 12.0, 0.6, 4.4, 2.2, 5.0];
    let energy: Vec<f64> = mass
        .iter()
        .zip(&speed)
        .map(|(m, v)| 0.5 * m * v * v)
        .collect();

    let names = ["m".to_string(), "v".to_string()];
    let results = search(&[mass, speed], &energy, &SearchConfig::default()).expect("valid data");

    println!("{:>10}  {:>10}  formula", "complexity", "nmse");
    for c in &results {
        let mark = if c.exact { " (exact)" } else { "" };
        println!(
            "{:>10}  {:>10.3e}  E = {}{mark}",
            c.complexity,
            c.nmse,
            c.expr.display_with(&names)
        );
    }
}
