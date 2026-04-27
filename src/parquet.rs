//! Parquet reader, backed by the
//! [`parquet`](https://crates.io/crates/parquet) crate.
//!
//! `tabkit`'s parquet support reads schema + sample rows via the
//! row-level `RowAccessor` API. We deliberately disable the
//! parquet crate's default features (full Arrow runtime, async
//! reader, CLI helpers) — none of which the schema-and-samples
//! surface needs — to keep the dep weight reasonable.
//!
//! ## Type mapping
//!
//! Parquet's `Field` enum carries a richer type system than
//! tabkit's narrow `Value`. The mapping (v0.2):
//!
//! | parquet `Field`             | tabkit `Value`              |
//! |-----------------------------|-----------------------------|
//! | `Null`                      | `Null`                      |
//! | `Bool`                      | `Bool`                      |
//! | `Byte` / `Short` / `Int`    | `Integer`                   |
//! | `Long`                      | `Integer`                   |
//! | `UByte` / `UShort` / `UInt` | `Integer`                   |
//! | `ULong` (fits `i64`)        | `Integer`                   |
//! | `ULong` (>`i64::MAX`)       | `Text` (decimal stringified)|
//! | `Float` / `Double`          | `Float`                     |
//! | `Str`                       | `Text`                      |
//! | `Decimal` / `Date` / `Timestamp*` / `Bytes` / `Group` / list / map | `Text` (parquet's `Display` form) |
//!
//! Typed dates / decimals / nested types are deliberately flattened
//! to `Text` in v0.2 — a future `dates` feature could carry typed
//! dates, and a `nested` feature could expose lists/maps as their
//! own variants. Round-tripping through `Text` keeps the JSON-IPC
//! contract simple.

use crate::{infer_column_type, Column, Error, ReadOptions, Reader, Result, Row, Table, Value};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use std::path::Path;

/// Parquet reader. Construct via [`ParquetReader::new`] (cannot
/// fail — pure-Rust, no runtime dependency to verify).
#[derive(Default)]
pub struct ParquetReader;

impl ParquetReader {
    /// Construct a reader. Cannot fail.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Reader for ParquetReader {
    fn extensions(&self) -> &[&'static str] {
        &["parquet"]
    }

    fn name(&self) -> &'static str {
        "parquet"
    }

    fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table> {
        let file = std::fs::File::open(path)?;
        let reader = SerializedFileReader::new(file)
            .map_err(|e| Error::ParseError(format!("parquet open failed: {e}")))?;

        let metadata = reader.metadata();
        let file_meta = metadata.file_metadata();
        let row_count = file_meta.num_rows();
        let schema = file_meta.schema();

        // Top-level field names. parquet schemas can nest (groups,
        // maps, lists) — we surface the top-level field name and
        // flatten the nested cell content into `Text` per the
        // mapping table in the module docs. v0.2 keeps the public
        // contract uniform with the calamine + csv readers; richer
        // nested-type support could land behind a future `nested`
        // feature.
        let column_names: Vec<String> = schema
            .get_fields()
            .iter()
            .map(|f| f.name().to_string())
            .collect();

        // Read the first N rows. parquet's row iterator is lazy
        // under the hood, so `take(N)` doesn't read more than N
        // rows from disk.
        let row_iter = reader
            .get_row_iter(None)
            .map_err(|e| Error::ParseError(format!("parquet row-iter failed: {e}")))?;

        let mut sample_rows: Vec<Row> = Vec::with_capacity(options.max_sample_rows);
        for row_result in row_iter.take(options.max_sample_rows) {
            let row = row_result
                .map_err(|e| Error::ParseError(format!("parquet row read failed: {e}")))?;
            let mut cells: Row = row
                .get_column_iter()
                .map(|(_, field)| field_to_value(field))
                .collect();
            // Defensive padding: if a row in the file is shorter
            // than the schema (shouldn't happen for well-formed
            // parquet, but guard anyway) we fill with nulls.
            while cells.len() < column_names.len() {
                cells.push(Value::Null);
            }
            cells.truncate(column_names.len());
            sample_rows.push(cells);
        }

        // Per-column type inference over the sample. parquet's own
        // schema carries type info too; using the inferred type
        // keeps tabkit's contract uniform across readers (a column
        // typed `INT64` in parquet but always-null in the file
        // surfaces as `Unknown`, matching the calamine + csv
        // behavior).
        let columns: Vec<Column> = column_names
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let col_samples: Vec<Value> = sample_rows
                    .iter()
                    .map(|r| r.get(idx).cloned().unwrap_or(Value::Null))
                    .collect();
                let (data_type, nullable) = infer_column_type(&col_samples);
                Column {
                    name: name.clone(),
                    data_type,
                    nullable,
                }
            })
            .collect();

        let mut metadata_map = std::collections::HashMap::new();
        // num_rows can be -1 for streamed/unknown writers; clamp to
        // u64 with a sane fallback. The public contract is
        // "data row count when known."
        let public_row_count: u64 = u64::try_from(row_count).unwrap_or(0);
        metadata_map.insert(
            "num_row_groups".into(),
            metadata.num_row_groups().to_string(),
        );

        Ok(Table {
            columns,
            sample_rows,
            row_count: Some(public_row_count),
            metadata: metadata_map,
        })
    }
}

