//! Human-readable and JSON reports.
//!
//! The JSON output is versioned with `schema_version` and is meant to be
//! stable: fields may be added, but existing ones keep their meaning.

use crate::Loaded;
use regrex_core::expr::format_number;
use regrex_core::{
    diagnose_exception, find_copies, inputs_tried, Candidate, DerivedColumn, NearMiss, Rejection,
    ScanConfig, Table,
};
use serde::Serialize;
use std::fmt::Write;

/// Version of the JSON output format.
pub const SCHEMA_VERSION: u32 = 1;

/// Result of scanning a file, see [`regrex_core::scan()`].
#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    /// Version of this format, see [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The file that was scanned.
    pub file: String,
    /// Number of data rows.
    pub rows: usize,
    /// Numeric columns.
    pub columns: Vec<String>,
    /// Text columns left out (free text, about one value per row).
    pub skipped_columns: Vec<String>,
    /// Text columns read as categories: they tell rows apart but are not
    /// computed with.
    pub category_columns: Vec<String>,
    /// How the table is laid out and what each column is for.
    pub structure: StructureJson,
    /// Columns that were not plain numbers but were read as numbers
    /// (`true`/`false`, times, dates, numbers with a unit), and how.
    pub converted_columns: Vec<ConvertedJson>,
    /// Columns computed from others.
    pub derived: Vec<DerivedJson>,
    /// Columns that were searched without finding a formula, with the
    /// columns tried as their inputs.
    pub not_found: Vec<NotFoundJson>,
    /// Relations between text columns: concatenations and codes that match
    /// names one to one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text_relations: Vec<TextRelationJson>,
    /// For a quick scan (`--quick`), what was searched on a sample only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick: Option<QuickJson>,
    /// When a second file was given (`--with`), how its rows were matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined: Option<crate::JoinedJson>,
    /// Formulas the scan found but did not report, because they hold on too
    /// few rows or only within a group: where to look next. Most rows
    /// matched first.
    pub near_misses: Vec<NearMissJson>,
}

/// How a quick scan was done (see [`regrex_core::progressive`]).
#[derive(Debug, Clone, Serialize)]
pub struct QuickJson {
    /// Rows of the sample every column was searched on.
    pub sample_rows: usize,
    /// With 95% confidence, a formula that matched every sampled row is
    /// broken by less than this share of all rows (3 / sample_rows).
    pub max_broken_share: f64,
    /// Columns without a formula on the sample, not searched on all rows.
    pub not_searched: Vec<String>,
    /// Formulas found on the sample that did not hold on all rows.
    pub dropped: Vec<String>,
}

impl QuickJson {
    /// What the quick scan `q` of `table` searched on its sample only.
    pub fn new(table: &Table, q: &regrex_core::QuickScan) -> Self {
        let names = &table.names;
        QuickJson {
            sample_rows: q.sample_rows,
            max_broken_share: q.sample_bound(),
            not_searched: q.not_searched.iter().map(|&c| names[c].clone()).collect(),
            dropped: q
                .dropped
                .iter()
                .map(|d| format!("{} = {}", names[d.target], d.formula_text))
                .collect(),
        }
    }
}

/// A relation between text columns (see [`crate::text_relations()`]).
#[derive(Debug, Clone, Serialize)]
pub struct TextRelationJson {
    /// The column explained.
    pub column: String,
    /// What holds, as in `line + "|" + track` or `set by code`.
    pub formula: String,
    /// The other columns of the relation.
    pub inputs: Vec<String>,
    /// Rows where it holds.
    pub matched_rows: usize,
    /// Rows where every column of the relation has a value.
    pub checked_rows: usize,
}

impl TextRelationJson {
    /// The relations between the text columns of `loaded`.
    pub fn all(loaded: &Loaded) -> Vec<Self> {
        crate::text_relations(loaded)
            .into_iter()
            .map(|r| TextRelationJson {
                column: r.column,
                formula: r.formula,
                inputs: r.inputs,
                matched_rows: r.matched,
                checked_rows: r.checked,
            })
            .collect()
    }
}

/// How the table is laid out (see [`mod@regrex_core::profile`]).
#[derive(Debug, Clone, Serialize)]
pub struct StructureJson {
    /// Columns that split the rows into independent sequences.
    pub groups: Vec<String>,
    /// The column that orders the rows within each group.
    pub order: Option<String>,
    /// What each column is for, in column order.
    pub roles: Vec<RoleJson>,
}

/// What one column is for.
#[derive(Debug, Clone, Serialize)]
pub struct RoleJson {
    /// The column.
    pub column: String,
    /// `measurement`, `regime`, `group key`, `order`, `code`,
    /// `identifier`, `flag` or `constant`.
    pub role: String,
    /// Whether the column is text read as categories.
    pub category: bool,
    /// Different values.
    pub distinct: usize,
    /// Average run of equal values along the order, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_run: Option<f64>,
}

