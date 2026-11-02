//! # regrex-core
//!
//! The engine behind [Regrex](https://github.com/dabupr/regrex): given input
//! columns and a target column, find readable formulas that explain the
//! target, ranked by accuracy and simplicity.
//!
//! ```
//! use regrex_core::{search, SearchConfig};
//!
//! // Total = Price * Quantity * 1.21
//! let price = vec![10.0, 5.5, 3.2, 8.0, 1.25];
//! let quantity = vec![2.0, 4.0, 1.0, 3.0, 10.0];
//! let total: Vec<f64> = price.iter().zip(&quantity).map(|(p, q)| p * q * 1.21).collect();
//!
//! let config = SearchConfig { max_size: 5, ..SearchConfig::default() };
//! let results = search(&[price, quantity], &total, &config).unwrap();
//! let names = ["price".to_string(), "quantity".to_string()];
//! let best = &results[0];
//! assert!(best.exact);
//! assert_eq!(best.expr.display_with(&names).to_string(), "1.21 * price * quantity");
//! ```
//!
//! To find which columns of a table are computed from the others, see
//! [`scan`](crate::scan::scan).
//!
//! How it works:
//!
//! 1. [`enumerate`](crate::enumerate::enumerate) builds every distinct formula
//!    up to a size limit, skipping duplicates by canonical form and by value.
//! 2. Free constants are fitted with Levenberg–Marquardt and snapped to nice
//!    values (`0.5`, `π`, `1.21`) when that does not hurt accuracy.
//! 3. Candidates are scored by error and complexity and the Pareto front is
//!    returned.

#![warn(missing_docs)]

pub mod enumerate;
pub mod error;
pub mod expr;
pub mod fit;
pub mod scan;
pub mod search;
pub mod simplify;

pub use error::Error;
pub use expr::{BinaryOp, Expr, UnaryOp};
pub use scan::{explain_column, scan, DerivedColumn, ScanConfig, Table};
pub use search::{search, Candidate, SearchConfig};
