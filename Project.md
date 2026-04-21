# logdive

## Goal

> A single self-contained binary that ingests structured JSON logs from any source, indexes them locally, and lets engineers query them instantly from the CLI or HTTP — no infrastructure, no setup, no cloud.

## Why this exists

Every backend engineer eventually hits the same wall: your app is producing JSON logs, something went wrong in production, and your options are `grep`, `jq` chained into an unreadable pipe, or spinning up a full observability stack (Loki, Datadog, Elastic) that requires infrastructure, cost, and configuration you don't have time for in a side project or small team environment.

`logdive` sits in the gap. It's a single Rust binary you drop anywhere. You point it at a log file — or pipe Docker output into it — and you get a fast, queryable index on your local machine. You can ask it `level=error AND service=payments last 2h` and get results in milliseconds. You can expose a lightweight HTTP endpoint so a minimal UI or a curl script can query it remotely.

The target user is a backend engineer who wants `jq` with memory, filters, and time ranges — without YAML files, without a running daemon they didn't ask for, without a monthly bill.

Rust makes this credible: single binary with no runtime, zero-copy parsing, SQLite for persistence, and real concurrency for ingestion pipelines. This is exactly what the language is built for.

---

## Decisions log

|Date|Decision|Reasoning|
|---|---|---|
|2026-04-19|Use SQLite via `rusqlite` (not `sqlx`) for the core index|`sqlx` requires async and a running pool — overkill for an embedded local store. `rusqlite` is synchronous, battle-tested, zero-infrastructure, and ships inside the binary via `bundled` feature. Fits the "no setup" goal.|
|2026-04-19|Workspace with three crates: `core`, `cli`, `api`|`core` is a pure library — no I/O, fully unit testable, potentially publishable to crates.io independently. `cli` and `api` are thin consumers. This enforces clean boundaries and makes the HTTP layer optional.|
|2026-04-19|Hand-written recursive descent query parser (no parser combinator library)|The query language is intentionally small. A hand-written parser using Rust enums is ~200 lines, teaches the real skill, has zero extra dependencies, and produces far better error messages than generated parsers.|
|2026-04-19|Target structured JSON logs only (no plaintext, no logfmt in v1)|Scope control. JSON is the dominant format for modern backend logs (Docker, structured loggers in Go/Rust/Node). Plaintext and logfmt can be v2 with a `--format` flag.|
|2026-04-19|`clap` with derive macros for CLI|Industry standard, integrates cleanly with Rust's type system, generates `--help` automatically, and the derive style keeps `main.rs` clean.|
|2026-04-19|HTTP API is a separate binary (`api` crate), not a CLI flag|Keeps the CLI binary lean. Users who only want the CLI don't pull in Axum and Tokio. The API binary is opt-in — built separately, deployed separately.|
|2026-04-19|Default index location: `~/.logdive/index.db`|Follows XDG-style convention, predictable, survives across working directory changes, easy to expose as a `--db` override flag.|
|2026-04-19|Hash-based deduplication on ingestion using `blake3`|Hash each raw line, store as a unique column. Re-ingesting the same file uses `INSERT OR IGNORE`. Eliminates duplicate data from log rotation or accidental double-ingestion at the cost of one hash per line — negligible.|
|2026-04-19|AND-only query language in v1, OR deferred to v2|OR requires precedence handling and two-level grammar, complicating both the parser and SQL generation significantly. AND-only covers the dominant query pattern. OR ships in v2.|
|2026-04-19|Hybrid storage: fixed columns + JSON blob for unknown fields|Known fields (`timestamp`, `level`, `message`, `tag`) are real indexed columns. Everything else is stored in a `fields TEXT` JSON blob, queryable via SQLite's `json_extract()`. Flattening unknown keys into columns is impossible for a generic tool.|
|2026-04-19|Output formats: `pretty` and `json` only|`pretty` is the default human-readable colored output. `json` is newline-delimited for piping into `jq` or scripts. `tsv` is redundant — json covers that use case.|
|2026-04-19|`--tag` flag on ingestion is optional, defaults to null|Required tags would break stdin piping in scripts. Null tag means "untagged" — queries without a tag filter match across all sources uniformly.|
|2026-04-19|HTTP API is read-only: `GET /query` and `GET /stats` only|Ingestion is the CLI's job. Accepting ingestion over HTTP introduces authentication requirements explicitly out of scope for v1. The API exists for querying — serving a future UI or remote curl usage.|
|2026-04-19|License: MIT OR Apache-2.0 dual|Rust ecosystem standard used by `tokio`, `serde`, `clap`, `axum`. MIT satisfies permissive users, Apache-2.0 adds patent protection. One line in `Cargo.toml`.|

---

## Tasks

### Phase 0 — Project Setup

- [ ] Initialize Cargo workspace with `core`, `cli`, `api` crates
- [ ] Set up shared `Cargo.toml` with workspace-level dependency versions
- [ ] Add `.env.example`, `.gitignore`, `README.md` skeleton
- [ ] Add `LICENSE-MIT` and `LICENSE-APACHE` files, set `license = "MIT OR Apache-2.0"` in `Cargo.toml`
- [ ] Set up GitHub repository with description and topics (`rust`, `logs`, `cli`, `observability`)

### Phase 1 — Core Library (`crates/core`)

