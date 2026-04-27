//! CSV / TSV reader, backed by the [`csv`](https://crates.io/crates/csv)
//! crate. Tab vs comma is auto-selected by extension (`.tsv` →
//! tab, everything else → comma).

use crate::{infer_column_type, Column, Error, ReadOptions, Reader, Result, Row, Table, Value};
use std::path::Path;

/// CSV / TSV reader.
#[derive(Default)]
pub struct CsvReader;

impl CsvReader {
    /// Construct a reader. Cannot fail.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Reader for CsvReader {
    fn extensions(&self) -> &[&'static str] {
        &["csv", "tsv"]
    }

    fn name(&self) -> &'static str {
        "csv"
    }

    fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table> {
        // Tab vs comma based on extension. The csv crate doesn't
        // sniff content; for that we'd need a separate detection
        // pass. Extension is right ~99% of the time.
        let delimiter = if path
            .extension()
            .and_then(|os| os.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
            == Some("tsv")
        {
            b'\t'
        } else {
            b','
        };

        let mut reader = ::csv::ReaderBuilder::new()
            .has_headers(options.has_header)
            .delimiter(delimiter)
            .flexible(true) // tolerate ragged rows; pad with nulls below
            .from_path(path)
            .map_err(|e| Error::ParseError(format!("csv open failed: {e}")))?;

        // Build column names. csv treats the first row as headers
        // when `has_headers(true)`; in headerless mode we generate
        // `column_<i>` names from the first record's width.
        let column_names: Vec<String> = if options.has_header {
            reader
                .headers()
                .map_err(|e| Error::ParseError(format!("csv headers read failed: {e}")))?
                .iter()
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
            // Peek the first record to learn the column count.
            // csv's StringRecord iter consumes the row, so we
            // store it for later.
            Vec::new()
        };

        let mut sample_rows: Vec<Row> = Vec::with_capacity(options.max_sample_rows);
        let mut row_count: u64 = 0;
        let mut headerless_width: Option<usize> = None;
        let mut pending_first_record: Option<Vec<String>> = None;

        for record in reader.records() {
            let record = record.map_err(|e| {
                Error::ParseError(format!("csv row {} parse failed: {e}", row_count + 1))
            })?;
            row_count += 1;

            // In headerless mode the very first record sets the
            // column count. Save it as a pending row so it lands
            // in `sample_rows` like any other data row.
            if !options.has_header && headerless_width.is_none() {
                let width = record.len();
                headerless_width = Some(width);
                pending_first_record = Some(record.iter().map(str::to_string).collect());
                continue;
            }

            if sample_rows.len() < options.max_sample_rows {
                sample_rows.push(record.iter().map(parse_cell).collect());
            }
        }

        // For headerless, generate column_<i> names + push the
        // pending first row.
        let final_column_names = if options.has_header {
            column_names
        } else {
            let width = headerless_width.unwrap_or(0);
            let names: Vec<String> = (0..width).map(|i| format!("column_{i}")).collect();
            if let Some(first) = pending_first_record {
                if sample_rows.len() < options.max_sample_rows {
                    sample_rows.insert(0, first.iter().map(|s| parse_cell(s.as_str())).collect());
                }
            }
            names
        };

        let columns = pad_and_infer(&final_column_names, &mut sample_rows);

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "delimiter".into(),
            if delimiter == b'\t' {
                "tab".into()
            } else {
                ",".into()
            },
        );

        Ok(Table {
            columns,
            sample_rows,
            row_count: Some(row_count),
            metadata,
        })
    }
}

/// Pad each sample row out to the column count (so ragged input
/// doesn't produce out-of-bounds reads later) and run type inference
/// per column. Pulled out of [`CsvReader::read`] to keep that
/// function under clippy's 100-line ceiling and to share the logic
/// with future callers.
fn pad_and_infer(column_names: &[String], sample_rows: &mut [Row]) -> Vec<Column> {
    let width = column_names.len();
    for row in sample_rows.iter_mut() {
        while row.len() < width {
            row.push(Value::Null);
        }
        row.truncate(width);
    }
    column_names
        .iter()
        .enumerate()
        .map(|(idx, name)| {
            let column_samples: Vec<Value> = sample_rows
                .iter()
                .map(|r| r.get(idx).cloned().unwrap_or(Value::Null))
                .collect();
            let (data_type, nullable) = infer_column_type(&column_samples);
            Column {
                name: name.clone(),
                data_type,
                nullable,
            }
        })
        .collect()
}

