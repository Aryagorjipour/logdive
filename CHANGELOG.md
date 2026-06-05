# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-06-05

### Added

- **M1 — Parenthesized query groups**
  - `(` `)` grouping in query language: `(level=error OR level=warn) AND service=payments`.
  - `Clause::Group(Box<QueryNode>)` variant added to query AST; parser and executor updated.

- **M2 — CLI pagination and output flag rename**
  - `logdive query --limit N` (unchanged default 1000; `0` = unlimited).
  - `logdive query --offset N` (new; skips leading results for page navigation).
  - `--format` on `logdive query` renamed to `--output` (`pretty|json`) to disambiguate from `ingest --format` (input format). **Breaking CLI flag change.**
  - `QueryOptions { limit, offset }` struct in `logdive-core` replaces bare `limit: Option<usize>` on `execute` / `execute_at`. **Breaking library change.**

- **M3 — HTTP pagination**
  - `GET /query?offset=<n>` query parameter added to `logdive-api`. Mirrors CLI `--offset` semantics: absent or `0` starts from the first result.

- **M4 — Case-insensitive level queries**
  - Expression index `idx_level_norm ON log_entries(lower(level))` added to schema; idempotent — existing databases pick it up on next `Indexer::open()`.
  - `level=ERROR`, `level=Error`, and `level=error` all match the same rows.
  - `level contains "ERR"` matches rows whose stored level is `"error"`.

- **M5 — Distroless Docker runtime**
  - Runtime stage changed from `debian:bookworm-slim` to `gcr.io/distroless/cc-debian12:nonroot`.
  - No shell, no curl, no root user in the final image.
  - `logdive-api --health-check` flag: TCP-connects to own port via stdlib `TcpStream` and exits 0/1 — works in distroless without any shell or HTTP client.
  - `HEALTHCHECK CMD ["/usr/local/bin/logdive-api", "--health-check"]` replaces the previous curl-based check.

### Breaking (library)

- `execute(query, conn, opts: QueryOptions)` — third parameter changed from `Option<usize>` (limit only) to `QueryOptions { limit, offset }` (M2).
- `execute_at(query, conn, opts: QueryOptions, now)` — same change (M2).

### Breaking (CLI)

- `logdive query --format` renamed to `logdive query --output` (M2).

## [0.2.1] - 2026-06-01

### Added

