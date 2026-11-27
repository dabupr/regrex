//! Two files read as one: the rows of both matched on the columns they
//! share (a line, a track and a chainage), so that the scan finds the
//! relations between them, such as a measured radius that equals the
//! designed one on circular curves.
//!
//! When the files share no such keys but both give coordinates (`x` and
//! `y` columns, or a WKT point read as `geom#x` and `geom#y`), each row of
//! the smaller file is matched to the nearest point of the other one,
//! within the rows that have the same text keys (a line, a track). Switches
//! given as points then get the chainage of the track they sit on.

use crate::{build_table, cells::is_missing, err, LoadError, Loaded};
use std::collections::HashMap;

/// How two files were matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    /// The columns of both files with the same name, matched on; for a
    /// match on the nearest point, also the coordinates of each file, as
    /// `(x, y) ~ (geom#x, geom#y)`.
    pub keys: Vec<String>,
    /// Rows matched: rows of the first file with a match in the second,
    /// or, for a match on the nearest point, rows of the smaller file.
    pub matched: usize,
    /// Rows of the first file, or of the smaller one for a match on the
    /// nearest point.
    pub rows: usize,
    /// Rows of the second file whose keys repeat an earlier row (the first
    /// one is used).
    pub repeated: usize,
    /// For a match on the nearest point, the median distance between the
    /// matched points.
    pub median_distance: Option<f64>,
}

/// The value of a key cell: numbers match when they are equal (`100` and
/// `100.0`), text when it is the same once trimmed.
fn key_cell(s: &str) -> Option<String> {
    let s = s.trim();
    if is_missing(s) {
        return None;
    }
    Some(match crate::parse_number_with(s, false) {
        Some((v, _)) => format!("{v}"),
        None => s.to_string(),
    })
}

/// The rows of `a` that have a row of `b` with the same values in the
/// columns both share, with the other columns of `b` added after those of
/// `a`. Shared columns of text or whole numbers are the keys; a column of
/// `b` whose name is also in `a` and is no key gets
/// ` (b_name)` after its name.
pub fn join(a: &Loaded, b: &Loaded, b_name: &str) -> Result<(Loaded, Joined), LoadError> {
    let equal = join_equal(a, b, b_name);
    // Keys that repeat in the second file (a line and a track, but no
    // chainage) match each row to an arbitrary one: points do better.
    let good = |j: &Joined| 2 * j.matched >= j.rows && j.repeated * 10 <= b.rows.len();
    match equal {
        Ok((l, j)) if good(&j) => Ok((l, j)),
        equal => match join_nearest(a, b, b_name) {
            Some(near) => Ok(near),
            None => equal,
        },
    }
}

