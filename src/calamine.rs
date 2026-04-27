//! XLSX / XLS / XLSB / XLSM / ODS reader, backed by the
//! [`calamine`](https://crates.io/crates/calamine) crate.
//!
//! `calamine` is the Rust ecosystem standard for reading legacy
//! and modern Excel formats — it's pure-Rust, supports all the
//! variants listed above, and handles the cell-format weirdness
//! (date serial numbers, scientific notation, formula results)
//! reasonably well. tabkit wraps it with type inference and a
//! sample-row cap.

use crate::{infer_column_type, Column, Error, ReadOptions, Reader, Result, Row, Table, Value};
use calamine::{open_workbook_auto, Data, Reader as CalamineReaderTrait};
use std::path::Path;

/// XLSX-family reader. Construct via [`CalamineReader::new`]
/// (cannot fail — the crate has no runtime dependency).
#[derive(Default)]
pub struct CalamineReader;

impl CalamineReader {
    /// Construct a reader. Cannot fail.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Reader for CalamineReader {
    fn extensions(&self) -> &[&'static str] {
        &["xlsx", "xls", "xlsb", "xlsm", "ods"]
    }

    fn name(&self) -> &'static str {
        "calamine"
    }

    fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table> {
        let mut workbook = open_workbook_auto(path).map_err(|e| {
            Error::ParseError(format!("calamine failed to open {}: {e}", path.display()))
        })?;

        // Pick the sheet: caller's choice, or the first non-empty
        // sheet by source order. calamine's `sheet_names` returns
        // sheets in workbook order.
        let sheet_names: Vec<String> = workbook.sheet_names().clone();
        if sheet_names.is_empty() {
            return Err(Error::ParseError(format!(
                "{} has no sheets",
                path.display()
            )));
        }

        let chosen_sheet = match &options.sheet_name {
            Some(requested) => {
                if !sheet_names.iter().any(|s| s == requested) {
                    return Err(Error::SheetNotFound {
                        requested: requested.clone(),
                        path: path.display().to_string(),
                        available: sheet_names.join(", "),
                    });
                }
                requested.clone()
            }
            None => sheet_names[0].clone(),
        };

        let range = workbook.worksheet_range(&chosen_sheet).map_err(|e| {
            Error::ParseError(format!(
                "calamine could not read sheet {chosen_sheet:?} in {}: {e}",
                path.display()
            ))
        })?;

        // calamine's `Range` iterates row-major. The first row is
        // either the header (default) or the first data row
        // (`has_header = false`).
        let mut rows_iter = range.rows();
        let (column_names, total_rows_seen) = if options.has_header {
            // Pull the header row up front. If the sheet is empty,
            // return an empty table — same shape downstream code
            // expects.
            let Some(header_row) = rows_iter.next() else {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("sheet".into(), chosen_sheet);
                return Ok(Table {
                    columns: Vec::new(),
                    sample_rows: Vec::new(),
                    row_count: Some(0),
                    metadata,
                });
            };
            let names = header_row
                .iter()
                .enumerate()
                .map(|(idx, cell)| {
                    let name = cell.to_string();
                    if name.trim().is_empty() {
                        format!("column_{idx}")
                    } else {
                        name
                    }
                })
                .collect::<Vec<_>>();
            (names, 1u64)
        } else {
            // Headerless mode: peek width from the first row to
            // generate column_0..column_(N-1) names. We can't pull
            // the row out without consuming it; collect a Vec and
            // re-iterate via sample collection below.
            let width = range.width();
            let names: Vec<String> = (0..width).map(|i| format!("column_{i}")).collect();
            (names, 0u64)
        };

        // Collect sample rows up to the cap. Track total row count
        // by counting the iterator (bounded by the sheet size, not
        // by the sample cap, so callers get the real row count).
        let mut sample_rows: Vec<Row> = Vec::with_capacity(options.max_sample_rows);
        let mut row_count = total_rows_seen;
        for raw_row in rows_iter {
            row_count += 1;
            if sample_rows.len() < options.max_sample_rows {
                sample_rows.push(raw_to_row(raw_row, column_names.len()));
            }
        }

        // Run type inference per column over the sample.
        let columns = column_names
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
            .collect();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("sheet".into(), chosen_sheet);

        // Subtract 1 for the header row when has_header. row_count
        // here counts the header itself; the public contract is
        // "data row count", excluding header.
        let public_row_count = if options.has_header {
            row_count.saturating_sub(1)
        } else {
            row_count
        };

        Ok(Table {
            columns,
            sample_rows,
            row_count: Some(public_row_count),
            metadata,
        })
    }
}

