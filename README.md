# tabkit

**Tabular files → schema + sample rows.** The shared spreadsheet
reader Tauri / Iced / native desktop apps reach for when they need
to introspect XLSX / CSV / TSV without inventing the same calamine-
plus-type-inference glue twice.

> **Status:** v0.3 — XLSX / XLS / XLSB / XLSM / ODS, CSV / TSV,
> Parquet (opt-in via `parquet` feature). Schema inference, sample
> row capping, header detection, ragged-row padding, AND typed
> `Date` / `DateTime` cells (ISO-8601 strings) emitted by all
> three readers. SQL-engine-backed SQL queries planned for v0.4 behind
> another opt-in feature.

## Why this exists

Every "show the user what's in their spreadsheet" project rebuilds
the same calamine wrapper, the same type-inference pass, the same
first-row-is-headers guess, the same ragged-row padding. Every
project gets it slightly wrong:

- Treats Excel's `Float(1.0)` as a Float, so a `qty` column that
  should infer to `Integer` ends up as `Float` in the schema.
- Forgets ragged rows, hands downstream code a `Vec<Vec<_>>` where
  rows have different lengths.
- Hard-codes `,` as the delimiter, breaks on `.tsv`.
- Reads the entire file into memory chasing a 'sample.'

`tabkit` ships these bits once, with the edge cases handled in one
place. It's deliberately **lower-level** than a full data tool — it
hands you a [`Table`] and gets out of the way. Pair it with
[`scankit`](https://crates.io/crates/scankit) for walk-and-watch
and [`mdkit`](https://crates.io/crates/mdkit) for documents →
markdown.

## Quick start

```rust
use tabkit::{Engine, ReadOptions};
use std::path::Path;

let engine = Engine::with_defaults();
let table = engine.read(
    Path::new("/Users/me/data/sales.xlsx"),
    &ReadOptions::default().max_sample_rows(10),
)?;

for col in &table.columns {
    println!("{} : {:?}", col.name, col.data_type);
}
for row in &table.sample_rows {
    println!("{row:?}");
}
# Ok::<(), tabkit::Error>(())
```

## Design principles

1. **Do one thing well.** Read tabular files → return `Table`.
   Anything richer (SQL, persistence, change tracking) is the
   consuming application's job.
2. **`Send + Sync` everywhere.** A single `Engine` shared across
   threads, a single `Reader` instance per format.
3. **JSON-friendly output.** `Value` has six narrow variants so
   the result serialises cleanly through Tauri IPC. Dates flatten
   to `Text` for now — a future `dates` feature could carry typed
   dates.
4. **Forward-compat defaults.** `Table`, `Column`, `Value`,
   `Error`, and `ReadOptions` are `#[non_exhaustive]` so we can
   add fields / variants without breaking downstream callers.
5. **Honest dep budget.** `calamine` + `csv` + `thiserror` are the
   only required deps. ~1 MB compiled with both default backends.

## Feature flags

| Feature | Adds | Approx. cost |
|---|---|---|
| `calamine` (default) | XLSX / XLS / XLSB / XLSM / ODS via `calamine` | ~600 KB compiled |
| `csv` (default) | CSV / TSV via the `csv` crate | ~100 KB compiled |
| `default` | both `calamine` + `csv` | ~700 KB compiled |
| `parquet` | Parquet via the `parquet` crate (default features off — no Arrow runtime) | ~3 MB compiled |
| `full` | `calamine` + `csv` + `parquet` | ~4 MB compiled |
| (planned) `sql-engine` | SQL queries on top of read tables | ~50 MB |
| (planned) `dates` | typed `Date` / `DateTime` variants on `Value` | <1 MB (chrono) |

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache 2.0](LICENSE-APACHE)
at your option. SPDX: `MIT OR Apache-2.0`.

## Status & roadmap

- [x] **v0.1 — schema + samples.** `Engine` + `Reader` trait +
      `Table` + `Column` + `Value`, calamine + csv backends, type
      inference (Bool / Integer / Float / Text / Unknown),
      header-or-not, sheet selection for multi-sheet XLSX, ragged
      row padding.
- [x] **v0.2 — `parquet` feature.** Apache Parquet read support
      via the `parquet` crate (default features off — no Arrow
      runtime). Same schema-and-samples surface, same type-
      inference rules.
- [x] **v0.3 — typed dates.** `Value::Date(String)` /
      `Value::DateTime(String)` with ISO-8601 string payloads
      (no chrono dep). All three readers emit typed dates for
      source values that carry date semantics. `Value` is now
      `#[non_exhaustive]` for forward-compat.
- [ ] v0.4 — SQL feature (optional SQL query interface on top
      of any read table; opt-in because a SQL engine is a ~50 MB dep).
- [ ] v0.5 — audit pass + first stable trait release (1.0
      candidate).

Issues, PRs, and design discussion welcome at
<https://github.com/seryai/tabkit/issues>.

## Used by

`tabkit` was extracted from the schema-extraction layer of
[Sery Link][sery], a privacy-respecting data network for the files
on your machines. If you use `tabkit` in your project, please open
a PR to add yourself here.

## The kit family

`tabkit` is part of a coordinated suite of focused single-purpose
Rust crates extracted from Sery Link:

- [`mdkit`](https://crates.io/crates/mdkit) — documents → markdown
  (PDF, DOCX, PPTX, HTML, IPYNB, OCR).
- [`scankit`](https://crates.io/crates/scankit) — walk + watch
  directory trees with exclude-glob and size-cap filters.
- **`tabkit`** — spreadsheets → schema + sample rows (this crate).

Use them together, use them separately. The trait surfaces are
designed to compose without forcing a particular runtime.

## Acknowledgements

- [`calamine`](https://crates.io/crates/calamine) — `tafia`'s
  industry-standard Rust XLSX/XLS/ODS parser. Does the heavy
  lifting for the `calamine` feature.
- [`csv`](https://crates.io/crates/csv) — `BurntSushi`'s
  battle-tested CSV reader. The fast path for CSV/TSV.

[sery]: https://sery.ai
