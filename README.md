# tabkit

**Tabular files → schema + sample rows.** The shared spreadsheet
reader Tauri / Iced / native desktop apps reach for when they need
to introspect XLSX / CSV / TSV without inventing the same calamine-
plus-type-inference glue twice.

> **Status:** v0.4 — **API stability candidate for 1.0**. Format
> coverage closed in v0.3 (XLSX-family + CSV/TSV + Parquet, with
> typed `Date` / `DateTime` cells). v0.4 freezes the public
> surface — see the [stability section](#stability-v04) below
> for what's locked in. v0.4.x will iterate on examples + cookbook
> docs. 1.0 ships once the API is exercised by at least one
> downstream production user.

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

## Stability (v0.4+) {#stability-v04}

v0.4 is the **API stability candidate** for 1.0. The following
surface is committed to and will only change with a major version
bump:

- The `Reader` trait shape — required methods, default
  implementations, `Send + Sync` bound. Future trait methods land
  with default impls so existing implementors don't break.
- `Engine` construction + dispatch — `new`, `with_defaults`,
  `register`, `read`, `len`, `is_empty`.
- `Table`, `Column`, `Value`, `DataType`, `Error`, `ReadOptions`
  field/variant sets. All `#[non_exhaustive]` so we can grow
  them without major bumps. **Pattern-matchers must include a
  wildcard arm.**
- Feature flag names: `calamine`, `csv`, `parquet`, `full`. Each
  reader's per-format extension list is also stable.
- Per-reader `name()` strings (`"calamine"`, `"csv"`,
  `"parquet"`).

The following are **implementation details** and may change in
minor versions:

- Internal layout of any specific reader (private fields, helper
  methods, type-inference heuristics).
- Exact set of `Table.metadata` keys per backend (new keys may
  appear; documented keys stay).
- Auto-registration order in `Engine::with_defaults` (the rule
  "first registered wins for overlapping extensions" stays; the
  specific order doesn't).

1.0 will be cut once the API is exercised by at least one
downstream production user.

## Composing with DuckDB {#composing-with-duckdb}

When you need SQL queries on tabular data, use [`duckdb`][duckdb]
directly — DuckDB has excellent native readers for CSV and Parquet,
and it's purpose-built for this. Use `tabkit` for "what's in the
file" (schema, samples, type inference for the UI / agent
grounding); use DuckDB for "compute over the data" (joins,
aggregates, projections):

```rust
// 1. tabkit for schema + samples (fast, lightweight)
let table = tabkit::Engine::with_defaults().read(path, &Default::default())?;
println!("columns: {:?}", table.columns);

// 2. DuckDB for the SQL surface (when you actually need it)
let conn = duckdb::Connection::open_in_memory()?;
let path_str = path.display();
conn.execute(
    &format!("CREATE TABLE t AS SELECT * FROM read_csv_auto('{path_str}')"),
    [],
)?;
let mut stmt = conn.prepare("SELECT region, SUM(amount) FROM t GROUP BY region")?;
// ...
```

Same composition shape works for XLSX (read with calamine, write
intermediate CSV/Parquet, query with DuckDB) and Parquet (DuckDB
reads natively).

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
- [x] **v0.4 — audit pass + 1.0 candidate.** Stability
      commitments doc in `lib.rs` + README. `#[non_exhaustive]`
      already on every public struct + enum (added incrementally
      v0.1 → v0.3); `#[must_use]` already on every constructor +
      builder + accessor. Documentation-only release — no API-
      shape changes.
- [ ] **v1.0** — once exercised by at least one downstream
      production user. Sery Link is the canonical integration
      target; v1.0 ships once the API survives real use without
      breaking changes.

### Why no `duckdb` feature

Earlier roadmaps mentioned a v0.x `duckdb` feature for SQL queries
on top of any read table. We dropped that plan because:

1. **Dep weight.** The bundled `duckdb` crate is ~50 MB compiled
   — tabkit's current ~4 MB would 13× by adding it. That violates
   the "small focused kit" aesthetic.
2. **Scope creep.** tabkit's contract is "schema + samples from a
   file." SQL queries are a fundamentally different abstraction:
   compute over data, not introspect a file.
3. **DuckDB has native CSV/Parquet readers.** A tabkit-DuckDB
   feature would duplicate functionality — users would have two
   readers for the same format and not know which to pick.
4. **Composition is cleaner.** See the next section.

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

[duckdb]: https://crates.io/crates/duckdb
[sery]: https://sery.ai
