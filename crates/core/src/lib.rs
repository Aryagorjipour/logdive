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

pub use entry::LogEntry;
pub use indexer::{
    db_path, init_schema, insert_batch, open, IndexerError, IngestStats, BATCH_SIZE,
};
pub use parser::parse_line;
