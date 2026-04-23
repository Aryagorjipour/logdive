# logdive

**Fast, self-hosted query engine for structured JSON logs.**

[![CI](https://github.com/Aryagorjipour/logdive/actions/workflows/ci.yml/badge.svg)](https://github.com/Aryagorjipour/logdive/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/logdive.svg)](https://crates.io/crates/logdive)
[![Docs.rs](https://img.shields.io/docsrs/logdive-core)](https://docs.rs/logdive-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A single Rust binary that ingests structured JSON logs, indexes them locally in SQLite, and lets you query them instantly from the CLI or an HTTP API. No infrastructure, no daemons, no cloud.

```bash
# Ingest logs from a file or pipe from stdin.
logdive ingest --file ./logs/app.log
docker logs my-container | logdive ingest --tag my-container

# Query the index.
logdive query 'level=error AND service=payments last 2h'
logdive query 'message contains "timeout"' --format json

# Inspect the index.
logdive stats

# Optionally expose a read-only HTTP API for remote querying.
logdive-api --db ./logdive.db --port 4000
curl 'http://127.0.0.1:4000/query?q=level%3Derror&limit=100'
```

> **Status: v0.1.0 (early release).** Core feature set is complete and tested. Scope is deliberately small — see [v1 non-goals](#v1-non-goals) for what's explicitly out of scope.

---

## Table of contents

- [Why logdive](#why-logdive)
- [Install](#install)
- [Quick start](#quick-start)
- [The `logdive` CLI](#the-logdive-cli)
- [The `logdive-api` HTTP server](#the-logdive-api-http-server)
- [Query language reference](#query-language-reference)
- [Configuration reference](#configuration-reference)
- [Architecture](#architecture)
- [Performance](#performance)
- [Development](#development)
- [v1 non-goals](#v1-non-goals)
- [License](#license)

---

## Why logdive

Every backend engineer has hit the same wall: the app is producing JSON logs, something went wrong in production, and the options are `grep`, an unreadable chain of `jq` pipes, or spinning up a full observability stack (Loki, Datadog, Elastic) that requires infrastructure, cost, and configuration you don't have time for in a side project or small team.

logdive sits in the gap. It's a single binary you drop anywhere. Point it at a log file — or pipe Docker output into it — and you get a fast, queryable index on your local machine. You can ask it `level=error AND service=payments last 2h` and get results in milliseconds. You can expose a lightweight HTTP endpoint so a minimal UI or a curl script can query it remotely.

The target user is a backend engineer who wants `jq` with memory, filters, and time ranges — without YAML files, without a running daemon they didn't ask for, without a monthly bill.

---

## Install

logdive ships two binaries: `logdive` (CLI) and `logdive-api` (HTTP server). They share a database format; you can ingest with the CLI and serve queries over HTTP, or vice versa.

### From crates.io (Rust users)

```bash
cargo install logdive logdive-api
```

Both binaries land in `~/.cargo/bin/` — make sure it's on your `PATH`.

### From prebuilt binaries

Download the latest release for your platform from the [GitHub Releases](https://github.com/Aryagorjipour/logdive/releases) page. Binaries are currently built for:

- Linux x86_64
- macOS arm64

Extract the archive and move the binaries to any directory on your `PATH`.

### From source

```bash
git clone https://github.com/Aryagorjipour/logdive
cd logdive
cargo build --release
```

The compiled binaries will be at `target/release/logdive` and `target/release/logdive-api`.

MSRV: Rust 1.85 (edition 2024).

---

## Quick start

The `examples/` directory ships with two sample log files. Let's ingest them and run a few queries.

```bash
# Ingest both sample files into a throwaway database.
logdive --db /tmp/demo.db ingest --file examples/app.log
logdive --db /tmp/demo.db ingest --file examples/nginx.log

# See what we've got.
logdive --db /tmp/demo.db stats

# Find every error across both files.
logdive --db /tmp/demo.db query 'level=error'

# Find slow nginx requests.
logdive --db /tmp/demo.db query 'tag=nginx AND request_time > 1.0'

# Get structured output for further processing.
logdive --db /tmp/demo.db query 'service=payments' --format json | jq
```

See [`examples/README.md`](examples/README.md) for a longer walkthrough of what these files contain and what queries are interesting against them.

---

## The `logdive` CLI

Three subcommands: `ingest`, `query`, `stats`.

### `logdive ingest`

Reads newline-delimited JSON from a file or stdin and inserts it into the index.

```bash
logdive ingest --file ./logs/app.log
logdive ingest --file ./logs/app.log --tag production
docker logs my-container | logdive ingest --tag my-container
journalctl --output=json | logdive ingest --tag systemd
```

Flags:

- `--file <PATH>` / `-f` — Read from a file. Mutually exclusive with stdin.
- `--tag <TAG>` / `-t` — Attach a tag to every ingested entry whose JSON does not already contain a `tag` field. Useful for distinguishing sources.
- `--db <PATH>` — Override the default `~/.logdive/index.db` location (global, applies to all subcommands).

Behavior:

- **Deduplication**: Every row is fingerprinted with a blake3 hash. Re-ingesting the same file (or a log rotation producing overlapping lines) results in zero duplicate rows.
- **Graceful skip**: Malformed JSON lines are counted and skipped, not fatal. Blank lines are silently ignored.
- **No-timestamp skip**: Lines without a `timestamp` field are skipped rather than fabricated with an ingestion-time fallback — fabricating would corrupt `last Nh` queries.
- **Progress**: TTY-aware status on stderr. A final summary always prints inserted / deduplicated / no-timestamp / malformed counts.

### `logdive query`

Runs a query against the index and renders matching entries.

```bash
logdive query 'level=error'
logdive query 'level=error AND service=payments last 24h'
logdive query 'message contains "timeout"' --format json
logdive query 'since 2026-01-01' --limit 0
```

Flags:

- `--format pretty|json` — Output format. Default `pretty` (colored, human-readable). `json` is newline-delimited, pipe-friendly for `jq`.
- `--limit <N>` — Maximum results to return. Default `1000`. Use `0` for unlimited.
- `--db <PATH>` — Database path override.

Pretty output honors `NO_COLOR` and auto-strips ANSI when piped. JSON output is identical in shape to the HTTP API's `/query` response.

See the [Query language reference](#query-language-reference) for the full grammar and operator list.

### `logdive stats`

Reports aggregate metadata about the index.

```bash
logdive stats
```

Sample output:

```
logdive index: /home/user/.logdive/index.db
  Entries:       42,317
  Time range:    2026-03-14T08:22:01Z → 2026-04-22T19:45:03Z
  Tags:          api, nginx, payments, worker, (untagged)
  DB size:       8.4 MB (8,400,000 bytes)
```

Errors out (exit code 1) if the configured index file does not exist, rather than silently creating an empty one. This catches typos in `--db` paths early.

---

## The `logdive-api` HTTP server

A read-only HTTP server for remote querying. Useful when you want a browser-based UI, a CI check, or a shell one-liner hitting a centrally hosted index.

```bash
logdive-api --db ~/logdive.db --port 4000
```

Flags (with environment-variable fallbacks):

- `--db <PATH>` / `$LOGDIVE_DB` — Database to serve. Defaults to `~/.logdive/index.db`.
- `--port <N>` / `$LOGDIVE_API_PORT` — Port to listen on. Default 4000.
- `--host <HOST>` / `$LOGDIVE_API_HOST` — Host to bind. Default `127.0.0.1` (loopback only). Explicitly set to `0.0.0.0` to expose beyond localhost.

### Endpoints

#### `GET /query`

Runs a query and returns matching entries as newline-delimited JSON.

Query parameters:

- `q` (required) — Query expression. URL-encoded.
- `limit` (optional) — Maximum results. Default 1000. `0` means unlimited.

Response:

- Status 200: `Content-Type: application/x-ndjson`, one JSON object per line.
- Status 400: `{"error": "..."}` on missing/empty `q` or a malformed query expression.
- Status 500: `{"error": "internal server error"}` on storage failures (logged server-side).

```bash
curl 'http://127.0.0.1:4000/query?q=level%3Derror&limit=50'
curl 'http://127.0.0.1:4000/query?q=service%3Dpayments+AND+level%3Derror' | jq -s .
```

#### `GET /stats`

Returns aggregate metadata as a single JSON object.

```bash
curl 'http://127.0.0.1:4000/stats' | jq
```

Response shape:

```json
{
  "entries": 42317,
  "min_timestamp": "2026-03-14T08:22:01Z",
  "max_timestamp": "2026-04-22T19:45:03Z",
  "tags": [null, "api", "nginx", "payments", "worker"],
  "db_size_bytes": 8400000,
  "db_path": "/home/user/.logdive/index.db"
}
```

`null` in the `tags` array represents untagged rows. `min_timestamp` and `max_timestamp` are `null` on an empty index.

### Security

- **Read-only**: The API opens the database with `SQLITE_OPEN_READ_ONLY`. Writes are rejected at the SQLite level.
- **No authentication in v1**: The server assumes the network layer handles access control. Do not expose it publicly without a reverse proxy providing authentication.
- **Fail-fast on missing DB**: The server refuses to start if the configured database does not exist.
- **Graceful shutdown**: Ctrl-C and SIGTERM (Unix) trigger a clean shutdown.

---

## Query language reference

logdive queries are a small expression language with `AND`-separated clauses.

### Grammar

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

Keywords (`AND`, `CONTAINS`, `last`, `since`, `true`, `false`) are case-insensitive.

### Fields

Two kinds of fields are supported:

- **Known fields** — `timestamp`, `level`, `message`, `tag`. These are indexed columns on the SQLite table. Queries on them are very fast.
- **Unknown fields** — anything else. These are read from the JSON `fields` blob via SQLite's `json_extract()`. Slower than known-field queries but works across arbitrary JSON shapes.

Field names must match `[a-zA-Z_][a-zA-Z0-9_.]*`. Nested access uses dot notation (e.g. `user.id`).

### Operators

| Operator | Meaning | Example |
|---|---|---|
| `=` | Equals | `level=error` |
| `!=` | Not equals | `level!=debug` |
| `>` | Greater than | `duration_ms > 1000` |
| `<` | Less than | `status < 500` |
| `CONTAINS` | Substring match (case-insensitive) | `message contains "timeout"` |
| `last` | Time window ending now | `last 2h` |
| `since` | Time window starting at a given datetime | `since 2026-01-01` |

Comparisons work on strings, integers, floats, and booleans. `true`/`false` are stored as `1`/`0`.

### Time ranges

`last` takes a number followed by a unit:

- `m` — minutes (`last 30m`)
- `h` — hours (`last 2h`)
- `d` — days (`last 7d`)

`since` accepts three formats:

- RFC 3339 / ISO 8601 with timezone: `since 2024-01-01T10:00:00Z`
- ISO naive datetime (interpreted as UTC): `since "2024-01-01 10:00:00"` or `since 2024-01-01T10:00:00`
- ISO date (interpreted as UTC midnight): `since 2024-01-01`

Timestamps in the index are compared as text. This is correct for ISO-8601-shaped timestamps because they sort lexicographically in chronological order. Non-ISO-shaped timestamps will compare incorrectly.

### Quoting

Bare words work for simple values. Use double quotes for anything containing spaces, punctuation, or a value that starts with a digit and contains letters (e.g. `version="3beta"` — without quotes, `3beta` tokenizes as `3` + `beta`).

```
level=error                       # bare word
message contains "bad request"    # quotes needed for space
version="3beta"                   # quotes needed for digit-letter mix
since "2024-01-01 10:00:00"       # quotes needed for space
```

### Combining clauses

Clauses are joined with `AND` (case-insensitive). `OR` is not yet supported — see [v1 non-goals](#v1-non-goals).

```
level=error AND service=payments
level=error AND message contains "timeout" last 1h
tag=nginx AND status > 499 since 2026-04-15
```

### Examples

```bash
# All errors.
logdive query 'level=error'

# Errors from the payments service in the last 2 hours.
logdive query 'level=error AND service=payments last 2h'

# Anything mentioning "timeout" in the last day.
logdive query 'message contains "timeout" last 24h'

# Slow requests over 500ms.
logdive query 'duration_ms > 500'

# Everything from a specific user ID.
logdive query 'user_id=4812'

# Everything from a specific time range.
logdive query 'since 2026-04-15T09:00:00Z'

# Everything that isn't a health check.
logdive query 'message!="health check ok"'
```

---

## Configuration reference

All configuration is via command-line flags, with environment-variable fallbacks on the HTTP API for convenience in containerized deployments.

### Environment variables

| Variable | Applies to | Purpose |
|---|---|---|
| `LOGDIVE_LOG` | both binaries | Verbosity filter for internal diagnostics (passed to `tracing_subscriber::EnvFilter`). Default `warn`. Try `info` or `debug` for troubleshooting. |
| `LOGDIVE_DB` | `logdive-api` | Database path fallback. CLI flag `--db` takes precedence. |
| `LOGDIVE_API_PORT` | `logdive-api` | Port fallback. CLI flag `--port` takes precedence. Default `4000`. |
| `LOGDIVE_API_HOST` | `logdive-api` | Bind host fallback. CLI flag `--host` takes precedence. Default `127.0.0.1`. |
| `NO_COLOR` | `logdive query` | Standard `NO_COLOR` convention — suppresses ANSI color output when set. |
| `HOME` | both binaries | Used to resolve the default `~/.logdive/index.db` path on POSIX. |

### Default paths

- **Index database**: `~/.logdive/index.db`. Override per invocation with `--db`.
- **Parent directory**: Auto-created on first `logdive ingest`. Not auto-created by `logdive-api`.

---

## Architecture

logdive is a three-crate Rust workspace:

- **`logdive-core`** — Pure library. Owns the log entry type, the JSON parser, the SQLite-backed indexer, the query AST + parser, and the query executor. No I/O at the module level. Publishable to crates.io as a reusable library.
- **`logdive`** — The CLI binary. Thin wrapper around `logdive-core` that adds `clap` parsing, progress output, and rendering.
- **`logdive-api`** — The HTTP server binary. Axum router over `logdive-core`, opened in read-only mode.

Key architectural choices (see the project's design document for full rationale):

- **SQLite via `rusqlite`** with the `bundled` feature — zero infrastructure, ships inside the binary, battle-tested.
- **Hybrid storage** — known fields (`timestamp`, `level`, `message`, `tag`) are real indexed columns; everything else is stored in a JSON blob and queried via `json_extract()`. Flattening unknown keys into columns is impossible for a generic tool.
- **Hand-written recursive descent query parser** — ~200 lines of pure Rust enums, no parser combinator library, excellent error messages.
- **Blake3 row hashing** for deduplication — `INSERT OR IGNORE` on a unique hash column means re-ingesting a file is free.
- **Batched inserts** at 1000 rows per transaction.

The HTTP API is a separate binary, not a CLI flag — users who only want the CLI don't pull in Axum and Tokio.

---

## Performance

Benchmarks live in `crates/core/benches/` and run via:

```bash
cargo bench
```

Representative numbers on a modern laptop (Acer Nitro 5, Linux):

| Operation | Throughput / Latency |
|---|---|
| Ingestion, batched insert (10k rows) | ~210k lines/sec |
| Ingestion, parse + insert end-to-end (10k rows) | ~166k lines/sec |
| Query on known field, empty result (100k rows) | ~17 μs |
| Query on known field, 25% match (100k rows, LIMIT 1000) | ~39 ms |
| Query on JSON field, 25% match (100k rows, LIMIT 1000) | ~3.6 ms |
| Query on JSON field, 0% match — full scan (100k rows) | ~68 ms |
| `CONTAINS` full-table scan (100k rows) | ~36–40 ms |
| 3-clause `AND` chain (100k rows) | ~22 ms |


Numbers from criterion benchmarks on an unspecified modern laptop — run `cargo bench` for your own baseline. 

Release-profile binary sizes:

- `logdive`: 3.7 MB
- `logdive-api`: 4.1 MB

Targets: both binaries under 10 MB. Run `scripts/check-binary-size.sh` to verify.

---

## Development

```bash
# Clone and build.
git clone https://github.com/Aryagorjipour/logdive
cd logdive
cargo build --workspace

# Run tests.
cargo test --workspace

# Lints and formatting (run before every commit).
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Run the CLI during development.
cargo run --bin logdive -- --help
cargo run --bin logdive -- ingest --file examples/app.log

# Run the API.
cargo run --bin logdive-api -- --db /tmp/demo.db
```

MSRV: Rust 1.85. Edition 2024.

### Changelog

See [`CHANGELOG.md`](CHANGELOG.md) for release notes.

### Contributing

Bug reports and pull requests welcome. Before submitting a PR, please ensure:

1. `cargo test --workspace` passes.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
3. `cargo fmt --all --check` is clean.
4. Any new feature lands behind a discussion in an issue first, to avoid scope creep against the [v1 non-goals](#v1-non-goals).

---

## v1 non-goals

The following are **intentionally** out of scope for v0.1.0 and may or may not land in future versions:

- **`OR` operator in queries** — v1 is AND-only. Parser and SQL generation for disjunction are non-trivial and deferred to v2.
- **Non-JSON log formats** — plaintext, logfmt, syslog. v1 targets structured JSON only.
- **Real-time tailing (`-f` / follow mode)** — no continuous ingestion from a growing file.
- **Authentication on the HTTP API** — the API trusts its network layer.
- **Ingestion over HTTP** — the API is read-only. Ingestion goes through the CLI.
- **Multi-machine or networked indexes** — single-host only.
- **Log shipping, agents, or daemons** — logdive is a tool, not a service.
- **A browser UI** — curl and the CLI are the intended interfaces. Third parties can build UIs against the HTTP API.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
