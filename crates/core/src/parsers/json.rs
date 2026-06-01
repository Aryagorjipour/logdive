//! JSON line parser — the original v0.1 ingest path, moved into the
//! `parsers` subdirectory in v0.2.0 alongside `logfmt` and `plain`.
//!
//! This module produces a [`LogEntry`] from one line of JSON-formatted
//! input. It is invoked through the format dispatcher in
//! [`crate::parsers::parse_line`] when the caller selects
//! [`crate::parsers::LogFormat::Json`].
//!
//! Behavior is bit-for-bit identical to v0.1: malformed input is silently
//! skipped (returns `None`), known keys are lifted into struct fields, and
//! unknown keys are preserved in `LogEntry::fields` for `json_extract()`-
//! based querying downstream.

use serde_json::Value;

use crate::entry::LogEntry;

/// Parse a single line of JSON log input.
///
/// Returns `Some(LogEntry)` if `line` is a non-empty JSON object, otherwise
/// `None`. Callers iterate over an input source and discard `None` results
/// (optionally incrementing a "lines skipped" counter — the CLI does this
/// during ingestion).
///
/// # Behaviour
///
/// - Empty or whitespace-only lines return `None`.
/// - Lines that are not valid JSON return `None`.
/// - Lines that are valid JSON but not objects (e.g. `42`, `"hi"`, `[1,2]`)
///   return `None`, because logdive's ingestion contract restricts JSON
///   input to top-level objects.
/// - Within an object, keys matching [`LogEntry::KNOWN_KEYS`] populate the
///   corresponding struct fields; all other keys go into `LogEntry::fields`.
/// - For the known string-typed fields, non-string scalar values (numbers,
///   booleans, null) are stringified so information is preserved. Object
///   and array values for known fields are *not* coerced — instead they
///   remain in `fields` under their original key, leaving the known field
///   as `None`.
pub fn parse_line(line: &str) -> Option<LogEntry> {
    if line.trim().is_empty() {
        return None;
    }

    let value: Value = serde_json::from_str(line).ok()?;
    let obj = match value {
        Value::Object(map) => map,
        _ => return None,
    };

    let mut entry = LogEntry::new(line);

    for (key, value) in obj {
        match key.as_str() {
            "timestamp" => match coerce_scalar_to_string(&value) {
                Some(s) => entry.timestamp = Some(s),
                None => {
                    entry.fields.insert(key, value);
                }
            },
            "level" => match coerce_scalar_to_string(&value) {
                Some(s) => entry.level = Some(s),
                None => {
                    entry.fields.insert(key, value);
                }
            },
            "message" => match coerce_scalar_to_string(&value) {
                Some(s) => entry.message = Some(s),
                None => {
                    entry.fields.insert(key, value);
                }
            },
            "tag" => match coerce_scalar_to_string(&value) {
                Some(s) => entry.tag = Some(s),
                None => {
                    entry.fields.insert(key, value);
                }
            },
            _ => {
                entry.fields.insert(key, value);
            }
        }
    }

    Some(entry)
}