impl StructureJson {
    /// The structure of `table`.
    pub fn new(table: &regrex_core::Table) -> Self {
        let p = regrex_core::profile(table);
        let names = &table.names;
        StructureJson {
            groups: p.groups.iter().map(|&g| names[g].clone()).collect(),
            order: p.order.map(|o| names[o].clone()),
            roles: p
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| RoleJson {
                    column: names[i].clone(),
                    role: c.role.name().to_string(),
                    category: c.category,
                    distinct: c.distinct,
                    mean_run: c.mean_run.map(|r| (r * 10.0).round() / 10.0),
                })
                .collect(),
        }
    }

    /// One or two lines for people, such as `Rows: grouped by line, track,
    /// in order of pk. Regimes: curve_type.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(order) = &self.order {
            if self.groups.is_empty() {
                parts.push(format!("Rows in order of {order}."));
            } else {
                parts.push(format!(
                    "Rows grouped by {}, in order of {order}.",
                    self.groups.join(", ")
                ));
            }
        }
        // Numbers with few values are codes for the profile, but in a
        // summary they would read as if they were not computed with.
        let with = |role: &str| -> Vec<String> {
            self.roles
                .iter()
                .filter(|r| r.role == role && (r.category || role == "regime"))
                .map(|r| r.column.clone())
                .collect()
        };
        for (role, label) in [
            ("regime", "Regimes"),
            ("code", "Codes"),
            ("identifier", "Identifiers"),
        ] {
            let cols = with(role);
            if !cols.is_empty() {
                parts.push(format!("{label}: {}.", cols.join(", ")));
            }
        }
        parts.join(" ")
    }
}

/// Rows of a near miss listed in [`NearMissJson::missed_rows`].
pub const MAX_MISSED_ROWS: usize = 20;

/// A formula found but not reported, see [`regrex_core::NearMiss`].
#[derive(Debug, Clone, Serialize)]
pub struct NearMissJson {
    /// The column it computes.
    pub column: String,
    /// The formula, using column names.
    pub formula: String,
    /// Columns the formula uses.
    pub inputs: Vec<String>,
    /// Weighted complexity of the formula.
    pub complexity: usize,
    /// Rows where it holds.
    pub matched_rows: usize,
    /// Rows it was checked on.
    pub checked_rows: usize,
    /// `matched_rows / checked_rows`.
    pub match_ratio: f64,
    /// Why it was not reported: `coverage` (too few rows match),
    /// `usual_value` (right mostly on the column's most common value) or
    /// `no_split_rule` (a second formula for the rows that break the first
    /// one, with no rule on another column to tell them apart) or
    /// `other_values` (the column holds one value, often 0, on most rows,
    /// and this formula is for the others).
    pub reason: String,
    /// The reason in words.
    pub reason_text: String,
    /// A rule on another column that picks out the rows where it holds,
    /// if one exists: the formula may be right for that group.
    pub holds_when: Option<SplitJson>,
    /// The value of the regime column (such as the kind of track section)
    /// it was searched within, when the scan searched each regime apart.
    pub regime: Option<String>,
    /// Number of checked rows where it does not hold.
    pub missed_count: usize,
    /// The first [`MAX_MISSED_ROWS`] of them (1 = first row after the
    /// header).
    pub missed_rows: Vec<usize>,
}

impl NearMissJson {
    /// Builds the JSON for a near miss found in `table`.
    pub fn new(table: &Table, m: &NearMiss) -> Self {
        let names = &table.names;
        let percent = |x: f64| format!("{}%", (100.0 * x).floor());
        let reason_text = match &m.reason {
            Rejection::Coverage { needed } => format!(
                "holds on {} of the rows, {} needed",
                percent(m.match_ratio()),
                percent(*needed)
            ),
            Rejection::UsualValue => {
                "right mostly where the column has its most common value".to_string()
            }
            Rejection::OtherValues { usual, rows } => format!(
                "only for the rows that are not {} ({rows} rows are)",
                format_number(*usual)
            ),
            Rejection::NoSplitRule { main } => format!(
                "fits the rows that break {} = {main}, but no other column tells them apart",
                names[m.target]
            ),
        };
        NearMissJson {
            column: names[m.target].clone(),
            formula: m.formula_text.clone(),
            inputs: m.inputs.iter().map(|&i| names[i].clone()).collect(),
            complexity: m.complexity,
            matched_rows: m.matched,
            checked_rows: m.checked,
            match_ratio: m.match_ratio(),
            reason: m.reason.code().to_string(),
            reason_text,
            holds_when: m.holds_when.as_ref().map(|split| SplitJson {
                column: names[split.column].clone(),
                rule: split.text(names),
                accuracy: split.accuracy,
            }),
            regime: None,
            missed_count: m.missed.len(),
            missed_rows: m
                .missed
                .iter()
                .take(MAX_MISSED_ROWS)
                .map(|r| r + 1)
                .collect(),
        }
    }

