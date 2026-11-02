//! `regrex`: find the formulas behind your data, from the command line.

use clap::{Args, Parser, Subcommand};
use regrex_core::{explain_column, scan, search, ScanConfig, SearchConfig};
use regrex_io::report as output;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod mcp;

#[derive(Parser)]
#[command(name = "regrex", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Find columns that are computed from other columns.
    Scan {
        #[command(flatten)]
        file: FileArgs,
        /// Only check these columns (comma separated).
        #[arg(long, value_delimiter = ',')]
        columns: Vec<String>,
        /// Report a column when its formula matches at least this fraction of rows.
        #[arg(long, default_value_t = 0.9)]
        min_match: f64,
        /// Maximum formula size; larger finds more but is slower.
        #[arg(long, default_value_t = 5)]
        max_size: usize,
        /// Print the column dependencies as a Mermaid diagram (renders on GitHub).
        #[arg(long, conflicts_with = "json")]
        mermaid: bool,
    },
    /// Find formulas for one column.
    Fit {
        #[command(flatten)]
        file: FileArgs,
        /// The column to explain.
        #[arg(long, short)]
        target: String,
        /// Columns to use as inputs (comma separated; default: all others).
        #[arg(long, value_delimiter = ',')]
        inputs: Vec<String>,
        /// Maximum formula size; larger finds more but is slower.
        #[arg(long, default_value_t = 7)]
        max_size: usize,
        /// Number of formulas to show.
        #[arg(long, default_value_t = 10)]
        results: usize,
    },
    /// Run a Model Context Protocol server on stdin/stdout, so AI assistants
    /// can scan datasets and fit formulas.
    Mcp,
}

#[derive(Args)]
struct FileArgs {
    /// A CSV file or a spreadsheet (.xlsx, .xls, .ods) with column names in the first row.
    file: PathBuf,
    /// Sheet to read from a spreadsheet (default: the first one).
    #[arg(long)]
    sheet: Option<String>,
    /// Columns to ignore (comma separated), for example IDs.
    #[arg(long, value_delimiter = ',')]
    ignore: Vec<String>,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn load(file: &FileArgs) -> Result<regrex_io::Loaded, String> {
    load_file(&file.file, file.sheet.as_deref(), &file.ignore)
}

/// Reads a data file and drops the `ignore`d columns.
pub(crate) fn load_file(
    path: &Path,
    sheet: Option<&str>,
    ignore: &[String],
) -> Result<regrex_io::Loaded, String> {
    let mut loaded = regrex_io::load_path(path, sheet).map_err(|e| e.to_string())?;
    for name in ignore {
        let index = column_index(&loaded.table, name)?;
        loaded.table.remove_column(index);
    }
    if loaded.table.columns.is_empty() {
        return Err("no numeric columns found".into());
    }
    Ok(loaded)
}

pub(crate) fn column_index(table: &regrex_core::Table, name: &str) -> Result<usize, String> {
    table
        .names
        .iter()
        .position(|n| n == name)
        .or_else(|| {
            table
                .names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| {
            format!(
                "no numeric column named {name:?} (columns: {})",
                table.names.join(", ")
            )
        })
}

/// Runs a scan of every column, or of the given ones.
pub(crate) fn scan_columns(
    loaded: &regrex_io::Loaded,
    columns: &[String],
    min_match: f64,
    max_size: usize,
) -> Result<Vec<regrex_core::DerivedColumn>, String> {
    let mut config = ScanConfig {
        min_match,
        ..ScanConfig::default()
    };
    config.search.max_size = max_size;
    if columns.is_empty() {
        return Ok(scan(&loaded.table, &config));
    }
    let mut out = Vec::new();
    for name in columns {
        let index = column_index(&loaded.table, name)?;
        out.extend(explain_column(&loaded.table, index, &config));
    }
    Ok(out)
}

/// Searches formulas for one column, using the rows with no missing values.
pub(crate) fn fit_column(
    loaded: &regrex_io::Loaded,
    target: &str,
    inputs: &[String],
    max_size: usize,
    results: usize,
) -> Result<output::FitReport, String> {
    let table = &loaded.table;
    let t = column_index(table, target)?;
    let input_cols: Vec<usize> = if inputs.is_empty() {
        (0..table.columns.len()).filter(|&c| c != t).collect()
    } else {
        inputs
            .iter()
            .map(|n| column_index(table, n))
            .collect::<Result<_, _>>()?
    };
    if input_cols.is_empty() {
        return Err("at least one input column is needed".into());
    }
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&r| {
            table.columns[t][r].is_finite()
                && input_cols.iter().all(|&c| table.columns[c][r].is_finite())
        })
        .collect();
    let x: Vec<Vec<f64>> = input_cols
        .iter()
        .map(|&c| rows.iter().map(|&r| table.columns[c][r]).collect())
        .collect();
    let y: Vec<f64> = rows.iter().map(|&r| table.columns[t][r]).collect();
    // Use units only when every column involved has one.
    let mut config = SearchConfig {
        max_size,
        max_results: results,
        ..SearchConfig::default()
    };
    let dims: Option<Vec<_>> = input_cols
        .iter()
        .map(|&c| table.unit(c).map(|u| u.dim))
        .collect();
    if let (Some(dims), Some(unit)) = (dims, table.unit(t)) {
        config.input_dims = Some(dims);
        config.target_dim = Some(unit.dim);
    }
    let found = search(&x, &y, &config).map_err(|e| e.to_string())?;
    let names: Vec<String> = input_cols.iter().map(|&c| table.names[c].clone()).collect();
    Ok(output::FitReport::new(
        &table.names[t],
        &names,
        rows.len(),
        &found,
    ))
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Scan {
            file,
            columns,
            min_match,
            max_size,
            mermaid,
        } => {
            let loaded = load(&file)?;
            let derived = scan_columns(&loaded, &columns, min_match, max_size)?;
            let report =
                output::ScanReport::new(&file.file.display().to_string(), &loaded, &derived);
            if file.json {
                println!("{}", report.to_json());
            } else if mermaid {
                print!("{}", report.to_mermaid());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        Command::Fit {
            file,
            target,
            inputs,
            max_size,
            results,
        } => {
            let loaded = load(&file)?;
            let report = fit_column(&loaded, &target, &inputs, max_size, results)?;
            if file.json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        Command::Mcp => {
            mcp::serve(std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())
        }
    }
}
