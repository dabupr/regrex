//! One line per relation.
//!
//! When `target = m * g * z`, the scan explains every column of it, each
//! solved for itself: `m = target / (g * z)`, `g = target / (m * z)` and so
//! on. [`same_relations`] tells which of the formulas found are such
//! rearrangements of another one, so a report can show the relation once,
//! with the other forms as alternatives.

use crate::scan::{matching_rows, row_tolerance, DerivedColumn, ScanConfig, Table};

/// Share of the rows a formula matches on which another must hold too to be
/// the same relation.
const SAME_ROWS: f64 = 0.9;

/// For each formula of `derived`, found in `table`, the index (in
/// `derived`) of the formula it is another form of, or `None` for the form
/// to show.
///
/// Two formulas are forms of one relation when they link the same columns
/// and each holds on the rows where the other does. Of each relation, the
/// form shown is the one for a column named like a result (`target`,
/// `total`, `y`…), then the simplest, then the one for the column furthest
/// right. Copies, formulas per regime, formulas between rows and columns
/// with two formulas are always shown.
///
/// ```
/// use regrex_core::{relations::same_relations, scan, ScanConfig, Table};
///
/// let m: Vec<f64> = (1..=30).map(|i| i as f64).collect();
/// let v: Vec<f64> = (1..=30).map(|i| (i * 7 % 11 + 1) as f64).collect();
/// let p: Vec<f64> = m.iter().zip(&v).map(|(m, v)| m * v).collect();
/// let table = Table {
///     names: vec!["m".into(), "v".into(), "p".into()],
///     columns: vec![m, v, p],
///     ..Table::default()
/// };
/// let found = scan(&table, &ScanConfig::default());
/// let same = same_relations(&table, &found, &ScanConfig::default());
/// let shown: Vec<&str> = found
///     .iter()
///     .zip(&same)
///     .filter(|(_, s)| s.is_none())
///     .map(|(d, _)| d.formula_text.as_str())
///     .collect();
/// assert_eq!(shown, ["m * v"]);
/// ```
pub fn same_relations(
    table: &Table,
    derived: &[DerivedColumn],
    config: &ScanConfig,
) -> Vec<Option<usize>> {
    let n = derived.len();
    let plain = |d: &DerivedColumn| !d.copy && d.regime.is_none() && !d.steps && d.other.is_none();
    let columns = |d: &DerivedColumn| {
        let mut c = d.inputs.clone();
        c.push(d.target);
        c.sort_unstable();
        c.dedup();
        c
    };
    let sets: Vec<Vec<usize>> = derived.iter().map(columns).collect();
    // Rows each formula holds on.
    let holds: Vec<Vec<usize>> = derived
        .iter()
        .map(|d| {
            let rows: Vec<usize> = (0..table.n_rows()).collect();
            let tolerance = row_tolerance(table, d.target, config);
            let y = &table.columns[d.target];
            let rows: Vec<usize> = rows
                .into_iter()
                .filter(|&r| {
                    y[r].is_finite() && d.inputs.iter().all(|&c| table.columns[c][r].is_finite())
                })
                .collect();
            matching_rows(&d.formula, table, &rows, y, &tolerance)
        })
        .collect();
    let same = |i: usize, j: usize| {
        if !plain(&derived[i]) || !plain(&derived[j]) || sets[i] != sets[j] {
            return false;
        }
        let (a, b) = (&holds[i], &holds[j]);
        let both = a.iter().filter(|r| b.binary_search(r).is_ok()).count() as f64;
        both >= SAME_ROWS * a.len() as f64 && both >= SAME_ROWS * b.len() as f64
    };
    // Groups of forms of one relation, by union-find.
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            if same(i, j) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut shown: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let r = root(&mut parent, i);
        let best = shown[r].map_or(i, |b| {
            if better(table, &derived[i], &derived[b]) {
                i
            } else {
                b
            }
        });
        shown[r] = Some(best);
    }
    (0..n)
        .map(|i| {
            let best = shown[root(&mut parent, i)].expect("every group has a form");
            (best != i).then_some(best)
        })
        .collect()
}

/// Words of column names that say the column is a result.
const RESULT_WORDS: [&str; 10] = [
    "target", "y", "total", "subtotal", "sum", "result", "output", "amount", "outcome", "label",
];

fn named_like_a_result(name: &str) -> bool {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| RESULT_WORDS.contains(&w))
}

/// Whether `a` is a better form to show than `b`.
fn better(table: &Table, a: &DerivedColumn, b: &DerivedColumn) -> bool {
    let result = |d: &DerivedColumn| named_like_a_result(&table.names[d.target]);
    match (result(a), result(b)) {
        (true, false) => return true,
        (false, true) => return false,
        _ => {}
    }
    (a.complexity, std::cmp::Reverse(a.target)) < (b.complexity, std::cmp::Reverse(b.target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan;

    fn table(names: &[&str], columns: Vec<Vec<f64>>) -> Table {
        Table {
            names: names.iter().map(|s| s.to_string()).collect(),
            columns,
            ..Table::default()
        }
    }

    fn shown(t: &Table) -> Vec<String> {
        let config = ScanConfig::default();
        let found = scan(t, &config);
        let same = same_relations(t, &found, &config);
        found
            .iter()
            .zip(&same)
            .filter(|(_, s)| s.is_none())
            .map(|(d, _)| format!("{} = {}", t.names[d.target], d.formula_text))
            .collect()
    }

    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                1.0 + 9.0 * ((state >> 11) as f64 / (1u64 << 53) as f64)
            })
            .collect()
    }

    #[test]
    fn a_product_of_three_columns_is_shown_once_for_the_result() {
        let n = 200;
        let (m, g, z) = (noise(n, 1), noise(n, 2), noise(n, 3));
        let target: Vec<f64> = (0..n).map(|i| m[i] * g[i] * z[i]).collect();
        let t = table(&["m", "g", "z", "target"], vec![m, g, z, target]);
        assert_eq!(shown(&t), ["target = m * g * z"]);
    }

    #[test]
    fn different_relations_stay_apart() {
        let n = 200;
        let net = noise(n, 4);
        let vat: Vec<f64> = net.iter().map(|x| 0.21 * x).collect();
        let total: Vec<f64> = net.iter().zip(&vat).map(|(a, b)| a + b).collect();
        let t = table(&["net", "vat", "total"], vec![net, vat, total]);
        let found = shown(&t);
        // vat and total are both proportional to net here: several
        // relations, never all of them folded into one.
        assert!(found.len() >= 2, "{found:?}");
    }

    #[test]
    fn result_names_win() {
        assert!(named_like_a_result("Total (EUR)"));
        assert!(named_like_a_result("line_total"));
        assert!(!named_like_a_result("totality"));
        assert!(!named_like_a_result("price"));
    }
}
