//! A synthetic sensor export, wide and messy like a real one: 250
//! columns, most of them constant, flags, zeros, copies or plain noise, with
//! a few planted formulas. Checks that every planted formula is found and
//! nothing else is made up.
//!
//! In a debug build the scan is slow, so the test is ignored there; CI runs
//! it in release mode (`cargo test --release -p regrex-cli --test wide --
//! --include-ignored`). Set `REGREX_TIME_BUDGET` (seconds) to also fail when
//! the scan takes longer.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::process::Command;
use std::time::Instant;

const ROWS: usize = 1200;

/// A small deterministic generator, so the file is the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() * (hi - lo + 1) as f64) as i64
    }
}

struct Export {
    names: Vec<String>,
    columns: Vec<Vec<String>>,
}

impl Export {
    fn push(&mut self, name: impl Into<String>, cells: Vec<String>) {
        self.names.push(name.into());
        self.columns.push(cells);
    }

    fn numbers(&mut self, name: impl Into<String>, values: &[f64]) {
        self.push(name, values.iter().map(|v| fmt(*v)).collect());
    }

    fn csv(&self) -> String {
        let mut s = self.names.join(",");
        s.push('\n');
        for r in 0..ROWS {
            let row: Vec<&str> = self.columns.iter().map(|c| c[r].as_str()).collect();
            s.push_str(&row.join(","));
            s.push('\n');
        }
        s
    }
}