    /// `cant = 120000 / radius   holds on 60% of the rows, 90% needed;
    /// holds where kind = 1`.
    pub fn summary(&self) -> String {
        let mut s = format!("{} = {}   {}", self.column, self.formula, self.reason_text);
        if let Some(split) = &self.holds_when {
            let _ = write!(s, "; holds where {}", split.rule);
        }
        s
    }
}

/// A column searched without finding a formula.
#[derive(Debug, Clone, Serialize)]
pub struct NotFoundJson {
    /// The column.
    pub column: String,
    /// The columns tried as its inputs (the ones that predict it best).
    /// A formula over other columns cannot have been found.
    pub inputs_tried: Vec<String>,
}

/// A column read as numbers although its cells are not plain numbers.
#[derive(Debug, Clone, Serialize)]
pub struct ConvertedJson {
    /// The column, as written in the file.
    pub column: String,
    /// How its cells were read, such as `times as minutes`.
    pub read_as: String,
}

/// One derived column in a [`ScanReport`].
#[derive(Debug, Clone, Serialize)]
pub struct DerivedJson {
    /// The derived column.
    pub column: String,
    /// Its formula, using column names.
    pub formula: String,
    /// Columns the formula uses.
    pub inputs: Vec<String>,
    /// Weighted complexity of the formula.
    pub complexity: usize,
    /// Rows that follow the formula.
    pub matched_rows: usize,
    /// Rows that could be checked (no missing values).
    pub checked_rows: usize,
    /// `matched_rows / checked_rows`.
    pub match_ratio: f64,
    /// Data rows (1 = first row after the header) that break the formula.
    pub exception_rows: Vec<usize>,
    /// Details of the first [`MAX_EXCEPTION_DETAILS`] exceptions.
    pub exceptions: Vec<ExceptionJson>,
    /// The well-known formula this is, if it is one.
    pub known: Option<KnownJson>,
    /// A second formula that many of the exceptions follow, when the column
    /// mixes two kinds of rows.
    pub other: Option<OtherJson>,
    /// When the column is a copy of another one (the same values, maybe
    /// shifted or scaled, such as `serial + 1` or `1000 * km`), that column.
    pub copy_of: Option<String>,
    /// When the formula changes with a regime column (a curve type, a
    /// region), that column and one piece per value; `formula` then sums
    /// them up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regime: Option<RegimeJson>,
    /// Whether the formula links each row to the previous one, in the
    /// order of the table (a running total, a chainage along coordinates).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub between_rows: bool,
    /// For an approximate relation, the largest error on the matched rows;
    /// absent when the formula holds exactly, up to rounding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<f64>,
}

/// A formula per value of a regime column.
#[derive(Debug, Clone, Serialize)]
pub struct RegimeJson {
    /// The regime column.
    pub column: String,
    /// One piece per value.
    pub pieces: Vec<PieceJson>,
}

/// The formula for the rows with one value of the regime column.
#[derive(Debug, Clone, Serialize)]
pub struct PieceJson {
    /// The value, as written in the file.
    pub value: String,
    /// The formula, such as `0`, `0.21 * net`, `constant per run` or
    /// `c0 + c1 * pk, constants per run`.
    pub formula: String,
    /// Whether the constants are fitted again on each run of consecutive
    /// rows (each curve, each section).
    pub per_run: bool,
    /// Rows with this value.
    pub rows: usize,
    /// Rows the formula reproduces.
    pub matched_rows: usize,
    /// Runs of consecutive rows, for `per_run` pieces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<usize>,
    /// For an approximate piece, its largest error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<f64>,
}

/// A second formula for some rows of a derived column.
#[derive(Debug, Clone, Serialize)]
pub struct OtherJson {
    /// The formula, using column names.
    pub formula: String,
    /// Data rows (1 = first row after the header) that follow it.
    pub rows: Vec<usize>,
    /// A rule on another column that tells these rows apart, if one exists.
    pub split: Option<SplitJson>,
}

/// A rule that separates the rows of two formulas.
#[derive(Debug, Clone, Serialize)]
pub struct SplitJson {
    /// The column the rule is about.
    pub column: String,
    /// The rule, such as `year >= 2023` or `region in {2, 3}`.
    pub rule: String,
    /// Fraction of the rows of both formulas it classifies correctly.
    pub accuracy: f64,
}