- [ ] Define `LogEntry` struct: `timestamp`, `level`, `message`, `tag`, `raw`, `fields: HashMap<String, Value>`
- [ ] Write `parser.rs`: line-by-line JSON parsing with graceful skip on malformed lines
- [ ] Write `indexer.rs`: SQLite schema, `blake3` row hashing, `INSERT OR IGNORE`, batch insert per 1000 lines, `--db` path resolution
- [ ] Define `Query` enum and AST types in `query.rs`
- [ ] Write recursive descent parser: tokenizer → AST (AND-only, `=`, `!=`, `>`, `<`, `contains`, time ranges)
- [ ] Write query executor: AST → SQL `WHERE` clause builder using `json_extract()` for unknown field queries
- [ ] Unit tests for parser (valid JSON, malformed lines, missing fields)
- [ ] Unit tests for query parser (all operators, time ranges, edge cases)
- [ ] Unit tests for query executor (AST to SQL string correctness)

### Phase 2 — CLI (`crates/cli`)

- [ ] Set up `clap` with subcommands: `ingest`, `query`, `stats`
- [ ] Implement `ingest`: file path arg + stdin support + optional `--tag` flag + `--db` override
- [ ] Implement `query`: query string arg + `--format pretty|json` flag + `--db` override
- [ ] Implement `stats`: show index size, entry count, tags, time range of indexed data + `--db` override
- [ ] Progress output for large file ingestion (entries/sec, lines skipped)
- [ ] End-to-end test: ingest a sample log file, query it, assert results

### Phase 3 — HTTP API (`crates/api`)

- [ ] Set up Axum router with `GET /query` and `GET /stats`
- [ ] Shared `AppState` with db path (read-only handle)
- [ ] `GET /query?q=...&format=json` — run query against index, return newline-delimited JSON array
- [ ] `GET /stats` — return index metadata as JSON
- [ ] Error types implementing `IntoResponse` with correct status codes
- [ ] Integration tests with `tower::ServiceExt::oneshot`

### Phase 4 — OSS Polish

- [ ] README with install instructions, usage examples, full query language reference
- [ ] Sample log files in `examples/` for new users to try immediately
- [ ] Benchmark: ingestion throughput (lines/sec), query latency by result set size
- [ ] GitHub Actions CI: `cargo test`, `cargo clippy`, `cargo fmt --check`
- [ ] `cargo build --release` binary size check (target: under 10MB)
- [ ] Publish `logdive-core` to crates.io
- [ ] Cut v0.1.0 release with compiled binaries for Linux x86_64, macOS arm64

---

## Resources & links

**Rust crates**

- [`rusqlite`](https://docs.rs/rusqlite) — SQLite bindings, use `features = ["bundled"]`
- [`blake3`](https://docs.rs/blake3) — fast hashing for deduplication
- [`clap`](https://docs.rs/clap) — CLI framework, use derive macros
- [`serde_json`](https://docs.rs/serde_json) — JSON parsing, `Value` type for unknown fields
- [`chrono`](https://docs.rs/chrono) — timestamp parsing and time range arithmetic
- [`axum`](https://docs.rs/axum) — HTTP API layer (Phase 3)
- [`tracing`](https://docs.rs/tracing) — structured logging inside logdive itself

**Reference implementations to study**

- [`lnav`](https://lnav.org) — terminal log viewer (feature reference, not architecture)
- [`jql`](https://github.com/yamafaktory/jql) — JSON query CLI in Rust (query UX reference)
- [`ripgrep`](https://github.com/BurntSushi/ripgrep) — gold standard for Rust CLI performance and output UX

**Learning references**

- [Crafting Interpreters](https://craftinginterpreters.com) — recursive descent parser chapter (free online) — read before writing `query.rs`
- [SQLite JSON functions](https://www.sqlite.org/json1.html) — `json_extract()` reference for field queries
- [SQLite query planning](https://www.sqlite.org/queryplanner.html) — relevant once index grows large
- [Cargo workspaces guide](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)

---

## Notes

**Query language grammar (v1)**

```
query     := clause (AND clause)*
clause    := field OP value
           | field CONTAINS string
           | TIME_RANGE
field     := [a-zA-Z_][a-zA-Z0-9_.]*
OP        := "=" | "!=" | ">" | "<"
value     := string | number | bool
string    := '"' .* '"' | bare_word
TIME_RANGE := "last" duration | "since" datetime
duration  := number ("m" | "h" | "d")
```

**SQLite schema**

```sql
CREATE TABLE log_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,
    level       TEXT,
    message     TEXT,
    tag         TEXT,
    fields      TEXT,  -- JSON blob for all unknown keys, queryable via json_extract()
    raw         TEXT NOT NULL,
    raw_hash    TEXT NOT NULL UNIQUE,  -- blake3 hash for deduplication
    ingested_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_level     ON log_entries(level);
CREATE INDEX idx_tag       ON log_entries(tag);
CREATE INDEX idx_timestamp ON log_entries(timestamp);
```

**Naming**

- CLI binary: `logdive`
- Core library crate: `logdive-core`
- API binary: `logdive-api`
- Default DB path: `~/.logdive/index.db`

**v1 non-goals**

- Log shipping / agents / daemons
- Multi-machine / networked indexes
- Authentication on the HTTP API
- Real-time log tailing (follow mode)
- Non-JSON log formats (plaintext, logfmt)
- OR operator in queries
- A browser UI (curl and CLI only in v1)
- Ingestion over HTTP
