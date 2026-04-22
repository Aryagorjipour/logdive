//! HTTP-facing error type for the `logdive-api` server.
//!
//! Classifies errors into three HTTP-shaped buckets:
//!   - [`AppError::BadRequest`] — the client sent something malformed
//!     (missing `q`, unparseable query expression, bad datetime).
//!   - [`AppError::NotFound`] — an explicit miss by an endpoint, reserved
//!     for future endpoints that look up specific records. Not used for
//!     route-level misses; Axum handles unknown-route 404s on its own.
//!   - [`AppError::Internal`] — any failure below the request boundary
//!     (SQLite error, corrupt JSON in the index, I/O failure). These
//!     are logged in full to tracing but shown to the client only as a
//!     generic `"internal server error"` message.
//!
//! The status-code mapping lives in exactly one place: the `From<LogdiveError>`
//! impl. Handlers can therefore use `?` on any `LogdiveError`-returning
//! operation and get correct classification for free.
//!
//! For source types that are *not* `LogdiveError` but whose `LogdiveError`
//! conversion is already defined in core (e.g. [`QueryParseError`] via its
//! `#[from]` variant on `LogdiveError::QueryParse`), this module provides
//! an explicit shim `From` impl. Rust's `?` operator only performs a single
//! `From` conversion, so `Result<T, QueryParseError> -> Result<T, AppError>`
//! needs its own direct impl rather than going through `LogdiveError`
//! implicitly.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use logdive_core::{LogdiveError, QueryParseError};

/// HTTP error surface used across all handlers.
#[derive(Debug)]
pub enum AppError {
    /// Client sent a malformed request. Message is user-facing.
    BadRequest(String),

    /// A specific resource was not present. Message is user-facing.
    ///
    /// Reserved for future endpoints that look up by id. The current
    /// `GET /query` and `GET /stats` never emit this; returning zero
    /// matches from a query is a `200 OK` with an empty body, not a 404.
    #[allow(dead_code)]
    NotFound(String),

    /// Unexpected internal failure. The underlying `LogdiveError` is
    /// kept for operator-side logging and is never exposed to the
    /// client.
    Internal(LogdiveError),
}

impl AppError {
    /// Convenience constructor for 400 responses with a `Display` source.
    pub fn bad_request<M: std::fmt::Display>(msg: M) -> Self {
        Self::BadRequest(msg.to_string())
    }
}

/// Map `LogdiveError` variants to appropriate HTTP error classes.
///
/// This is the single source of truth for classification — handlers simply
/// use `?` and rely on this impl to do the right thing. Anything that
/// looks like "the user sent something bad" becomes `BadRequest`;
/// anything else becomes `Internal`.
impl From<LogdiveError> for AppError {
    fn from(err: LogdiveError) -> Self {
        match &err {
            LogdiveError::QueryParse(_)
            | LogdiveError::InvalidDatetime { .. }
            | LogdiveError::UnsafeFieldName(_) => AppError::BadRequest(err.to_string()),
            _ => AppError::Internal(err),
        }
    }
}

/// Explicit bridge from `QueryParseError` to `AppError`.
///
/// `parse_query` in core returns `Result<_, QueryParseError>` directly,
/// not wrapped in `LogdiveError`. Rust's `?` only performs a single
/// conversion via `From`, so even though `LogdiveError: From<QueryParseError>`
/// is defined in core, callers using `?` on `parse_query(...)?` from an
/// `AppError`-returning function need a direct impl. We delegate to the
/// `LogdiveError` path so classification stays in one place.
impl From<QueryParseError> for AppError {
    fn from(err: QueryParseError) -> Self {
        AppError::from(LogdiveError::from(err))
    }
}

