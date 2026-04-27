# Changelog

All notable changes to tabkit are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and tabkit
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

tabkit is pre-1.0 — the public API surface (`Engine`, `Reader`,
`Table`, `Column`, `Value`, `Error`) is intended to stay stable, but
minor versions may introduce additive changes to feature flags and
auxiliary types until 1.0 lands.

## [Unreleased]

## [0.4.3] — 2026-04-27

### Changed

- **Generalised SQL-engine references throughout the docs.**
  v0.4.x release notes and README sections previously named a
  specific embedded SQL engine as the recommended composition
  partner. The architectural advice was right; naming a specific
  crate was overspecified. README's "Composing with X" section
  becomes "When you need SQL"; the "Why no SQL feature"
  section becomes "Why no SQL feature"; the deferred-roadmap
  rationale stays intact but no longer prescribes a particular
  engine. Pick whichever SQL crate fits your runtime — the
  composition shape is the same.

### Notes

- Docs-only release; no API or behavior changes. Bump from
  v0.4.2 with no code changes required.

## [0.4.2] — 2026-04-27

### Added

- **`Table::new(columns, sample_rows)`** and
  **`Column::new(name, data_type, nullable)`** — constructors for
  the two `#[non_exhaustive]` structs. Required because external
  crates implementing a custom [`Reader`] can't construct via
  struct-literal syntax once `#[non_exhaustive]` is on the type.
  Surfaced when writing `examples/custom_reader.rs` and
  immediately filled — a real API gap, not just example
  ergonomics.
- **`examples/custom_reader.rs`** — implements the `Reader` trait
  for a toy semicolon-separated `.ssv` format, demonstrating the
  registration pattern + the new `Table::new` / `Column::new`
  constructors. Run with:
  ```bash
  cargo run --example custom_reader -- /path/to/data.ssv
  ```
