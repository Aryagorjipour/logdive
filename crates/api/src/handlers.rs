//! Axum handlers for the three public endpoints.
//!
//! All handlers are async, but every one that touches the database
//! delegates SQLite work to [`AppState::with_connection`], which runs on
//! Tokio's blocking-task pool. The handlers themselves do parsing,
//! parameter validation, and response shaping — nothing else.
//!
//! `GET /version` is the exception: it returns compile-time constants only
//! and never opens the database.

use axum::{
    Json,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use logdive_core::{LogEntry, LogFormat, QueryOptions, Stats, execute, parse_query};

use crate::error::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// GET /query
// ---------------------------------------------------------------------------

/// Query-string parameters accepted by `GET /query`.
///
/// `q` is modeled as `Option<String>` rather than a required field so the
/// "missing `q`" error message comes from our own `AppError::bad_request`
/// path rather than from Axum's generic extractor rejection.
///
/// `offset` is optional; absent or `0` both mean "start from the first result".
/// Mirrors the CLI's `--offset` behaviour so both surfaces page identically.
#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub q: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Default cap on result set size when `limit` is not supplied. Mirrors
/// the CLI's `--limit` default so the two surfaces behave identically.
const DEFAULT_LIMIT: usize = 1000;

/// `GET /query?q=<expr>&limit=<n>&offset=<m>`
///
/// Returns matching log entries as newline-delimited JSON, one entry per
/// line. A missing `limit` defaults to 1000; `limit=0` means unlimited.
/// A missing or zero `offset` starts from the first result.
pub async fn query_handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Result<Response, AppError> {
    // Validate: `q` is required and non-empty (post-trim).
    let query_str = params
        .q
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("missing or empty `q` parameter"))?;

    // Apply the same "0 = unlimited" and "0 = absent" rules as the CLI.
    let limit = match params.limit.unwrap_or(DEFAULT_LIMIT) {
        0 => None,
        n => Some(n),
    };
    let offset = match params.offset.unwrap_or(0) {
        0 => None,
        n => Some(n),
    };

    // Parse the query up-front so parse errors are classified by the
    // `From<LogdiveError>` impl before we touch the DB.
    let ast = parse_query(&query_str)?;

    tracing::debug!(query = %query_str, ?limit, ?offset, "executing query over HTTP");

    let rows: Vec<LogEntry> = state
        .with_connection(move |indexer| {
            execute(&ast, indexer.connection(), QueryOptions { limit, offset })
        })
        .await?;

    tracing::debug!(
        result_count = rows.len(),
        "query returned results over HTTP"
    );

    Ok(build_ndjson_response(&rows))
}

/// Serialize a slice of entries into a newline-delimited JSON body and
/// wrap it in a response with the correct `Content-Type`.
///
/// Empty result sets produce an empty body (zero bytes, zero lines),
/// which is valid NDJSON and pipeline-friendly. Status is 200 OK in
/// both the empty and non-empty cases — "no matches" is a successful
/// query, not a not-found.
fn build_ndjson_response(rows: &[LogEntry]) -> Response {
    let mut body = String::with_capacity(rows.len() * 256);
    for row in rows {
        body.push_str(&entry_to_json_string(row));
        body.push('\n');
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        body,
    )
        .into_response()
}

/// Render a single `LogEntry` as a JSON object string.
///
/// The shape matches the CLI's `--format json` output so clients can treat
/// both surfaces interchangeably: `timestamp`, `level`, `message`, `tag`,
/// `fields`, `raw`. `tag` is `null` when absent; `fields` is always an
/// object.
fn entry_to_json_string(entry: &LogEntry) -> String {
    serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string())
}

// ---------------------------------------------------------------------------
// GET /stats
// ---------------------------------------------------------------------------

/// Wire shape for `GET /stats` responses.
///
/// Intentionally decoupled from `logdive_core::Stats` so the library stays
/// serde-agnostic on its public output types. Renaming core fields is
/// then a non-breaking change for HTTP clients; conversely, the HTTP
/// shape can evolve without touching core.
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub entries: u64,
    pub min_timestamp: Option<String>,
    pub max_timestamp: Option<String>,
    /// Distinct tag values. `None` (untagged) appears first when present,
    /// then non-null tags in ascending alphabetical order — identical to
    /// the `Stats.tags` contract from core. Clients presenting this to
    /// humans can reshuffle (e.g. put `null` last, label it "(untagged)")
    /// the way the CLI does.
    pub tags: Vec<Option<String>>,
    pub db_size_bytes: u64,
    pub db_path: String,
}

impl StatsResponse {
    fn from_core(stats: Stats, db_path: String, db_size_bytes: u64) -> Self {
        Self {
            entries: stats.entries,
            min_timestamp: stats.min_timestamp,
            max_timestamp: stats.max_timestamp,
            tags: stats.tags,
            db_size_bytes,
            db_path,
        }
    }
}

