//! Unified error type for `logdive-core`.
//!
//! All fallible APIs in this crate return [`Result<T>`], aliased to
//! `std::result::Result<T, LogdiveError>`. Module-local error types
//! from earlier milestones (e.g. [`QueryParseError`] from the `query`
//! module) are preserved as public types and convert into `LogdiveError`
//! via `From` impls, so `?` works seamlessly at API boundaries while
//! callers that need structured access (the CLI rendering parse-error
//! carets, for example) can still match against the richer inner type.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::query::QueryParseError;

/// Convenient crate-wide result alias.
pub type Result<T> = std::result::Result<T, LogdiveError>;

/// Every error the core crate can produce.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LogdiveError {
    /// Wrapper around a [`QueryParseError`], preserved as-is so callers
    /// that want the structured position/message can match on it.
    #[error(transparent)]
    QueryParse(#[from] QueryParseError),

    /// The `since <datetime>` clause contained a string that did not
    /// parse as any accepted datetime format.
    #[error("invalid datetime {input:?}: {reason}")]
    InvalidDatetime { input: String, reason: String },

    /// A field name slipped through the parser's validation. This is a
    /// defense-in-depth guard at the SQL-generation boundary and should
    /// be unreachable in practice.
    #[error("unsafe field name {0:?}")]
    UnsafeFieldName(String),

    /// A row came back from SQLite with a malformed `fields` JSON column.
    /// Indicates corruption or an out-of-band write to the database.
    #[error("corrupt fields JSON in row: {0}")]
    CorruptFieldsJson(#[source] serde_json::Error),

    /// Underlying SQLite error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// I/O error while creating the index directory, opening the database
    /// file, or reading a log file for ingestion.
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Generic I/O error without an associated path (for stream-style
    /// ingestion from stdin, where there isn't a meaningful path).
    #[error("io error: {0}")]
    IoBare(#[from] io::Error),

    /// Miscellaneous serde error not covered by a more specific variant.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl LogdiveError {
    /// Construct an [`LogdiveError::Io`] with the offending path attached.
    /// Preferred over the bare `From<io::Error>` conversion when a path
    /// is known — the error message is markedly more useful.
    pub fn io_at(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parse_error_converts_via_question_mark() {
        // Structural test: `QueryParseError` must be `?`-convertible into
        // LogdiveError so consumer modules can `use crate::Result;` and
        // propagate parse errors without an explicit `.map_err`.
        fn try_parse() -> Result<()> {
            let ast = crate::query::parse("level=")?; // malformed
            let _ = ast;
            Ok(())
        }
        let err = try_parse().unwrap_err();
        assert!(matches!(err, LogdiveError::QueryParse(_)));
    }

    #[test]
    fn sqlite_error_converts_via_question_mark() {
        fn do_thing() -> Result<()> {
            let conn = rusqlite::Connection::open_in_memory()?;
            conn.execute("this is not valid SQL", [])?;
            Ok(())
        }
        let err = do_thing().unwrap_err();
        assert!(matches!(err, LogdiveError::Sqlite(_)));
    }

    #[test]
    fn json_error_converts_via_question_mark() {
        fn do_thing() -> Result<serde_json::Value> {
            let v = serde_json::from_str("not json")?;
            Ok(v)
        }
        let err = do_thing().unwrap_err();
        assert!(matches!(err, LogdiveError::Json(_)));
    }

    #[test]
    fn io_at_attaches_path_to_error_message() {
        let src = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = LogdiveError::io_at("/tmp/never-exists.db", src);
        let msg = format!("{err}");
        assert!(msg.contains("/tmp/never-exists.db"));
        assert!(msg.contains("missing"));
    }

    #[test]
    fn invalid_datetime_formats_both_input_and_reason() {
        let err = LogdiveError::InvalidDatetime {
            input: "not-a-date".to_string(),
            reason: "expected RFC3339".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("not-a-date"));
        assert!(msg.contains("RFC3339"));
    }
}
