//! # logdive-core
//!
//! Core library for `logdive` — structured log parsing, SQLite-backed
//! indexing, and a hand-written query engine.
//!
//! This crate is pure library code with no I/O side effects at the module
//! level. It is consumed by the `logdive` CLI binary and the `logdive-api`
//! HTTP server binary.
//!
//! v0.2.0 introduced multi-format ingestion: JSON, logfmt, and plain text.
//! See [`parsers`] for the format-specific parsers and the format-aware
//! [`parse_line`] dispatcher.
//!
//! v0.2.0 also introduces [`follow`] (Unix-only), which provides
//! [`FileTailer`] for tracking a growing file and detecting log rotation
//! and truncation. Used by the CLI's `--follow` flag.

pub mod entry;
pub mod error;
pub mod executor;
pub mod indexer;
pub mod parsers;
pub mod query;

#[cfg(unix)]
pub mod follow;

pub use entry::LogEntry;
pub use error::{LogdiveError, Result};
pub use executor::{QueryOptions, execute, execute_at};
pub use indexer::{BATCH_SIZE, Indexer, InsertStats, Stats, db_path};
pub use parsers::{LogFormat, parse_line};
pub use query::{
    AndGroup, Clause, CompareOp, Duration, DurationUnit, QueryNode, QueryParseError, QueryValue,
    parse as parse_query,
};

#[cfg(unix)]
pub use follow::FileTailer;
