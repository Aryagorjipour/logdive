//! Format-aware line parsing for log ingestion.
//!
//! v0.2.0 introduced multiple input formats. This module contains:
//!   - the per-format parsers as submodules (`json`, `logfmt`, `plain`),
//!   - the [`LogFormat`] enum used to select among them,
//!   - and the [`parse_line`] dispatcher that routes each line to the
//!     right submodule based on the chosen format.
//!
//! All three submodules expose a `parse_line(line: &str) -> Option<LogEntry>`
//! function with the same graceful-skip contract: returns `None` on empty,
//! whitespace-only, or unparseable input. The dispatcher is a thin
//! `match` over the format selector — power users who already know the
//! format ahead of time can call the submodule directly.
//!
//! Submodules are intentionally `pub mod`: third-party consumers (a
//! plugin doing a custom ingestion pipeline, say) sometimes want to bypass
//! the dispatcher. The dispatcher remains the canonical entry point and
//! is what the CLI ingest path uses.

pub mod json;
pub mod logfmt;
pub mod plain;

use crate::entry::LogEntry;

/// Selects which line parser the dispatcher uses.
///
/// `Default` is [`LogFormat::Json`] — the v0.1.0 default carried forward
/// so callers that don't explicitly pick a format get the same behavior
/// they used to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Structured JSON, one object per line. The v0.1.0 default.
    #[default]
    Json,
    /// logfmt — `key=value` pairs, see `parsers::logfmt`.
    Logfmt,
    /// Unstructured plaintext. The whole line becomes `LogEntry::message`.
    Plain,
}

impl LogFormat {
    /// Every [`LogFormat`] variant, in declaration order.
    ///
    /// Use this when you need to enumerate all supported formats without
    /// hard-coding the list at a call site — for example, the API
    /// `/version` endpoint reports this slice so clients know what the
    /// running binary accepts. Adding a new variant here automatically
    /// propagates to every such consumer.
    pub const ALL: &'static [Self] = &[Self::Json, Self::Logfmt, Self::Plain];

    /// Parse a CLI-style format name. Case-insensitive.
    ///
    /// Returns `None` for unrecognized names. The CLI wraps this in a
    /// `clap` value parser that surfaces the unknown name as a usage
    /// error; library consumers can call it directly.
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "logfmt" => Some(Self::Logfmt),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }

    /// Canonical short name used in CLI flags, configuration, and any
    /// future `Display`-based contexts. Always one lowercase word that
    /// round-trips through [`Self::from_name`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Logfmt => "logfmt",
            Self::Plain => "plain",
        }
    }
}

impl std::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Parse a single log line according to the chosen format.
///
/// Routes to the format-specific parser. Returns `None` under the standard
/// graceful-skip contract — empty input, whitespace-only input, or
/// format-specific malformed input (unterminated quote in logfmt,
/// non-object JSON, etc.).
pub fn parse_line(format: LogFormat, line: &str) -> Option<LogEntry> {
    match format {
        LogFormat::Json => json::parse_line(line),
        LogFormat::Logfmt => logfmt::parse_line(line),
        LogFormat::Plain => plain::parse_line(line),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- LogFormat name parsing ---------------------------------------

    #[test]
    fn from_name_recognizes_known_formats() {
        assert_eq!(LogFormat::from_name("json"), Some(LogFormat::Json));
        assert_eq!(LogFormat::from_name("logfmt"), Some(LogFormat::Logfmt));
        assert_eq!(LogFormat::from_name("plain"), Some(LogFormat::Plain));
    }

    #[test]
    fn from_name_is_case_insensitive() {
        assert_eq!(LogFormat::from_name("JSON"), Some(LogFormat::Json));
        assert_eq!(LogFormat::from_name("Logfmt"), Some(LogFormat::Logfmt));
        assert_eq!(LogFormat::from_name("PLAIN"), Some(LogFormat::Plain));
    }

    #[test]
    fn from_name_returns_none_for_unknown() {
        assert!(LogFormat::from_name("yaml").is_none());
        assert!(LogFormat::from_name("plaintext").is_none()); // no aliases
        assert!(LogFormat::from_name("").is_none());
    }

    #[test]
    fn name_round_trips_through_from_name() {
        for variant in [LogFormat::Json, LogFormat::Logfmt, LogFormat::Plain] {
            assert_eq!(LogFormat::from_name(variant.name()), Some(variant));
        }
    }

    #[test]
    fn default_is_json() {
        assert_eq!(LogFormat::default(), LogFormat::Json);
    }

    #[test]
    fn display_uses_canonical_name() {
        assert_eq!(format!("{}", LogFormat::Json), "json");
        assert_eq!(format!("{}", LogFormat::Logfmt), "logfmt");
        assert_eq!(format!("{}", LogFormat::Plain), "plain");
    }

    // ----- ALL const ---------------------------------------------------

    #[test]
    fn all_contains_every_variant() {
        assert_eq!(LogFormat::ALL.len(), 3);
        assert!(LogFormat::ALL.contains(&LogFormat::Json));
        assert!(LogFormat::ALL.contains(&LogFormat::Logfmt));
        assert!(LogFormat::ALL.contains(&LogFormat::Plain));
    }

    #[test]
    fn all_names_round_trip_through_from_name() {
        // LogFormat: Copy, so *format is fine when iterating &[LogFormat].
        for format in LogFormat::ALL {
            assert_eq!(LogFormat::from_name(format.name()), Some(*format));
        }
    }

    // ----- Dispatcher routing -------------------------------------------
    //
    // The dispatcher is one match over three arms — the trivial-mistake
    // failure mode is "right format dispatched to wrong parser." Each
    // routing test compares the dispatcher's output with the same call
    // made directly to the format-specific submodule. If the match arms
    // ever get swapped, these tests fail loudly.

    #[test]
    fn dispatcher_json_matches_direct_json_call() {
        let line = r#"{"timestamp":"2026-04-15T09:00:00Z","level":"info","message":"hi"}"#;
        let direct = json::parse_line(line);
        let routed = parse_line(LogFormat::Json, line);
        assert_eq!(direct, routed);
        assert!(direct.is_some(), "fixture line should parse");
    }

    #[test]
    fn dispatcher_logfmt_matches_direct_logfmt_call() {
        let line = "level=info service=payments req_id=42";
        let direct = logfmt::parse_line(line);
        let routed = parse_line(LogFormat::Logfmt, line);
        assert_eq!(direct, routed);
        assert!(direct.is_some());
    }

    #[test]
    fn dispatcher_plain_matches_direct_plain_call() {
        let line = "starting service version 2.4.1";
        let direct = plain::parse_line(line);
        let routed = parse_line(LogFormat::Plain, line);
        assert_eq!(direct, routed);
        assert!(direct.is_some());
    }

    #[test]
    fn dispatcher_returns_none_for_empty_line_in_every_format() {
        // Each parser has its own None-on-empty rule — the dispatcher
        // mustn't accidentally treat one of them differently.
        for format in [LogFormat::Json, LogFormat::Logfmt, LogFormat::Plain] {
            assert!(parse_line(format, "").is_none(), "format {format} on empty");
            assert!(
                parse_line(format, "   \t  ").is_none(),
                "format {format} on whitespace"
            );
        }
    }
}
