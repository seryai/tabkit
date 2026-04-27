//! # tabkit — tabular files → schema + sample rows.
//!
//! `tabkit` is the shared spreadsheet reader that Tauri / Iced /
//! native desktop apps reach for when they need to introspect
//! XLSX / CSV / TSV files without reinventing the same calamine-
//! plus-type-inference glue twice.
//!
//! Its job is small but easy to get wrong:
//!
//! 1. Open a tabular file by extension (XLSX, CSV, TSV, ODS, ...).
//! 2. Read enough of it to produce a schema (column name + inferred
//!    data type) and a sample of rows (first N).
//! 3. Hand those back as a [`Table`] — typed enough for the
//!    downstream UI to render and the downstream agent to reason
//!    about, but JSON-friendly so it serialises cleanly across the
//!    Tauri IPC boundary.
//!
//! What `tabkit` deliberately does NOT do:
//!
//! - SQL queries. v0.2 may add an optional DuckDB-backed query
//!   feature; v0.1 stops at schema + samples.
//! - Full table iteration. Use the underlying crate
//!   ([`calamine`](https://crates.io/crates/calamine),
//!   [`csv`](https://crates.io/crates/csv)) directly if you need
//!   to stream every row.
//! - Persistence, caching, change tracking. Those are the
//!   consuming application's concerns — pair `tabkit` with
//!   [`scankit`](https://crates.io/crates/scankit) for
//!   walk-and-watch and persist however suits.
//!
//! ## Quick start
//!
//! ```no_run
//! use tabkit::{Engine, ReadOptions};
//! use std::path::Path;
//!
//! let engine = Engine::with_defaults();
//! let table = engine.read(
//!     Path::new("/Users/me/data/sales.xlsx"),
//!     &ReadOptions::default().max_sample_rows(10),
//! )?;
//!
//! for col in &table.columns {
//!     println!("{} : {:?}", col.name, col.data_type);
//! }
//! for row in &table.sample_rows {
//!     println!("{row:?}");
//! }
//! # Ok::<(), tabkit::Error>(())
//! ```
//!
//! ## Why a separate crate
//!
//! Every "show the user what's in their spreadsheet" project
//! rebuilds the same calamine wrapper, the same type-inference
//! pass, the same first-row-is-headers guess, the same
//! ragged-row padding. `tabkit` ships the bits once with the
//! edge cases (empty sheets, headerless CSVs, mixed-type
//! columns) handled in one place.

#![doc(html_root_url = "https://docs.rs/tabkit")]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::path::Path;

mod error;
pub use error::{Error, Result};

#[cfg(feature = "calamine")]
mod calamine;
#[cfg(feature = "calamine")]
pub use crate::calamine::CalamineReader;

#[cfg(feature = "csv")]
mod csv;
#[cfg(feature = "csv")]
pub use crate::csv::CsvReader;

#[cfg(feature = "parquet")]
mod parquet;
#[cfg(feature = "parquet")]
pub use crate::parquet::ParquetReader;

// ---------------------------------------------------------------------------
// Table — the unit of output
// ---------------------------------------------------------------------------

/// One file's worth of structured tabular content.
///
/// `#[non_exhaustive]` so we can grow the struct (add `row_count`,
/// `sheet_names`, `metadata`) in minor versions without breaking
/// external struct-literal construction.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Table {
    /// One per column, in source order. Header inferred from the
    /// first row when not configured otherwise.
    pub columns: Vec<Column>,
    /// First N rows of data (after the header). N is bounded by
    /// [`ReadOptions::max_sample_rows`]; the source may have far
    /// more rows that aren't surfaced here.
    pub sample_rows: Vec<Row>,
    /// Total row count (excluding the header) when known, `None`
    /// when the backend can't compute it without a full scan and
    /// the caller didn't ask for one.
    pub row_count: Option<u64>,
    /// Backend-specific metadata. Stable keys are documented per-
    /// backend; callers should treat unknown keys as opaque. Common
    /// keys: `"sheet"` (for multi-sheet XLSX), `"delimiter"` (for
    /// CSV/TSV).
    pub metadata: std::collections::HashMap<String, String>,
}

/// One column's name + inferred type. `nullable` is `true` if any
/// sample row had a missing/empty value in this position.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Column {
    /// Column header. Falls back to `column_<idx>` when the
    /// underlying file has no header row or the cell is empty.
    pub name: String,
    /// Type inferred from the sample rows. `Unknown` when every
    /// sample row's cell was empty/null in this position.
    pub data_type: DataType,
    /// `true` if any sample row had a null/empty cell here.
    pub nullable: bool,
}

