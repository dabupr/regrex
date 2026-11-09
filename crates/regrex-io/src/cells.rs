//! Cells that are numbers in disguise: missing-value markers, `true` and
//! `false`, times, dates, and numbers followed by a unit (`24.1 km`).
//!
//! Real exports (sensor logs, timetables, Kibana or database dumps) are full
//! of them, and a column of times is exactly what a delay is computed from.

use crate::parse_number_with;

/// Whether a cell stands for a missing value: empty, `-`, `NA`, `null`...
pub fn is_missing(s: &str) -> bool {
    let s = s.trim();
    s.is_empty()
        || matches!(
            s,
            "-" | "--" | "—" | "?" | "#N/A" | "#NA" | "#VALUE!" | "#DIV/0!"
        )
        || ["na", "n/a", "nan", "null", "none", "nil", "missing"]
            .iter()
            .any(|m| s.eq_ignore_ascii_case(m))
}

/// A column read from cells that are not plain numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The value of each cell (`None` when missing or unreadable).
    pub values: Vec<Option<f64>>,
    /// Decimals to tolerate when the column is computed from others.
    pub decimals: Option<u32>,
    /// The unit written after the numbers, if any.
    pub unit_text: Option<String>,
    /// How the cells were read, for people (`times as minutes`).
    pub note: String,
    /// Whether the cells are dates or times of day (see
    /// [`regrex_core::Table::timestamps`]).
    pub timestamp: bool,
}

/// Reads a column whose cells are not plain numbers, if at least 90% of its
/// non-missing cells are of one kind this module knows.
pub fn read_column(cells: &[&str], comma_decimal: bool) -> Option<Reading> {
    let present: Vec<&str> = cells.iter().copied().filter(|s| !is_missing(s)).collect();
    if present.is_empty() {
        return None;
    }
    let enough = |n: usize| n * 10 >= present.len() * 9;
    let read_all = |f: &dyn Fn(&str) -> Option<f64>| -> Vec<Option<f64>> {
        cells
            .iter()
            .map(|s| if is_missing(s) { None } else { f(s) })
            .collect()
    };
    let count = |values: &[Option<f64>]| values.iter().filter(|v| v.is_some()).count();

    // true / false
    let values = read_all(&parse_bool);
    if enough(count(&values)) {
        return Some(Reading {
            values,
            decimals: Some(0),
            unit_text: None,
            note: "true/false as 1/0".into(),
            timestamp: false,
        });
    }

    // Times, dates, both. Day-first (15/10/2026) unless the cells say
    // otherwise.
    let stamps = |day_first: bool| -> Vec<Option<Stamp>> {
        cells
            .iter()
            .map(|s| {
                if is_missing(s) {
                    None
                } else {
                    parse_stamp(s, day_first)
                }
            })
            .collect()
    };
    let (by_day, by_month) = (stamps(true), stamps(false));
    let n = |v: &[Option<Stamp>]| v.iter().filter(|s| s.is_some()).count();
    let parsed = if n(&by_month) > n(&by_day) {
        by_month
    } else {
        by_day
    };
    if enough(n(&parsed)) {
        if let Some(reading) = stamp_column(&parsed) {
            return Some(reading);
        }
    }

    // Numbers followed by the same unit: `24.1 km`, `5 min`.
    let split: Vec<Option<(f64, u32, &str)>> = cells
        .iter()
        .map(|s| {
            if is_missing(s) {
                None
            } else {
                number_and_unit(s, comma_decimal)
            }
        })
        .collect();
    let unit = split.iter().flatten().map(|(_, _, u)| *u).next()?;
    let same: Vec<Option<(f64, u32)>> = split
        .iter()
        .map(|p| p.and_then(|(v, d, u)| (u == unit).then_some((v, d))))
        .collect();
    if enough(same.iter().filter(|p| p.is_some()).count()) {
        return Some(Reading {
            values: same.iter().map(|p| p.map(|(v, _)| v)).collect(),
            decimals: same.iter().flatten().map(|(_, d)| *d).max(),
            unit_text: Some(unit.to_string()),
            note: format!("numbers in {unit}"),
            timestamp: false,
        });
    }
    None
}

fn parse_bool(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("true") {
        Some(1.0)
    } else if s.eq_ignore_ascii_case("false") {
        Some(0.0)
    } else {
        None
    }
}

/// A number followed by a unit, such as `24.1 km` or `120km/h`.
fn number_and_unit(s: &str, comma_decimal: bool) -> Option<(f64, u32, &str)> {
    let s = s.trim();
    let bytes = s.as_bytes();
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        let exponent = matches!(c, 'e' | 'E')
            && i > 0
            && bytes[i - 1].is_ascii_digit()
            && s[i + 1..]
                .trim_start_matches(['+', '-'])
                .starts_with(|d: char| d.is_ascii_digit());
        if (c.is_alphabetic() || "°µ/".contains(c)) && !exponent {
            end = i;
            break;
        }
    }
    let (number, unit) = (s[..end].trim(), s[end..].trim());
    if unit.is_empty() || number.is_empty() || unit.len() > 12 || unit.contains(char::is_whitespace)
    {
        return None;
    }
    let (value, decimals) = parse_number_with(number, comma_decimal)?;
    Some((value, decimals, unit))
}

