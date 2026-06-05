# CLAUDE.md — Working with logdive

This file orients Claude Code (and any AI assistant) before touching the codebase.
Read this first. The rules here override any defaults.

## What this project is

logdive is a fast, self-hosted query engine for structured logs. Three-crate
Rust workspace (edition 2024, MSRV 1.85). Ships two binaries:

- `logdive` — CLI: ingest, query, stats, prune.
- `logdive-api` — read-only HTTP server: GET /query, /stats, /version.

Library half: `logdive-core` is publishable as a standalone crate.

Current version: **0.3.0** (released 2026-06-05).
Next milestone: **0.4.0** (yaml/csv output, configurable retention by source, Windows --follow).

## Commands

```bash
# Compile checkpoint — run after every meaningful change
cargo build --workspace
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Run binaries
cargo run --bin logdive -- --help
cargo run --bin logdive-api -- --help

# Benchmarks
cargo bench   # reports in target/criterion/

# Pre-release verification (full battery)
./scripts/prerelease-check.sh
```

## Crate layout

```
crates/
  core/   — logdive-core: library (LogEntry, parsers, Indexer, query, executor, FileTailer)
  cli/    — logdive: binary (subcommands: ingest, query, stats, prune)
  api/    — logdive-api: binary (axum router, AppState, handlers)
```

CLI's crate path is `crates/cli/` but the **binary name and crate name are both `logdive`**. Never call it `cli`.

## Locked architecture decisions (do not revisit)

- SQLite via `rusqlite` (bundled feature). No separate DB process.
- Hybrid schema: `timestamp`, `level`, `message`, `tag` as indexed columns; everything else in a `fields TEXT` JSON blob queried via `json_extract()`.
- Hand-written recursive descent query parser. No parser-combinator libs.
- blake3 row hashing → `INSERT OR IGNORE` on `raw_hash UNIQUE`.
- 1000 rows per insert transaction.
- CLI is fully synchronous (no tokio). Only the API uses tokio.
- API opens DB with `SQLITE_OPEN_READ_ONLY`. Fresh connection per request via `AppState::with_connection` → `spawn_blocking`.
- `--follow` is Unix-only (uses `(dev, ino)` rotation detection from `std::os::unix::fs::MetadataExt`). Windows --follow is v0.4+.
- Query language v0.3: AND + OR + parenthesised groups via `Clause::Group(Box<QueryNode>)`. Full grammar in `.context/architecture.md`.
- Formats: JSON (default), logfmt, plain. Dispatcher in `parsers::mod`.
- CORS disabled by default. GET-only when enabled. No credentials.
- HTTP API has no authentication. Read-only is the defence-in-depth answer; deployment is responsible for putting auth in front.

## Git workflow

- Integration branch per milestone series: e.g. `release/v0.3.0`.
- One PR per milestone, squash-merged from `feat/v0.3.0/<slug>` or `chore/v0.3.0/<slug>` branches.
- One final merge-commit PR `release/vX.Y.Z` → `main` ships the version.
- Force-push with `--force-with-lease` only.
- Conventional Commits format.

## Operating rules

These override any default behaviour. Honor every one.

1. **Plan before any non-trivial code.** Write a plan, get explicit approval, then execute milestone-by-milestone.
2. **Full files only.** Never deliver diffs. Edit by writing the new full file.
3. **Zero placeholders or TODOs.** If you can't finish something, surface the blocker; don't write `// TODO`.
4. **One patch per error report.** Fix what was reported; surface other findings separately.
5. **Don't invent scope.** If the user didn't ask, don't add it.
6. **Flag design decisions upfront.** List options, recommend one with justification, get approval, then write code.
7. **Compile checkpoint** after every meaningful change. Zero warnings on clippy. Zero diff on fmt --check.
8. **Conventional Commits.**
9. **Web-search before stating present-day facts** (versions, API shapes, GHA syntax). Don't guess from training data.
10. **`cargo clean`** if stale-cache symptoms appear.
11. **Honor MSRV 1.85.** Don't use features from later editions.
12. **Use the right binary name.** CLI is `logdive`. Not `cli`.

## File-creation policy

If the user gives a path explicitly, use it. Otherwise:

- New core modules → `crates/core/src/<name>.rs` and export from `lib.rs`.
- New CLI subcommand modules → `crates/cli/src/<name>_cmd.rs`.
- New API handlers → add to `crates/api/src/handlers.rs` and wire in `router.rs`.
- New integration tests → `crates/<crate>/tests/<topic>.rs`.
- New benches → `crates/core/benches/bench_<name>.rs`, register in `crates/core/Cargo.toml`.
- New scripts → `scripts/<name>.sh`, POSIX sh, executable bit set.

## Token management & long sessions

This codebase is ~10k lines. A full read of every file blows the context.

- Read CLAUDE.md, README.md, and the relevant crate's source. Don't read all three crates unprompted.
- Use `@path/to/file` references rather than dumping file contents.
- For multi-step milestones, work one file at a time; commit between files.
- `/compact` early if a milestone is going to span many tool calls.

## Test baseline (as of v0.3.0)

417 tests passing across 9 test binaries:

- `cargo test -p logdive-core` — parsers, indexer, query, executor, follow.
- `cargo test -p logdive-core --test security` — SQL injection, wildcard escaping, resource exhaustion.
- `cargo test -p logdive-core --test functional` — proptest (arbitrary input, AST shape), cross-format dedup, concurrent ingest, UTF-8 edge cases.
- `cargo test -p logdive --bin logdive` — CLI internals (render, stats, prune).
- `cargo test -p logdive-api --lib` — handlers, router, error, state.
- `cargo test -p logdive-api --test integration` — end-to-end HTTP via `oneshot`.

All 417 tests pass. No test may be deleted; regressions are blockers.

## What to do first in a new session

1. `git status` and `git log --oneline -10` to see where main is.
2. Read CHANGELOG.md's `[Unreleased]` section for in-flight work.
3. Read this file (you just did).
4. Ask the user what milestone is next if it's not obvious from context.
5. Propose a plan. Wait for approval. Execute.

## Caveman mode

The maintainer sometimes asks for `/caveman` mode in chat — terse, articles dropped, fillers gone. Never apply this to code, commits, ADRs, mentor lessons, or anything that goes into the repo. Caveman is for chat only.

## Maintainer

Arya Gorjipour. Iranian backend engineer, Rust learner. Senior in C# and Go, working learner in Rust. Treat as senior in the first two, working learner in Rust — explain what the borrow checker is enforcing in the specific case, don't explain what a `for` loop is.

## When the build, test, or release breaks

Don't panic. Don't roll back unprompted. Tell the user what failed, in one line, with the file and the symptom. Propose one fix. Wait for approval.
