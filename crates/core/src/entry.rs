//! The `LogEntry` type — a single parsed log line.
//!
//! This is the canonical in-memory representation used by every layer of
//! logdive: the parser produces `LogEntry` values, the indexer consumes them,
//! and the executor returns them from queries.
//!
//! Field layout follows the hybrid storage decision in the project doc
//! (decisions log, 2026-04-19): the four "known" fields (`timestamp`, `level`,
//! `message`, `tag`) are first-class members that map to indexed SQLite
//! columns, while everything else lives in the `fields` map and is persisted
//! as a JSON blob queried via `json_extract()`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A single structured log entry.
///
/// All four known fields are `Option<String>` because a JSON line may omit
/// any of them and still be worth indexing — the parser's contract per the
/// milestone 1 spec is "missing optional fields use `None` not panic".
///
/// `fields` holds only keys that are *not* among the known four. The parser
/// is responsible for lifting known keys out of the JSON object and into
/// their dedicated fields before populating this map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// The entry's timestamp as it appeared in the source line. Stored as a
    /// string rather than a parsed `DateTime` so unusual formats survive
    /// ingestion; time-range filtering is applied at query time.
    pub timestamp: Option<String>,

    /// Log severity ("error", "warn", "info", ...). Indexed column.
    pub level: Option<String>,

    /// The human-readable message body.
    pub message: Option<String>,

    /// Optional source tag supplied at ingestion time (the `--tag` CLI flag,
    /// per the "Decisions log" entry on optional tags). `None` means
    /// "untagged" and such entries match queries without a tag filter.
    pub tag: Option<String>,

    /// Arbitrary additional keys from the JSON object. Serialized to the
    /// `fields TEXT` column and queried via SQLite's `json_extract()`.
    pub fields: Map<String, Value>,

    /// The original, unmodified source line. Feeds both the `raw` column
    /// and the `blake3`-based dedup hash.
    pub raw: String,
}

impl LogEntry {
    /// The set of top-level JSON keys that are promoted to first-class
    /// struct fields during parsing. Any key *not* in this set is preserved
    /// in [`LogEntry::fields`].
    ///
    /// Kept in a single place so the parser and any future schema-related
    /// code agree on which keys are "known".
    pub const KNOWN_KEYS: &'static [&'static str] = &["timestamp", "level", "message", "tag"];

    /// Construct a new entry from the raw source line, with all optional
    /// fields unset and an empty `fields` map. The parser uses this as a
    /// starting point and fills in the pieces it finds.
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            timestamp: None,
            level: None,
            message: None,
            tag: None,
            fields: Map::new(),
            raw: raw.into(),
        }
    }

    /// Override the tag. Used by the indexer to apply the `--tag` flag
    /// supplied at ingestion time: if the source JSON did not carry its own
    /// `tag`, the CLI-provided one is substituted here.
    pub fn with_tag(mut self, tag: Option<String>) -> Self {
        if tag.is_some() {
            self.tag = tag;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entry_has_no_known_fields_and_empty_map() {
        let e = LogEntry::new("hello");
        assert_eq!(e.raw, "hello");
        assert!(e.timestamp.is_none());
        assert!(e.level.is_none());
        assert!(e.message.is_none());
        assert!(e.tag.is_none());
        assert!(e.fields.is_empty());
    }

    #[test]
    fn with_tag_sets_when_some() {
        let e = LogEntry::new("x").with_tag(Some("api".to_string()));
        assert_eq!(e.tag.as_deref(), Some("api"));
    }

    #[test]
    fn with_tag_is_a_noop_when_none() {
        let e = LogEntry::new("x")
            .with_tag(Some("first".to_string()))
            .with_tag(None);
        // An existing tag is NOT cleared by passing None — None means
        // "no override supplied", not "clear the tag".
        assert_eq!(e.tag.as_deref(), Some("first"));
    }

    #[test]
    fn known_keys_are_the_four_documented_ones() {
        // Guards against accidental drift — if someone adds a known field,
        // this test makes them update both the constant and this assertion.
        assert_eq!(
            LogEntry::KNOWN_KEYS,
            &["timestamp", "level", "message", "tag"]
        );
    }

    #[test]
    fn roundtrips_through_serde_json() {
        let mut e = LogEntry::new(r#"{"level":"error","service":"pay"}"#);
        e.level = Some("error".to_string());
        e.fields
            .insert("service".to_string(), Value::String("pay".to_string()));

        let s = serde_json::to_string(&e).expect("serialize");
        let back: LogEntry = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(e, back);
    }
}
