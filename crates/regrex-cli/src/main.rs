//! `regrex`: find the formulas behind your data, from the command line.

use clap::{Args, Parser, Subcommand};
use regrex_core::{
    explain_column_detailed, find_copies, scan_detailed, search, ScanConfig, ScanEvent, ScanResult,
    SearchConfig, Table,
};
use regrex_io::report as output;
use std::path::PathBuf;
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
        /// Search a sample of rows first (blocks spread over the file), then
        /// confirm on all rows only the formulas found there. Much faster on
        /// large files; columns without a formula on the sample are not
        /// searched again.
        #[arg(long, conflicts_with = "columns")]
        quick: bool,
        /// Rows in the sample of --quick (default: 2000 or 5% of the rows,
        /// whichever is larger).
        #[arg(long, requires = "quick")]
        sample: Option<usize>,
        /// Print a compact JSON report for AI agents: every column with what
        /// it holds, each formula with its errors, the near misses and the
        /// inputs tried for the columns left unexplained.
        #[arg(long, conflicts_with_all = ["json", "mermaid"])]
        agent: bool,
        /// Tell on stderr what the search is doing, column by column: the
        /// inputs it tries, the best formula so far and how it ended.
        #[arg(long, short)]
        verbose: bool,
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
    /// Tell what a table holds without searching for formulas: every
    /// column with its role (group key, order, category, code, measurement),
    /// values and range, and the relations between text columns.
    Describe {
        #[command(flatten)]
        file: FileArgs,
    },
    /// Run a Model Context Protocol server on stdin/stdout, so AI assistants
    /// can scan datasets and fit formulas.
    Mcp,
}

#[derive(Args)]
struct FileArgs {
    /// A CSV file or a spreadsheet (.xlsx, .xls, .ods) with column names in the first row, a JSON file (.json, .ndjson, .jsonl) with one object per row, a Parquet or Arrow file (.parquet, .arrow, .feather), or a SQLite database (.sqlite, .db).
    file: PathBuf,
    /// Sheet of a spreadsheet, or table of a SQLite database, to read (default: the first one).
    #[arg(long)]
    sheet: Option<String>,
    /// A second file whose rows are matched with those of the first on the
    /// columns both have (a line, a track and a chainage), to find the
    /// relations between the two files.
    #[arg(long, value_name = "FILE")]
    with: Option<PathBuf>,
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

/// Reads the file (and the `--with` file), telling on stderr how the rows
/// of the two files were matched.
fn load(file: &FileArgs) -> Result<regrex_io::Opened, String> {
    let opened = regrex_io::open(
        &file.file,
        file.sheet.as_deref(),
        file.with.as_deref(),
        &file.ignore,
    )?;
    if let Some(j) = &opened.joined {
        eprintln!("{}", j.summary());
    }
    Ok(opened)
}

pub(crate) use regrex_io::open::column_index;

/// Runs a scan of every column, or of the given ones, telling `on_event`
/// what the search is doing.
pub(crate) fn scan_columns(
    loaded: &regrex_io::Loaded,
    columns: &[String],
    min_match: f64,
    max_size: usize,
    on_event: &(dyn Fn(ScanEvent) + Sync),
) -> Result<ScanResult, String> {
    let mut config = ScanConfig {
        min_match,
        ..ScanConfig::default()
    };
    config.search.max_size = max_size;
    if columns.is_empty() {
        return Ok(scan_detailed(&loaded.table, &config, on_event));
    }
    // As in a full scan: copies are not used as inputs.
    let copies = find_copies(&loaded.table, &config);
    let mut out = ScanResult::default();
    for name in columns {
        let index = column_index(&loaded.table, name)?;
        let e = explain_column_detailed(&loaded.table, index, &config, &copies, on_event);
        out.derived.extend(e.found);
        out.near_misses.extend(e.near_misses);
    }
    Ok(out)
}

/// One line on stderr per [`ScanEvent`], for `--verbose`.
fn print_event(table: &Table, event: &ScanEvent) {
    let name = |c: usize| table.names[c].as_str();
    let line = match event {
        ScanEvent::Started {
            target,
            inputs,
            rows,
        } => {
            let inputs: Vec<&str> = inputs.iter().map(|&c| name(c)).collect();
            format!(
                "{}: searching {rows} rows with {}",
                name(*target),
                inputs.join(", ")
            )
        }
        ScanEvent::Best {
            target,
            formula,
            matched,
            checked,
        } => format!(
            "{}: best so far {formula} ({matched}/{checked} rows)",
            name(*target)
        ),
        ScanEvent::SecondFormula { target, rows } => format!(
            "{}: looking for a second formula on the {rows} rows that break it",
            name(*target)
        ),
        ScanEvent::Done {
            target,
            found,
            near_misses,
        } => {
            let what = if *found { "found" } else { "no formula" };
            let almost = match near_misses {
                0 => String::new(),
                1 => ", 1 near miss".to_string(),
                n => format!(", {n} near misses"),
            };
            format!("{}: {what}{almost}", name(*target))
        }
    };
    eprintln!("{line}");
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
            quick,
            sample,
            agent,
            verbose,
        } => {
            let opened = load(&file)?;
            let loaded = &opened.loaded;
            let table = &loaded.table;
            let name = file.file.display().to_string();
            let mut report = if quick {
                let mut config = ScanConfig {
                    min_match,
                    ..ScanConfig::default()
                };
                config.search.max_size = max_size;
                let size = sample.unwrap_or(regrex_io::open::default_sample(table.n_rows()));
                let q = regrex_core::scan_quick(table, &config, size, |d, confirmed| {
                    let stage = if confirmed {
                        "confirmed"
                    } else {
                        "provisional"
                    };
                    eprintln!("{stage}: {} = {}", table.names[d.target], d.formula_text);
                });
                if agent {
                    let report =
                        regrex_io::agent::AgentReport::new(&name, loaded, &q.derived, &[], &[])
                            .with_join(opened.joined.as_ref())
                            .with_quick(table, &q);
                    println!("{}", report.to_json());
                    return Ok(());
                }
                output::ScanReport::new(&name, loaded, &q.derived).with_quick(table, &q)
            } else {
                let on_event = |e: ScanEvent| {
                    if verbose {
                        print_event(table, &e);
                    }
                };
                let found = scan_columns(loaded, &columns, min_match, max_size, &on_event)?;
                if agent {
                    let report = regrex_io::agent::AgentReport::new(
                        &name,
                        loaded,
                        &found.derived,
                        &found.near_misses,
                        &columns,
                    )
                    .with_join(opened.joined.as_ref());
                    println!("{}", report.to_json());
                    return Ok(());
                }
                output::ScanReport::new(&name, loaded, &found.derived)
                    .with_near_misses(table, &found.near_misses)
            };
            report = report.with_join(opened.joined.as_ref());
            if !columns.is_empty() {
                report = report.only_columns(&columns);
            }
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
            let opened = load(&file)?;
            let report = fit_column(&opened.loaded, &target, &inputs, max_size, results)?;
            if file.json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        Command::Describe { file } => {
            let opened = load(&file)?;
            let d = regrex_io::describe::Description::new(
                &file.file.display().to_string(),
                &opened.loaded,
                opened.joined.as_ref(),
            );
            if file.json {
                println!("{}", d.to_json());
            } else {
                print!("{}", d.to_text());
            }
            Ok(())
        }
        Command::Mcp => {
            mcp::serve(std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())
        }
    }
}
