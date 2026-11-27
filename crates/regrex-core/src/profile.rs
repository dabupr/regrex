//! What each column of a table is for, worked out before any search.
//!
//! A formula search treats every column as a measurement. Real tables also
//! hold columns that say *which* formula applies (a curve type, a tax
//! regime), columns that split the rows into independent groups (a line, a
//! device, an account), a column that orders the rows of each group (a
//! chainage, a timestamp), codes and identifiers. [`profile`] tells them
//! apart with cheap counting, no search and no AI, so that the scan can
//! use each column for what it is and people (or an AI reading the result)
//! see how the table was understood.

use crate::scan::Table;
use std::collections::HashMap;

/// What a column is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// One value (or none): nothing to explain, nothing it explains.
    Constant,
    /// Two values: on/off, yes/no, a direction.
    Flag,
    /// Part of the key that splits the rows into independent sequences
    /// (a line and track, a device, an account).
    GroupKey,
    /// Orders the rows within each group, with a near-regular step (a
    /// chainage every metre, a timestamp every second).
    Order,
    /// A few values that hold over long runs of rows and decide which
    /// formula applies (a curve type, a mode, a regime).
    Regime,
    /// A code or label with a moderate number of values (a product code,
    /// a station name).
    Code,
    /// Almost one value per row, or per few rows: names an object.
    Identifier,
    /// A number with many values: a target or an input of formulas.
    Measurement,
}

impl Role {
    /// The role in one word, for reports.
    pub fn name(self) -> &'static str {
        match self {
            Role::Constant => "constant",
            Role::Flag => "flag",
            Role::GroupKey => "group key",
            Role::Order => "order",
            Role::Regime => "regime",
            Role::Code => "code",
            Role::Identifier => "identifier",
            Role::Measurement => "measurement",
        }
    }
}

/// What [`profile`] found about one column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfile {
    /// What the column is used for.
    pub role: Role,
    /// Whether the column holds text read as categories (see
    /// [`Table::labels`]).
    pub category: bool,
    /// Rows with a value.
    pub present: usize,
    /// Different values.
    pub distinct: usize,
    /// Average number of consecutive rows with the same value, along the
    /// order axis within each group, when the table has one.
    pub mean_run: Option<f64>,
}

/// How a table is laid out, and what each column is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// One entry per column of the table.
    pub columns: Vec<ColumnProfile>,
    /// Columns whose values together split the rows into independent
    /// sequences; empty when the whole table is one sequence or has no
    /// order.
    pub groups: Vec<usize>,
    /// The column that orders the rows within each group, if any.
    pub order: Option<usize>,
}

impl Profile {
    /// The role of column `i`.
    pub fn role(&self, i: usize) -> Option<Role> {
        self.columns.get(i).map(|c| c.role)
    }

    /// Columns with the given role.
    pub fn with_role(&self, role: Role) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|&i| self.columns[i].role == role)
            .collect()
    }

    /// The rows sorted by group and then by the order column, or `None`
    /// when the table has no order column. Rows missing the order value
    /// are left out.
    pub fn sorted_rows(&self, table: &Table) -> Option<Vec<usize>> {
        let order = self.order?;
        Some(sorted_rows(table, &self.groups, order))
    }

    /// The group (index into [`Profile::groups`] combinations) of each row,
    /// numbered in order of first appearance; all 0 without groups.
    pub fn group_of_rows(&self, table: &Table) -> Vec<usize> {
        group_ids(table, &self.groups)
    }
}

/// A text column with at most this many labels can be a regime.
const MAX_REGIME_VALUES: usize = 50;
/// A regime holds its value over this many rows on average.
const MIN_REGIME_RUN: f64 = 5.0;
/// A group key has at most this many values.
const MAX_GROUP_VALUES: usize = 1000;
/// Rows that may repeat a (group, order) pair: exports have a few.
const MAX_DUPLICATE_SHARE: f64 = 0.01;
/// Share of steps along the order axis that must equal the usual step.
const MIN_REGULAR_STEPS: f64 = 0.5;
/// At most this many key columns are combined.
const MAX_GROUP_COLUMNS: usize = 3;
/// Numeric columns with at most this many different whole numbers are
/// codes or regimes rather than measurements (text columns can have more).
const MAX_CODE_VALUES: usize = 12;

