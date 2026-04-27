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
//! - SQL queries. Different consumers want different SQL
//!   engines; pick yours and call it directly. tabkit stops at
//!   schema + samples.
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
//!
//! ## Stability commitment (v0.4+)
//!
//! v0.4 marks the **API stability candidate** for 1.0. The
//! following surface is committed to and will only change with a
//! major version bump:
//!
//! - The [`Reader`] trait shape — required methods, default
//!   implementations, `Send + Sync` bound. Future trait methods
//!   will land with default impls so existing implementors don't
//!   break.
//! - [`Engine`] construction + dispatch — `new`, `with_defaults`,
//!   `register`, `read`, `len`, `is_empty`.
//! - [`Table`] field set + the [`ReadOptions`] builder methods.
//!   Marked `#[non_exhaustive]` so we can add fields without major
//!   bumps.
//! - [`Column`], [`DataType`], [`Value`], [`Error`] enums + structs.
//!   All `#[non_exhaustive]` for the same forward-compat reason —
//!   pattern-matchers must include a wildcard arm.
//! - Feature flag names: `calamine`, `csv`, `parquet`, `full`.
//!   Each backend's per-format extension list (`xlsx` / `csv` /
//!   `parquet` / etc.) is also stable.
//! - Per-reader `name()` strings (`"calamine"`, `"csv"`,
//!   `"parquet"`) — used by callers for filtering / logging.
//!
//! The following are **implementation details** and may change in
//! minor versions:
//!
//! - The internal layout of any specific reader (private fields,
//!   helper methods, type-inference heuristics).
//! - The exact set of `Table.metadata` keys per backend (new keys
//!   may appear; documented keys stay).
//! - The auto-registration order in [`Engine::with_defaults`] (the
//!   fact that the first registered wins for overlapping
//!   extensions stays; the specific order doesn't).
//!
//! 1.0 will be cut once the API is exercised by at least one
//! downstream production user. [Sery Link](https://sery.ai) is
//! the canonical integration target.

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

impl Table {
    /// Construct a `Table` with the given columns and sample rows.
    /// `row_count` defaults to `None`; `metadata` to empty.
    /// Mutate the fields after construction if you need to set
    /// either — the fields stay `pub`.
    ///
    /// External crates implementing a custom [`Reader`] go through
    /// `Table::new` instead of struct-literal syntax (`Table` is
    /// `#[non_exhaustive]`).
    #[must_use]
    pub fn new(columns: Vec<Column>, sample_rows: Vec<Row>) -> Self {
        Self {
            columns,
            sample_rows,
            row_count: None,
            metadata: std::collections::HashMap::new(),
        }
    }
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

impl Column {
    /// Construct a `Column` from name + inferred type + nullability.
    /// Required because `Column` is `#[non_exhaustive]` so external
    /// crates implementing a custom [`Reader`] can't construct via
    /// struct-literal syntax.
    #[must_use]
    pub fn new(name: impl Into<String>, data_type: DataType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable,
        }
    }
}

/// Coarse-grained data types the inference pass produces. Designed
/// to round-trip through JSON.
///
/// `#[non_exhaustive]` so we can grow the enum (e.g. add a
/// dedicated `Decimal` once we have a place to put one) without
/// breaking external matches. Always include a wildcard arm.
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
    /// Calendar date (no time component). Backends emit this when
    /// the source format flags a column as date-typed (calamine's
    /// `DateTime`, parquet's `Date`, ISO-8601 `YYYY-MM-DD` strings
    /// in CSV).
    Date,
    /// Date + time (with optional sub-second precision). Backends
    /// emit this for calamine `DateTime`, parquet `TimestampMillis`/
    /// `TimestampMicros`, ISO-8601 datetime strings in CSV.
    DateTime,
    /// Free-text strings. Default when we can't pin to a more
    /// specific type — mixed-type columns, decimals with arbitrary
    /// precision, formula results, etc. all land here.
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
/// (decimals with arbitrary precision, embedded formulas, currency
/// types) degrades to `Text` so callers don't have to handle a
/// combinatorial explosion of types. Round-trips cleanly through
/// `serde_json::Value` for any caller that adds `serde`.
///
/// `#[non_exhaustive]` so we can grow the enum (a future
/// `Decimal` variant, for instance) without breaking external
/// matches. Always include a wildcard arm when matching.
///
/// **Date / `DateTime` payloads are ISO-8601 strings.** v0.3
/// surfaces dates as their canonical text representation rather
/// than carrying a `chrono` dependency in the public API; callers
/// that need typed dates parse the string with `chrono::NaiveDate::parse_from_str`
/// (or equivalent). A future `dates` feature could carry typed
/// values alongside the strings, but the contract here is stable:
/// `Value::Date(s)` always means `s` parses as ISO-8601
/// `YYYY-MM-DD`; `Value::DateTime(s)` means
/// `YYYY-MM-DDTHH:MM:SS[.fff][±HH:MM|Z]`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// Empty / missing cell.
    Null,
    /// Truthy / falsy cell.
    Bool(bool),
    /// Whole number.
    Integer(i64),
    /// Decimal / floating-point.
    Float(f64),
    /// ISO-8601 calendar date, `YYYY-MM-DD`.
    Date(String),
    /// ISO-8601 date + time. Format may include sub-second
    /// precision and a timezone designator.
    DateTime(String),
    /// Anything else: mixed-type cells, formula results, decimals
    /// outside `f64` precision, currency strings, etc. Caller
    /// decides how to parse further.
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
            Self::Date(_) => Some(DataType::Date),
            Self::DateTime(_) => Some(DataType::DateTime),
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
/// - `Integer` + `Float` → `Float` (numeric widening)
/// - `Date` + `DateTime` → `DateTime` (date widening)
/// - Anything else mixed → `Text`
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
                current = Some(promote(current, t));
            }
        }
    }
    (current.unwrap_or(DataType::Unknown), nullable)
}

/// Combine an existing column-level inferred type with the
/// type of a new cell. Pulled out so promotion rules live in one
/// readable place — the `match` was getting deep when we added
/// date-widening for v0.3.
fn promote(current: Option<DataType>, new: DataType) -> DataType {
    match (current, new) {
        (None, t) => t,
        (Some(c), t) if c == t => c,
        // Numeric widening: Integer + Float → Float.
        (Some(DataType::Integer), DataType::Float) | (Some(DataType::Float), DataType::Integer) => {
            DataType::Float
        }
        // Date widening: Date + DateTime → DateTime.
        (Some(DataType::Date), DataType::DateTime) | (Some(DataType::DateTime), DataType::Date) => {
            DataType::DateTime
        }
        // Anything else mixed → Text.
        _ => DataType::Text,
    }
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