impl OtherJson {
    /// `0.1 * net (24 rows, where year >= 2023)`.
    pub fn summary(&self) -> String {
        let n = self.rows.len();
        let rows = if n == 1 {
            "1 row".to_string()
        } else {
            format!("{n} rows")
        };
        match &self.split {
            Some(split) => format!("{} ({rows}, where {})", self.formula, split.rule),
            None => format!("{} ({rows})", self.formula),
        }
    }
}

/// A row that does not follow the formula of its column.
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionJson {
    /// Data row (1 = first row after the header).
    pub row: usize,
    /// The value in the file.
    pub actual: f64,
    /// The value the formula gives.
    pub expected: f64,
    /// The most likely mistake that gives the value in the file, if a simple
    /// one does.
    pub diagnosis: Option<DiagnosisJson>,
}

/// A well-known formula that a result matches, from the built-in library
/// (`regrex-formulas`).
#[derive(Debug, Clone, Serialize)]
pub struct KnownJson {
    /// Library id, such as `kinetic_energy`.
    pub id: String,
    /// Name, such as `Kinetic energy`.
    pub name: String,
    /// Domain id, such as `physics`.
    pub domain: String,
    /// What it computes, when the library says.
    pub description: Option<String>,
    /// The library's expression, with its own symbols (`0.5 * m * v^2`).
    pub expression: String,
    /// What the formula computes, with its symbol and unit.
    pub output: KnownQuantityJson,
    /// Each variable of the formula and the column or value that gives it.
    pub variables: Vec<KnownQuantityJson>,
    /// A library constant with another value in the data, such as `g = 10`.
    pub changed_constant: Option<ChangedConstantJson>,
    /// Whether the result is exactly the library formula (every variable is a
    /// column and constants have their usual values).
    pub exact: bool,
    /// Where the formula comes from.
    pub source_title: String,
    /// Link to read more about it.
    pub source_url: Option<String>,
}

/// A quantity of a [`KnownJson`] formula.
#[derive(Debug, Clone, Serialize)]
pub struct KnownQuantityJson {
    /// Symbol in the library expression.
    pub symbol: String,
    /// What it is.
    pub name: String,
    /// Its unit in the library.
    pub unit: String,
    /// The column that gives it, if any.
    pub column: Option<String>,
    /// The fixed value it takes in the data, if it is not a column.
    pub value: Option<f64>,
}

/// A library constant with another value, in a [`KnownJson`].
#[derive(Debug, Clone, Serialize)]
pub struct ChangedConstantJson {
    /// Constant id, such as `g`.
    pub id: String,
    /// Its usual value.
    pub library_value: f64,
    /// The value the data uses.
    pub value: f64,
}

impl KnownJson {
    /// Looks up a function of some named columns in the built-in library.
    pub fn identify(
        candidate: &dyn Fn(&[f64]) -> f64,
        input_names: &[String],
        output_name: &str,
    ) -> Option<KnownJson> {
        use regrex_formulas::{Binding, Library};
        let names: Vec<&str> = input_names.iter().map(String::as_str).collect();
        let found = Library::builtin().identify(candidate, &names, Some(output_name));
        let m = found.into_iter().next()?;
        let f = m.formula;
        let quantity =
            |q: &regrex_formulas::Quantity, column: Option<String>, value| KnownQuantityJson {
                symbol: q.symbol.clone(),
                name: q.name.clone(),
                unit: q.unit.clone(),
                column,
                value,
            };
        Some(KnownJson {
            id: f.id.clone(),
            name: f.name.clone(),
            domain: f.domain.clone(),
            description: f.description.clone(),
            expression: f.expr_text.clone(),
            output: quantity(&f.output, Some(output_name.to_string()), None),
            variables: f
                .variables
                .iter()
                .zip(&m.variables)
                .map(|(q, b)| match b {
                    Binding::Input(i) => quantity(q, Some(input_names[*i].clone()), None),
                    Binding::Value(v) => quantity(q, None, Some(*v)),
                })
                .collect(),
            exact: m.is_exact(),
            changed_constant: m.changed_constant.map(|c| ChangedConstantJson {
                id: c.id,
                library_value: c.library_value,
                value: c.value,
            }),
            source_title: f.source.title.clone(),
            source_url: f.source.url.clone(),
        })
    }

    /// One line for text output: `kinetic energy (physics)`, with what
    /// differs from the library.
    pub fn summary(&self) -> String {
        let mut s = format!("known formula: {}", self.name);
        let fixed: Vec<String> = self
            .variables
            .iter()
            .filter_map(|v| {
                v.value
                    .map(|x| format!("{} = {}", v.symbol, format_number(x)))
            })
            .collect();
        if !fixed.is_empty() {
            s += &format!(" with {}", fixed.join(", "));
        }
        if let Some(c) = &self.changed_constant {
            s += &format!(
                " but {} = {} instead of {}",
                c.id,
                format_number(c.value),
                format_number(c.library_value)
            );
        }
        s
    }
}