/// Parse a raw CSV cell string into a typed `Value`. Rules:
/// - Empty string → `Null` (CSV has no real null; this is the
///   conventional read).
/// - All-digits (with optional leading `-`) → `Integer` if it
///   fits `i64`, else `Text`.
/// - Decimal-looking → `Float` if `parse::<f64>` accepts it.
/// - `true` / `false` (case-insensitive) → `Bool`.
/// - Anything else → `Text`.
fn parse_cell(raw: &str) -> Value {
    if raw.is_empty() {
        return Value::Null;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Text(raw.to_string());
    }
    // Bool first — narrowest match.
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    // Integer next — all-digits with optional leading `-`. We
    // check the byte pattern before parse() because parse() also
    // accepts `+1` / `0x1f` etc. that don't read as plain integers
    // to a human eye.
    if is_plain_integer(trimmed) {
        if let Ok(i) = trimmed.parse::<i64>() {
            return Value::Integer(i);
        }
    }
    // Float — parse if it has a `.` or `e`/`E`. We don't accept
    // bare integer strings as Float; that'd defeat the
    // Integer-first path.
    if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
        if let Ok(f) = trimmed.parse::<f64>() {
            return Value::Float(f);
        }
    }
    Value::Text(raw.to_string())
}

fn is_plain_integer(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let start = usize::from(bytes[0] == b'-');
    if start == bytes.len() {
        return false;
    }
    bytes[start..].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn extensions_handles_csv_and_tsv() {
        assert_eq!(CsvReader.extensions(), &["csv", "tsv"]);
    }

    #[test]
    fn name_identifies_backend() {
        assert_eq!(CsvReader.name(), "csv");
    }

    #[test]
    fn reads_basic_csv_with_header() {
        let f = write_csv("name,age\nAlice,30\nBob,25\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.columns[0].name, "name");
        assert_eq!(table.columns[1].name, "age");
        assert_eq!(table.sample_rows.len(), 2);
        assert_eq!(table.row_count, Some(2));
    }

    #[test]
    fn type_inference_picks_integer_for_age() {
        let f = write_csv("name,age\nAlice,30\nBob,25\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns[1].data_type, crate::DataType::Integer);
    }

    #[test]
    fn type_inference_picks_float_for_mixed_int_and_float() {
        let f = write_csv("v\n1\n2.5\n3\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns[0].data_type, crate::DataType::Float);
    }

    #[test]
    fn type_inference_falls_back_to_text_on_mixed() {
        let f = write_csv("v\n1\nhello\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns[0].data_type, crate::DataType::Text);
    }

    #[test]
    fn empty_cells_become_null_and_mark_column_nullable() {
        // The csv crate skips lines containing zero bytes (a bare
        // `\n` between rows is treated as no record), so we test
        // empty-cell handling on a multi-column CSV where one row
        // has an explicitly empty first field.
        let f = write_csv("v,name\n1,a\n,b\n3,c\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns[0].data_type, crate::DataType::Integer);
        assert!(table.columns[0].nullable);
    }

    #[test]
    fn ragged_rows_get_padded_with_nulls() {
        // Second row has only one cell; should be padded to 2.
        let f = write_csv("a,b\n1,2\n3\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.sample_rows[1].len(), 2);
        assert_eq!(table.sample_rows[1][1], Value::Null);
    }

    #[test]
    fn sample_cap_limits_rows() {
        use std::fmt::Write as _;
        let mut content = String::from("v\n");
        for i in 0..200 {
            writeln!(content, "{i}").unwrap();
        }
        let f = write_csv(&content);
        let table = CsvReader
            .read(f.path(), &ReadOptions::default().max_sample_rows(10))
            .unwrap();
        assert_eq!(table.sample_rows.len(), 10);
        // row_count counts every row, not just sampled ones.
        assert_eq!(table.row_count, Some(200));
    }

    #[test]
    fn empty_header_cell_falls_back_to_column_index() {
        let f = write_csv(",b\n1,2\n");
        let table = CsvReader.read(f.path(), &ReadOptions::default()).unwrap();
        assert_eq!(table.columns[0].name, "column_0");
        assert_eq!(table.columns[1].name, "b");
    }

    #[test]
    fn headerless_mode_generates_column_names() {
        let f = write_csv("1,2\n3,4\n");
        let table = CsvReader
            .read(f.path(), &ReadOptions::default().has_header(false))
            .unwrap();
        assert_eq!(table.columns[0].name, "column_0");
        assert_eq!(table.columns[1].name, "column_1");
        assert_eq!(table.sample_rows.len(), 2);
        assert_eq!(table.row_count, Some(2));
    }

    #[test]
    fn missing_file_returns_typed_error() {
        let result = CsvReader.read(Path::new("/nonexistent.csv"), &ReadOptions::default());
        assert!(matches!(result, Err(Error::ParseError(_))));
    }

    #[test]
    fn parse_cell_recognises_basic_types() {
        assert_eq!(parse_cell(""), Value::Null);
        assert_eq!(parse_cell("42"), Value::Integer(42));
        assert_eq!(parse_cell("-7"), Value::Integer(-7));
        assert_eq!(parse_cell("2.5"), Value::Float(2.5));
        assert_eq!(parse_cell("true"), Value::Bool(true));
        assert_eq!(parse_cell("FALSE"), Value::Bool(false));
        assert_eq!(parse_cell("hello"), Value::Text("hello".into()));
    }
}
