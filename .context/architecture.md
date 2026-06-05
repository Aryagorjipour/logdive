# architecture.md

## Crate map

```
crates/
  core/                        logdive-core — publishable library
    src/
      lib.rs                   Public re-exports; gated follow module
      entry.rs                 LogEntry struct + KNOWN_KEYS + with_tag()
      error.rs                 LogdiveError (thiserror), Result alias
      indexer.rs               Indexer (SQLite handle), InsertStats, PruneStats, Stats
                               db_path(), BATCH_SIZE=1000
      query.rs                 Tokenizer, AST (QueryNode/AndGroup/Clause), parse()
      executor.rs              build_sql(), translate_clause(), execute(), execute_at()
                               QueryOptions { limit: Option<usize>, offset: Option<usize> }
      parsers/
        mod.rs                 LogFormat enum, LogFormat::ALL, parse_line() dispatcher
        json.rs                JSON-object-per-line parser
        logfmt.rs              logfmt key=value parser (hand-written tokenizer)
        plain.rs               Whole-line → message parser
      follow.rs                FileTailer — Unix-only, #[cfg(unix)]
    tests/
      cross_format.rs          5 integration tests: dedup across formats
      proptest_query.rs        4 property-based tests: parser never panics
      security.rs              10 security tests: SQLi, LIKE injection, resource exhaustion
      functional.rs            28 functional tests: time-range, follow, API, prune boundary
    benches/
      bench_ingest.rs          Criterion: insert throughput
      bench_query.rs           Criterion: query latency at scale

  cli/                         logdive — CLI binary (crate name: logdive)
    src/
      main.rs                  Clap struct, handle_ingest, handle_query, run_watch_loop
      render.rs                OutputFormat (Pretty/Json), render(), ANSI + NO_COLOR
      stats_cmd.rs             run_stats() — wraps Indexer::stats(), formats output
      prune_cmd.rs             run_prune() — cutoff parsing, confirmation prompt
    tests/
      concurrent.rs            2 tests: two processes writing same DB simultaneously

  api/                         logdive-api — HTTP binary (crate name: logdive-api)
    src/
      lib.rs                   Module re-exports for test access
      main.rs                  Clap, ensure_index_exists, parse_cors_origins, axum::serve
                               --health-check flag: TcpStream::connect to own port, exit 0/1
      router.rs                build_router(), build_cors_layer()
      handlers.rs              query_handler, stats_handler, version_handler
                               StatsResponse, VersionResponse, QueryParams { q, limit, offset }
      error.rs                 AppError (maps LogdiveError → HTTP status codes)
      state.rs                 AppState { db_path }, with_connection()
    tests/
      integration.rs           21 end-to-end HTTP tests via tower::ServiceExt::oneshot
```

## Locked decisions

**SQLite via rusqlite (bundled)**
- What: single embedded database, no external process
- Why: zero runtime dependencies; ships inside the binary; battle-tested at
  internet scale; adequate performance for single-host log volume
- What breaks if changed: everything — all storage, query, dedup logic is SQL

**Hybrid schema — known fields as columns, rest in JSON blob**
- What: `timestamp`, `level`, `message`, `tag` are real indexed columns;
  everything else is serialized into `fields TEXT` and queried via
  `json_extract()`
- Why: indexed columns give sub-millisecond point lookups on the four most
  common filter fields; JSON blob makes the schema open — any log shape ingests
  without migration
- What breaks if changed: the executor's `column_for_field()` routing, all
  existing indexes, all existing stored data

**Hand-written recursive descent query parser**
- What: ~400 lines in `query.rs`; no parser-combinator library
- Why: the grammar is small and stable; a combinator library adds a compile-time
  dependency that outlives its usefulness; hand-written makes error messages and
  the grammar itself fully controllable
- What breaks if changed: nothing isolated, but any rewrite risks regressions in
  the 60+ parser tests