/// A mistake that explains an exception, see [`regrex_core::diagnose`].
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisJson {
    /// Kind of mistake: `rounding`, `typo`, `sign`, `factor`,
    /// `wrong_constant`, `wrong_variable`, `swapped_variables`,
    /// `missing_operation`, `missing_term`, `wrong_operation`,
    /// `swapped_operands`, `other_cell`, `midnight` or `other_formula` (the
    /// row follows the second formula of a column that mixes two kinds of
    /// rows).
    pub kind: String,
    /// What went wrong, such as `used 0.1 instead of 0.21`.
    pub description: String,
    /// The formula the row follows instead, when the mistake is in the formula.
    pub formula: Option<String>,
}

/// At most this many exceptions are detailed in a report.
pub const MAX_EXCEPTION_DETAILS: usize = 100;

impl DerivedJson {
    /// Converts a [`DerivedColumn`] found in `table`.
    pub fn new(table: &Table, d: &DerivedColumn) -> Self {
        Self::with_context(table, d, &[])
    }

    /// Like [`DerivedJson::new`], with every column found in the same scan,
    /// so a wrong cell that breaks several formulas is blamed only once.
    pub fn with_context(table: &Table, d: &DerivedColumn, all: &[DerivedColumn]) -> Self {
        let names = &table.names;
        let other = d.other.as_ref().map(|o| OtherJson {
            formula: o.formula_text.clone(),
            rows: o.rows.iter().map(|r| r + 1).collect(),
            split: o.split.as_ref().map(|split| SplitJson {
                column: names[split.column].clone(),
                rule: split.text(names),
                accuracy: split.accuracy,
            }),
        });
        // A piecewise formula has no single expected value to compare with.
        let details = if d.regime.is_some() || d.steps {
            0
        } else {
            MAX_EXCEPTION_DETAILS
        };
        let exceptions = d
            .exceptions
            .iter()
            .take(details)
            .map(|&r| {
                let row: Vec<f64> = table.columns.iter().map(|c| c[r]).collect();
                let follows_other = other
                    .as_ref()
                    .filter(|o| o.rows.binary_search(&(r + 1)).is_ok());
                let diagnosis = match follows_other {
                    Some(o) => Some(DiagnosisJson {
                        kind: "other_formula".to_string(),
                        description: format!("follows the other formula, {}", o.summary()),
                        formula: Some(o.formula.clone()),
                    }),
                    None => diagnose_exception(table, d, all, r, &ScanConfig::default())
                        .into_iter()
                        .next()
                        .map(|m| DiagnosisJson {
                            kind: m.kind.id().to_string(),
                            description: m.description,
                            formula: m.formula.map(|f| f.display_with(names).to_string()),
                        }),
                };
                ExceptionJson {
                    row: r + 1,
                    actual: table.columns[d.target][r],
                    expected: d.formula.eval_row(&row, &[]),
                    diagnosis,
                }
            })
            .collect();
        DerivedJson {
            column: names[d.target].clone(),
            formula: d.formula_text.clone(),
            inputs: d.inputs.iter().map(|&i| names[i].clone()).collect(),
            complexity: d.complexity,
            matched_rows: d.matched,
            checked_rows: d.checked,
            match_ratio: d.match_ratio(),
            exception_rows: d.exceptions.iter().map(|r| r + 1).collect(),
            exceptions,
            known: if d.copy || d.regime.is_some() || d.steps {
                None
            } else {
                known_column(table, d)
            },
            other,
            copy_of: d
                .copy
                .then(|| d.inputs.first().map(|&i| names[i].clone()))
                .flatten(),
            regime: d.regime.as_ref().map(|g| RegimeJson {
                column: names[g.column].clone(),
                pieces: g
                    .pieces
                    .iter()
                    .map(|p| PieceJson {
                        value: p.label.clone(),
                        formula: p.text.clone(),
                        per_run: p.per_run,
                        rows: p.rows,
                        matched_rows: p.matched,
                        runs: p.per_run.then_some(p.runs),
                        within: p.within,
                    })
                    .collect(),
            }),
            between_rows: d.steps,
            within: d.within,
        }
    }
}

/// Looks up the formula of a derived column in the library.
fn known_column(table: &Table, d: &DerivedColumn) -> Option<KnownJson> {
    // The library is matched on random real inputs, where `x div 100` looks
    // like `0.01 * x`; none of its formulas uses whole-number arithmetic.
    if d.formula.has_integer_ops() {
        return None;
    }
    let n = table.columns.len();
    let candidate = |values: &[f64]| {
        let mut row = vec![0.0; n];
        for (k, &c) in d.inputs.iter().enumerate() {
            row[c] = values[k];
        }
        d.formula.eval_row(&row, &[])
    };
    let names: Vec<String> = d.inputs.iter().map(|&i| table.names[i].clone()).collect();
    KnownJson::identify(&candidate, &names, &table.names[d.target])
}