fn fmt(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// `Oct 15, 2026 @ 14:24:00.000` for `minute` minutes after 2026-10-15 00:00.
fn kibana(minute: i64) -> String {
    let day = 15 + minute / 1440;
    let m = minute % 1440;
    format!("\"Oct {day}, 2026 @ {:02}:{:02}:00.000\"", m / 60, m % 60)
}

/// The export, and the columns that may be reported: the planted formulas
/// (in any direction, since `a = total - b` holds as well as `total = a + b`)
/// and the copies.
fn export() -> (String, BTreeSet<String>) {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut e = Export {
        names: Vec::new(),
        columns: Vec::new(),
    };
    let mut allowed = BTreeSet::new();
    let ids: Vec<f64> = (0..ROWS).map(|r| 380_000.0 + r as f64).collect();
    e.numbers("PKTren", &ids);
    e.push("_id", (0..ROWS).map(|r| format!("x{r:06}")).collect());
    e.push("_score", vec!["-".to_string(); ROWS]);

    // Planted formulas.
    let voltage: Vec<f64> = (0..ROWS).map(|_| rng.int(500, 900) as f64).collect();
    let current: Vec<f64> = (0..ROWS).map(|_| rng.int(10, 400) as f64 / 10.0).collect();
    let power: Vec<f64> = voltage.iter().zip(&current).map(|(v, i)| v * i).collect();
    let speed_ms: Vec<f64> = (0..ROWS).map(|_| rng.int(0, 2500) as f64 / 100.0).collect();
    let speed_kmh: Vec<f64> = speed_ms
        .iter()
        .map(|v| (v * 3.6 * 100.0).round() / 100.0)
        .collect();
    let service: Vec<f64> = (0..ROWS).map(|_| rng.int(100, 999) as f64).collect();
    let line: Vec<f64> = service.iter().map(|s| (s / 100.0).floor()).collect();
    let load_a: Vec<f64> = (0..ROWS).map(|_| rng.int(0, 5000) as f64).collect();
    let load_b: Vec<f64> = (0..ROWS).map(|_| rng.int(0, 5000) as f64).collect();
    let load: Vec<f64> = load_a.iter().zip(&load_b).map(|(a, b)| a + b).collect();
    let start: Vec<i64> = (0..ROWS).map(|_| rng.int(0, 1200)).collect();
    let minutes: Vec<i64> = (0..ROWS).map(|_| rng.int(2, 9)).collect();
    for (name, values) in [
        ("tension", &voltage),
        ("corriente", &current),
        ("potencia", &power),
        ("velocidad_ms", &speed_ms),
        ("velocidad_kmh", &speed_kmh),
        ("servicio", &service),
        ("linea", &line),
        ("carga_eje1", &load_a),
        ("carga_eje2", &load_b),
        ("carga_total", &load),
    ] {
        e.numbers(name, values);
        allowed.insert(name.to_string());
    }
    e.push("salida", start.iter().map(|&m| kibana(m)).collect());
    e.push(
        "llegada",
        start
            .iter()
            .zip(&minutes)
            .map(|(&s, &m)| kibana(s + m))
            .collect(),
    );
    e.numbers(
        "duracion_min",
        &minutes.iter().map(|&m| m as f64).collect::<Vec<_>>(),
    );
    for name in ["salida", "llegada", "duracion_min"] {
        allowed.insert(name.to_string());
    }

    // Noise: measurements, flags, counters, columns that are zero almost
    // everywhere, constants, and copies of the measurements.
    let mut noise = Vec::new();
    for car in 0..4 {
        for k in 0..12 {
            let base = rng.int(10, 90) as f64;
            let mut v = base;
            let values: Vec<f64> = (0..ROWS)
                .map(|_| {
                    v += rng.next() - 0.5;
                    v
                })
                .collect();
            let name = format!("coche{car}_temperatura{k}");
            e.numbers(&name, &values);
            noise.push((name, values));
        }
        for k in 0..8 {
            let p = rng.next();
            let flags: Vec<f64> = (0..ROWS).map(|_| (rng.next() < p) as u8 as f64).collect();
            e.numbers(format!("coche{car}_puerta{k}"), &flags);
        }
        for k in 0..6 {
            let zeros: Vec<f64> = (0..ROWS)
                .map(|_| {
                    if rng.next() < 0.97 {
                        0.0
                    } else {
                        rng.int(1, 40) as f64
                    }
                })
                .collect();
            e.numbers(format!("coche{car}_alarma{k}"), &zeros);
        }
        for k in 0..30 {
            let value = rng.int(0, 3) as f64;
            e.numbers(format!("coche{car}_config{k}"), &vec![value; ROWS]);
        }
        let codes: Vec<f64> = (0..ROWS).map(|_| rng.int(1, 6) as f64).collect();
        e.numbers(format!("coche{car}_modo"), &codes);
        let sparse: Vec<String> = (0..ROWS)
            .map(|_| {
                if rng.next() < 0.3 {
                    "-".to_string()
                } else {
                    fmt(rng.int(0, 100) as f64)
                }
            })
            .collect();
        e.push(format!("coche{car}_humedad"), sparse);
    }
    for (k, (name, values)) in noise.iter().step_by(6).enumerate() {
        let copy: Vec<f64> = match k % 3 {
            0 => values.clone(),
            1 => values.iter().map(|v| v + 1.0).collect(),
            _ => values.iter().map(|v| v * 1000.0).collect(),
        };
        let copy_name = format!("{name}_copia");
        e.numbers(&copy_name, &copy);
        allowed.insert(name.clone());
        allowed.insert(copy_name);
    }
    (e.csv(), allowed)
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "slow in debug builds; CI runs it in release mode"
)]
fn wide_sensor_export_finds_the_planted_formulas_and_nothing_else() {
    let (csv, allowed) = export();
    let dir = std::env::temp_dir().join(format!("regrex-wide-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("export.csv");
    std::fs::write(&file, &csv).unwrap();

    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_regrex"))
        .args(["scan", file.to_str().unwrap(), "--json"])
        .output()
        .expect("binary runs");
    let seconds = started.elapsed().as_secs_f64();
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let derived = report["derived"].as_array().unwrap();
    let mut summary = String::new();
    for d in derived {
        let _ = writeln!(summary, "  {} = {}", d["column"], d["formula"]);
    }
    let formula = |column: &str| {
        derived
            .iter()
            .find(|d| d["column"] == column)
            .map(|d| d["formula"].as_str().unwrap().to_string())
            .unwrap_or_else(|| panic!("no formula for {column}; found:\n{summary}"))
    };
    assert_eq!(formula("potencia"), "tension * corriente", "{summary}");
    assert_eq!(formula("velocidad_kmh"), "3.6 * velocidad_ms", "{summary}");
    assert_eq!(formula("linea"), "servicio div 100", "{summary}");
    assert_eq!(
        formula("carga_total"),
        "carga_eje1 + carga_eje2",
        "{summary}"
    );
    // Dates read from Kibana's format. The duration itself is hard to
    // find (it varies little next to the dates), but the relation is.
    assert_eq!(formula("llegada"), "salida + duracion_min", "{summary}");
    for name in [
        "coche0_temperatura0",
        "coche1_temperatura6",
        "coche3_temperatura0",
    ] {
        let copy = derived
            .iter()
            .find(|d| d["column"] == format!("{name}_copia").as_str())
            .unwrap_or_else(|| panic!("{name}_copia not found:\n{summary}"));
        assert_eq!(copy["copy_of"], name);
    }
    let made_up: Vec<&str> = derived
        .iter()
        .map(|d| d["column"].as_str().unwrap())
        .filter(|c| !allowed.contains(*c))
        .collect();
    assert!(
        made_up.is_empty(),
        "made-up formulas for {made_up:?}:\n{summary}"
    );

    eprintln!(
        "wide export: {} columns scanned in {seconds:.1} s",
        report["columns"].as_array().unwrap().len()
    );
    if let Some(budget) = std::env::var("REGREX_TIME_BUDGET")
        .ok()
        .and_then(|b| b.parse::<f64>().ok())
    {
        assert!(
            seconds <= budget,
            "the scan took {seconds:.1} s, over the budget of {budget} s"
        );
    }
}
