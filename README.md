<p align="center">
  <img src="assets/banner.png" alt="logdive — query your logs. no daemon. no yaml." width="700" />
</p>

<p align="center">
  <a href="https://github.com/Aryagorjipour/logdive/actions/workflows/ci.yml"><img src="https://github.com/Aryagorjipour/logdive/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/Aryagorjipour/logdive/actions/workflows/docker.yml"><img src="https://github.com/Aryagorjipour/logdive/actions/workflows/docker.yml/badge.svg" alt="Docker" /></a>
  <a href="https://crates.io/crates/logdive"><img src="https://img.shields.io/crates/v/logdive.svg" alt="Crates.io" /></a>
  <a href="https://docs.rs/logdive-core"><img src="https://img.shields.io/docsrs/logdive-core" alt="Docs.rs" /></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0" /></a>
</p>

# logdive

**Fast, self-hosted query engine for structured JSON logs.**

A single Rust binary that ingests structured logs, indexes them locally in SQLite, and lets you query them instantly from the CLI or an HTTP API. No infrastructure, no daemons, no cloud.

```bash
# Ingest JSON, logfmt, or plain-text logs from a file or stdin.
logdive ingest --file ./logs/app.log
logdive ingest --file ./logs/app.log --format logfmt --tag production
docker logs my-container | logdive ingest --tag my-container

# Tail a growing log file in real time (Ctrl-C to stop).
logdive ingest --file ./logs/app.log --follow

# Query with AND and OR.
logdive query 'level=error AND service=payments last 2h'
logdive query 'level=error OR level=warn' --output json

# Prune old entries to keep the index lean.
logdive prune --older-than 30d

# Inspect the index.
logdive stats

# Expose a read-only HTTP API for remote querying.
logdive-api --db ./logdive.db --port 4000
curl 'http://127.0.0.1:4000/query?q=level%3Derror&limit=100'
curl 'http://127.0.0.1:4000/version'
```