/// The searched columns of `table` missing from `derived`, with the inputs
/// a scan with the default settings tries for them.
fn not_found(table: &Table, derived: &[DerivedColumn]) -> Vec<NotFoundJson> {
    let config = ScanConfig::default();
    let copies = find_copies(table, &config);
    (0..table.columns.len())
        .filter(|&c| !derived.iter().any(|d| d.target == c))
        .filter_map(|c| {
            let inputs = inputs_tried(table, c, &config, &copies);
            (!inputs.is_empty()).then(|| NotFoundJson {
                column: table.names[c].clone(),
                inputs_tried: inputs.iter().map(|&i| table.names[i].clone()).collect(),
            })
        })
        .collect()
}

impl ScanReport {
    /// Builds the report for `derived` columns found in `loaded`.
    pub fn new(file: &str, loaded: &Loaded, derived: &[DerivedColumn]) -> Self {
        let names = &loaded.table.names;
        ScanReport {
            schema_version: SCHEMA_VERSION,
            file: file.to_string(),
            rows: loaded.table.n_rows(),
            columns: names.clone(),
            skipped_columns: loaded.skipped.clone(),
            category_columns: loaded.categories.clone(),
            structure: StructureJson::new(&loaded.table),
            converted_columns: loaded
                .converted
                .iter()
                .map(|(column, read_as)| ConvertedJson {
                    column: column.clone(),
                    read_as: read_as.clone(),
                })
                .collect(),
            derived: derived
                .iter()
                .map(|d| DerivedJson::with_context(&loaded.table, d, derived))
                .collect(),
            not_found: not_found(&loaded.table, derived),
            text_relations: TextRelationJson::all(loaded),
            quick: None,
            joined: None,
            near_misses: Vec::new(),
        }
    }

    /// Adds how the rows of a second file were matched.
    pub fn with_join(mut self, joined: Option<&crate::JoinedJson>) -> Self {
        self.joined = joined.cloned();
        self
    }

    /// Adds what a quick scan left out; the columns not searched on all
    /// rows are not listed as searched without a formula.
    pub fn with_quick(mut self, table: &regrex_core::Table, q: &regrex_core::QuickScan) -> Self {
        let quick = QuickJson::new(table, q);
        self.not_found
            .retain(|m| !quick.not_searched.contains(&m.column));
        self.quick = Some(quick);
        self
    }

    /// Adds the near misses of the scan ([`regrex_core::scan_detailed`]).
    pub fn with_near_misses(mut self, table: &Table, near_misses: &[NearMiss]) -> Self {
        self.near_misses = near_misses
            .iter()
            .map(|m| NearMissJson::new(table, m))
            .collect();
        self.near_misses
            .sort_by_key(|m| std::cmp::Reverse(m.matched_rows));
        self
    }

    /// Keeps in [`ScanReport::not_found`] only the given columns, when the
    /// scan was limited to them.
    pub fn only_columns(mut self, columns: &[String]) -> Self {
        self.not_found.retain(|m| columns.contains(&m.column));
        self.near_misses.retain(|m| columns.contains(&m.column));
        self
    }

    /// Pretty-printed JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }

    /// A Mermaid flowchart: an arrow from each input to each derived column,
    /// labelled with the formula.
    pub fn to_mermaid(&self) -> String {
        let id = |name: &str| {
            let i = self.columns.iter().position(|c| c == name).unwrap_or(0);
            format!("c{i}")
        };
        let mut s = String::from("```mermaid\nflowchart LR\n");
        for (i, name) in self.columns.iter().enumerate() {
            let _ = writeln!(s, "  c{i}[\"{}\"]", name.replace('"', "'"));
        }
        for d in &self.derived {
            let label = format!("{} = {}", d.column, d.formula).replace('"', "'");
            for input in &d.inputs {
                let _ = writeln!(s, "  {} -->|\"{label}\"| {}", id(input), id(&d.column));
            }
        }
        s.push_str("```\n");
        s
    }

