//! Inspect any tabular file: print schema + a few sample rows.
//!
//! Run with the default features (XLSX + CSV / TSV):
//!
//! ```bash
//! cargo run --example inspect -- /path/to/data.xlsx
//! ```
//!
//! For Parquet files, enable the `parquet` feature:
//!
//! ```bash
//! cargo run --example inspect --features parquet -- /path/to/data.parquet
//! ```

use std::env;
use std::path::Path;
use std::process::ExitCode;

use tabkit::{Engine, ReadOptions};

fn main() -> ExitCode {
    // First positional arg is the file to inspect. We don't pull in
    // `clap` — the example should compile cleanly with just the
    // crate's own deps so users can read the surface without
    // wading through a CLI parser.
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: inspect <path-to-tabular-file>");
        eprintln!();
        eprintln!("Supports: .xlsx, .xls, .xlsb, .xlsm, .ods, .csv, .tsv");
        eprintln!("With --features parquet: also .parquet");
        return ExitCode::FAILURE;
    };

    let engine = Engine::with_defaults();
    let options = ReadOptions::default().max_sample_rows(5);

    match engine.read(Path::new(&path), &options) {
        Ok(table) => {
            print_table(&table);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_table(table: &tabkit::Table) {
    println!("Schema:");
    if table.columns.is_empty() {
        println!("  (empty)");
    } else {
        // Padding the column-name column so the type column lines
        // up. Useful when the file has long header strings.
        let max_name_len = table
            .columns
            .iter()
            .map(|c| c.name.len())
            .max()
            .unwrap_or(0);
        for col in &table.columns {
            let null_marker = if col.nullable { " ?" } else { "" };
            println!(
                "  {:<width$}  {:?}{}",
                col.name,
                col.data_type,
                null_marker,
                width = max_name_len,
            );
        }
    }

    if !table.metadata.is_empty() {
        println!();
        println!("Metadata:");
        let mut keys: Vec<&String> = table.metadata.keys().collect();
        keys.sort();
        for key in keys {
            println!("  {key}: {}", table.metadata[key]);
        }
    }

    if let Some(n) = table.row_count {
        println!();
        println!("Row count: {n} (excluding header)");
    }

    if !table.sample_rows.is_empty() {
        println!();
        println!("Sample rows ({}):", table.sample_rows.len());
        for (idx, row) in table.sample_rows.iter().enumerate() {
            println!("  [{idx}] {row:?}");
        }
    }
}