/// Translate a calamine row slice into a tabkit row of `Value`s.
/// Pad to `width` with nulls for ragged sheets (calamine returns
/// the actual row length, which can be shorter than the header
/// row width when the source has trailing-empty cells).
fn raw_to_row(raw: &[Data], width: usize) -> Row {
    let mut row: Row = Vec::with_capacity(width);
    for cell in raw {
        row.push(data_to_value(cell));
    }
    while row.len() < width {
        row.push(Value::Null);
    }
    row.truncate(width);
    row
}

/// calamine's `Data` enum → tabkit's `Value`. `DateTime` and
/// `Duration` flatten to their string form for v0.1 — a future
/// `dates` feature could carry typed dates through.
fn data_to_value(data: &Data) -> Value {
    match data {
        Data::Empty => Value::Null,
        Data::String(s) => Value::Text(s.clone()),
        Data::Bool(b) => Value::Bool(*b),
        Data::Int(i) => Value::Integer(*i),
        Data::Float(f) => {
            // Detect whole-number floats and demote to Integer.
            // Excel stores everything as f64 by default; "1" comes
            // through as `Float(1.0)` and we'd rather call it an
            // integer for the schema.
            //
            // The `i64::MIN/MAX as f64` cast is exact for the
            // boundaries we care about (powers of 2), and
            // `f as i64` is a saturating cast guarded by the
            // bound check — both are intentional. Clippy's
            // cast-precision-loss + cast-possible-truncation
            // warnings are noise here.
            #[allow(
                clippy::float_cmp,
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation
            )]
            if f.is_finite() && f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                Value::Integer(*f as i64)
            } else {
                Value::Float(*f)
            }
        }
        // DateTime / Duration / DurationIso / DateTimeIso / Error
        // all flatten to their Display form. Errors include
        // formula-error cells (`#REF!`, `#DIV/0!`, etc.) — we
        // surface them as text rather than dropping so the user
        // sees the cell's actual content.
        other => Value::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_cover_xlsx_family() {
        let exts = CalamineReader.extensions();
        for required in ["xlsx", "xls", "xlsb", "xlsm", "ods"] {
            assert!(exts.contains(&required), "missing .{required}");
        }
    }

    #[test]
    fn name_identifies_backend() {
        assert_eq!(CalamineReader.name(), "calamine");
    }

    #[test]
    fn missing_file_returns_typed_error() {
        let result = CalamineReader.read(Path::new("/nonexistent.xlsx"), &ReadOptions::default());
        assert!(matches!(result, Err(Error::ParseError(_))));
    }

    #[test]
    #[ignore = "requires a real XLSX file at tests/fixtures/sample.xlsx"]
    fn extracts_schema_and_samples_from_real_xlsx() {
        // Skipped by default. To run: drop a `sample.xlsx` with a
        // header row + a few data rows at tests/fixtures/, then
        //   cargo test --features calamine -- --ignored
        let table = CalamineReader
            .read(
                Path::new("tests/fixtures/sample.xlsx"),
                &ReadOptions::default(),
            )
            .expect("read failed");
        assert!(!table.columns.is_empty());
        assert!(!table.sample_rows.is_empty());
        assert!(table.metadata.contains_key("sheet"));
    }

    #[test]
    fn whole_number_floats_demote_to_integer() {
        assert_eq!(data_to_value(&Data::Float(1.0)), Value::Integer(1));
        assert_eq!(data_to_value(&Data::Float(-7.0)), Value::Integer(-7));
        // Fractional values stay Float.
        assert_eq!(data_to_value(&Data::Float(2.5)), Value::Float(2.5));
    }

    #[test]
    fn empty_cells_become_null() {
        assert_eq!(data_to_value(&Data::Empty), Value::Null);
    }
}