    /// Plain-text table for terminals.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{}: {} rows, {} numeric columns",
            self.file,
            self.rows,
            self.columns.len() - self.category_columns.len()
        );
        if !self.category_columns.is_empty() {
            let _ = writeln!(
                s,
                "Text read as categories: {}",
                self.category_columns.join(", ")
            );
        }
        if !self.skipped_columns.is_empty() {
            let _ = writeln!(
                s,
                "Skipped (free text): {}",
                self.skipped_columns.join(", ")
            );
        }
        let summary = self.structure.summary();
        if !summary.is_empty() {
            let _ = writeln!(s, "{summary}");
        }
        if let Some(q) = &self.quick {
            let _ = writeln!(
                s,
                "Quick scan: every column searched on {} of {} rows, formulas confirmed on all rows.",
                q.sample_rows, self.rows
            );
            if !q.not_searched.is_empty() {
                let _ = writeln!(
                    s,
                    "Not searched on all rows (no formula on the sample): {}",
                    q.not_searched.join(", ")
                );
            }
            if !q.dropped.is_empty() {
                let _ = writeln!(
                    s,
                    "Found on the sample but not on all rows: {}",
                    q.dropped.join("; ")
                );
            }
        }
        if !self.converted_columns.is_empty() {
            let list: Vec<String> = self
                .converted_columns
                .iter()
                .map(|c| format!("{} ({})", c.column, c.read_as))
                .collect();
            let _ = writeln!(s, "Read as numbers: {}", list.join(", "));
        }
        let _ = writeln!(s);
        if self.derived.is_empty() {
            let _ = writeln!(s, "No derived columns found.");
            s += &self.near_misses_text();
            return s;
        }
        let (copies, main): (Vec<&DerivedJson>, Vec<&DerivedJson>) =
            self.derived.iter().partition(|d| d.copy_of.is_some());
        let width = main
            .iter()
            .map(|d| d.column.chars().count())
            .max()
            .unwrap_or(0);
        let fwidth = main
            .iter()
            .filter(|d| d.regime.is_none())
            .map(|d| d.formula.chars().count())
            .max()
            .unwrap_or(0);
        for d in main {
            if let Some(g) = &d.regime {
                // One line per piece: `  Clothoid  1 / (c0 + c1 * pk), ...`.
                let _ = write!(
                    s,
                    "  {:<width$} = by {}   {}/{} rows",
                    d.column, g.column, d.matched_rows, d.checked_rows
                );
                let lw = g
                    .pieces
                    .iter()
                    .map(|p| p.value.chars().count())
                    .max()
                    .unwrap_or(0);
                for p in &g.pieces {
                    let runs = p.runs.map(|n| format!(", {n} runs")).unwrap_or_default();
                    let _ = write!(
                        s,
                        "\n  {:width$}     {:<lw$}  {}   ({}/{} rows{runs})",
                        "", p.value, p.formula, p.matched_rows, p.rows
                    );
                }
                let _ = writeln!(s);
                continue;
            }
            let _ = write!(
                s,
                "  {:<width$} = {:<fwidth$}   {}/{} rows",
                d.column, d.formula, d.matched_rows, d.checked_rows
            );
            let mut known = d
                .known
                .as_ref()
                .map(|k| format!("\n  {:width$}   {}", "", k.summary()))
                .unwrap_or_default();
            if let Some(o) = &d.other {
                let _ = write!(known, "\n  {:width$}   other rows: {}", "", o.summary());
            }
            let why = |x: &ExceptionJson| {
                x.diagnosis
                    .as_ref()
                    .map(|m| format!(" ({})", m.description))
                    .unwrap_or_default()
            };
            if let [only] = d.exceptions.as_slice() {
                let _ = writeln!(
                    s,
                    "   exception: row {} is {}, formula gives {}{}",
                    only.row,
                    format_number(only.actual),
                    format_number(only.expected),
                    why(only)
                );
                if !known.is_empty() {
                    let _ = writeln!(s, "{}", known.trim_start_matches('\n'));
                }
                continue;
            }
            // Rows that follow the other formula are not exceptions to report.
            let other_rows = d.other.as_ref().map_or(&[][..], |o| o.rows.as_slice());
            let wrong: Vec<usize> = d
                .exception_rows
                .iter()
                .copied()
                .filter(|r| other_rows.binary_search(r).is_err())
                .collect();
            if !wrong.is_empty() {
                let shown: Vec<String> = wrong.iter().take(10).map(|r| r.to_string()).collect();
                let more = wrong.len().saturating_sub(10);
                let _ = write!(s, "   exceptions: row {}", shown.join(", "));
                if more > 0 {
                    let _ = write!(s, " and {more} more");
                }
                // Explain the first few, when there is something to say.
                for x in d
                    .exceptions
                    .iter()
                    .filter(|x| {
                        x.diagnosis
                            .as_ref()
                            .is_some_and(|m| m.kind != "other_formula")
                    })
                    .take(5)
                {
                    let _ = write!(
                        s,
                        "\n      row {}: {} instead of {}{}",
                        x.row,
                        format_number(x.actual),
                        format_number(x.expected),
                        why(x)
                    );
                }
            }
            let _ = writeln!(s, "{known}");
        }
        if !copies.is_empty() {
            let _ = writeln!(s, "\nCopies of other columns:");
            s += &copies_text(&copies);
        }
        if !self.text_relations.is_empty() {
            let _ = writeln!(s, "\nText relations:");
            for r in &self.text_relations {
                let rows = if r.matched_rows == r.checked_rows {
                    String::new()
                } else {
                    format!(" ({}/{} rows)", r.matched_rows, r.checked_rows)
                };
                let _ = writeln!(s, "  {} = {}{rows}", r.column, r.formula);
            }
        }
        s += &self.near_misses_text();
        s
    }

    /// The near misses, one per line, or nothing when there are none.
    fn near_misses_text(&self) -> String {
        if self.near_misses.is_empty() {
            return String::new();
        }
        let mut s = String::from("\nAlmost (not reported, worth a look):\n");
        for m in &self.near_misses {
            let _ = writeln!(s, "  {}", m.summary());
        }
        s
    }
}