/// Convert a JSON scalar to its string form for storage in a known
/// `Option<String>` field.
///
/// Returns `None` for objects and arrays — the caller preserves those under
/// their original key in `LogEntry::fields` instead of losing structure via
/// stringification.
fn coerce_scalar_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Object(_) | Value::Array(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_fully_populated_line() {
        let line = r#"{"timestamp":"2026-04-19T10:00:00Z","level":"error","message":"boom","service":"payments","req_id":42}"#;
        let e = parse_line(line).expect("should parse");

        assert_eq!(e.timestamp.as_deref(), Some("2026-04-19T10:00:00Z"));
        assert_eq!(e.level.as_deref(), Some("error"));
        assert_eq!(e.message.as_deref(), Some("boom"));
        assert!(e.tag.is_none());
        assert_eq!(e.fields.get("service"), Some(&json!("payments")));
        assert_eq!(e.fields.get("req_id"), Some(&json!(42)));
        assert_eq!(e.raw, line);
    }

    #[test]
    fn missing_known_fields_become_none_without_panic() {
        // Only one known key present; three missing.
        let e = parse_line(r#"{"level":"info"}"#).expect("should parse");
        assert_eq!(e.level.as_deref(), Some("info"));
        assert!(e.timestamp.is_none());
        assert!(e.message.is_none());
        assert!(e.tag.is_none());
        assert!(e.fields.is_empty());
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_line(r#"{"level": "error""#).is_none()); // truncated
        assert!(parse_line("not json at all").is_none());
        assert!(parse_line("{this is broken}").is_none());
    }

    #[test]
    fn empty_and_whitespace_lines_return_none() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
        assert!(parse_line("\t\n").is_none());
    }

    #[test]
    fn valid_json_but_not_an_object_returns_none() {
        // Ingestion scope: structured JSON *objects* only.
        assert!(parse_line("42").is_none());
        assert!(parse_line(r#""hello""#).is_none());
        assert!(parse_line("[1,2,3]").is_none());
        assert!(parse_line("true").is_none());
        assert!(parse_line("null").is_none());
    }

    #[test]
    fn unknown_keys_land_in_fields_map() {
        let e =
            parse_line(r#"{"user_id":"u-1","duration_ms":123,"ok":true}"#).expect("should parse");
        assert_eq!(e.fields.len(), 3);
        assert_eq!(e.fields.get("user_id"), Some(&json!("u-1")));
        assert_eq!(e.fields.get("duration_ms"), Some(&json!(123)));
        assert_eq!(e.fields.get("ok"), Some(&json!(true)));
    }

    #[test]
    fn numeric_level_is_stringified() {
        // Syslog-style numeric severities are common. Preserve the info.
        let e = parse_line(r#"{"level":3}"#).expect("should parse");
        assert_eq!(e.level.as_deref(), Some("3"));
        // The numeric value was consumed into `level`, not duplicated into fields.
        assert!(e.fields.is_empty());
    }

    #[test]
    fn boolean_and_null_known_fields_are_stringified() {
        let e = parse_line(r#"{"tag":true,"message":null}"#).expect("should parse");
        assert_eq!(e.tag.as_deref(), Some("true"));
        assert_eq!(e.message.as_deref(), Some("null"));
    }

    #[test]
    fn object_valued_known_field_is_preserved_in_fields_map() {
        // `message` is an object — we refuse to stringify lossily. Instead the
        // original key/value is kept in `fields`, and the known field stays None.
        let line = r#"{"message":{"code":500,"text":"err"}}"#;
        let e = parse_line(line).expect("should parse");
        assert!(e.message.is_none());
        assert_eq!(
            e.fields.get("message"),
            Some(&json!({"code": 500, "text": "err"}))
        );
    }

    #[test]
    fn array_valued_known_field_is_preserved_in_fields_map() {
        let e = parse_line(r#"{"tag":["a","b"]}"#).expect("should parse");
        assert!(e.tag.is_none());
        assert_eq!(e.fields.get("tag"), Some(&json!(["a", "b"])));
    }

    #[test]
    fn raw_is_preserved_verbatim_including_whitespace() {
        // Dedup hashing depends on byte-exact preservation.
        let line = "  {\"level\":\"info\"}  ";
        let e = parse_line(line).expect("should parse");
        assert_eq!(e.raw, line);
    }

    #[test]
    fn empty_json_object_is_a_valid_entry() {
        // `{}` parses, produces an entry with everything None and no fields.
        // Whether the indexer accepts such a row is the indexer's call.
        let e = parse_line("{}").expect("should parse");
        assert!(e.timestamp.is_none());
        assert!(e.level.is_none());
        assert!(e.message.is_none());
        assert!(e.tag.is_none());
        assert!(e.fields.is_empty());
        assert_eq!(e.raw, "{}");
    }

    #[test]
    fn utf8_bom_prefix_returns_none() {
        // A UTF-8 BOM (\xEF\xBB\xBF) before `{` produces bytes that
        // serde_json cannot parse as JSON — the parser must return None
        // gracefully rather than propagating an error.
        let line_with_bom = "\u{FEFF}{\"level\":\"info\"}";
        assert!(
            parse_line(line_with_bom).is_none(),
            "BOM-prefixed line must be rejected"
        );
    }

    #[test]
    fn deeply_nested_object_in_known_field_is_preserved_in_fields_map() {
        // A deeply nested JSON value under `message` (a known key) must not
        // panic and must land in `fields` under its original key, with
        // `message` remaining None (non-string complex values are not coerced).
        let line = r#"{"message":{"a":{"b":{"c":{"d":{"e":"deep"}}}}}}"#;
        let e = parse_line(line).expect("valid JSON must parse");
        assert!(e.message.is_none(), "nested object must not be coerced");
        assert!(
            e.fields.contains_key("message"),
            "nested object must be preserved in fields"
        );
    }

    #[test]
    fn whitespace_in_known_field_value_is_preserved_not_trimmed() {
        // The parser must not trim whitespace from string values.
        // "  warn  " is stored verbatim so substring queries still match.
        let e = parse_line(r#"{"level":"  warn  "}"#).expect("should parse");
        assert_eq!(e.level.as_deref(), Some("  warn  "));
    }
}
