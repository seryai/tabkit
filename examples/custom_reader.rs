//! Implement the `Reader` trait for a custom format.
//!
//! Toy format: a simple semicolon-separated `.ssv` file. The
//! point of the example is the trait shape, not the format —
//! real implementations would handle proprietary spreadsheet
//! formats, line-delimited JSON, etc.
//!
//! ```bash
//! cargo run --example custom_reader -- /path/to/data.ssv
//! ```
//!
//! The mechanics that ARE realistic:
//!
//! 1. Implement `Reader::extensions` to claim file extensions.
//! 2. Implement `Reader::read` to produce a `Table`. Reuse
//!    `Value` for cell payloads so type inference (and the
//!    JSON-IPC contract for Tauri callers) stays uniform.
//! 3. Register your reader on a fresh `Engine`. Order matters
//!    for overlapping extensions — first registered wins.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tabkit::{Column, Engine, Error, ReadOptions, Reader, Result, Row, Table, Value};

/// Reads `*.ssv` files: rows separated by `\n`, cells separated
/// by `;`. First row is headers. No quoting / escaping rules —
/// embedded `;` characters split the field. Real-world readers
/// would handle escapes; we keep this minimal so the trait
/// shape stays visible.
struct SsvReader;

impl Reader for SsvReader {
    fn extensions(&self) -> &[&'static str] {
        &["ssv"]
    }

    fn name(&self) -> &'static str {
        "ssv-example"
    }

    fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::ParseError(format!("ssv: read failed: {e}")))?;

        let mut lines = content.lines();
        let column_names: Vec<String> = if options.has_header {
            let Some(header) = lines.next() else {
                return Ok(Table::default());
            };
            header
                .split(';')
                .enumerate()
                .map(|(idx, h)| {
                    if h.trim().is_empty() {
                        format!("column_{idx}")
                    } else {
                        h.to_string()
                    }
                })
                .collect()
        } else {
            // Headerless mode: peek first row's width to size the
            // column-name list. We're holding the iterator above so
            // the peeked row is consumed; for production code you'd
            // collect it as the first sample row.
            return Err(Error::ParseError(
                "ssv example: headerless mode left as exercise to the reader".into(),
            ));
        };

        let mut sample_rows: Vec<Row> = Vec::new();
        let mut row_count: u64 = 0;
        for line in lines {
            row_count += 1;
            if sample_rows.len() < options.max_sample_rows {
                let cells: Row = line
                    .split(';')
                    .map(parse_cell)
                    .chain(std::iter::repeat_with(|| Value::Null))
                    .take(column_names.len())
                    .collect();
                sample_rows.push(cells);
            }
        }

        // Type inference is one of tabkit's main jobs. Reusing the
        // crate's own `Value::data_type()` keeps the schema output
        // semantically identical to what calamine / csv produce.
        // For readers that don't need it, `DataType::Unknown` is
        // a fine default.
        let columns: Vec<Column> = column_names
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let inferred = sample_rows
                    .iter()
                    .map(|r| r[idx].data_type())
                    .fold(None, |acc, t| match (acc, t) {
                        (None, x) => x,
                        (Some(a), Some(b)) if a == b => Some(a),
                        // For brevity we don't replicate the
                        // numeric-widening + date-widening rules
                        // tabkit's own infer_column_type uses;
                        // mixed types just become None → Unknown.
                        _ => None,
                    })
                    .unwrap_or(tabkit::DataType::Unknown);
                let nullable = sample_rows.iter().any(|r| matches!(r[idx], Value::Null));
                Column::new(name.clone(), inferred, nullable)
            })
            .collect();

        let mut table = Table::new(columns, sample_rows);
        table.row_count = Some(row_count);
        table.metadata.insert("delimiter".into(), ";".into());
        Ok(table)
    }
}

fn parse_cell(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return Value::Integer(i);
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return Value::Float(f);
    }
    Value::Text(trimmed.to_string())
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: custom_reader <path-to-.ssv-file>");
        return ExitCode::FAILURE;
    };

    // Construct an empty engine + register the custom reader. We
    // could ALSO call Engine::with_defaults() and add SsvReader
    // on top — order matters for overlapping extensions, but
    // .ssv doesn't overlap with anything tabkit ships, so either
    // construction order works.
    let mut engine = Engine::new();
    engine.register(Box::new(SsvReader));

    match engine.read(Path::new(&path), &ReadOptions::default().max_sample_rows(5)) {
        Ok(table) => {
            println!("Schema:");
            for col in &table.columns {
                let null_marker = if col.nullable { " ?" } else { "" };
                println!("  {} : {:?}{}", col.name, col.data_type, null_marker);
            }
            if let Some(n) = table.row_count {
                println!();
                println!("Row count: {n}");
            }
            println!();
            println!("Sample rows:");
            for row in &table.sample_rows {
                println!("  {row:?}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
