//! End-to-end integration tests for the HTTP API.
//!
//! These tests build a real [`Router`] via [`logdive_api::router::build_router`]
//! against a tempfile-backed SQLite database, then exercise it via
//! [`tower::ServiceExt::oneshot`] — which bypasses the network stack while
//! still running the full extractor → handler → `IntoResponse` pipeline.
//!
//! Coverage spans the two endpoints' happy paths, their user-fault error
//! paths (400), and the default unknown-route behavior (404). Server-fault
//! paths (500) are exercised by the unit tests inside `error.rs`; surfacing
//! them through a live request-response cycle would require deliberately
//! corrupting the DB mid-flight, which adds noise without meaningful
//! additional coverage.

use std::path::PathBuf;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt; // for `.oneshot`

use logdive_api::{router::build_router, state::AppState};
use logdive_core::{Indexer, LogEntry};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Create an empty, schema-initialized index in a fresh temp directory.
///
/// Returns the `TempDir` (must be kept alive for the lifetime of the test
/// to prevent early cleanup) and the DB path within it.
fn empty_db() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("index.db");
    // Touch the DB file and create the schema via the core writable opener.
    let _ = Indexer::open(&db).expect("init empty db");
    (dir, db)
}

/// Create a populated index with three entries spanning two tags and two
/// levels, suitable for exercising /query and /stats against real data.
fn populated_db() -> (TempDir, PathBuf) {
    let (dir, db) = empty_db();
    let mut idx = Indexer::open(&db).expect("reopen for populate");
    idx.insert_batch(&[
        entry(
            "2026-04-20T10:00:00Z",
            "error",
            "payment failed",
            Some("api"),
        ),
        entry(
            "2026-04-20T11:00:00Z",
            "info",
            "health check ok",
            Some("api"),
        ),
        entry("2026-04-20T12:00:00Z", "error", "database timeout", None),
    ])
    .expect("insert fixtures");
    (dir, db)
}

fn entry(ts: &str, level: &str, message: &str, tag: Option<&str>) -> LogEntry {
    let tag_part = tag.map(|t| format!(r#","tag":"{t}""#)).unwrap_or_default();
    let raw =
        format!(r#"{{"timestamp":"{ts}","level":"{level}","message":"{message}"{tag_part}}}"#);
    let mut e = LogEntry::new(raw);
    e.timestamp = Some(ts.to_string());
    e.level = Some(level.to_string());
    e.message = Some(message.to_string());
    e.tag = tag.map(|t| t.to_string());
    e
}

fn app(db: PathBuf) -> axum::Router {
    build_router(AppState::new(db))
}

async fn body_bytes(resp: axum::response::Response) -> Vec<u8> {
    resp.into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
        .to_vec()
}

async fn body_text(resp: axum::response::Response) -> String {
    String::from_utf8(body_bytes(resp).await).expect("utf-8 body")
}

async fn body_json(resp: axum::response::Response) -> Value {
    let text = body_text(resp).await;
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("body is not JSON: {e}; body=`{text}`"))
}

/// Parse an NDJSON body into a `Vec<Value>`. Empty bodies yield an empty vec.
fn parse_ndjson(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("each NDJSON line is valid JSON"))
        .collect()
}

// ---------------------------------------------------------------------------
// GET /stats
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stats_on_populated_db_returns_expected_shape() {
    let (_dir, db) = populated_db();
    let router = app(db.clone());

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(content_type.starts_with("application/json"));

    let v = body_json(resp).await;
    assert_eq!(v["entries"], 3);
    assert_eq!(v["min_timestamp"], "2026-04-20T10:00:00Z");
    assert_eq!(v["max_timestamp"], "2026-04-20T12:00:00Z");

    // Tags: null (untagged row) first, then "api". Clients get the raw
    // core ordering; no "(untagged)" rewriting at this layer.
    let tags = v["tags"].as_array().expect("tags is array");
    assert_eq!(tags.len(), 2);
    assert!(tags[0].is_null());
    assert_eq!(tags[1], "api");

    assert_eq!(v["db_path"], db.display().to_string());
    assert!(v["db_size_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn stats_on_empty_db_returns_zeroed_values() {
    let (_dir, db) = empty_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["entries"], 0);
    assert!(v["min_timestamp"].is_null());
    assert!(v["max_timestamp"].is_null());
    assert_eq!(v["tags"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// GET /query
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_with_matching_expression_returns_ndjson() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                // level=error matches two of the three fixture rows.
                .uri("/query?q=level%3Derror")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(ct, "application/x-ndjson");

    let text = body_text(resp).await;
    let rows = parse_ndjson(&text);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["level"] == "error"));
}

#[tokio::test]
async fn query_with_and_expression_narrows_results() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                // level=error AND tag=api → one row
                .uri("/query?q=level%3Derror+AND+tag%3Dapi")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let rows = parse_ndjson(&body_text(resp).await);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["message"], "payment failed");
}

#[tokio::test]
async fn query_with_or_expression_returns_union() {
    // OR is fully supported in v0.2.0. Both error rows and the info row
    // should be returned.
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                // level=error OR level=info → all three fixture rows
                .uri("/query?q=level%3Derror+OR+level%3Dinfo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let rows = parse_ndjson(&body_text(resp).await);
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn query_missing_q_parameter_returns_400() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/query")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    let msg = v["error"].as_str().unwrap_or_default();
    assert!(msg.contains('q'), "error message should mention `q`: {msg}");
}

#[tokio::test]
async fn query_empty_q_parameter_returns_400() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/query?q=")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_malformed_expression_returns_400() {
    let (_dir, db) = populated_db();
    let router = app(db);

    // A syntactically broken query: `level =` has no value after the
    // operator and is rejected by the parser regardless of grammar version.
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/query?q=level+%3D")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    assert!(!v["error"].as_str().unwrap_or_default().is_empty());
}

#[tokio::test]
async fn query_with_zero_results_returns_empty_ndjson_body() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/query?q=level%3Dnonsense")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let text = body_text(resp).await;
    // Empty body → zero NDJSON lines; still a successful response.
    assert!(text.is_empty() || text == "\n");
    assert_eq!(parse_ndjson(&text).len(), 0);
}

#[tokio::test]
async fn query_with_limit_zero_returns_all_matches_unlimited() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                // Match everything via a "since" clause far enough in the past.
                .uri("/query?q=since+2020-01-01&limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let rows = parse_ndjson(&body_text(resp).await);
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn query_respects_explicit_limit() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/query?q=since+2020-01-01&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let rows = parse_ndjson(&body_text(resp).await);
    assert_eq!(rows.len(), 1);
    // Default ordering is newest-first — expect the 12:00 row.
    assert_eq!(rows[0]["timestamp"], "2026-04-20T12:00:00Z");
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_route_returns_404() {
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/no-such-endpoint")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_query_endpoint_returns_405() {
    // GET-only: Axum rejects other methods with 405 Method Not Allowed.
    let (_dir, db) = populated_db();
    let router = app(db);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query?q=level%3Derror")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
