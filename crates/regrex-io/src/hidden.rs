//! Numbers hidden in text columns.
//!
//! A text column can hold numbers that a formula may use:
//!
//! - prefix + number, as in `GIS5550` or `VIEW_EXPORT_ELEMENTS.10385`, read
//!   as `codi#num`;
//! - a fraction, as in `Crossing 1/8,5`, read as its value in `nom#ratio`;
//! - a WKT point, `POINT (431923.55 4586893.94)`, read as `geom#x`,
//!   `geom#y` (and `geom#z`);
//! - a list, `A1288, A1286`, read as its length in `list#count`.
//!
//! Each reading needs most of the column's cells to follow it, and the new
//! column keeps the text column's name before the `#`, so a formula found
//! for it is a formula for that column.

/// A numeric column read out of a text column.
pub(crate) struct Hidden {
    /// `column#what`.
    pub name: String,
    pub values: Vec<Option<f64>>,
    pub decimals: Option<u32>,
    /// How it was read, for [`crate::Loaded::converted`].
    pub note: String,
}

/// Share of the non-empty cells a reading must cover.
const MIN_SHARE: f64 = 0.9;
/// Share of the cells that a fraction, or the most common prefix, must be
/// found in: the others may be named in another way (`Switch`, `VCAN0143`).
const MIN_PART: f64 = 0.3;
/// Share of the cells of a list column that must hold more than one item.
const MIN_LISTS: f64 = 0.1;

/// Every hidden number found in `cells`, the cells of the text column
/// `name`.
pub(crate) fn hidden_numbers(name: &str, cells: &[&str], comma_decimal: bool) -> Vec<Hidden> {
    let present: Vec<&str> = cells
        .iter()
        .map(|s| s.trim())
        .filter(|s| !crate::cells::is_missing(s))
        .collect();
    if present.is_empty() {
        return Vec::new();
    }
    if let Some(points) = wkt_points(name, cells, present.len()) {
        return points;
    }
    let mut out = Vec::new();
    if let Some(h) = prefixed(name, cells, present.len()) {
        out.push(h);
    } else if let Some(h) = fraction(name, cells, present.len(), comma_decimal) {
        out.push(h);
    }
    if let Some(h) = list_length(name, cells, &present) {
        out.push(h);
    }
    out
}

fn enough(found: usize, present: usize) -> bool {
    found as f64 >= MIN_SHARE * present as f64
}

/// At least `min` different values.
fn varies(values: &[Option<f64>], min: usize) -> bool {
    let mut seen: Vec<f64> = values.iter().flatten().copied().collect();
    seen.sort_by(f64::total_cmp);
    seen.dedup();
    seen.len() >= min
}

/// Decimals of a plain number written in `s`.
fn decimals(s: &str) -> u32 {
    s.find(['.', ',']).map_or(0, |i| {
        s[i + 1..].chars().filter(char::is_ascii_digit).count() as u32
    })
}

