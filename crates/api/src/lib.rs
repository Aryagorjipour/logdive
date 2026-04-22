//! # logdive-api
//!
//! Read-only HTTP API server over a logdive index.
//!
//! Exposes two endpoints, per the decisions log:
//!   - `GET /query?q=<expr>&limit=<n>` — runs a query and returns matching
//!     log entries as newline-delimited JSON.
//!   - `GET /stats` — returns aggregate metadata about the index as a
//!     single JSON object.
//!
//! The server is strictly read-only. Ingestion is the CLI's responsibility
//! and is out of scope here; authentication is similarly out of scope for
//! v1 per the decisions log entry on HTTP surface area.
//!
//! This crate is both a binary (`logdive-api`) and a library. The library
//! half exists so integration tests can construct the router without
//! duplicating its definition — they use [`router::build_router`] the
//! same way the binary does.

pub mod error;
pub mod handlers;
pub mod router;
pub mod state;