/// Coarse-grained data types the inference pass produces. Designed
/// to round-trip through JSON, so dates are surfaced as `Text`
/// (ISO-8601 string) rather than carrying a chrono dependency. A
/// future `dates` feature could add `Date` / `DateTime` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DataType {
    /// `true`/`false`, or backend-specific representations
    /// (`TRUE`/`FALSE`, `1`/`0`-when-bool-coded, etc.).
    Bool,
    /// Whole-number values that fit in `i64`.
    Integer,
    /// Decimal / floating-point values.
    Float,
    /// Free-text strings. Default when we can't pin to a more
    /// specific type — date/time strings, mixed-type columns, and
    /// empty-but-not-null columns all land here.
    Text,
    /// Every sample row's cell was empty or null in this position.
    /// The column exists; we just couldn't infer a type.
    #[default]
    Unknown,
}

/// One row of sampled data. `Row[i]` corresponds to
/// [`Table::columns`]`[i]`. Length is always equal to the column
/// count, with `Value::Null` filling positions where the source
/// row was shorter than the header.
pub type Row = Vec<Value>;

/// One cell value. Keep the variants narrow — anything richer
/// (dates, decimals with arbitrary precision, embedded formulas)
/// degrades to `Text` so callers don't have to handle a
/// combinatorial explosion of types. Round-trips cleanly through
/// `serde_json::Value` for any caller that adds `serde`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Empty / missing cell.
    Null,
    /// Truthy / falsy cell.
    Bool(bool),
    /// Whole number.
    Integer(i64),
    /// Decimal / floating-point.
    Float(f64),
    /// Anything else, including dates, times, currencies,
    /// formulas, and mixed-type cells. Caller decides how to
    /// parse further.
    Text(String),
}

impl Value {
    /// Best-guess data type for a single cell. Used by the
    /// per-column inference pass — an *Integer* and a *Float* in
    /// the same column promote to `Float`; an *Integer* and a
    /// *Text* promote to `Text`; nulls don't constrain inference.
    #[must_use]
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Self::Null => None,
            Self::Bool(_) => Some(DataType::Bool),
            Self::Integer(_) => Some(DataType::Integer),
            Self::Float(_) => Some(DataType::Float),
            Self::Text(_) => Some(DataType::Text),
        }
    }
}

// ---------------------------------------------------------------------------
// ReadOptions — the policy
// ---------------------------------------------------------------------------

/// Per-call read configuration. Construct via
/// [`ReadOptions::default`] then layer on with the builder methods.
///
/// `#[non_exhaustive]` for forward-compat — same reasoning as
/// [`Table`] / [`Column`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReadOptions {
    /// Maximum number of rows to surface in [`Table::sample_rows`].
    /// Default: 100. Bounded so callers don't accidentally pull
    /// 10M rows into memory chasing a 'sample.'
    pub max_sample_rows: usize,
    /// For multi-sheet XLSX/ODS, which sheet to read. `None` =
    /// the first non-empty sheet.
    pub sheet_name: Option<String>,
    /// `true` (default) treats the first row as column headers.
    /// `false` generates `column_0`, `column_1`, ... names and
    /// includes the first row in `sample_rows`.
    pub has_header: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_sample_rows: 100,
            sheet_name: None,
            has_header: true,
        }
    }
}

impl ReadOptions {
    /// Override the maximum sample-row count.
    #[must_use]
    pub fn max_sample_rows(mut self, n: usize) -> Self {
        self.max_sample_rows = n;
        self
    }

    /// Select a specific sheet by name. Only meaningful for
    /// multi-sheet formats (XLSX, ODS); a no-op on CSV/TSV.
    #[must_use]
    pub fn sheet_name(mut self, name: impl Into<String>) -> Self {
        self.sheet_name = Some(name.into());
        self
    }

    /// Toggle header-row treatment. Defaults to `true`.
    #[must_use]
    pub fn has_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }
}

// ---------------------------------------------------------------------------
// Reader — the per-format trait
// ---------------------------------------------------------------------------

