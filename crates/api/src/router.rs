//! Axum router construction.
//!
//! Extracted from `main.rs` so integration tests can build the same router
//! the binary uses without duplicating route definitions. The router is
//! pure data — no I/O happens here; `AppState` carries the configuration
//! and all I/O is deferred into the handler layer.
//!
//! [`build_router`] accepts a pre-parsed list of allowed CORS origins.
//! Parsing, validation, and the wildcard `*` check happen in `main.rs`,
//! keeping this module free of clap and environment-variable concerns.

use axum::{
    Router,
    http::{HeaderValue, Method},
    routing::get,
};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::handlers::{query_handler, stats_handler, version_handler};
use crate::state::AppState;

/// Build the application router with all endpoints wired up.
///
/// `cors_origins` controls the CORS policy applied to every route:
///
/// - `[]` (empty) — CORS disabled; no `Access-Control-Allow-Origin` header
///   is ever added. This is the default and appropriate for local or
///   server-side-only consumers.
/// - `[HeaderValue::from_static("*")]` — wildcard; any origin is reflected.
///   The single-element invariant is enforced by the caller in `main.rs`.
/// - Any other non-empty list — exactly those origins are allowed; the
///   matching origin is reflected back in the response header.
///
/// The returned router is ready for `axum::serve` in the binary or
/// `tower::ServiceExt::oneshot` in tests.
pub fn build_router(state: AppState, cors_origins: Vec<HeaderValue>) -> Router {
    let router = Router::new()
        .route("/query", get(query_handler))
        .route("/stats", get(stats_handler))
        .route("/version", get(version_handler))
        .with_state(state);

    match build_cors_layer(cors_origins) {
        Some(cors) => router.layer(cors),
        None => router,
    }
}

/// Construct a [`CorsLayer`] from the parsed origin list, or return `None`
/// when CORS should be disabled entirely (empty list).
///
/// Kept private and separate from [`build_router`] so the CORS policy
/// logic is testable without constructing a full router.
///
/// Allowed methods are locked to `GET` only — the API is read-only and
/// must never advertise write methods to cross-origin callers.
fn build_cors_layer(origins: Vec<HeaderValue>) -> Option<CorsLayer> {
    if origins.is_empty() {
        return None;
    }

    let base = CorsLayer::new().allow_methods([Method::GET]);

    // A single raw `*` byte sequence means "allow any origin". Comparing
    // bytes rather than going through `HeaderValue::from_static` is
    // deliberate: it matches regardless of how the caller constructed the
    // HeaderValue and avoids a redundant allocation.
    if origins.len() == 1 && origins[0].as_bytes() == b"*" {
        Some(base.allow_origin(Any))
    } else {
        Some(base.allow_origin(AllowOrigin::list(origins)))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tower::ServiceExt; // for `oneshot`

    fn make_state() -> AppState {
        AppState::new(PathBuf::from("/tmp/does-not-need-to-exist-yet.db"))
    }

    /// Construct a GET /version request with the given Origin header.
    /// /version needs no DB, so make_state's phantom path is fine.
    fn version_request(origin: &str) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .uri("/version")
            .header("Origin", origin)
            .body(axum::body::Body::empty())
            .unwrap()
    }

    #[test]
    fn build_router_produces_a_router_from_a_valid_state() {
        // Compile-time and type-plumbing smoke test. Real behaviour is
        // validated by the integration test suite and the CORS tests below.
        let _router: Router = build_router(make_state(), vec![]);
    }

    #[tokio::test]
    async fn no_cors_origins_does_not_add_acao_header() {
        let resp = build_router(make_state(), vec![])
            .oneshot(version_request("https://example.com"))
            .await
            .unwrap();
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "CORS disabled: ACAO header must be absent"
        );
    }

    #[tokio::test]
    async fn wildcard_cors_adds_acao_star_for_any_origin() {
        let resp = build_router(make_state(), vec![HeaderValue::from_static("*")])
            .oneshot(version_request("https://example.com"))
            .await
            .unwrap();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*"),
        );
    }

    #[tokio::test]
    async fn specific_origin_reflects_matching_origin() {
        let allowed: HeaderValue = "https://app.example.com".parse().unwrap();
        let resp = build_router(make_state(), vec![allowed])
            .oneshot(version_request("https://app.example.com"))
            .await
            .unwrap();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://app.example.com"),
        );
    }

    #[tokio::test]
    async fn specific_origin_omits_acao_for_unmatched_origin() {
        let allowed: HeaderValue = "https://app.example.com".parse().unwrap();
        let resp = build_router(make_state(), vec![allowed])
            .oneshot(version_request("https://evil.example.com"))
            .await
            .unwrap();
        // CORS enforcement is the browser's job. The server omits ACAO for
        // a non-matching origin but does not reject the request outright —
        // that is correct per spec.
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "non-allowed origin must not receive ACAO header"
        );
    }
}