/// JSON body shape returned for every error response.
///
/// Private to this module — handlers never construct one directly.
#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => {
                tracing::debug!(%msg, "400 bad request");
                (StatusCode::BAD_REQUEST, msg)
            }
            AppError::NotFound(msg) => {
                tracing::debug!(%msg, "404 not found");
                (StatusCode::NOT_FOUND, msg)
            }
            AppError::Internal(err) => {
                // Log the full underlying error for operators, but return
                // a sanitized message to the client. Users should never
                // see a SQLite error string or a filesystem path.
                tracing::warn!(error = %err, "500 internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        (status, Json(ErrorBody { error: &message })).into_response()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use logdive_core::parse_query;
    use serde_json::Value;

    /// Collect the response body into a UTF-8 string for assertion.
    async fn read_body(resp: Response) -> (StatusCode, String) {
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf-8 body");
        (status, text)
    }

    fn parse_error_body(text: &str) -> String {
        let v: Value = serde_json::from_str(text).expect("response body is JSON");
        v.get("error")
            .and_then(|e| e.as_str())
            .expect("body has `error` string field")
            .to_string()
    }

    #[tokio::test]
    async fn bad_request_renders_400_with_user_message() {
        let err = AppError::BadRequest("missing `q` parameter".to_string());
        let (status, text) = read_body(err.into_response()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parse_error_body(&text), "missing `q` parameter");
    }

    #[tokio::test]
    async fn not_found_renders_404_with_user_message() {
        let err = AppError::NotFound("no such entry".to_string());
        let (status, text) = read_body(err.into_response()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parse_error_body(&text), "no such entry");
    }

    #[tokio::test]
    async fn internal_renders_500_with_generic_message() {
        // Construct a real Sqlite error by trying to open a non-existent
        // read-only database — this gives us a genuine `LogdiveError::Sqlite`
        // without having to build rusqlite internals by hand.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.db");
        let inner =
            logdive_core::Indexer::open_read_only(&missing).expect_err("should fail on missing db");

        let err = AppError::Internal(inner);
        let (status, text) = read_body(err.into_response()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

        // Client sees the sanitized message, never the raw sqlite error.
        assert_eq!(parse_error_body(&text), "internal server error");
    }

    #[tokio::test]
    async fn from_logdive_error_maps_query_parse_to_bad_request() {
        // Parse a clearly malformed query to get a real QueryParse error.
        let query_err = parse_query("level =").expect_err("should not parse");
        let app_err: AppError = LogdiveError::from(query_err).into();
        let (status, text) = read_body(app_err.into_response()).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        // 400s surface the real message to the client.
        assert_ne!(parse_error_body(&text), "internal server error");
        assert!(!parse_error_body(&text).is_empty());
    }

    #[tokio::test]
    async fn from_query_parse_error_directly_maps_to_bad_request() {
        // Exercise the direct `From<QueryParseError> for AppError` bridge
        // — this is the shim that makes `parse_query(...)?` work inside
        // AppError-returning handlers.
        let query_err = parse_query("level =").expect_err("should not parse");
        let app_err: AppError = query_err.into();
        let (status, text) = read_body(app_err.into_response()).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_ne!(parse_error_body(&text), "internal server error");
    }

    #[tokio::test]
    async fn from_logdive_error_maps_invalid_datetime_to_bad_request() {
        let err = LogdiveError::InvalidDatetime {
            input: "not-a-date".to_string(),
            reason: "bad format".to_string(),
        };
        let app_err: AppError = err.into();
        let (status, text) = read_body(app_err.into_response()).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parse_error_body(&text)
            .to_lowercase()
            .contains("not-a-date"));
    }

    #[tokio::test]
    async fn from_logdive_error_maps_unsafe_field_name_to_bad_request() {
        let err = LogdiveError::UnsafeFieldName("service; DROP TABLE--".to_string());
        let app_err: AppError = err.into();
        let (status, _) = read_body(app_err.into_response()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn from_logdive_error_maps_other_to_internal() {
        // Sqlite error from a missing read-only DB — this path classifies
        // as internal because it's not a user-fault variant.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("also-missing.db");
        let inner =
            logdive_core::Indexer::open_read_only(&missing).expect_err("should fail on missing db");

        let app_err: AppError = inner.into();
        let (status, text) = read_body(app_err.into_response()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parse_error_body(&text), "internal server error");
    }

    #[test]
    fn bad_request_constructor_accepts_anything_displayable() {
        // Compile-time check that the helper works with &str, String, and
        // formatted strings uniformly.
        let _a: AppError = AppError::bad_request("literal");
        let _b: AppError = AppError::bad_request(String::from("owned"));
        let _c: AppError = AppError::bad_request(format_args!("formatted {}", 1));
    }
}
