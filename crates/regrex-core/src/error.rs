//! Error type.

use std::fmt;

/// Errors returned when the input data cannot be searched.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// There are no rows.
    EmptyData,
    /// There are no input columns.
    NoInputs,
    /// An input column does not have as many rows as the target.
    LengthMismatch {
        /// Index of the offending input column.
        column: usize,
        /// Number of rows in the target.
        expected: usize,
        /// Number of rows in the column.
        found: usize,
    },
    /// A value is NaN or infinite.
    NonFinite {
        /// Column index, or `None` for the target.
        column: Option<usize>,
        /// Row index.
        row: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EmptyData => write!(f, "the data has no rows"),
            Error::NoInputs => write!(f, "at least one input column is needed"),
            Error::LengthMismatch {
                column,
                expected,
                found,
            } => write!(
                f,
                "input column {column} has {found} rows but the target has {expected}"
            ),
            Error::NonFinite {
                column: Some(c),
                row,
            } => write!(f, "input column {c} has a non-finite value at row {row}"),
            Error::NonFinite { column: None, row } => {
                write!(f, "the target has a non-finite value at row {row}")
            }
        }
    }
}

impl std::error::Error for Error {}