> **Status: v0.3.0.** Adds parenthesised query groups, CLI pagination (`--offset`), case-insensitive level queries, and a distroless Docker runtime. Breaking: `logdive query --format` renamed to `--output`; `execute()` now takes `QueryOptions { limit, offset }`. See [v1 non-goals](#v1-non-goals) for what is explicitly out of scope.

---

## Table of contents

- [Why logdive](#why-logdive)
- [Install](#install)
- [Quick start](#quick-start)
- [The `logdive` CLI](#the-logdive-cli)
- [Running with Docker](#running-with-docker)
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

### From crates.io

```bash
cargo install logdive logdive-api
```

Both binaries land in `~/.cargo/bin/` — make sure it's on your `PATH`.

### From Docker

Official multi-arch images (linux/amd64 and linux/arm64) are available on GHCR:

```bash
docker pull ghcr.io/aryagorjipour/logdive:latest
```

See [Running with Docker](#running-with-docker) for usage.

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

# Find errors or warnings from a specific service.
logdive --db /tmp/demo.db query 'level=error OR level=warn AND service=payments'

# Find slow nginx requests.
logdive --db /tmp/demo.db query 'tag=nginx AND request_time > 1.0'

# Get structured output for further processing.
logdive --db /tmp/demo.db query 'service=payments' --output json | jq

# Prune entries older than 7 days.
logdive --db /tmp/demo.db prune --older-than 7d
```

See [`examples/README.md`](examples/README.md) for a longer walkthrough of what these files contain and what queries are interesting against them.

---

## The `logdive` CLI

Four subcommands: `ingest`, `query`, `stats`, `prune`.

### `logdive ingest`

Reads log lines from a file or stdin, parses them, and inserts them into the index.

```bash
# JSON (default)
logdive ingest --file ./logs/app.log
logdive ingest --file ./logs/app.log --tag production

# logfmt
logdive ingest --file ./logs/app.log --format logfmt

# Plain text (whole line becomes `message`)
logdive ingest --file ./logs/app.log --format plain

# Pipe from any source
docker logs my-container | logdive ingest --tag my-container
journalctl --output=json | logdive ingest --tag systemd

# Tail a growing file in real time
logdive ingest --file ./logs/app.log --follow
```

Flags:

- `--file <PATH>` / `-f` — Read from a file. Mutually exclusive with stdin.
- `--format json|logfmt|plain` — Input format. Default `json`.
- `--tag <TAG>` / `-t` — Attach a tag to every ingested entry that does not already contain a `tag` field.
- `--timestamp-now` — Assign the current UTC time (RFC 3339) to entries that lack a `timestamp` field, instead of skipping them. Useful for formats that do not include timestamps.
- `--follow` — Keep the file open and ingest new lines as they are appended, similar to `tail -f`. Detects log rotation (inode change) and truncation and reopens the file automatically. Ctrl-C exits cleanly. Requires `--file`.
- `--db <PATH>` — Override the default `~/.logdive/index.db` location (global, applies to all subcommands). Also settable via `$LOGDIVE_DB`.

Behavior:

- **Deduplication**: Every row is fingerprinted with a blake3 hash. Re-ingesting the same file (or a log rotation producing overlapping lines) results in zero duplicate rows.
- **Graceful skip**: Lines that cannot be parsed in the selected format are counted and skipped, not fatal. Blank lines are silently ignored.
- **No-timestamp skip**: By default, lines without a `timestamp` field are skipped. Pass `--timestamp-now` to assign the current UTC time to such entries instead.
- **Progress**: TTY-aware status on stderr. A final summary always prints inserted / deduplicated / skipped counts.

### `logdive query`

Runs a query against the index and renders matching entries.

```bash
logdive query 'level=error'
logdive query 'level=error AND service=payments last 24h'
logdive query 'level=error OR level=warn'
logdive query '(level=error OR level=warn) AND service=payments'
logdive query 'message contains "timeout"' --output json
logdive query 'since 2026-01-01' --limit 0
```

Flags:

- `--output pretty|json` — Output format. Default `pretty` (colored, human-readable). `json` is newline-delimited, pipe-friendly for `jq`.
- `--limit <N>` — Maximum results to return. Default `1000`. Use `0` for unlimited.
- `--offset <N>` — Skip the first N results. Use with `--limit` for page navigation. Default `0`.
- `--db <PATH>` — Database path override. Also settable via `$LOGDIVE_DB`.

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

Errors out (exit code 1) if the configured index file does not exist. This catches typos in `--db` paths early.

### `logdive prune`

Deletes entries from the index that fall outside a retention window, then vacuums the database file to reclaim disk space.

```bash
# Delete everything older than 30 days.
logdive prune --older-than 30d

# Delete everything before a specific date.
logdive prune --before 2026-01-01

# Skip the interactive confirmation prompt.
logdive prune --older-than 7d --yes
```

Flags:

- `--older-than <DURATION>` — Delete entries older than this duration. Format: a positive integer followed by `m` (minutes), `h` (hours), or `d` (days). Examples: `30d`, `24h`, `90m`. Mutually exclusive with `--before`.
- `--before <DATETIME>` — Delete entries with a timestamp before this datetime. Accepts the same three formats as the `since` query operator (RFC 3339, ISO naive datetime, ISO date). Mutually exclusive with `--older-than`.
- `--yes` — Skip the interactive `[y/N]` confirmation. Useful in scripts and cron jobs.
- `--db <PATH>` — Database path override. Also settable via `$LOGDIVE_DB`.

By default `prune` shows the number of rows that would be deleted and asks for confirmation before proceeding. If the count is zero it exits immediately with "Nothing to prune."

---

## Running with Docker

Official images for `linux/amd64` and `linux/arm64` are published to GHCR on every merge to `main` and on every version tag.

```bash
docker pull ghcr.io/aryagorjipour/logdive:latest
# or pin to a specific version:
docker pull ghcr.io/aryagorjipour/logdive:0.3.0
```

### Start the API server

```bash
# Create a named volume for the index.
docker volume create logdive-data

# Start the server. The index is auto-created on first run.
docker run -d \
  --name logdive \
  -v logdive-data:/data \
  -p 4000:4000 \
  ghcr.io/aryagorjipour/logdive

curl 'http://localhost:4000/stats'
curl 'http://localhost:4000/version'
```

### Ingest logs with the CLI

The default entrypoint is `logdive-api`. Override it with `--entrypoint logdive` to run the CLI against the same volume:

```bash
docker run --rm \
  -v logdive-data:/data \
  -v /path/to/your/logs:/logs:ro \
  --entrypoint logdive \
  ghcr.io/aryagorjipour/logdive \
  ingest --file /logs/app.log --tag production
```

### Environment variables

The image pre-sets two variables for container-native behavior:

- `LOGDIVE_DB=/data/index.db` — points both binaries at the persistent volume.
- `LOGDIVE_API_HOST=0.0.0.0` — binds the API to all container interfaces so `-p 4000:4000` works.

Override any variable with `-e`:

```bash
docker run -d \
  -v logdive-data:/data \
  -p 4000:4000 \
  -e LOGDIVE_API_CORS_ORIGINS='https://app.example.com' \
  -e LOGDIVE_API_PORT=8080 \
  -p 8080:8080 \
  ghcr.io/aryagorjipour/logdive
```

### Health check

The image declares a Docker HEALTHCHECK using the `--health-check` flag on `logdive-api`. This opens a TCP connection to the server's own port via stdlib `TcpStream` — no curl, no shell, no HTTP client required. Works correctly in the distroless runtime image.

```bash
docker inspect --format='{{.State.Health.Status}}' logdive
```

---

## The `logdive-api` HTTP server

A read-only HTTP server for remote querying. Useful when you want a browser-based UI, a CI check, or a shell one-liner hitting a centrally hosted index.

```bash
logdive-api --db ~/logdive.db --port 4000
```

Flags (with environment-variable fallbacks):

- `--db <PATH>` / `$LOGDIVE_DB` — Database to serve. Defaults to `~/.logdive/index.db`.
- `--port <N>` / `$LOGDIVE_API_PORT` — Port to listen on. Default 4000.
- `--host <HOST>` / `$LOGDIVE_API_HOST` — Host to bind. Default `127.0.0.1` (loopback only). Set to `0.0.0.0` to expose beyond localhost.
- `--cors-origins <ORIGINS>` / `$LOGDIVE_API_CORS_ORIGINS` — Comma-separated list of allowed CORS origins. Use `*` to allow any origin. Omit to disable CORS (same-origin only). Invalid values cause a startup error.

```bash
# Allow a specific frontend origin.
logdive-api --cors-origins 'https://app.example.com'

# Allow any origin (useful for local development).
logdive-api --cors-origins '*'
```

### Endpoints

#### `GET /query`

Runs a query and returns matching entries as newline-delimited JSON.

Query parameters:

- `q` (required) — Query expression. URL-encoded.
- `limit` (optional) — Maximum results. Default 1000. `0` means unlimited.
- `offset` (optional) — Skip the first N results. Default 0. Use with `limit` for pagination.

Response:

- Status 200: `Content-Type: application/x-ndjson`, one JSON object per line.
- Status 400: `{"error": "..."}` on missing/empty `q` or a malformed query expression.
- Status 500: `{"error": "internal server error"}` on storage failures (logged server-side).

```bash
curl 'http://127.0.0.1:4000/query?q=level%3Derror&limit=50'
curl 'http://127.0.0.1:4000/query?q=level%3Derror+OR+level%3Dwarn' | jq -s .
curl 'http://127.0.0.1:4000/query?q=level%3Derror&limit=20&offset=40'
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

#### `GET /version`

Returns the server's version and supported capabilities as a JSON object. Designed for client-side feature detection — call this first to discover which formats and endpoints the running server supports.

```bash
curl 'http://127.0.0.1:4000/version' | jq
```

Response shape:

```json
{
  "version": "0.3.0",
  "formats": ["json", "logfmt", "plain"],
  "capabilities": ["query", "stats", "version"]
}
```

Always returns 200 OK. Never touches the database.

### Security

- **Read-only**: The API opens the database with `SQLITE_OPEN_READ_ONLY`. Writes are rejected at the SQLite level.
- **No authentication**: The server assumes the network layer handles access control. Do not expose it publicly without a reverse proxy providing authentication.
- **Auto-creates empty index on first run**: If the configured database does not exist, the server creates it with an initialized schema and starts cleanly, returning zero results until logs are ingested via the CLI. Genuinely bad paths (wrong directory, permission denied) still cause a startup failure with a clear error message.
- **CORS disabled by default**: Cross-origin requests are blocked unless `--cors-origins` is explicitly configured.
- **Graceful shutdown**: Ctrl-C and SIGTERM (Unix) trigger a clean shutdown.

---

## Query language reference

logdive queries are a small expression language supporting `AND` within groups and `OR` between groups.

### Grammar

```
query    := or_expr [ TIME_RANGE ]
or_expr  := and_expr (OR and_expr)*
and_expr := clause (AND clause)*
clause   := field OP value
           | field CONTAINS string
           | "(" or_expr ")"
           | TIME_RANGE
field    := [a-zA-Z_][a-zA-Z0-9_.]*
OP       := "=" | "!=" | ">" | "<"
value    := string | number | bool
string   := '"' .* '"' | bare_word
TIME_RANGE := "last" duration | "since" datetime
duration := number ("m" | "h" | "d")
```

Keywords (`AND`, `OR`, `CONTAINS`, `last`, `since`, `true`, `false`) are case-insensitive.

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

Timestamps in the index are compared as text. This is correct for ISO-8601-shaped timestamps because they sort lexicographically in chronological order.

### Combining clauses

Clauses are joined with `AND` (case-insensitive). Groups of AND-clauses can be separated with `OR`. `AND` binds more tightly than `OR`. Parentheses are supported for explicit grouping.

```bash
# AND only.
logdive query 'level=error AND service=payments'

# OR between two simple clauses.
logdive query 'level=error OR level=warn'

# AND within each OR branch.
logdive query 'level=error AND service=payments OR level=warn AND tag=worker'
# Equivalent to: (level=error AND service=payments) OR (level=warn AND tag=worker)

# Parentheses change precedence explicitly.
logdive query '(level=error OR level=warn) AND service=payments'
```

### Quoting

Bare words work for simple values. Use double quotes for anything containing spaces, punctuation, or a value that starts with a digit and contains letters.

```
level=error                       # bare word
message contains "bad request"    # quotes needed for space
version="3beta"                   # quotes needed for digit-letter mix
since "2024-01-01 10:00:00"       # quotes needed for space
```

### Examples

```bash
# All errors.
logdive query 'level=error'

# Errors or warnings.
logdive query 'level=error OR level=warn'

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

# Errors from payments OR any warn from worker, last hour.
logdive query 'level=error AND service=payments last 1h OR level=warn AND tag=worker last 1h'

# Parenthesised OR, then AND narrows further.
logdive query '(level=error OR level=warn) AND service=payments last 2h'
```

---

## Configuration reference

All configuration is via command-line flags, with environment-variable fallbacks for convenience in containerized deployments.

### Environment variables

| Variable | Applies to | Purpose |
|---|---|---|
| `LOGDIVE_LOG` | both binaries | Verbosity filter for internal diagnostics (passed to `tracing_subscriber::EnvFilter`). Default `warn`. Try `info` or `debug` for troubleshooting. |
| `LOGDIVE_DB` | both binaries | Database path fallback for `--db`. CLI flag takes precedence when both are set. Default `~/.logdive/index.db`. |
| `LOGDIVE_API_PORT` | `logdive-api` | Port fallback for `--port`. Default `4000`. |
| `LOGDIVE_API_HOST` | `logdive-api` | Bind host fallback for `--host`. Default `127.0.0.1`. |
| `LOGDIVE_API_CORS_ORIGINS` | `logdive-api` | Allowed CORS origins fallback for `--cors-origins`. Comma-separated list or `*`. Default: empty (CORS disabled). |
| `NO_COLOR` | `logdive query` | Standard `NO_COLOR` convention — suppresses ANSI color output when set. |
| `HOME` | both binaries | Used to resolve the default `~/.logdive/index.db` path on POSIX. |

### Default paths

- **Index database**: `~/.logdive/index.db`. Override with `--db` or `$LOGDIVE_DB`.
- **Parent directory**: Auto-created on first `logdive ingest` (CLI) or on first `logdive-api` startup when the database path does not yet exist.

---

## Architecture

logdive is a three-crate Rust workspace:

- **`logdive-core`** — Pure library. Owns the log entry type, the multi-format parser (JSON, logfmt, plain), the SQLite-backed indexer, the query AST + parser (AND + OR), and the query executor. No I/O at the module level. Publishable to crates.io as a reusable library.
- **`logdive`** — The CLI binary. Thin wrapper around `logdive-core` that adds `clap` parsing, follow-mode file tailing, progress output, and rendering.
- **`logdive-api`** — The HTTP server binary. Axum router over `logdive-core`, opened in read-only mode.

Key architectural choices (see the project's design document for full rationale):

- **SQLite via `rusqlite`** with the `bundled` feature — zero infrastructure, ships inside the binary, battle-tested.
- **Hybrid storage** — known fields (`timestamp`, `level`, `message`, `tag`) are real indexed columns; everything else is stored in a JSON blob and queried via `json_extract()`.
- **Hand-written recursive descent query parser** — ~300 lines of pure Rust enums, no parser combinator library, supports AND + OR with correct precedence.
- **Blake3 row hashing** for deduplication — `INSERT OR IGNORE` on a unique hash column means re-ingesting a file is free.
- **Batched inserts** at 1000 rows per transaction.
- **Separate binaries** — users who only want the CLI don't pay the Axum + Tokio compile cost.

---

## Performance

Benchmarks live in `crates/core/benches/` and run via:

```bash
cargo bench
```

Representative numbers on a modern laptop (Acer Nitro 5, Linux):

| Operation | Throughput / Latency |
|---|---|
| Ingestion, batched insert (10k rows) | ~189k lines/sec |
| Ingestion, parse + insert end-to-end (10k rows) | ~150k lines/sec |
| Query on known field, empty result (100k rows) | ~23 μs |
| Query on known field, 25% match (100k rows, LIMIT 1000) | ~49 ms |
| Query on JSON field, 25% match (100k rows, LIMIT 1000) | ~4.1 ms |
| Query on JSON field, 0% match — full scan (100k rows) | ~69 ms |
| `CONTAINS` full-table scan (100k rows) | ~35–38 ms |
| 3-clause `AND` chain (100k rows) | ~22 ms |
| `OR` query, 2-branch 50% match (100k rows, LIMIT 1000) | ~68 ms |
| Parenthesised group `(A OR B) AND C`, 12.5% match (100k rows) | ~45 ms |
| Case-insensitive level query — uppercase, lowercase, mixed | ~51 ms (identical) |
| Pagination deep page overhead (offset 2450 vs offset 0) | +8 ms |

Numbers from criterion benchmarks — run `cargo bench` for your own baseline.

Release-profile binary sizes:

- `logdive`: 3.9 MB
- `logdive-api`: 4.2 MB

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
cargo run --bin logdive -- ingest --file examples/app.log --format logfmt
cargo run --bin logdive -- query 'level=error OR level=warn'
cargo run --bin logdive -- prune --older-than 7d

# Run the API.
cargo run --bin logdive-api -- --db /tmp/demo.db

# Build the Docker image locally.
docker build --platform linux/amd64 -t logdive:local .
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

The following are **intentionally** out of scope and may or may not land in future versions:

- **Authentication on the HTTP API** — the API trusts its network layer.
- **Ingestion over HTTP** — the API is read-only. Ingestion goes through the CLI.
- **Multi-machine or networked indexes** — single-host only. Use Loki if you have a fleet.
- **Real-time analytics or aggregation at scale** — logdive is a query tool, not a streaming analytics engine. Use Loki or ClickHouse for millions of rows with group-by and aggregations.
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