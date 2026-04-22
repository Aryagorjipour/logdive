//! # logdive-core
//!
//! Core library for `logdive` — structured JSON log parsing, SQLite-backed
//! indexing, and a hand-written query engine.
//!
//! This crate is pure library code with no I/O side effects at the module
//! level. It is consumed by the `logdive` CLI binary and the `logdive-api`
//! HTTP server binary.

pub mod entry;
pub mod indexer;
pub mod parser;
pub mod query;

pub use entry::LogEntry;
pub use indexer::{db_path, Indexer, InsertStats, BATCH_SIZE};
pub use parser::parse_line;
pub use query::{
    parse as parse_query, Clause, CompareOp, Duration, DurationUnit, QueryNode, QueryParseError,
    QueryValue,
};
