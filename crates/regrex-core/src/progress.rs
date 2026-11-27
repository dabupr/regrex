//! What a scan is doing while it runs, and the formulas it came close to
//! accepting.
//!
//! A scan only reports formulas that hold on most rows. The ones that hold
//! on 60% of them, or only within a group, are dropped, yet they are often
//! the most interesting part: they say where to look next. [`NearMiss`]
//! keeps them, with the reason they were rejected and, when a rule on
//! another column picks out the rows where they hold, that rule.
//!
//! [`ScanEvent`]s are sent while a column is searched, so a front end can
//! show what the search is doing instead of a bare "checking column 3".

use crate::expr::Expr;
use crate::scan::Split;

/// Why a formula was not reported as the formula of its column.
#[derive(Debug, Clone, PartialEq)]
pub enum Rejection {
    /// It matches too few rows: `needed` is the share required
    /// ([`ScanConfig::min_match`](crate::ScanConfig::min_match)).
    Coverage {
        /// Share of rows needed.
        needed: f64,
    },
    /// It is right on the rows holding the column's most common value
    /// (often 0) but not on enough of the others, so it explains little.
    UsualValue,
    /// It fits most of the rows that break the column's formula (`main`,
    /// written with column names), but it is more complex than that formula
    /// and no rule on another column tells its rows apart, so it may be a
    /// fluke.
    NoSplitRule {
        /// The formula whose broken rows it fits.
        main: String,
    },
}

impl Rejection {
    /// A short name, stable for JSON: `coverage`, `usual_value`
    /// or `no_split_rule`.
    pub fn code(&self) -> &'static str {
        match self {
            Rejection::Coverage { .. } => "coverage",
            Rejection::UsualValue => "usual_value",
            Rejection::NoSplitRule { .. } => "no_split_rule",
        }
    }
}

/// A formula a scan found but did not report.
#[derive(Debug, Clone)]
pub struct NearMiss {
    /// Index of the column it computes.
    pub target: usize,
    /// The formula, over column indices.
    pub formula: Expr,
    /// The formula written with column names.
    pub formula_text: String,
    /// Columns the formula uses.
    pub inputs: Vec<usize>,
    /// Size of the formula.
    pub complexity: usize,
    /// Rows where it gives the column's value.
    pub matched: usize,
    /// Rows it was checked on.
    pub checked: usize,
    /// Checked rows where it does not hold, in increasing order.
    pub missed: Vec<usize>,
    /// A rule on another column that picks out the rows where it holds,
    /// when there is one: the formula may be right for that group.
    pub holds_when: Option<Split>,
    /// Why it was not reported.
    pub reason: Rejection,
}

impl NearMiss {
    /// Share of the checked rows where it holds.
    pub fn match_ratio(&self) -> f64 {
        if self.checked == 0 {
            0.0
        } else {
            self.matched as f64 / self.checked as f64
        }
    }
}

/// Something a scan did, sent while it runs.
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// The search for a column started, with the columns it may use and the
    /// rows where they all have a value.
    Started {
        /// The column searched.
        target: usize,
        /// Columns tried as inputs.
        inputs: Vec<usize>,
        /// Rows searched.
        rows: usize,
    },
    /// The best formula found so far for a column, and the rows it matches.
    Best {
        /// The column searched.
        target: usize,
        /// The formula, written with column names.
        formula: String,
        /// Rows it matches.
        matched: usize,
        /// Rows it was checked on.
        checked: usize,
    },
    /// The rows that break the best formula are searched for a second one.
    SecondFormula {
        /// The column searched.
        target: usize,
        /// Rows searched.
        rows: usize,
    },
    /// The search for a column ended.
    Done {
        /// The column searched.
        target: usize,
        /// Whether a formula was found.
        found: bool,
        /// Formulas kept as near misses.
        near_misses: usize,
    },
}

impl ScanEvent {
    /// The column the event is about.
    pub fn target(&self) -> usize {
        match self {
            ScanEvent::Started { target, .. }
            | ScanEvent::Best { target, .. }
            | ScanEvent::SecondFormula { target, .. }
            | ScanEvent::Done { target, .. } => *target,
        }
    }
}

/// What a search for one column gives: the formula, if one was found, and
/// the near misses, best first.
#[derive(Debug, Clone, Default)]
pub struct Explanation {
    /// The formula found, if any.
    pub found: Option<crate::scan::DerivedColumn>,
    /// Formulas found but not reported, most rows matched first.
    pub near_misses: Vec<NearMiss>,
}