/// Works out what each column of `table` is for, and the table's groups
/// and order column. Cheap: a few passes of counting per column.
pub fn profile(table: &Table) -> Profile {
    let n = table.columns.len();
    let counts: Vec<(usize, usize)> = table
        .columns
        .iter()
        .map(|c| {
            let present = c.iter().filter(|v| v.is_finite()).count();
            (present, distinct(c))
        })
        .collect();
    let mut columns: Vec<ColumnProfile> = (0..n)
        .map(|i| {
            let (present, distinct) = counts[i];
            ColumnProfile {
                role: Role::Measurement,
                category: table.is_category(i),
                present,
                distinct,
                mean_run: None,
            }
        })
        .collect();

    let (groups, order) = find_structure(table, &columns);
    let sorted = order.map(|o| sorted_rows(table, &groups, o));
    let group_of = group_ids(table, &groups);

    for (i, col) in columns.iter_mut().enumerate() {
        col.mean_run = sorted
            .as_ref()
            .map(|rows| mean_run(&table.columns[i], rows, &group_of));
        col.role = if col.distinct <= 1 {
            Role::Constant
        } else if groups.contains(&i) {
            Role::GroupKey
        } else if order == Some(i) {
            Role::Order
        } else if col.distinct == 2
            && !(col.category && col.mean_run.is_some_and(|r| r >= MIN_REGIME_RUN))
        {
            // Text with two values that holds over runs of rows (slope or
            // vertical curve) is a regime; anything else with two values
            // is a flag.
            Role::Flag
        } else if col.category || is_code(&table.columns[i], col.distinct) {
            if col.distinct * 2 > col.present {
                Role::Identifier
            } else if col.distinct <= MAX_REGIME_VALUES
                && col.mean_run.is_none_or(|r| r >= MIN_REGIME_RUN)
            {
                Role::Regime
            } else {
                Role::Code
            }
        } else {
            Role::Measurement
        };
    }
    Profile {
        columns,
        groups,
        order,
    }
}

/// Number of different finite values.
fn distinct(x: &[f64]) -> usize {
    let mut seen: HashMap<u64, ()> = HashMap::new();
    for v in x.iter().filter(|v| v.is_finite()) {
        seen.insert(key(*v), ());
    }
    seen.len()
}

/// A hashable key for a value (`-0.0` and `0.0` are the same).
fn key(v: f64) -> u64 {
    if v == 0.0 {
        0
    } else {
        v.to_bits()
    }
}

/// Whether a numeric column looks like a code: few different whole numbers.
fn is_code(x: &[f64], distinct: usize) -> bool {
    distinct <= MAX_CODE_VALUES && whole_numbers(x)
}

/// Whether every value is a whole number.
fn whole_numbers(x: &[f64]) -> bool {
    x.iter().filter(|v| v.is_finite()).all(|v| v.fract() == 0.0)
}

/// The group keys and the order column, if the table has them.
///
/// The order column is a numeric column that is (almost) unique within
/// groups and whose sorted values advance by a near-regular step: a
/// chainage every metre, a timestamp every second. The groups are the
/// fewest low-cardinality columns that make it unique.
fn find_structure(table: &Table, columns: &[ColumnProfile]) -> (Vec<usize>, Option<usize>) {
    let n_rows = table.n_rows();
    if n_rows < 10 {
        return (Vec::new(), None);
    }
    let key_candidates: Vec<usize> = (0..columns.len())
        .filter(|&i| {
            let c = &columns[i];
            c.distinct >= 2
                && c.distinct <= MAX_GROUP_VALUES
                && c.present * 10 >= n_rows * 9
                && (c.category || whole_numbers(&table.columns[i]))
        })
        .collect();
    let mut best: Option<(f64, Vec<usize>, usize)> = None;
    for (o, c) in columns.iter().enumerate() {
        if c.category || c.present * 10 < n_rows * 9 || c.distinct * 20 < c.present {
            continue;
        }
        let groups = group_keys(table, o, &key_candidates);
        let Some(groups) = groups else { continue };
        let regular = regular_steps(table, &groups, o);
        if regular < MIN_REGULAR_STEPS {
            continue;
        }
        // The more regular step; then an axis within groups (a chainage
        // per track) over one that runs through the whole table (a row
        // counter).
        let score = regular + 0.01 * groups.len() as f64;
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, groups, o));
        }
    }
    match best {
        Some((_, groups, o)) => {
            let groups = with_block_columns(table, groups, o, &key_candidates, columns);
            (groups, Some(o))
        }
        None => (Vec::new(), None),
    }
}

