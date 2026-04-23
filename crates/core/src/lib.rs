//! # logdive-core
//!
//! Core library for `logdive` — structured JSON log parsing, SQLite-backed
//! indexing, and a hand-written query engine.
//!
//! This crate is pure library code with no I/O side effects at the module
//! level. It is consumed by the `logdive` CLI binary and the `logdive-api`
//! HTTP server binary.

pub mod entry;
pub mod error;
pub mod executor;
pub mod indexer;
pub mod parser;
pub mod query;

pub use entry::LogEntry;
pub use error::{LogdiveError, Result};
pub use executor::{execute, execute_at};
pub use indexer::{BATCH_SIZE, Indexer, InsertStats, Stats, db_path};
pub use parser::parse_line;
pub use query::{
    Clause, CompareOp, Duration, DurationUnit, QueryNode, QueryParseError, QueryValue,
    parse as parse_query,
};
