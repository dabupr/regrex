//! What a table holds, without searching for formulas.
//!
//! [`Description`] is what an AI agent reads before deciding what to scan:
//! every column with what it holds, how the table is laid out (groups, the
//! order of the rows, what each column is for) and the relations between
//! its text columns. It takes about as long as reading the file.

use crate::agent::{agent_columns, AgentColumn};
use crate::report::{ConvertedJson, StructureJson, TextRelationJson};
use crate::{JoinedJson, Loaded};
use regrex_core::{find_copies, ScanConfig};
use serde::Serialize;

/// Version of the description format. Fields may be added; existing ones
/// keep their meaning.
pub const DESCRIBE_SCHEMA_VERSION: u32 = 1;

/// What a table holds, see the [module](self) documentation.
#[derive(Debug, Clone, Serialize)]
pub struct Description {
    /// Version of this format, see [`DESCRIBE_SCHEMA_VERSION`].
    pub describe_schema_version: u32,
    /// The file read.
    pub file: String,
    /// Data rows.
    pub rows: usize,
    /// Every column of the file, in order, as in the agent report:
    /// `searched` tells whether a scan would search it, and
    /// `not_searched` why not.
    pub columns: Vec<AgentColumn>,
    /// How the table is laid out and what each column is for.
    pub structure: StructureJson,
    /// Columns whose cells were read as numbers although they are not
    /// plain numbers, and how.
    pub converted_columns: Vec<ConvertedJson>,
    /// Relations between text columns.
    pub text_relations: Vec<TextRelationJson>,
    /// When a second file was given, how its rows were matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined: Option<JoinedJson>,
}

impl Description {
    /// Describes `loaded`, read from `file`.
    pub fn new(file: &str, loaded: &Loaded, joined: Option<&JoinedJson>) -> Self {
        let copies = find_copies(&loaded.table, &ScanConfig::default());
        Description {
            describe_schema_version: DESCRIBE_SCHEMA_VERSION,
            file: file.to_string(),
            rows: loaded.table.n_rows(),
            columns: agent_columns(loaded, &copies, &[]),
            structure: StructureJson::new(&loaded.table),
            converted_columns: loaded
                .converted
                .iter()
                .map(|(column, read_as)| ConvertedJson {
                    column: column.clone(),
                    read_as: read_as.clone(),
                })
                .collect(),
            text_relations: TextRelationJson::all(loaded),
            joined: joined.cloned(),
        }
    }

    /// Compact JSON, on one line.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("description is serializable")
    }

    /// A few lines for a person.
    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut s = format!(
            "{}: {} rows, {} columns\n",
            self.file,
            self.rows,
            self.columns.len()
        );
        if let Some(j) = &self.joined {
            let _ = writeln!(s, "{}", j.summary());
        }
        let summary = self.structure.summary();
        if !summary.is_empty() {
            let _ = writeln!(s, "{summary}");
        }
        s.push('\n');
        let width = self
            .columns
            .iter()
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0);
        for c in &self.columns {
            let role = self
                .structure
                .roles
                .iter()
                .find(|r| r.column == c.name)
                .map_or(c.kind, |r| r.role.as_str());
            let distinct = if c.distinct >= crate::agent::MAX_DISTINCT {
                format!("{}+", c.distinct)
            } else {
                c.distinct.to_string()
            };
            let mut line = format!("  {:<width$}  {:<11}  {distinct} values", c.name, role);
            if let (Some(lo), Some(hi)) = (c.min, c.max) {
                let _ = write!(line, ", {} to {}", fmt(lo), fmt(hi));
            }
            if let Some(how) = &c.read_as {
                let _ = write!(line, ", {how}");
            }
            if c.missing > 0 {
                let _ = write!(line, ", {} missing", c.missing);
            }
            let _ = writeln!(s, "{}", line.trim_end());
        }
        if !self.text_relations.is_empty() {
            s.push_str("\nText relations:\n");
            for r in &self.text_relations {
                let _ = writeln!(
                    s,
                    "  {} = {}   {}/{} rows",
                    r.column, r.formula, r.matched_rows, r.checked_rows
                );
            }
        }
        s
    }
}

fn fmt(v: f64) -> String {
    regrex_core::expr::format_number(v)
}
