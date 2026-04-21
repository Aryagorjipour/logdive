//! # logdive-core
//!
//! Core library for `logdive` — structured JSON log parsing, SQLite-backed
//! indexing, and a hand-written query engine.
//!
//! This crate is pure library code with no I/O side effects at the module
//! level. It is consumed by the `logdive` CLI binary and the `logdive-api`
//! HTTP server binary.
//!
//! Modules will be added in subsequent milestones:
//!
//! - `entry`    — the `LogEntry` type (milestone 1)
//! - `parser`   — JSON line parsing (milestone 1)
//! - `indexer`  — SQLite schema, batched inserts, dedup (milestone 2)
//! - `query`    — tokenizer, AST, recursive descent parser (milestone 3)
//! - `executor` — AST → SQL translation and execution (milestone 4)
//! - `error`    — unified `LogdiveError` type (milestone 5)