fn number(s: &str, comma_decimal: bool) -> Option<f64> {
    let s = if comma_decimal || (s.matches(',').count() == 1 && !s.contains('.')) {
        s.replace(',', ".")
    } else {
        s.to_string()
    };
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `POINT (x y)` or `POINT Z (x y z)`.
fn wkt_points(name: &str, cells: &[&str], present: usize) -> Option<Vec<Hidden>> {
    let parse = |s: &str| -> Option<Vec<(f64, u32)>> {
        let s = s.trim();
        let rest = s
            .strip_prefix("POINT")
            .or_else(|| s.strip_prefix("point"))?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix('Z')
            .or_else(|| rest.strip_prefix('z'))
            .unwrap_or(rest)
            .trim();
        let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
        let coords: Vec<(f64, u32)> = inner
            .split_whitespace()
            .map(|t| t.parse::<f64>().ok().map(|v| (v, decimals(t))))
            .collect::<Option<_>>()?;
        (2..=3).contains(&coords.len()).then_some(coords)
    };
    let parsed: Vec<Option<Vec<(f64, u32)>>> = cells.iter().map(|s| parse(s)).collect();
    let found = parsed.iter().flatten().count();
    if found == 0 || !enough(found, present) {
        return None;
    }
    let dims = parsed.iter().flatten().map(Vec::len).min()?;
    Some(
        ["x", "y", "z"][..dims]
            .iter()
            .enumerate()
            .map(|(k, axis)| {
                let values: Vec<Option<f64>> =
                    parsed.iter().map(|p| p.as_ref().map(|c| c[k].0)).collect();
                Hidden {
                    name: format!("{name}#{axis}"),
                    decimals: parsed.iter().flatten().map(|c| c[k].1).max(),
                    values,
                    note: format!("{axis} of a WKT point"),
                }
            })
            .collect(),
    )
}

/// The same text prefix followed by a whole number in every cell.
fn prefixed(name: &str, cells: &[&str], present: usize) -> Option<Hidden> {
    let split = |s: &str| -> Option<(String, f64)> {
        let s = s.trim();
        let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 || digits == s.len() || digits > 15 {
            return None;
        }
        let (prefix, num) = s.split_at(s.len() - digits);
        // `A12.5` is a decimal number, not a prefix and a number.
        if prefix.ends_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        Some((prefix.to_string(), num.parse().ok()?))
    };
    let parsed: Vec<Option<(String, f64)>> = cells.iter().map(|s| split(s)).collect();
    // The most common prefix.
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (p, _) in parsed.iter().flatten() {
        *counts.entry(p.as_str()).or_default() += 1;
    }
    // Most cells are a prefix and a number; the number is read after the
    // most common prefix (`GIS5550` but not `VCAN0143`), whose numbers may
    // follow their own rule.
    let all: usize = counts.values().sum();
    let (prefix, &n) = counts
        .iter()
        .max_by_key(|(p, &n)| (n, std::cmp::Reverse(*p)))?;
    if !enough(all, present) || (n as f64) < MIN_PART * present as f64 {
        return None;
    }
    let values: Vec<Option<f64>> = parsed
        .iter()
        .map(|p| p.as_ref().filter(|(q, _)| q == prefix).map(|(_, v)| *v))
        .collect();
    varies(&values, 3).then(|| Hidden {
        name: format!("{name}#num"),
        values,
        decimals: Some(0),
        note: format!("number after \"{prefix}\""),
    })
}

/// A fraction `a/b` somewhere in the text: `Crossing 1/8,5` is 1/8.5.
fn fraction(name: &str, cells: &[&str], present: usize, comma_decimal: bool) -> Option<Hidden> {
    let find = |s: &str| -> Option<f64> {
        let b = s.as_bytes();
        let slash = s.find('/')?;
        let num_char = |c: u8| c.is_ascii_digit() || c == b'.' || c == b',';
        let mut i = slash;
        while i > 0 && num_char(b[i - 1]) {
            i -= 1;
        }
        let mut j = slash + 1;
        while j < b.len() && num_char(b[j]) {
            j += 1;
        }
        let (a, d) = (&s[i..slash], &s[slash + 1..j]);
        // Dates like 1/2/2024 are not fractions.
        if s[j..].starts_with('/') || s[..i].ends_with('/') {
            return None;
        }
        let (a, d) = (number(a, comma_decimal)?, number(d, comma_decimal)?);
        (d != 0.0).then(|| a / d)
    };
    let values: Vec<Option<f64>> = cells.iter().map(|s| find(s)).collect();
    let found = values.iter().flatten().count();
    let share = found as f64 >= MIN_PART * present as f64;
    (found > 0 && share && varies(&values, 2)).then(|| Hidden {
        name: format!("{name}#ratio"),
        values,
        decimals: None,
        note: "value of the fraction in the text".to_string(),
    })
}

/// The number of items of a list cell, `A1288, A1286` or `['a' 'b']`.
fn list_length(name: &str, cells: &[&str], present: &[&str]) -> Option<Hidden> {
    let items = |s: &str| -> usize {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            // Python-style lists: quoted items.
            let quotes = inner.matches(['\'', '"']).count();
            return if quotes >= 2 {
                quotes / 2
            } else {
                inner.split(',').filter(|t| !t.trim().is_empty()).count()
            };
        }
        if s.contains("; ") || s.contains(", ") {
            s.split([';', ',']).filter(|t| !t.trim().is_empty()).count()
        } else {
            1
        }
    };
    let lists = present.iter().filter(|s| items(s) > 1).count();
    if (lists as f64) < MIN_LISTS * present.len() as f64 {
        return None;
    }
    let values: Vec<Option<f64>> = cells
        .iter()
        .map(|s| (!crate::cells::is_missing(s)).then(|| items(s) as f64))
        .collect();
    varies(&values, 2).then(|| Hidden {
        name: format!("{name}#count"),
        values,
        decimals: Some(0),
        note: "number of items in the list".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cells: &[&str]) -> Vec<String> {
        hidden_numbers("c", cells, false)
            .into_iter()
            .map(|h| h.name)
            .collect()
    }

    #[test]
    fn reads_a_prefix_and_number() {
        let h = &hidden_numbers("codi", &["GIS5550", "GIS5551", "", "GIS5600"], false)[0];
        assert_eq!(h.name, "codi#num");
        assert_eq!(
            h.values,
            vec![Some(5550.0), Some(5551.0), None, Some(5600.0)]
        );
        let mixed = ["GIS1", "GIS2", "VCAN7", "GIS3", "VCAN8"];
        let h = &hidden_numbers("codi", &mixed, false)[0];
        assert_eq!(h.values, vec![Some(1.0), Some(2.0), None, Some(3.0), None]);
        let fid = ["VIEW.10385", "VIEW.10386", "VIEW.10390"];
        assert_eq!(names(&fid), ["c#num"]);
    }

    #[test]
    fn reads_fractions_with_a_decimal_comma() {
        let h = &hidden_numbers(
            "t",
            &["Crossing 1/8,5", "Crossing 1/9", "Switch 1/12"],
            false,
        )[0];
        assert_eq!(h.name, "t#ratio");
        assert!((h.values[0].unwrap() - 1.0 / 8.5).abs() < 1e-12);
        assert!(names(&["1/2/2024", "3/4/2024", "5/6/2024"]).is_empty());
    }

    #[test]
    fn reads_wkt_points() {
        let h = hidden_numbers("g", &["POINT (1.5 2.25)", "POINT (3 4)"], false);
        let n: Vec<&str> = h.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(n, ["g#x", "g#y"]);
        assert_eq!(h[1].values, vec![Some(2.25), Some(4.0)]);
        assert_eq!(h[1].decimals, Some(2));
    }

    #[test]
    fn counts_list_items() {
        let h = &hidden_numbers("l", &["A1, A2", "A3", "A4, A5, A6", "A7"], false)[0];
        assert_eq!(h.name, "l#count");
        assert_eq!(h.values, vec![Some(2.0), Some(1.0), Some(3.0), Some(1.0)]);
    }

    #[test]
    fn plain_names_hide_nothing() {
        assert!(names(&["Straight", "Circular", "Clothoid", "Straight"]).is_empty());
        assert!(names(&["V1", "V2", "V1", "V3"]).len() <= 1);
    }
}