/// One line per copied column: `a = b = c`, then the shifted or scaled
/// copies, `d = a + 1, e = a + 2`.
fn copies_text(copies: &[&DerivedJson]) -> String {
    let mut originals: Vec<&str> = Vec::new();
    for d in copies {
        let original = d.copy_of.as_deref().unwrap_or_default();
        if !originals.contains(&original) {
            originals.push(original);
        }
    }
    let differ = |d: &DerivedJson| {
        if d.exception_rows.is_empty() {
            String::new()
        } else {
            format!(" ({}/{} rows)", d.matched_rows, d.checked_rows)
        }
    };
    let mut s = String::new();
    for original in originals {
        let group = copies
            .iter()
            .filter(|d| d.copy_of.as_deref() == Some(original));
        let (same, changed): (Vec<&&DerivedJson>, Vec<&&DerivedJson>) =
            group.partition(|d| d.formula == original);
        let mut parts: Vec<String> = Vec::new();
        if !same.is_empty() {
            let mut line = original.to_string();
            for d in same {
                let _ = write!(line, " = {}{}", d.column, differ(d));
            }
            parts.push(line);
        }
        parts.extend(
            changed
                .iter()
                .map(|d| format!("{} = {}{}", d.column, d.formula, differ(d))),
        );
        let _ = writeln!(s, "  {}", parts.join(", "));
    }
    s
}

/// Candidate formulas for one column, see [`regrex_core::search()`].
#[derive(Debug, Clone, Serialize)]
pub struct FitReport {
    /// Version of this format, see [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The column explained.
    pub target: String,
    /// Columns used as inputs.
    pub inputs: Vec<String>,
    /// Rows used (rows with missing values are left out).
    pub rows: usize,
    /// Formulas, best first.
    pub candidates: Vec<CandidateJson>,
}

/// One formula in a [`FitReport`].
#[derive(Debug, Clone, Serialize)]
pub struct CandidateJson {
    /// The formula, using column names.
    pub formula: String,
    /// Weighted complexity.
    pub complexity: usize,
    /// Mean squared error.
    pub mse: f64,
    /// Coefficient of determination.
    pub r2: f64,
    /// Largest absolute error.
    pub max_abs_error: f64,
    /// Whether the formula is exact (up to floating point error).
    pub exact: bool,
    /// The well-known formula this is, if it is one.
    pub known: Option<KnownJson>,
}

impl FitReport {
    /// Builds the report for candidate formulas of `target`.
    pub fn new(target: &str, inputs: &[String], rows: usize, found: &[Candidate]) -> Self {
        FitReport {
            schema_version: SCHEMA_VERSION,
            target: target.to_string(),
            inputs: inputs.to_vec(),
            rows,
            candidates: found
                .iter()
                .map(|c| CandidateJson {
                    formula: c.expr.display_with(inputs).to_string(),
                    complexity: c.complexity,
                    mse: c.mse,
                    r2: c.r2,
                    max_abs_error: c.max_abs_error,
                    exact: c.exact,
                    known: KnownJson::identify(
                        &|x: &[f64]| c.expr.eval_row(x, &[]),
                        inputs,
                        target,
                    ),
                })
                .collect(),
        }
    }

    /// Pretty-printed JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }

    /// Plain-text table for terminals.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{} from {} ({} rows)\n",
            self.target,
            self.inputs.join(", "),
            self.rows
        );
        let _ = writeln!(s, "  {:>10}  {:>10}  formula", "complexity", "R²");
        for c in &self.candidates {
            let mark = if c.exact { "  (exact)" } else { "" };
            let known = c
                .known
                .as_ref()
                .map(|k| format!("  [{}]", k.summary()))
                .unwrap_or_default();
            let _ = writeln!(
                s,
                "  {:>10}  {:>10.6}  {} = {}{mark}{known}",
                c.complexity, c.r2, self.target, c.formula
            );
        }
        s
    }
}