**blake3 row hashing → INSERT OR IGNORE on raw_hash UNIQUE**
- What: `raw_hash TEXT NOT NULL UNIQUE`; every insert tries INSERT OR IGNORE;
  duplicates are counted and silently dropped
- Why: re-ingesting a file (rotation recovery, repeated --follow startup)
  produces zero duplicates; no separate dedup pass needed
- What breaks if changed: dedup guarantee; 417 tests assert on InsertStats

**1000 rows per insert transaction (BATCH_SIZE)**
- What: `ingest_reader` batches parsed entries in chunks of 1000 before each
  `INSERT` transaction
- Why: SQLite transaction overhead is per-transaction not per-row; 1000 is
  empirically near the knee of the latency/throughput curve
- What breaks if changed: throughput numbers in README; InsertStats counting

**CLI fully synchronous, no tokio**
- What: `crates/cli/src/main.rs` has no `#[tokio::main]`; follow loop uses
  `notify` + `std::sync::mpsc` + `ctrlc`
- Why: ingest is I/O-bound sequential work; async adds complexity with no
  benefit; keeps compile time and binary size down
- What breaks if changed: binary size, compile time; must not add tokio

**API opens DB SQLITE_OPEN_READ_ONLY, fresh connection per request**
- What: `AppState::with_connection` calls `Indexer::open_read_only` on every
  request inside `spawn_blocking`
- Why: read-only is defense-in-depth; fresh connection avoids shared mutable
  state across requests without a mutex; `spawn_blocking` prevents blocking
  the async executor
- What breaks if changed: the read-only guarantee; concurrent request safety

**--follow is Unix-only**
- What: `crates/core/src/follow.rs` is gated `#[cfg(unix)]`; uses
  `std::os::unix::fs::MetadataExt` for `(dev, ino)` rotation detection
- Why: Windows rotation detection requires `ReadDirectoryChangesW`, deferred to v0.4+
- What breaks if changed: cross-platform compilation

**Query language v0.3 — AND + OR + parenthesised groups**
- What: grammar supports `or_expr := and_expr (OR and_expr)*`; clauses can now
  be `Clause::Group(Box<QueryNode>)` — a parenthesised sub-expression; executor
  wraps groups in a nested SQL sub-expression
- Why: shipped in v0.3.0; hand-written recursive descent parser extended with
  `parse_primary()` that recognises `(` and recurses into `parse_or_expr()`
- What breaks if changed: all 60+ query tests; public `QueryNode`/`Clause` enum shapes

**QueryOptions replaces bare limit**
- What: `execute(query, conn, opts: QueryOptions)` and `execute_at(query, conn, opts, now)`
  take `QueryOptions { limit: Option<usize>, offset: Option<usize> }` since v0.3.0
- Why: pagination requires both limit and offset; bundling in a struct avoids
  arity growth as options expand
- What breaks if changed: every call site in CLI, API, and all tests

## Schema

Exact DDL from `crates/core/src/indexer.rs`:

```sql
CREATE TABLE IF NOT EXISTS log_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,
    level       TEXT,
    message     TEXT,
    tag         TEXT,
    fields      TEXT,
    raw         TEXT NOT NULL,
    raw_hash    TEXT NOT NULL UNIQUE,
    ingested_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_level      ON log_entries(level);
CREATE INDEX IF NOT EXISTS idx_level_norm ON log_entries(lower(level));
CREATE INDEX IF NOT EXISTS idx_tag        ON log_entries(tag);
CREATE INDEX IF NOT EXISTS idx_timestamp  ON log_entries(timestamp);
```

Notes:
- `timestamp TEXT NOT NULL` — rows without a timestamp are skipped at ingest,
  never fabricated
- `fields TEXT` — serialized `serde_json::Map<String, Value>`; queried via
  `json_extract(fields, '$.fieldname')`