/// A time, a date or both.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stamp {
    /// Days since 1970-01-01, when there is a date.
    days: Option<i64>,
    /// Minutes since midnight (or the minutes of a duration), when there is
    /// a time.
    minutes: Option<f64>,
}

fn stamp_column(stamps: &[Option<Stamp>]) -> Option<Reading> {
    let present: Vec<Stamp> = stamps.iter().flatten().copied().collect();
    let with_date = present.iter().filter(|s| s.days.is_some()).count();
    let with_time = present.iter().filter(|s| s.minutes.is_some()).count();
    // Dates in some rows and bare times in others cannot be compared.
    if with_date != 0 && with_date != present.len() {
        return None;
    }
    let (values, note): (Vec<Option<f64>>, &str) = if with_time == 0 {
        let values = stamps
            .iter()
            .map(|s| s.and_then(|s| s.days).map(|d| d as f64))
            .collect();
        (values, "dates as days since 1970-01-01")
    } else if with_date == 0 {
        let values = stamps.iter().map(|s| s.and_then(|s| s.minutes)).collect();
        (values, "times as minutes")
    } else {
        let values = stamps
            .iter()
            .map(|s| s.map(|s| s.days.unwrap_or(0) as f64 * 1440.0 + s.minutes.unwrap_or(0.0)))
            .collect();
        (values, "dates and times as minutes since 1970-01-01")
    };
    // Whole minutes, seconds, or finer.
    let decimals = if values.iter().flatten().all(|v| v.fract() == 0.0) {
        0
    } else if values
        .iter()
        .flatten()
        .all(|v| ((v * 60.0) - (v * 60.0).round()).abs() < 1e-6)
    {
        2
    } else {
        4
    };
    Some(Reading {
        values,
        decimals: Some(decimals),
        unit_text: None,
        note: note.into(),
        timestamp: true,
    })
}

/// Parses `08:15`, `1:24:00`, `2026-10-15`, `15/10/2026`,
/// `2026-10-15T14:24:47.000Z`, `Oct 15, 2026 @ 14:24:47.000` and the like.
fn parse_stamp(s: &str, day_first: bool) -> Option<Stamp> {
    let s = s.trim();
    // Kibana writes `Oct 15, 2026 @ 14:24:47.000`.
    let s = s.replacen(" @ ", " ", 1);
    let s = s.as_str();
    if let Some(minutes) = parse_clock(s) {
        return Some(Stamp {
            days: None,
            minutes: Some(minutes),
        });
    }
    // Split the date from the time at a `T` or the last space.
    let (date, time) = match s.find('T').filter(|&i| i >= 8) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => match s.rfind(' ') {
            Some(i) if s[i + 1..].contains(':') => (&s[..i], Some(&s[i + 1..])),
            _ => (s, None),
        },
    };
    let days = parse_date(date.trim(), day_first)?;
    let minutes = match time {
        None => None,
        Some(t) => {
            let (clock, offset) = split_offset(t.trim())?;
            Some(parse_clock(clock)? - offset)
        }
    };
    Some(Stamp {
        days: Some(days),
        minutes,
    })
}

/// A time zone at the end of a time (`Z`, `+02:00`, `-0500`), as minutes.
fn split_offset(t: &str) -> Option<(&str, f64)> {
    if let Some(clock) = t.strip_suffix(['Z', 'z']) {
        return Some((clock, 0.0));
    }
    if let Some(i) = t.rfind(['+', '-']) {
        let zone = t[i + 1..].replace(':', "");
        if zone.len() == 4 && zone.bytes().all(|b| b.is_ascii_digit()) {
            let hours: f64 = zone[..2].parse().ok()?;
            let minutes: f64 = zone[2..].parse().ok()?;
            let sign = if t.as_bytes()[i] == b'-' { -1.0 } else { 1.0 };
            return Some((&t[..i], sign * (hours * 60.0 + minutes)));
        }
    }
    Some((t, 0.0))
}