/// A backend that knows how to read one or more tabular formats.
/// Implementors register themselves with an [`Engine`].
///
/// `Send + Sync` so engines can be shared across threads. All
/// methods take `&self` so implementors can wrap their internals
/// in `Arc<Mutex<...>>` if they need interior state.
pub trait Reader: Send + Sync {
    /// Lowercase file extensions this reader handles, **without**
    /// the leading dot (e.g. `&["xlsx", "xls"]`).
    fn extensions(&self) -> &[&'static str];

    /// Open and read the file at `path` per the supplied options.
    fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table>;

    /// Human-readable backend name for diagnostics
    /// (`"calamine"`, `"csv"`).
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ---------------------------------------------------------------------------
// Engine — the dispatcher
// ---------------------------------------------------------------------------

/// Dispatches `read` calls to the registered [`Reader`] for the
/// file's extension. Construct with [`Engine::new`] for an empty
/// engine, or [`Engine::with_defaults`] for the readers matching
/// enabled feature flags.
pub struct Engine {
    readers: Vec<Box<dyn Reader>>,
}

impl Engine {
    /// New engine with no readers registered.
    #[must_use]
    pub fn new() -> Self {
        Self {
            readers: Vec::new(),
        }
    }

    /// New engine with the default readers for enabled feature
    /// flags. With both `calamine` and `csv` features on (the
    /// default), this engine handles XLSX/XLS/XLSB/XLSM/ODS plus
    /// CSV/TSV out of the box.
    #[must_use]
    pub fn with_defaults() -> Self {
        #[allow(unused_mut)]
        let mut engine = Self::new();
        #[cfg(feature = "calamine")]
        {
            engine.register(Box::new(CalamineReader::new()));
        }
        #[cfg(feature = "csv")]
        {
            engine.register(Box::new(CsvReader::new()));
        }
        #[cfg(feature = "parquet")]
        {
            engine.register(Box::new(ParquetReader::new()));
        }
        engine
    }

    /// Register a backend. Multiple backends can claim the same
    /// extension; the first registered wins on dispatch (so you
    /// can override defaults by registering your own reader first).
    pub fn register(&mut self, reader: Box<dyn Reader>) -> &mut Self {
        self.readers.push(reader);
        self
    }

    /// Returns the number of registered readers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.readers.len()
    }

    /// Returns true when no readers are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }

    /// Read `path` as a [`Table`], dispatching by file extension.
    pub fn read(&self, path: &Path, options: &ReadOptions) -> Result<Table> {
        let ext = extension_of(path).ok_or_else(|| {
            Error::UnsupportedFormat(format!("no file extension on {}", path.display()))
        })?;
        let reader = self
            .find(&ext)
            .ok_or_else(|| Error::UnsupportedFormat(format!("no reader registered for .{ext}")))?;
        reader.read(path, options)
    }

    fn find(&self, ext: &str) -> Option<&dyn Reader> {
        self.readers
            .iter()
            .find(|r| r.extensions().contains(&ext))
            .map(std::convert::AsRef::as_ref)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|os| os.to_str())
        .map(str::to_ascii_lowercase)
}

// ---------------------------------------------------------------------------
// Type inference — used by all readers
// ---------------------------------------------------------------------------

/// Promote inferred types as new sample values arrive.
///
/// Rules (most-specific to most-general):
/// - All-null → `Unknown`
/// - All same type → that type
/// - `Integer` + `Float` → `Float`
/// - `Bool` + anything-else → `Text`
/// - Anything-else mixed → `Text`
#[cfg_attr(
    not(any(feature = "calamine", feature = "csv", feature = "parquet")),
    allow(dead_code)
)]
pub(crate) fn infer_column_type(samples: &[Value]) -> (DataType, bool) {
    let mut current: Option<DataType> = None;
    let mut nullable = false;
    for v in samples {
        match v.data_type() {
            None => nullable = true,
            Some(t) => {
                current = Some(match current {
                    None => t,
                    Some(c) if c == t => c,
                    Some(DataType::Integer) if t == DataType::Float => DataType::Float,
                    Some(DataType::Float) if t == DataType::Integer => DataType::Float,
                    _ => DataType::Text,
                });
            }
        }
    }
    (current.unwrap_or(DataType::Unknown), nullable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_rejects_all_files() {
        let engine = Engine::new();
        let result = engine.read(Path::new("anything.xlsx"), &ReadOptions::default());
        assert!(matches!(result, Err(Error::UnsupportedFormat(_))));
    }

    #[test]
    fn missing_extension_is_a_clean_error() {
        let engine = Engine::with_defaults();
        let result = engine.read(Path::new("/no-extension"), &ReadOptions::default());
        assert!(matches!(result, Err(Error::UnsupportedFormat(_))));
    }

    #[test]
    fn read_options_builders_chain() {
        let opts = ReadOptions::default()
            .max_sample_rows(50)
            .sheet_name("Q1")
            .has_header(false);
        assert_eq!(opts.max_sample_rows, 50);
        assert_eq!(opts.sheet_name.as_deref(), Some("Q1"));
        assert!(!opts.has_header);
    }

    #[test]
    fn infer_all_integers_yields_integer_not_nullable() {
        let samples = vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)];
        assert_eq!(infer_column_type(&samples), (DataType::Integer, false));
    }

    #[test]
    fn infer_int_plus_float_promotes_to_float() {
        let samples = vec![Value::Integer(1), Value::Float(2.5)];
        assert_eq!(infer_column_type(&samples), (DataType::Float, false));
    }

    #[test]
    fn infer_int_plus_text_falls_back_to_text() {
        let samples = vec![Value::Integer(1), Value::Text("two".into())];
        assert_eq!(infer_column_type(&samples), (DataType::Text, false));
    }

    #[test]
    fn infer_with_null_marks_nullable() {
        let samples = vec![Value::Integer(1), Value::Null, Value::Integer(2)];
        assert_eq!(infer_column_type(&samples), (DataType::Integer, true));
    }

    #[test]
    fn infer_all_null_is_unknown() {
        let samples = vec![Value::Null, Value::Null];
        assert_eq!(infer_column_type(&samples), (DataType::Unknown, true));
    }

    #[test]
    fn empty_samples_default_to_unknown() {
        assert_eq!(infer_column_type(&[]), (DataType::Unknown, false));
    }
}