- `raw TEXT NOT NULL` — original unparsed line; used as blake3 hash input
- `raw_hash TEXT NOT NULL UNIQUE` — hex-encoded blake3 of `raw`; the dedup key
- `ingested_at` — wall-clock UTC at insert time; not exposed in query language
- `idx_level_norm` — functional expression index on `lower(level)`; added in v0.3.0;
  idempotent — `CREATE INDEX IF NOT EXISTS` runs on every `Indexer::open()` so
  existing databases pick it up automatically

## Query grammar (as-built)

From `crates/core/src/query.rs` implementation:

```
query     := or_expr [ TIME_RANGE ]
or_expr   := and_expr (OR and_expr)*
and_expr  := clause (AND clause)*
clause    := field OP value
           | field CONTAINS string
           | "(" or_expr ")"
           | TIME_RANGE
field     := [a-zA-Z_][a-zA-Z0-9_.]*
OP        := "=" | "!=" | ">" | "<"
value     := string | number | bool
string    := '"' .* '"' | bare_word
TIME_RANGE := "last" duration | "since" datetime
duration  := number ("m" | "h" | "d")
```

`AND` binds tighter than `OR`. Parentheses supported since v0.3.0. All keywords
case-insensitive.

Tokenizer is more permissive than the grammar: allows `-` and `:` inside idents
so bare-word datetime literals (`2024-01-01T10:00:00Z`) and hyphenated values
(`x-request-id`) tokenize correctly. `validate_field_name` in the parser then
enforces the stricter field-name regex `[a-zA-Z_][a-zA-Z0-9_.]*` — hyphens and
colons are allowed in *values* but not in *field names*.

The executor re-validates field names at `column_for_field()` (defense-in-depth)
before embedding them in `json_extract(fields, '$.fieldname')`.

Level field queries are routed through `lower(level) = ?` with Rust-lowercased
bind values, hitting the `idx_level_norm` functional index. This makes
`level=ERROR`, `level=Error`, and `level=error` all match the same rows.

## Dependency inventory

| Crate | Version | Used in | Why | Would replace with |
|---|---|---|---|---|
| rusqlite | 0.32 (bundled) | core | SQLite bindings; bundled = no system dep | Nothing — locked decision |
| blake3 | 1.5 | core | Fast non-cryptographic hash for dedup | Nothing — fast enough, stable API |
| serde | 1 (derive) | core, api | Serialization framework | Nothing |
| serde_json | 1 | core, cli, api | JSON parsing and serialization | Nothing |
| chrono | 0.4 (serde) | core, cli | DateTime parsing for `since` clauses | Nothing |
| thiserror | 1 | core | Error type derive macro | Nothing |
| tracing | 0.1 | core, cli, api | Structured logging macros | Nothing |
| tracing-subscriber | 0.3 (env-filter) | cli, api | Log subscriber, LOGDIVE_LOG env var | Nothing |
| clap | 4.5 (derive, env) | cli, api | CLI argument parsing | Nothing |
| anstream | 0.6 | cli | ANSI-aware output stream, NO_COLOR support | Nothing |
| anstyle | 1 | cli | ANSI style types | Nothing |
| notify | 6.1 | cli only | Filesystem event watcher for --follow | Nothing (--follow is Unix, notify is fine) |
| ctrlc | 3.5 | cli only | Ctrl-C / SIGINT handler for follow loop | tokio::signal if CLI ever adds tokio |
| axum | 0.7 | api | Async HTTP framework | Nothing |
| tokio | 1 (full) | api | Async runtime | Nothing |
| tower | 0.4 (util) | api | Service abstraction | Nothing |
| tower-http | 0.6 (cors) | api | CORS middleware | Nothing — must match axum/tower versions |
| tempfile | 3 | dev | Temp dirs in tests | Nothing |
| criterion | 0.5 | dev | Benchmarks | Nothing |
| proptest | 1 | dev | Property-based tests for query parser | Nothing |
| http-body-util | 0.1 | api dev | Body collection in integration tests | Nothing |
