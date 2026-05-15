# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **M5 — API capability endpoints + CORS**
  - `GET /version` endpoint on `logdive-api` returning `version`,
    `formats` (ingest formats the binary was compiled with), and
    `capabilities` (available endpoint names) as a JSON object — designed
    for client-side feature detection.
  - `--cors-origins` flag on `logdive-api` (env: `LOGDIVE_API_CORS_ORIGINS`)
    accepting a comma-separated list of allowed origins. Defaults to
    disabled (same-origin only). Use `*` as the sole value to allow any
    origin. Invalid values or mixing `*` with specific origins cause a
    fast startup error.
  - `tower-http` `CorsLayer` wired into the router when origins are
    configured; GET-only, no credentials, preflight handled automatically.
  - `LogFormat::ALL` const on `logdive-core`'s `LogFormat` enum exposing
    all supported ingest format variants — used by `/version` and available
    to any downstream consumer.

- **M4 — `prune` subcommand + `LOGDIVE_DB` env var**
  - `logdive prune` subcommand removes entries older than a given duration
    (`--older-than 30d`) or before a specific datetime (`--before
    2026-01-01`); the two flags are mutually exclusive and exactly one is
    required.
  - Interactive `[y/N]` confirmation by default (shows the row count to be
    deleted); `--yes` bypasses.
  - `LOGDIVE_DB` environment variable accepted on the global `--db` flag;
    CLI flag takes precedence when both are provided.

- **M3 — Follow mode**
  - `logdive ingest --follow` tails a file for new lines, similar to
    `tail -f`. Detects log rotation (inode/device change) and truncation
    and reopens the file automatically.
  - Uses `notify` 6.1 for cross-platform filesystem events and `ctrlc` 3.5
    for clean Ctrl-C shutdown. CLI remains fully synchronous — no Tokio
    dependency added.
  - Starts at end-of-file (follow semantics, not from-start).
  - `--follow` requires `--file`; rejected at parse time when used with
    stdin.

- **M2 — logfmt and plain-text ingestion**
  - `--format json|logfmt|plain` flag on `logdive ingest` (default `json`).
  - logfmt parser: hand-written tokenizer supporting bareword booleans,
    escaped quotes, hyphenated/dotted keys, and last-write-wins on
    duplicate keys.
  - Plain parser: entire line becomes `message`; no timestamp or level
    extraction.
  - `--timestamp-now` flag assigns RFC 3339 UTC timestamps to entries that
    lack one, applicable to all three formats.

- **M1 — OR operator in query language**
  - Query language extended to support `OR` between `AND`-groups:
    `level=error OR level=warn`.
  - `AND` binds tighter than `OR`; SQL generation always parenthesises
    each AND-group: `WHERE (a=? AND b=?) OR (c=?)`.
  - Breaking change to `QueryNode`: replaced `And(Vec<Clause>)` with
    `Or(Vec<AndGroup>)` where `AndGroup { clauses: Vec<Clause> }`.

### Changed

- `build_router` in `logdive-api` gains a `cors_origins: Vec<HeaderValue>`
  parameter. Integration tests updated to pass `vec![]` (CORS disabled).

## [0.1.0] - 2026-04-19

### Added

- `logdive ingest` — ingest structured JSON logs from a file or stdin into
  a local SQLite index (`~/.logdive/index.db` by default).
- `logdive query` — query the index with a typed expression language
  supporting `=`, `!=`, `>`, `<`, `contains`, `last <duration>`, and
  `since <datetime>` operators combined with `AND`.
- `logdive stats` — display index metadata (entry count, time range, tags,
  DB size).
- `logdive-api` — read-only HTTP API server with `GET /query` (NDJSON) and
  `GET /stats` (JSON).
- SQLite-backed storage via `rusqlite` (bundled); hybrid schema with fixed
  columns for known fields and a JSON blob for unknown fields queryable via
  `json_extract()`.
- `blake3` row hashing for deduplication via `INSERT OR IGNORE`.
- Dual MIT OR Apache-2.0 license.