/// `H:MM`, `HH:MM:SS` or `HH:MM:SS.fff`, as minutes. Hours may go past 23
/// (durations); a leading `-` is a negative duration.
fn parse_clock(s: &str) -> Option<f64> {
    let (sign, s) = match s.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, s),
    };
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let (h, m) = (parts[0], parts[1]);
    if !digits(h) || h.len() > 3 || !digits(m) || m.len() != 2 {
        return None;
    }
    let minutes: f64 = m.parse().ok()?;
    if minutes >= 60.0 {
        return None;
    }
    let seconds = match parts.get(2) {
        None => 0.0,
        Some(sec) => {
            let (whole, frac) = sec.split_once(['.', ',']).unwrap_or((sec, ""));
            if !digits(whole) || whole.len() != 2 || !(frac.is_empty() || digits(frac)) {
                return None;
            }
            let value: f64 = format!("{whole}.{frac}0").parse().ok()?;
            if value >= 61.0 {
                return None;
            }
            value
        }
    };
    let hours: f64 = h.parse().ok()?;
    Some(sign * (hours * 60.0 + minutes + seconds / 60.0))
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// `2026-10-15`, `2026/10/15`, `15/10/2026`, `15.10.2026` (or month first
/// when `day_first` is false), `Oct 15, 2026`, `15 Oct 2026`. Days since
/// 1970-01-01.
fn parse_date(s: &str, day_first: bool) -> Option<i64> {
    let number = |p: &str| -> Option<i64> {
        (!p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let month_name = |p: &str| -> Option<i64> {
        let p = p.trim_end_matches('.').to_ascii_lowercase();
        if p.len() < 3 {
            return None;
        }
        MONTHS
            .iter()
            .position(|m| p.starts_with(m))
            .map(|i| i as i64 + 1)
    };
    let parts: Vec<&str> = s
        .split(['-', '/', '.', ' ', ','])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = if let Some(m) = month_name(parts[0]) {
        (number(parts[2])?, m, number(parts[1])?)
    } else if let Some(m) = month_name(parts[1]) {
        (number(parts[2])?, m, number(parts[0])?)
    } else if parts[0].len() == 4 {
        (number(parts[0])?, number(parts[1])?, number(parts[2])?)
    } else if parts[2].len() == 4 {
        let (a, b) = (number(parts[0])?, number(parts[1])?);
        if day_first {
            (number(parts[2])?, b, a)
        } else {
            (number(parts[2])?, a, b)
        }
    } else {
        return None;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1000..=9999).contains(&y) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(cells: &[&str]) -> Option<Reading> {
        read_column(cells, false)
    }

    #[test]
    fn missing_markers() {
        for s in ["", " - ", "NA", "n/a", "NULL", "NaN", "#N/A", "None"] {
            assert!(is_missing(s), "{s:?}");
        }
        assert!(!is_missing("0"));
        assert!(!is_missing("-1"));
    }

    #[test]
    fn booleans() {
        let r = read(&["true", "false", "TRUE", "-"]).unwrap();
        assert_eq!(r.values, vec![Some(1.0), Some(0.0), Some(1.0), None]);
    }

    #[test]
    fn times_and_durations() {
        let r = read(&["08:15", "23:59", "0:05"]).unwrap();
        assert_eq!(r.values, vec![Some(495.0), Some(1439.0), Some(5.0)]);
        assert_eq!(r.decimals, Some(0));
        let r = read(&["1:24:00", "0:19:30", "-0:02:00"]).unwrap();
        assert_eq!(r.values, vec![Some(84.0), Some(19.5), Some(-2.0)]);
        assert_eq!(r.decimals, Some(2));
    }

    #[test]
    fn dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let r = read(&["2026-10-15", "2026-10-16"]).unwrap();
        assert_eq!(r.values, vec![Some(20_741.0), Some(20_742.0)]);
        // Day first, unless a cell shows the month comes first.
        let r = read(&["01/02/2026", "03/02/2026"]).unwrap();
        assert_eq!(r.values[0], Some(days_from_civil(2026, 2, 1) as f64));
        let r = read(&["01/02/2026", "01/13/2026"]).unwrap();
        assert_eq!(r.values[0], Some(days_from_civil(2026, 1, 2) as f64));
    }

    #[test]
    fn dates_with_times() {
        let day = days_from_civil(2026, 10, 15) as f64 * 1440.0;
        let r = read(&[
            "Oct 15, 2026 @ 14:24:47.000",
            "2026-10-15T14:24:00Z",
            "2026-10-15 16:24:00+02:00",
            "15/10/2026 14:25",
        ])
        .unwrap();
        let at = |m: f64| Some(day + m);
        assert_eq!(
            r.values,
            vec![at(864.0 + 47.0 / 60.0), at(864.0), at(864.0), at(865.0)]
        );
        assert_eq!(r.note, "dates and times as minutes since 1970-01-01");
        assert!(r.timestamp);
    }

    #[test]
    fn numbers_with_units() {
        let r = read(&["24.1 km", "140 km", "NA", "3.5km"]).unwrap();
        assert_eq!(r.values, vec![Some(24.1), Some(140.0), None, Some(3.5)]);
        assert_eq!(r.unit_text.as_deref(), Some("km"));
        assert_eq!(r.decimals, Some(1));
        let r = read(&["1.2e3 m/s", "5 m/s"]).unwrap();
        assert_eq!(r.values[0], Some(1200.0));
        // Different units, or words, are not a numeric column.
        assert_eq!(read(&["5 km", "3 m"]), None);
        assert_eq!(read(&["Barcelona", "Vic"]), None);
        assert_eq!(read(&["R2 Sud", "R1"]), None);
    }
}