/// `groups` plus the key candidates whose every value fills one block of
/// rows per group: when tracks 1 and 2 already make the chainage unique,
/// the line (one block of rows per line on each track) is a group key too.
fn with_block_columns(
    table: &Table,
    mut groups: Vec<usize>,
    order: usize,
    candidates: &[usize],
    columns: &[ColumnProfile],
) -> Vec<usize> {
    let rows = sorted_rows(table, &groups, order);
    let group_of = group_ids(table, &groups);
    let n_groups = group_of.iter().max().map_or(1, |m| m + 1);
    let o = &table.columns[order];
    let step = usual_step(table, &rows, &group_of, order);
    for &c in candidates {
        if c == order || groups.contains(&c) {
            continue;
        }
        let x = &table.columns[c];
        let (mut runs, mut seamless) = (1, 0);
        for w in rows.windows(2) {
            if group_of[w[0]] != group_of[w[1]] {
                runs += 1;
            } else if x[w[0]] != x[w[1]] {
                runs += 1;
                // The order goes on with its usual step across the change:
                // a section of one sequence (an interlocking area along a
                // line), not a separate sequence.
                if step.is_some_and(|s| same_step(o[w[1]] - o[w[0]], s)) {
                    seamless += 1;
                }
            }
        }
        let changes = runs - n_groups;
        // A block per value and group at most (a value present on only
        // some groups makes fewer), and a jump of the order at each block.
        if runs <= columns[c].distinct.max(1) * n_groups
            && runs < rows.len() / 20
            && seamless * 10 <= changes
        {
            groups.push(c);
        }
    }
    groups.sort_unstable();
    groups
}

/// The fewest of `candidates` that, with column `order`, identify nearly
/// every row; `None` if no such set of up to [`MAX_GROUP_COLUMNS`] exists.
fn group_keys(table: &Table, order: usize, candidates: &[usize]) -> Option<Vec<usize>> {
    let mut chosen: Vec<usize> = Vec::new();
    let mut dups = duplicates(table, &chosen, order);
    let allowed = (MAX_DUPLICATE_SHARE * table.n_rows() as f64) as usize;
    while dups > allowed {
        if chosen.len() >= MAX_GROUP_COLUMNS {
            return None;
        }
        let (d, c) = candidates
            .iter()
            .filter(|c| !chosen.contains(c) && **c != order)
            .map(|&c| {
                let mut with = chosen.clone();
                with.push(c);
                (duplicates(table, &with, order), c)
            })
            .min_by_key(|&(d, c)| (d, c))?;
        if d >= dups {
            return None;
        }
        chosen.push(c);
        dups = d;
    }
    chosen.sort_unstable();
    Some(chosen)
}

/// Rows whose (group values, order value) appeared before.
fn duplicates(table: &Table, groups: &[usize], order: usize) -> usize {
    let mut seen: HashMap<Vec<u64>, ()> = HashMap::with_capacity(table.n_rows());
    let mut dups = 0;
    for r in 0..table.n_rows() {
        let v = table.columns[order][r];
        if !v.is_finite() {
            continue;
        }
        let mut k: Vec<u64> = groups.iter().map(|&g| key(table.columns[g][r])).collect();
        k.push(key(v));
        if seen.insert(k, ()).is_some() {
            dups += 1;
        }
    }
    dups
}

/// Share of consecutive steps of `order` (sorted within each group) equal
/// to the most common step.
fn regular_steps(table: &Table, groups: &[usize], order: usize) -> f64 {
    let rows = sorted_rows(table, groups, order);
    let group_of = group_ids(table, groups);
    step_counts(table, &rows, &group_of, order)
        .map_or(0.0, |(_, most, total)| most as f64 / total as f64)
}

/// The most common step of `order` along `rows` within groups.
fn usual_step(table: &Table, rows: &[usize], group_of: &[usize], order: usize) -> Option<f64> {
    step_counts(table, rows, group_of, order).map(|(step, _, _)| step)
}

/// The most common nonzero step, how many steps equal it, and how many
/// nonzero steps there are.
fn step_counts(
    table: &Table,
    rows: &[usize],
    group_of: &[usize],
    order: usize,
) -> Option<(f64, usize, usize)> {
    let x = &table.columns[order];
    let mut steps: HashMap<u64, usize> = HashMap::new();
    let mut total = 0;
    for w in rows.windows(2) {
        if group_of[w[0]] != group_of[w[1]] {
            continue;
        }
        let step = x[w[1]] - x[w[0]];
        if step == 0.0 {
            continue;
        }
        *steps.entry(key(round_step(step))).or_default() += 1;
        total += 1;
    }
    let (&k, &most) = steps
        .iter()
        .max_by_key(|&(k, n)| (*n, std::cmp::Reverse(*k)))?;
    Some((f64::from_bits(k), most, total))
}

/// A step rounded to 9 significant digits, so that floating point noise in
/// the steps of a regular axis (0.1, 0.30000000000000004) disappears.
fn round_step(step: f64) -> f64 {
    format!("{step:.9e}").parse().unwrap_or(step)
}