/// `GET /stats`
///
/// Returns aggregate metadata about the index as a single JSON object.
pub async fn stats_handler(State(state): State<AppState>) -> Result<Json<StatsResponse>, AppError> {
    let db_path_for_response = state.db_path.display().to_string();
    let db_path_for_closure = state.db_path.clone();

    let (stats, size_bytes) = state
        .with_connection(move |indexer| {
            // Run both queries under the same blocking task so we don't
            // round-trip back to the async runtime between them.
            let stats = indexer.stats()?;
            let size_bytes = std::fs::metadata(&db_path_for_closure)
                .map(|m| m.len())
                .map_err(|e| logdive_core::LogdiveError::io_at(&db_path_for_closure, e))?;
            Ok::<_, logdive_core::LogdiveError>((stats, size_bytes))
        })
        .await?;

    let response = StatsResponse::from_core(stats, db_path_for_response, size_bytes);
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// GET /version
// ---------------------------------------------------------------------------

/// Wire shape for `GET /version` responses.
///
/// All fields are compile-time constants — no database access is required.
/// The primary use case is client-side feature detection: a UI or script
/// calls `/version` on startup to discover which formats and endpoints the
/// running server supports before making assumptions about its capabilities.
#[derive(Debug, Serialize)]
pub struct VersionResponse {
    /// Semver version of the running `logdive-api` binary, injected from
    /// `CARGO_PKG_VERSION` at compile time.
    pub version: &'static str,
    /// Ingest formats the binary was compiled with, sourced from
    /// [`LogFormat::ALL`]. Stays in sync with core without manual
    /// maintenance — adding a new format to core propagates here for free.
    pub formats: Vec<&'static str>,
    /// API endpoint names available on this server, sorted alphabetically.
    pub capabilities: Vec<&'static str>,
}

/// `GET /version`
///
/// Returns a JSON object describing the running server's version and
/// capabilities. Built entirely from compile-time constants; never touches
/// the database. Always returns `200 OK`.
pub async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
        formats: LogFormat::ALL.iter().map(|f| f.name()).collect(),
        capabilities: vec!["query", "stats", "version"],
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn sample_entry() -> LogEntry {
        let raw = r#"{"timestamp":"2026-04-22T14:03:21Z","level":"error","message":"boom"}"#;
        let mut e = LogEntry::new(raw.to_string());
        e.timestamp = Some("2026-04-22T14:03:21Z".to_string());
        e.level = Some("error".to_string());
        e.message = Some("boom".to_string());
        e
    }

    #[test]
    fn entry_to_json_string_is_valid_json_with_expected_shape() {
        let e = sample_entry();
        let s = entry_to_json_string(&e);
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["timestamp"], "2026-04-22T14:03:21Z");
        assert_eq!(v["level"], "error");
        assert_eq!(v["message"], "boom");
        assert!(v["tag"].is_null());
        assert!(v["fields"].is_object());
        assert!(v["raw"].is_string());
    }

    #[test]
    fn entry_to_json_string_preserves_tag_and_fields() {
        let mut e = sample_entry();
        e.tag = Some("api".to_string());
        e.fields
            .insert("service".to_string(), Value::String("payments".to_string()));

        let s = entry_to_json_string(&e);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["tag"], "api");
        assert_eq!(v["fields"]["service"], "payments");
    }

    #[test]
    fn stats_response_from_core_round_trips_all_fields() {
        let mut idx = logdive_core::Indexer::open_in_memory().unwrap();
        let mut e = sample_entry();
        e.tag = Some("api".to_string());
        idx.insert_batch(&[e]).unwrap();

        let stats = idx.stats().unwrap();
        let resp = StatsResponse::from_core(stats, "/tmp/idx.db".to_string(), 4096);

        assert_eq!(resp.entries, 1);
        assert_eq!(resp.db_path, "/tmp/idx.db");
        assert_eq!(resp.db_size_bytes, 4096);
        assert_eq!(resp.tags, vec![Some("api".to_string())]);

        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["entries"], 1);
        assert_eq!(v["db_path"], "/tmp/idx.db");
        assert_eq!(v["db_size_bytes"], 4096);
        assert_eq!(v["tags"][0], "api");
    }

    #[test]
    fn stats_response_renders_null_for_empty_time_bounds() {
        let idx = logdive_core::Indexer::open_in_memory().unwrap();
        let stats = idx.stats().unwrap();
        let resp = StatsResponse::from_core(stats, "/tmp/empty.db".to_string(), 0);

        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["min_timestamp"].is_null());
        assert!(v["max_timestamp"].is_null());
        assert_eq!(v["tags"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn build_ndjson_response_sets_correct_content_type() {
        let mut e = sample_entry();
        e.tag = Some("api".to_string());
        let resp = build_ndjson_response(&[e]);

        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(content_type, "application/x-ndjson");
    }

    #[test]
    fn build_ndjson_response_is_ok_for_empty_results() {
        let resp = build_ndjson_response(&[]);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ----- GET /version ------------------------------------------------

    #[tokio::test]
    async fn version_handler_returns_current_package_version() {
        let Json(resp) = version_handler().await;
        assert_eq!(resp.version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn version_handler_formats_match_logformat_all() {
        let Json(resp) = version_handler().await;
        let expected: Vec<&'static str> = LogFormat::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(resp.formats, expected);
    }

    #[tokio::test]
    async fn version_handler_capabilities_are_sorted_and_complete() {
        let Json(resp) = version_handler().await;
        assert_eq!(resp.capabilities, vec!["query", "stats", "version"]);
        // Guard: the list must stay sorted — future additions must maintain this.
        let mut sorted = resp.capabilities.clone();
        sorted.sort_unstable();
        assert_eq!(
            resp.capabilities, sorted,
            "capabilities must be sorted alphabetically"
        );
    }

    #[test]
    fn version_response_serializes_to_expected_json_shape() {
        let resp = VersionResponse {
            version: "0.2.0",
            formats: vec!["json", "logfmt", "plain"],
            capabilities: vec!["query", "stats", "version"],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["version"], "0.2.0");
        assert_eq!(v["formats"], serde_json::json!(["json", "logfmt", "plain"]));
        assert_eq!(
            v["capabilities"],
            serde_json::json!(["query", "stats", "version"])
        );
    }
}