/// Translate one parquet `Field` cell into a tabkit `Value`. See
/// the module-level docs for the full mapping table.
fn field_to_value(field: &Field) -> Value {
    match field {
        Field::Null => Value::Null,
        Field::Bool(b) => Value::Bool(*b),
        Field::Byte(i) => Value::Integer(i64::from(*i)),
        Field::Short(i) => Value::Integer(i64::from(*i)),
        Field::Int(i) => Value::Integer(i64::from(*i)),
        Field::Long(i) => Value::Integer(*i),
        Field::UByte(i) => Value::Integer(i64::from(*i)),
        Field::UShort(i) => Value::Integer(i64::from(*i)),
        Field::UInt(i) => Value::Integer(i64::from(*i)),
        Field::ULong(i) => {
            // u64 values larger than i64::MAX (i.e. >9.2e18) lose
            // precision when cast to i64 — keep them as decimal
            // strings so the magnitude survives the round-trip.
            i64::try_from(*i).map_or_else(|_| Value::Text(i.to_string()), Value::Integer)
        }
        Field::Float(f) => Value::Float(f64::from(*f)),
        Field::Double(f) => Value::Float(*f),
        Field::Str(s) => Value::Text(s.clone()),
        // Decimal, Date, TimestampMillis, TimestampMicros, Bytes,
        // Group, MapInternal, ListInternal. parquet's `Display`
        // produces a reasonable text form for all of these (ISO-
        // ish dates, hex for bytes, JSON-ish for groups/maps/lists).
        // A future v0.4 `dates` feature could carry typed dates;
        // a future `nested` feature could expose lists/maps as
        // their own Value variants.
        other => Value::Text(format!("{other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_is_parquet_only() {
        assert_eq!(ParquetReader.extensions(), &["parquet"]);
    }

    #[test]
    fn name_identifies_backend() {
        assert_eq!(ParquetReader.name(), "parquet");
    }

    #[test]
    fn missing_file_returns_io_error() {
        let result = ParquetReader.read(Path::new("/nonexistent.parquet"), &ReadOptions::default());
        // std::fs::File::open surfaces NotFound as Io.
        assert!(matches!(result, Err(Error::Io(_))));
    }

    #[test]
    fn invalid_parquet_returns_parse_error() {
        use std::io::Write;
        let mut f = tempfile::Builder::new()
            .suffix(".parquet")
            .tempfile()
            .unwrap();
        f.write_all(b"this is not a parquet file").unwrap();
        f.flush().unwrap();
        let result = ParquetReader.read(f.path(), &ReadOptions::default());
        assert!(matches!(result, Err(Error::ParseError(_))));
    }

    #[test]
    fn field_to_value_covers_basic_types() {
        assert_eq!(field_to_value(&Field::Null), Value::Null);
        assert_eq!(field_to_value(&Field::Bool(true)), Value::Bool(true));
        assert_eq!(field_to_value(&Field::Int(42)), Value::Integer(42));
        assert_eq!(
            field_to_value(&Field::Long(-1_234_567_890)),
            Value::Integer(-1_234_567_890)
        );
        assert_eq!(field_to_value(&Field::Double(2.5)), Value::Float(2.5));
        assert_eq!(
            field_to_value(&Field::Str("hi".into())),
            Value::Text("hi".into())
        );
    }

    #[test]
    fn ulong_within_i64_range_stays_integer() {
        let small = Field::ULong(42);
        assert_eq!(field_to_value(&small), Value::Integer(42));
    }

    #[test]
    fn ulong_beyond_i64_max_falls_back_to_text() {
        // u64 max = 18446744073709551615; i64::MAX = 9223372036854775807.
        let huge = Field::ULong(u64::MAX);
        match field_to_value(&huge) {
            Value::Text(s) => assert_eq!(s, "18446744073709551615"),
            other => panic!("expected Text fallback for u64::MAX, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "requires a real Parquet file at tests/fixtures/sample.parquet"]
    fn extracts_schema_and_samples_from_real_parquet() {
        // Skipped by default. Drop a `sample.parquet` with a few
        // rows at tests/fixtures/, then:
        //   cargo test --features parquet -- --ignored
        let table = ParquetReader
            .read(
                Path::new("tests/fixtures/sample.parquet"),
                &ReadOptions::default(),
            )
            .expect("read failed");
        assert!(!table.columns.is_empty());
        assert!(!table.sample_rows.is_empty());
    }
}