- **SQL-composition pattern** — documented in
  [README §"When you need SQL"](README.md#when-you-need-sql)
  rather than shipped as a runnable example. A prototype example
  was tried during v0.4.2 development but the bundled SQL engine
  introduced a Windows linker headache that wasn't worth the
  upkeep for a code snippet the README already covers.

### Notes

- The `Table::new` / `Column::new` additions are **API growth**,
  not API change. Existing callers reading produced tables (the
  default path) keep using struct field access; only callers
  *constructing* tables / columns from outside tabkit need the
  new methods. v0.4-stability commitments still hold — no
  variants removed, no signatures changed.
- See [README §"Why no SQL feature"](README.md#why-no-sql-feature)
  for why SQL engines stay out of tabkit's runtime surface
  entirely — composition is cleaner than nesting.

## [0.4.1] — 2026-04-27

### Added

- **`examples/inspect.rs`** — runnable CLI that takes a path to a
  tabular file and prints schema + sample rows. Demonstrates the
  v0.1 `Engine::read` surface, the v0.3 typed `Date` / `DateTime`
  cells, and (with `--features parquet`) the v0.2 Parquet path.
  Run with:
  ```bash
  cargo run --example inspect -- /path/to/data.xlsx
  cargo run --example inspect --features parquet -- /path/to/data.parquet
  ```
- README "Examples" section pointing at the new `examples/`
  directory.

### Notes

- v0.4.1 is the first of v0.4.x's planned "examples + cookbook"
  iteration. Future releases will add a `custom_reader.rs` example
  (showing how to implement the `Reader` trait for a new format)
  and SQL-composition documentation (the recommended pattern for
  pairing tabkit with a SQL engine).
- Examples are deliberately dep-light: no `clap` for arg parsing,
  no `serde` for output. Reading the surface should not require
  wading through unrelated crate ceremony.

## [0.4.0] — 2026-04-27

### API stability candidate (1.0 prep)

v0.4 is the **API stability candidate** for 1.0. Format coverage
closed in v0.3 — calamine + csv + parquet readers, typed
`Date` / `DateTime` cells. v0.4 freezes the public surface ahead
of 1.0 and locks in SemVer commitments. v0.4.x can iterate on
examples, docs polish, and niche reader additions without
changing the public API shape.

### Added

- **Stability section in `lib.rs` module docs** explicitly
  enumerates what's covered by the API freeze (Reader trait,
  Engine dispatch surface, Table / Column / Value / Error /
  ReadOptions / DataType field+variant sets, feature flag names,
  backend `name()` strings) and what stays implementation detail
  (private reader layout, exact Table.metadata key sets per
  backend, auto-registration order).
- README mirrors the same Stability section.

### Changed

- **No API-shape changes.** v0.4.0 is intentionally a
  documentation-only release. `#[non_exhaustive]` was already in
  place on every public struct + enum (added incrementally across
  v0.1 → v0.3); `#[must_use]` was already on every constructor +
  builder + accessor. The audit confirmed no gaps.

### Migration

For most callers: bump the dep, rebuild, ship. Zero code changes
required.

### Notes

- **Why no SQL feature.** Documented in
  [README §"Why no SQL feature"](README.md#why-no-sql-feature):
  dep weight (embedded SQL engines are tens of MB compiled),
  scope creep, duplicate-reader confusion, cleaner composition
  with whichever SQL crate fits the consumer's runtime. README
  has a brief "When you need SQL" section showing the
  composition shape.
- v0.4.x will iterate on **examples** (`examples/` directory),
  **cookbook**-style docs ("how to write a custom reader",
  "extending the Engine"), and any **niche backend polish** that
  doesn't change the public surface.
- 1.0 will be cut once the API is exercised by at least one
  downstream production user. Sery Link is the canonical
  integration target.

## [0.3.0] — 2026-04-27

### Added

- **Typed `Date` / `DateTime` cell values.** `Value` gains
  `Date(String)` and `DateTime(String)` variants whose payloads
  are ISO-8601 strings (`YYYY-MM-DD` / `YYYY-MM-DDTHH:MM:SS[.fff]
  [±HH:MM|Z]`). No `chrono` dep — the contract is "the string
  parses as ISO-8601"; callers wanting typed dates parse the
  string with `chrono::NaiveDate::parse_from_str` (or equivalent).
- **`DataType::Date` / `DataType::DateTime`** join the inferred
  type set. Type-promotion rule for date widening: `Date + DateTime
  → DateTime`. All other date-mixing falls back to `Text`.
- **All three readers emit typed dates** for source values that
  carry date semantics:
  - **calamine**: `Data::DateTime` → `Value::DateTime`;
    `Data::DateTimeIso` → `Value::Date` if the string matches
    `YYYY-MM-DD` exactly, otherwise `Value::DateTime`.
  - **csv**: rigid pattern-match on cell strings —
    `YYYY-MM-DD` (10 chars, dashes at positions 4 / 7) → `Date`;
    `YYYY-MM-DDTHH:MM:SS` or `YYYY-MM-DD HH:MM:SS` (≥19 chars,
    optional fractional + timezone tail) → `DateTime`. Other date
    dialects (`MM/DD/YYYY`, etc.) fall through to `Text` — caller
    can post-process.
  - **parquet**: `Field::Date(i32)` → `Value::Date` (parquet's
    own `Display` impl emits `YYYY-MM-DD`); `Field::TimestampMillis`
    / `TimestampMicros` → `Value::DateTime` (parquet's `Display`
    emits `YYYY-MM-DD HH:MM:SS`; we replace the space separator
    with `T` to conform to our ISO contract).

### Changed

- **`#[non_exhaustive]` on `Value`** so future variants
  (`Decimal`, `Time` for date-less time-of-day, etc.) can land in
  minor versions without breaking downstream `match` blocks.
  External pattern-matchers must include a wildcard arm.
- The `infer_column_type` promotion rules are extracted into a
  small `promote()` helper. Same outputs as v0.2; just easier to
  read once date-widening was added.

### Migration

For most callers: bump the dep, rebuild. The only breaking change
is `#[non_exhaustive]` on `Value` — callers exhaustively matching
on `Value` in another crate need a wildcard arm:

```rust
match value {
    Value::Null => ...,
    Value::Bool(b) => ...,
    // ... existing variants
    _ => panic!("new tabkit Value variant — check the changelog"),
}
```

Callers that consumed `Date` / `DateTime` cells as `Value::Text`
will now see them as `Value::Date` / `Value::DateTime` instead.
Both carry `String` payloads; the migration is `match value
{ Value::Text(s) | Value::Date(s) | Value::DateTime(s) => s }`.

### Notes

- 5 new csv tests covering: ISO-8601 date detection, datetime
  with `Z` / `.fff` / `±HH:MM`, space-separated form, rejection
  of `MM/DD/YYYY` and other non-ISO dialects, column-level Date
  inference, Date+DateTime widening to DateTime.

## [0.2.0] — 2026-04-27

### Added

- **`ParquetReader`** — Apache Parquet read support, gated behind
  the new `parquet` feature. Backed by the
  [`parquet`](https://crates.io/crates/parquet) crate (default
  features off — we don't need the Arrow runtime, async reader, or
  CLI helpers for the schema-and-samples surface).
- **`parquet` feature flag** — opt-in. Not part of `default` so
  consumers reading only XLSX/CSV don't pay the extra ~3 MB
  compile cost. The `full` feature enables it alongside calamine
  + csv.
- **Parquet `Field` → tabkit `Value` mapping** documented in the
  module-level docs as a table. Highlights:
  - `Byte` / `Short` / `Int` / `Long` and `UByte` / `UShort` /
    `UInt` → `Integer`
  - `ULong` (≤ `i64::MAX`) → `Integer`; `ULong` (> `i64::MAX`) →
    `Text` (decimal stringified, so the magnitude survives the
    JSON round-trip)
  - `Float` / `Double` → `Float` (lossless `f32`→`f64` widening)
  - `Decimal` / `Date` / `Timestamp*` / `Bytes` / `Group` / list /
    map → `Text` (parquet's `Display` form). Typed dates land in
    a future `dates` feature; nested types in a future `nested`
    feature.
- **Parquet metadata** surfaced via `Table.metadata`:
  `num_row_groups` (parquet's row-group count, useful for
  diagnostics on large files).
- 5 new unit tests covering: extensions, name, missing-file →
  `Error::Io`, invalid-content → `Error::ParseError`, basic
  field-to-value mapping, `ULong` overflow → `Text` fallback.

### Notes

- **Why not the full Arrow runtime?** The `parquet` crate's
  default feature set pulls in `arrow-array` + `arrow-buffer` +
  several other Arrow crates that together weigh ~10 MB compiled.
  tabkit's row-level reader API doesn't need any of that. If a
  future tabkit feature wants to expose Arrow-typed batches
  (e.g. for zero-copy hand-off to a SQL engine), that'd be its
  own feature with the heavier dep set.
- **`row_count` semantics** match the calamine + csv readers:
  `Some(n)` when known, where `n` excludes any header. Parquet
  has no header concept — every row is data — so `n` is the
  whole-file row count.
- **Streamed/unknown writers**: parquet's `num_rows` can be `-1`
  in some edge cases. We clamp to `0` rather than surfacing a
  signed integer in the public contract.

## [0.1.0] — 2026-04-27

### Added

- Initial release. Establishes the crate name on crates.io and the
  public API surface.
- **`Engine`** — the dispatcher. Routes `read(path, options)` calls
  to the registered `Reader` for the file's extension.
- **`Reader` trait** — per-format integration point. Implementors
  declare `extensions()`, `name()`, `read(path, options)`.
- **`Table`** — the unit of output. `columns` + `sample_rows` +
  optional `row_count` + backend-specific `metadata`.
- **`Column`** — name + inferred `data_type` + `nullable` flag.
- **`Value`** — six narrow variants: `Null` / `Bool` / `Integer` /
  `Float` / `Text`. JSON-round-trippable for clean Tauri IPC.
- **`DataType`** — `Bool` / `Integer` / `Float` / `Text` / `Unknown`.
  Type inference promotes Integer + Float → Float, anything-mixed
  → Text, all-null → Unknown.
- **`ReadOptions`** — `max_sample_rows` (default 100), `sheet_name`
  (multi-sheet XLSX picker), `has_header` (default `true`).
- **`CalamineReader`** — XLSX / XLS / XLSB / XLSM / ODS via the
  [`calamine`](https://crates.io/crates/calamine) crate. Detects
  whole-number floats (Excel stores `1` as `Float(1.0)`) and
  demotes them to `Integer` for the schema.
- **`CsvReader`** — CSV / TSV via the
  [`csv`](https://crates.io/crates/csv) crate. Tab vs comma
  selected by extension. Tolerates ragged rows (pads with
  `Value::Null`). Handles empty header cells (falls back to
  `column_<idx>`). Headerless mode generates `column_<i>` names.
- **Typed `Error` enum** — `Io` / `UnsupportedFormat` /
  `ParseError` / `SheetNotFound`. `#[non_exhaustive]` for
  forward-compat.
- **Feature flags pre-declared**:
  - `calamine` (default) — XLSX-family reader
  - `csv` (default) — CSV/TSV reader
  - `full` — both
- 30 unit tests covering: type inference (all-int, int+float,
  int+text, with-null, all-null, empty), CSV happy path, headerless
  mode, ragged rows, sample cap, Excel float-to-int demotion,
  sheet not-found, missing files, parse-cell rules.
- Dual-licensed under MIT OR Apache-2.0 (Rust ecosystem
  convention).
- CI workflow on Ubuntu + macOS + Windows (stable Rust + MSRV
  1.85 + clippy + rustfmt + cargo-audit gates) — same template
  as `mdkit` and `scankit`.
- `CONTRIBUTING.md`, `SECURITY.md` for repo hygiene.

### Notes

- **Why `unsafe_code = forbid` (not `deny`).** No FFI surface;
  every backend is pure Rust. Same posture as `scankit`.
- **Why MSRV 1.85.** Matches the kit-family floor — single MSRV
  across `mdkit` / `scankit` / `tabkit` so downstream Tauri apps
  don't manage divergent toolchains.
- **Why no `parquet` backend in v0.1.** Adds significant
  compile-time + binary-size cost; consumers reading only XLSX/
  CSV shouldn't pay for it. Planned for v0.2 behind an opt-in
  feature flag.

[Unreleased]: https://github.com/seryai/tabkit/compare/v0.4.3...HEAD
[0.4.3]: https://github.com/seryai/tabkit/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/seryai/tabkit/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/seryai/tabkit/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/seryai/tabkit/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/seryai/tabkit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/seryai/tabkit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/seryai/tabkit/releases/tag/v0.1.0