- **H1 — Security tests** (`crates/core/tests/security.rs`, 10 tests)
  - SQL injection via field name: tokenizer rejects `'`, `;`, Unicode
    lookalikes (`U+2019`) — parse fails before any SQL is generated.
  - SQL injection via value: bound parameters prevent `DROP TABLE` and
    `1=1` tautology payloads from affecting results.
  - LIKE wildcard escaping: `_` and `\` in `contains` queries match
    literally, not as SQL wildcards or escape characters.
  - Resource exhaustion: 1 000-disjunct OR query parses and executes
    without stack overflow (iterative `parse_or_expr`). 10 MB raw line
    ingested without panic or OOM.

- **H2 — Functional tests** (28 new tests across 7 suites)
  - Property-based (`proptest`): arbitrary input never panics; valid
    equality queries produce single-group ASTs; OR disjunct count matches
    input; printable ASCII in quoted values never panics.
  - Cross-format dedup: same raw line ingested twice is one row; JSON
    vs. logfmt with identical logical content are two distinct rows.
  - Concurrent CLI ingest: two `logdive ingest` processes on the same
    database produce no corruption and dedup is respected.
  - Parser edge cases: UTF-8 BOM prefix rejected; deeply nested object
    in a known field preserved in `fields` map; whitespace in known
    field value preserved verbatim.
  - Time-range: space-separator datetime accepted; boundary row at
    cutoff is included (`>=`); far-future timestamp returns empty; very
    large `last` duration saturates to epoch; UTC offset `+00:00`
    equivalent to `Z`.
  - Follow-mode: file deleted after open returns `Ok([])`, not error;
    burst of appended lines read completely in one call.
  - API integration: `limit` larger than match count returns all
    matches; `contains` operator; `since` time-range; CORS preflight
    with wildcard origin returns `access-control-allow-origin: *`; `raw`
    field present in every response entry.
  - Prune boundary: 1-second precision — entry at cutoff not deleted;
    idempotent second prune with same cutoff deletes nothing.

- **H3 — Supply-chain hardening**
  - `Cargo.lock` tracked for reproducible builds and deterministic audit
    scans.
  - `deny.toml`: `cargo-deny` configuration with license allowlist
    (MIT / Apache-2.0 / Apache-2.0 WITH LLVM-exception / BSD-2-Clause /
    BSD-3-Clause / ISC / CC0-1.0 / MIT-0 / Unicode-3.0 / Unlicense /
    Zlib / BSL-1.0), RUSTSEC advisory checks (`vulnerability = deny`,
    `unsound = deny`), and crates.io-only source policy.
  - `scripts/audit.sh`: `cargo-audit` runner; exits non-zero on any
    known vulnerability.
  - `scripts/sbom.sh`: generates a CycloneDX JSON SBOM via
    `cargo-cyclonedx`; intended for release artifact attachment.
  - `.github/workflows/audit.yml`: daily advisory scan and `cargo deny
    check` (informational; does not block merges).
  - `.github/workflows/ci.yml`: `permissions: contents: read` added;
    `cargo deny check` added to the lint job (merge-blocking).

### Changed

- `LogEntry::with_tag` signature changed from `Option<String>` to
  `Option<&str>`, eliminating a `String` clone per ingested entry at
  both CLI ingest call sites (`ingest_reader` and `ingest_lines`).
- `entry_to_json_string` in `logdive-api` now serialises directly via
  `serde_json::to_string(&entry)` instead of constructing a `json!`
  macro value with `entry.fields.clone()`. Eliminates an O(fields)
  heap allocation per HTTP response row.

## [0.2.0] - 2026-05-15

### Added

- **M6 — Docker image + multi-arch**
  - Multi-stage `Dockerfile` (cargo-chef caching, `debian:bookworm-slim`
    runtime) publishing both `logdive` and `logdive-api` binaries in a
    single image.
  - Default `ENTRYPOINT ["logdive-api"]`; CLI accessible via
    `--entrypoint logdive`.
  - `ENV LOGDIVE_DB=/data/index.db` and `ENV LOGDIVE_API_HOST=0.0.0.0`
    set sane container defaults without modifying binary source.
  - `VOLUME ["/data"]` and `EXPOSE 4000` declared.
  - `HEALTHCHECK` on `GET /version` (30 s interval, 5 s start period).
  - Non-root system user `logdive` (UID/GID 1000).
  - GitHub Actions workflow (`.github/workflows/docker.yml`): `linux/amd64`
    + `linux/arm64` via `docker buildx` + QEMU; GHA cache (`type=gha`,
      `mode=min`) for BuildKit layers; GHCR push via `GITHUB_TOKEN` (no PAT);
      semver tags on `v*` push, branch tags on `main`/`release/v*`,
      build-only (no cache write) on PRs.
  - `logdive-api` auto-creates an empty index with initialized schema on
    first run when the database file is absent, including any missing parent
    directories. Genuinely bad paths still surface as startup failures.

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
    all supported ingest format variants.

- **M4 — `prune` subcommand + `LOGDIVE_DB` env var**
  - `logdive prune` subcommand removes entries older than a given duration
    (`--older-than 30d`) or before a specific datetime (`--before
    2026-01-01`); the two flags are mutually exclusive and exactly one is
    required.
  - Interactive `[y/N]` confirmation by default (shows the row count to be
    deleted); `--yes` bypasses.
  - `LOGDIVE_DB` environment variable accepted on the global `--db` flag
    for both `logdive` and `logdive-api`; CLI flag takes precedence when
    both are provided.

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

### Changed

- `build_router` in `logdive-api` gains a `cors_origins: Vec<HeaderValue>`
  parameter (M5). Integration tests updated to pass `vec![]` (CORS disabled).

### Breaking (library)

- `QueryNode::And(Vec<Clause>)` replaced by `QueryNode::Or(Vec<AndGroup>)`
  where `AndGroup { clauses: Vec<Clause> }` (M1). Even single-clause
  queries are wrapped in the two-level structure.
- `parse_line` signature changed from `(line: &str)` to
  `(format: LogFormat, line: &str)` (M2).

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