fn same_step(a: f64, b: f64) -> bool {
    round_step(a) == b
}

/// Group number of each row, in order of first appearance.
fn group_ids(table: &Table, groups: &[usize]) -> Vec<usize> {
    let mut ids: HashMap<Vec<u64>, usize> = HashMap::new();
    (0..table.n_rows())
        .map(|r| {
            let k: Vec<u64> = groups.iter().map(|&g| key(table.columns[g][r])).collect();
            let next = ids.len();
            *ids.entry(k).or_insert(next)
        })
        .collect()
}

/// Rows sorted by group (in order of first appearance) and then by `order`;
/// rows without an order value are left out.
fn sorted_rows(table: &Table, groups: &[usize], order: usize) -> Vec<usize> {
    let group_of = group_ids(table, groups);
    let x = &table.columns[order];
    let mut rows: Vec<usize> = (0..table.n_rows()).filter(|&r| x[r].is_finite()).collect();
    rows.sort_by(|&a, &b| {
        group_of[a]
            .cmp(&group_of[b])
            .then(x[a].total_cmp(&x[b]))
            .then(a.cmp(&b))
    });
    rows
}

/// Average length of runs of equal values along `rows`, a run ending at a
/// change of value or of group. Missing values count as a value.
fn mean_run(x: &[f64], rows: &[usize], group_of: &[usize]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut runs = 1;
    for w in rows.windows(2) {
        let (a, b) = (x[w[0]], x[w[1]]);
        let same = a == b || a.is_nan() && b.is_nan();
        if !same || group_of[w[0]] != group_of[w[1]] {
            runs += 1;
        }
    }
    rows.len() as f64 / runs as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two tracks of 200 metres, with a curve type in runs, a measurement
    /// and an id per row.
    fn track() -> Table {
        let mut track = Vec::new();
        let mut pk = Vec::new();
        let mut kind = Vec::new();
        let mut radius = Vec::new();
        let mut id = Vec::new();
        for t in 0..2 {
            for m in 0..200 {
                track.push(t as f64);
                // The second track is listed backwards.
                pk.push(if t == 0 { 1000 + m } else { 1199 - m } as f64);
                let k = (m / 25) % 3;
                kind.push(k as f64);
                radius.push(if k == 0 { 0.0 } else { 300.5 + (m % 7) as f64 });
                id.push((t * 200 + m) as f64 * 1.5);
            }
        }
        Table {
            names: ["track", "pk", "kind", "radius", "id"]
                .map(String::from)
                .to_vec(),
            columns: vec![track, pk, kind, radius, id],
            labels: vec![
                Some(vec!["1".into(), "2".into()]),
                None,
                Some(vec![
                    "Straight".into(),
                    "Clothoid".into(),
                    "Circular".into(),
                ]),
                None,
                None,
            ],
            ..Table::default()
        }
    }

    #[test]
    fn finds_groups_order_and_regimes() {
        let t = track();
        let p = profile(&t);
        assert_eq!(p.groups, vec![0]);
        assert_eq!(p.order, Some(1));
        let roles: Vec<Role> = p.columns.iter().map(|c| c.role).collect();
        assert_eq!(
            roles,
            [
                Role::GroupKey,
                Role::Order,
                Role::Regime,
                Role::Measurement,
                Role::Measurement
            ]
        );
        let rows = p.sorted_rows(&t).unwrap();
        assert_eq!(rows[200], 399, "the second track starts at its lowest pk");
        assert!(p.columns[2].mean_run.unwrap() >= 20.0);
    }

    #[test]
    fn a_shuffled_category_is_a_code_not_a_regime() {
        let mut t = track();
        // Same labels, but changing at every row.
        t.columns[2] = (0..400).map(|r| ((r * 7) % 3) as f64).collect();
        let p = profile(&t);
        assert_eq!(p.columns[2].role, Role::Code);
    }

    #[test]
    fn a_table_without_order_has_no_groups() {
        let t = Table {
            names: vec!["a".into(), "b".into(), "c".into()],
            columns: vec![
                (0..50).map(|i| (((i * 37) % 101) as f64).sqrt()).collect(),
                (0..50).map(|i| (i % 4) as f64).collect(),
                (0..50).map(|i| (i % 2) as f64).collect(),
            ],
            ..Table::default()
        };
        let p = profile(&t);
        assert_eq!(p.order, None);
        assert!(p.groups.is_empty());
        assert_eq!(p.columns[1].role, Role::Regime);
        assert_eq!(p.columns[2].role, Role::Flag);
    }
}
