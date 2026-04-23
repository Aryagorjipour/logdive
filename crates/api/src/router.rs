//! Axum router construction.
//!
//! Extracted from `main.rs` so integration tests can build the same router
//! the binary uses without duplicating route definitions. The router is
//! pure data — no I/O happens here; `AppState` carries the configuration
//! and all I/O is deferred into the handler layer.

use axum::{Router, routing::get};

use crate::handlers::{query_handler, stats_handler};
use crate::state::AppState;

/// Build the application router with all endpoints wired up.
///
/// Caller supplies a fully-constructed [`AppState`]. The returned router
/// is ready to be handed to `axum::serve` in the binary, or to
/// `tower::ServiceExt::oneshot` in tests.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/query", get(query_handler))
        .route("/stats", get(stats_handler))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// The router itself is a thin composition layer; meaningful coverage of its
// behavior lives in the end-to-end integration tests (milestone 8 Unit D),
// which exercise actual HTTP requests against a real temporary database.
// A compile-time smoke test here guards against basic regressions in the
// wiring without duplicating the integration test surface.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn build_router_produces_a_router_from_a_valid_state() {
        let state = AppState::new(PathBuf::from("/tmp/does-not-need-to-exist-yet.db"));
        let _router: Router = build_router(state);
        // If this compiles and runs, the type plumbing between AppState,
        // the handlers, and axum::Router is sound. Real behavior is
        // validated by the integration test suite.
    }
}
