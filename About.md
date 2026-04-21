## The Project

**[[logdive]]** — A fast, self-hosted log query engine for structured JSON logs.

---

### What It Is

A CLI + HTTP API tool that ingests structured JSON log files (from Docker, your app, nginx, anything), indexes them locally in SQLite, and lets you query them with a simple query language. Think "grep on steroids for JSON logs, with an API."

```bash
# Ingest logs from a file or stdin
logdive ingest ./logs/app.log
docker logs my-container | logdive ingest --tag api

# Query from CLI
logdive query 'level=error AND service=payments last 2h'
logdive query 'message contains "timeout" last 24h' --format json

# Start HTTP API for a UI or curl
logdive serve --port 4000
curl 'http://localhost:4000/query?q=level%3Derror&from=2h'
```

---

### Why This Project

**Career value:** Every company has a logging problem. Knowing how to build a fast log pipeline in Rust is a concrete, demonstrable skill — especially relevant for your fintech background where observability matters.

**Open source value:** The gap this fills is real. `jq` is for single files. Datadog/Loki require infrastructure. `logdive` is the zero-setup, self-hosted middle ground — a single binary, no dependencies.

**Rust fit:** This plays to exactly what Rust is best at — high-throughput file parsing, concurrent ingestion, zero-copy string processing, SQLite via `rusqlite`, a CLI via `clap`, and an optional HTTP layer via Axum. Every lesson you've learned gets used.

---

### Scope — Deliberately Sized

```
Phase 1 — CLI (3–4 weeks)        ← start here
Phase 2 — HTTP API (1–2 weeks)   ← Axum, you already know this
Phase 3 — Polish + OSS release    ← README, benchmarks, crates.io
```

Not a platform. Not multi-tenant. One binary that does one thing well.

---

### Architecture

```
logdive/
├── crates/
│   ├── core/          ← parsing, indexing, query engine (library crate)
│   │   ├── src/
│   │   │   ├── parser.rs      ← JSON log parsing, field extraction
│   │   │   ├── indexer.rs     ← SQLite ingestion, batching
│   │   │   ├── query.rs       ← query language parser + executor
│   │   │   └── lib.rs
│   ├── cli/           ← clap CLI binary
│   │   └── src/main.rs
│   └── api/           ← Axum HTTP server (optional, feature-flagged)
│       └── src/main.rs
├── Cargo.toml         ← workspace
└── README.md
```

**Workspace** is a new Rust concept you haven't used — multiple crates in one repo, sharing dependencies. `core` is a pure library with no I/O — fully unit testable. `cli` and `api` are thin binaries that call into `core`.

---

### What You'll Use From This Curriculum

|Feature|Lessons Used|
|---|---|
|JSON parsing, field extraction|Structs, serde, iterators|
|SQLite batching|async, error hierarchy|
|Query language parser|Enums, pattern matching|
|CLI with `clap`|Traits, builder pattern|
|HTTP query endpoint|Axum, everything|
|Concurrent file ingestion|Arc, Mutex, tokio::spawn|
|Test suite|All testing lessons|

---

### The Query Language — A Concrete Design

Simple enough to implement in Phase 1, expressive enough to be useful:

```
level=error
level=error AND service=payments
message contains "database timeout"
level=error last 2h
level=warn OR level=error since 2024-01-01
tag=api AND level=error last 30m
```

This is a hand-written recursive descent parser — a classic systems project that looks great on a resume and is genuinely fun to build in Rust with enums.

---

### First Week Goal

```bash
cargo new --lib crates/core
cargo new --bin crates/cli
```

Build a function that:

1. Reads a JSON log file line by line
2. Parses each line into a `LogEntry { timestamp, level, message, fields: HashMap<String, Value> }`
3. Filters entries by level
4. Prints matching entries

That's day one. Everything else builds on it.