/// [`join`] on equal keys.
fn join_equal(a: &Loaded, b: &Loaded, b_name: &str) -> Result<(Loaded, Joined), LoadError> {
    // Text and whole numbers name rows (a line, a chainage); a measurement
    // in both files is one more thing to compare, not a key.
    let names_rows = |l: &Loaded, n: &str| {
        l.categories.iter().any(|c| c == n)
            || l.skipped.iter().any(|c| c == n)
            || l.table.names.iter().position(|m| m == n).is_some_and(|c| {
                l.table.columns[c]
                    .iter()
                    .all(|v| !v.is_finite() || v.fract() == 0.0)
            })
    };
    let keys: Vec<(usize, usize)> = a
        .header
        .iter()
        .enumerate()
        .filter_map(|(i, n)| b.header.iter().position(|m| m == n).map(|j| (i, j)))
        .filter(|&(i, j)| names_rows(a, &a.header[i]) && names_rows(b, &b.header[j]))
        .collect();
    if keys.is_empty() {
        return Err(err(format!(
            "{b_name} has no column named as one of the first file, to match rows on"
        )));
    }
    let key = |row: &[String], side: fn(&(usize, usize)) -> usize| -> Option<Vec<String>> {
        keys.iter()
            .map(|k| key_cell(row.get(side(k)).map_or("", String::as_str)))
            .collect()
    };
    let mut index: HashMap<Vec<String>, usize> = HashMap::new();
    let mut repeated = 0;
    for (r, row) in b.rows.iter().enumerate() {
        if let Some(k) = key(row, |k| k.1) {
            match index.entry(k) {
                std::collections::hash_map::Entry::Occupied(_) => repeated += 1,
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(r);
                }
            }
        }
    }
    let extra: Vec<usize> = (0..b.header.len())
        .filter(|j| !keys.iter().any(|k| k.1 == *j))
        .collect();
    let mut header = a.header.clone();
    for &j in &extra {
        let name = &b.header[j];
        header.push(if a.header.contains(name) {
            format!("{name} ({b_name})")
        } else {
            name.clone()
        });
    }
    let rows: Vec<Vec<String>> = a
        .rows
        .iter()
        .filter_map(|row| {
            let r = *index.get(&key(row, |k| k.0)?)?;
            let mut out = row.clone();
            out.resize(a.header.len(), String::new());
            out.extend(
                extra
                    .iter()
                    .map(|&j| b.rows[r].get(j).cloned().unwrap_or_default()),
            );
            Some(out)
        })
        .collect();
    if rows.is_empty() {
        return Err(err(format!(
            "no row of {b_name} has the same {} as a row of the first file",
            keys.iter()
                .map(|k| a.header[k.0].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let joined = Joined {
        keys: keys.iter().map(|k| a.header[k.0].clone()).collect(),
        matched: rows.len(),
        rows: a.rows.len(),
        repeated,
        median_distance: None,
    };
    Ok((build_table(header, rows, Vec::new()), joined))
}

/// The two numeric columns of `l` that give a point: `x` and `y`,
/// `coord_x` and `coord_y`, `geom#x` and `geom#y`, `lon` and `lat`, or
/// `easting` and `northing`, with the most rows that have both.
fn coordinates(l: &Loaded) -> Option<(usize, usize)> {
    let t = &l.table;
    let names: Vec<String> = t.names.iter().map(|n| n.to_lowercase()).collect();
    let partner = |n: &str| -> Option<String> {
        for (x, y) in [
            ("longitude", "latitude"),
            ("longitud", "latitud"),
            ("lon", "lat"),
            ("lng", "lat"),
            ("easting", "northing"),
        ] {
            if let Some(stem) = n.strip_suffix(x) {
                return Some(format!("{stem}{y}"));
            }
        }
        n.strip_suffix('x').map(|stem| format!("{stem}y"))
    };
    let mut best: Option<(usize, usize, usize)> = None;
    for (i, n) in names.iter().enumerate() {
        if t.is_category(i) {
            continue;
        }
        let Some(want) = partner(n) else { continue };
        // A bare stem such as `index` -> `indey` is not a coordinate: the
        // stem must end in a separator or be empty.
        let stem = &want[..want.len() - 1];
        if want.ends_with('y') && !(stem.is_empty() || stem.ends_with(['_', '#', ' ', '.', '-'])) {
            continue;
        }
        let Some(j) = names.iter().position(|m| *m == want) else {
            continue;
        };
        if t.is_category(j) {
            continue;
        }
        let both = t.columns[i]
            .iter()
            .zip(&t.columns[j])
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .count();
        if both > 0 && best.is_none_or(|b| both > b.2) {
            best = Some((i, j, both));
        }
    }
    best.map(|(i, j, _)| (i, j))
}

/// A 2-d tree over points, for nearest point queries.
struct Tree {
    /// (x, y, row), arranged so that each subtree is a contiguous slice
    /// split at its middle element.
    points: Vec<(f64, f64, usize)>,
}

impl Tree {
    fn new(mut points: Vec<(f64, f64, usize)>) -> Self {
        fn build(p: &mut [(f64, f64, usize)], depth: usize) {
            if p.len() <= 1 {
                return;
            }
            let mid = p.len() / 2;
            if depth.is_multiple_of(2) {
                p.select_nth_unstable_by(mid, |a, b| a.0.total_cmp(&b.0));
            } else {
                p.select_nth_unstable_by(mid, |a, b| a.1.total_cmp(&b.1));
            }
            let (left, right) = p.split_at_mut(mid);
            build(left, depth + 1);
            build(&mut right[1..], depth + 1);
        }
        build(&mut points, 0);
        Tree { points }
    }

    /// The row of the nearest point and the squared distance to it.
    fn nearest(&self, x: f64, y: f64) -> Option<(usize, f64)> {
        fn go(
            p: &[(f64, f64, usize)],
            depth: usize,
            q: (f64, f64),
            best: &mut Option<(usize, f64)>,
        ) {
            if p.is_empty() {
                return;
            }
            let mid = p.len() / 2;
            let (px, py, row) = p[mid];
            let d = (px - q.0).powi(2) + (py - q.1).powi(2);
            if best.is_none_or(|b| d < b.1) {
                *best = Some((row, d));
            }
            let diff = if depth.is_multiple_of(2) { q.0 - px } else { q.1 - py };
            let (near, far) = if diff < 0.0 {
                (&p[..mid], &p[mid + 1..])
            } else {
                (&p[mid + 1..], &p[..mid])
            };
            go(near, depth + 1, q, best);
            if best.is_none_or(|b| diff * diff < b.1) {
                go(far, depth + 1, q, best);
            }
        }
        let mut best = None;
        go(&self.points, 0, (x, y), &mut best);
        best
    }
}

/// [`join`] on the nearest point: each row of the smaller file gets the
/// row of the other one whose point is nearest, among the rows with the
/// same text keys. `None` when either file has no coordinates or their
/// points do not overlap.
fn join_nearest(a: &Loaded, b: &Loaded, b_name: &str) -> Option<(Loaded, Joined)> {
    let (ca, cb) = (coordinates(a)?, coordinates(b)?);
    // Text columns of both files with the same name narrow the search
    // (a line, a track); numbers in both are compared by the scan.
    let text = |l: &Loaded, n: &str| {
        l.categories.iter().any(|c| c == n) || l.skipped.iter().any(|c| c == n)
    };
    let keys: Vec<(usize, usize)> = a
        .header
        .iter()
        .enumerate()
        .filter_map(|(i, n)| b.header.iter().position(|m| m == n).map(|j| (i, j)))
        .filter(|&(i, j)| text(a, &a.header[i]) && text(b, &b.header[j]))
        .collect();
    let group = |row: &[String], side: fn(&(usize, usize)) -> usize| -> Option<Vec<String>> {
        keys.iter()
            .map(|k| key_cell(row.get(side(k)).map_or("", String::as_str)))
            .collect()
    };
    // The smaller file asks, the larger one answers.
    let b_asks = b.rows.len() <= a.rows.len();
    let (ask, answer, c_ask, c_answer) = if b_asks {
        (b, a, cb, ca)
    } else {
        (a, b, ca, cb)
    };
    let side_ask: fn(&(usize, usize)) -> usize = if b_asks { |k| k.1 } else { |k| k.0 };
    let side_answer: fn(&(usize, usize)) -> usize = if b_asks { |k| k.0 } else { |k| k.1 };
    let mut groups: HashMap<Vec<String>, Vec<(f64, f64, usize)>> = HashMap::new();
    let (ax, ay) = (
        &answer.table.columns[c_answer.0],
        &answer.table.columns[c_answer.1],
    );
    for (r, row) in answer.rows.iter().enumerate() {
        let (x, y) = (ax[r], ay[r]);
        if let (true, Some(g)) = (x.is_finite() && y.is_finite(), group(row, side_answer)) {
            groups.entry(g).or_default().push((x, y, r));
        }
    }
    // The usual spacing of the answering points, for the farthest match:
    // a point far from every row of the other file is not on it.
    let spacing = {
        let mut steps: Vec<f64> = groups
            .values()
            .flat_map(|p| {
                p.windows(2)
                    .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            })
            .filter(|d| *d > 0.0)
            .collect();
        steps.sort_by(f64::total_cmp);
        steps.get(steps.len() / 2).copied()?
    };
    let limit = 10.0 * spacing;
    let trees: HashMap<Vec<String>, Tree> =
        groups.into_iter().map(|(g, p)| (g, Tree::new(p))).collect();
    let (qx, qy) = (&ask.table.columns[c_ask.0], &ask.table.columns[c_ask.1]);
    let extra: Vec<usize> = (0..b.header.len())
        .filter(|j| !keys.iter().any(|k| k.1 == *j))
        .collect();
    let mut header = a.header.clone();
    for &j in &extra {
        let name = &b.header[j];
        header.push(if a.header.contains(name) {
            format!("{name} ({b_name})")
        } else {
            name.clone()
        });
    }
    let mut distances = Vec::new();
    let mut rows = Vec::new();
    for (r, row) in ask.rows.iter().enumerate() {
        let (x, y) = (qx[r], qy[r]);
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        let Some(tree) = group(row, side_ask).and_then(|g| trees.get(&g)) else {
            continue;
        };
        let Some((m, d2)) = tree.nearest(x, y) else {
            continue;
        };
        let d = d2.sqrt();
        if d > limit {
            continue;
        }
        distances.push(d);
        let (ra, rb) = if b_asks { (m, r) } else { (r, m) };
        let mut out = a.rows[ra].clone();
        out.resize(a.header.len(), String::new());
        out.extend(
            extra
                .iter()
                .map(|&j| b.rows[rb].get(j).cloned().unwrap_or_default()),
        );
        rows.push(out);
    }
    // Most points of the asking file must lie on the other one.
    if rows.is_empty() || 2 * rows.len() < ask.rows.len() {
        return None;
    }
    distances.sort_by(f64::total_cmp);
    let mut names: Vec<String> = keys.iter().map(|k| a.header[k.0].clone()).collect();
    let (na, nb) = (&a.table.names, &b.table.names);
    names.push(format!(
        "({}, {}) ~ ({}, {})",
        na[ca.0], na[ca.1], nb[cb.0], nb[cb.1]
    ));
    let joined = Joined {
        keys: names,
        matched: rows.len(),
        rows: ask.rows.len(),
        repeated: 0,
        median_distance: Some(distances[distances.len() / 2]),
    };
    Some((build_table(header, rows, Vec::new()), joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(header: &[&str], rows: &[&[&str]]) -> Loaded {
        build_table(
            header.iter().map(|s| s.to_string()).collect(),
            rows.iter()
                .map(|r| r.iter().map(|s| s.to_string()).collect())
                .collect(),
            Vec::new(),
        )
    }

    #[test]
    fn matches_rows_on_the_shared_columns() {
        let a = loaded(
            &["line", "pk", "radius"],
            &[
                &["L1", "1.0", "300.5"],
                &["L1", "2.0", "310.2"],
                &["L2", "1.0", "0"],
            ],
        );
        let b = loaded(
            &["pk", "line", "radius", "cant"],
            &[
                &["2", "L1", "311.1", "0.1"],
                &["1", "L2", "0", "0"],
                &["9", "L3", "5", "1"],
            ],
        );
        let (j, how) = join(&a, &b, "measured").unwrap();
        assert_eq!(how.keys, ["line", "pk"]);
        assert_eq!((how.matched, how.rows, how.repeated), (2, 3, 0));
        assert_eq!(
            j.header,
            ["line", "pk", "radius", "radius (measured)", "cant"]
        );
        assert_eq!(j.rows[0], ["L1", "2.0", "310.2", "311.1", "0.1"]);
    }

    #[test]
    fn matches_points_to_the_nearest_point_of_the_other_file() {
        // A track every metre along a bend, and two switches given as
        // points a few centimetres off it, on the same track name.
        let mut track = Vec::new();
        for i in 0..200 {
            let t = i as f64 / 100.0;
            track.push(vec![
                "L1".to_string(),
                format!("{}", 1000 + i),
                format!("{:.3}", 100.0 * t.cos()),
                format!("{:.3}", 100.0 * t.sin()),
            ]);
        }
        let a = build_table(
            ["line", "pk", "coord_x", "coord_y"]
                .map(String::from)
                .to_vec(),
            track,
            Vec::new(),
        );
        let point = |i: usize, dx: f64| {
            let t = i as f64 / 100.0;
            format!(
                "POINT ({:.3} {:.3})",
                100.0 * t.cos() + dx,
                100.0 * t.sin() - dx
            )
        };
        let b = loaded(
            &["line", "switch", "geom"],
            &[
                &["L1", "A1", &point(17, 0.05)],
                &["L1", "A2", &point(150, -0.08)],
                &["L1", "A3", "POINT (5000 5000)"],
            ],
        );
        let (j, how) = join(&a, &b, "switches").unwrap();
        assert_eq!(how.keys, ["line", "(coord_x, coord_y) ~ (geom#x, geom#y)"]);
        assert_eq!((how.matched, how.rows), (2, 3), "the far one is left out");
        assert!(how.median_distance.unwrap() < 0.12);
        assert_eq!(
            j.header,
            ["line", "pk", "coord_x", "coord_y", "switch", "geom"]
        );
        assert_eq!(j.rows[0][1], "1017");
        assert_eq!(j.rows[1][1], "1150");
    }

    #[test]
    fn needs_a_shared_column() {
        let a = loaded(&["x"], &[&["1"]]);
        let b = loaded(&["y"], &[&["1"]]);
        assert!(join(&a, &b, "other").is_err());
    }
}
