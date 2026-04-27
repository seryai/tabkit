# Changelog

All notable changes to tabkit are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and tabkit
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

tabkit is pre-1.0 — the public API surface (`Engine`, `Reader`,
`Table`, `Column`, `Value`, `Error`) is intended to stay stable, but
minor versions may introduce additive changes to feature flags and
auxiliary types until 1.0 lands.

## [Unreleased]

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
- **Why no `parquet` / `duckdb` backends in v0.1.** Both add
  significant compile-time + binary-size cost; consumers reading
  only XLSX/CSV shouldn't pay for them. Planned for v0.2 / v0.3
  behind opt-in features.

[Unreleased]: https://github.com/seryai/tabkit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/seryai/tabkit/releases/tag/v0.1.0
