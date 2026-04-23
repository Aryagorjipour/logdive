# Changelog

All notable changes to logdive will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Released]

## [0.1.0] - 2026-04-23

Initial release.

### Added

- `logdive` CLI binary with three subcommands:
  - `ingest` — read structured JSON logs from a file or stdin, parse, deduplicate via blake3 row hashing, and insert batched entries into a SQLite-backed index. Supports `--file` / stdin piping, `--tag` for applying source tags, `--db` for overriding the default `~/.logdive/index.db` location. TTY-aware progress output, summary statistics at completion.
  - `query` — parse a query expression and render matching log entries. Supports `--format pretty|json` (NDJSON for `jq`-friendly piping) and `--limit N` (default 1000, `0` = unlimited). Pretty output is colored, honors `NO_COLOR`, and auto-strips ANSI when piped.
  - `stats` — report aggregate metadata: entry count, time range, distinct tags, DB file size. Refuses to run against a missing index rather than auto-creating one.
- `logdive-api` HTTP server binary:
  - `GET /query?q=<expr>&limit=<n>` — newline-delimited JSON responses matching the CLI's JSON format, `Content-Type: application/x-ndjson`.
  - `GET /stats` — JSON metadata response with decoupled wire shape (`StatsResponse`) separate from the core library's `Stats` type.
  - Read-only access via `SQLITE_OPEN_READ_ONLY` — defense-in-depth against writes.
  - Configurable via CLI flags with environment-variable fallbacks (`LOGDIVE_DB`, `LOGDIVE_API_PORT`, `LOGDIVE_API_HOST`). Defaults to `127.0.0.1:4000`.
  - Graceful shutdown on Ctrl-C and SIGTERM (Unix).
- `logdive-core` library crate:
  - Structured JSON log parsing (`parse_line`), tolerant of malformed lines (graceful skip, not panic).
  - SQLite-backed `Indexer` with batched inserts (1000 rows per transaction), `INSERT OR IGNORE` deduplication on blake3 row hashes, and in-memory / on-disk / read-only opener variants.
  - Hand-written recursive descent query parser (AND-only grammar for v1 per decisions log). Operators: `=`, `!=`, `>`, `<`, `contains`, `last Nm/Nh/Nd`, `since <datetime>`.
  - Query executor translates AST to parameterized SQL, uses `json_extract()` for unknown fields, `LIKE ? ESCAPE '\'` for `CONTAINS`. Results ordered newest-first.
  - Unified `LogdiveError` via `thiserror`.
- Full query language reference and usage documentation in README.
- Sample log files in `examples/` for new users.

### Notes

- MSRV: Rust 1.85 (edition 2024).
- License: MIT OR Apache-2.0 (dual).
- Binary size target: under 10MB per release binary (Linux x86_64, macOS arm64).

[Unreleased]: https://github.com/Aryagorjipour/logdive/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Aryagorjipour/logdive/releases/tag/v0.1.0
