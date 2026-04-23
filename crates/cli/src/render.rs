//! Rendering of query results for the `query` subcommand.
//!
//! Two output formats are supported, per the "Output formats" decision in
//! the project doc (2026-04-19: `pretty` and `json` only):
//!
//! - `pretty`: human-readable, optionally colored. Colors are auto-disabled
//!   when stdout is not a terminal, when the `NO_COLOR` environment variable
//!   is set, or when the user explicitly pipes the output. All of these
//!   cases are handled transparently by `anstream::AutoStream::auto`.
//!
//! - `json`: newline-delimited JSON, one `LogEntry` per line. Suitable for
//!   piping into `jq` or any tool expecting NDJSON. Colors are never
//!   applied in this mode.
//!
//! # Broken-pipe handling
//!
//! When the downstream reader closes early (e.g. `logdive query ... | head`),
//! write calls return `ErrorKind::BrokenPipe`. The Unix convention is for
//! the producer to exit silently with success; we uphold that by short-
//! circuiting out of the render loop without surfacing an error.

use std::io::{self, IsTerminal, Write};

use anstream::AutoStream;
use anstyle::{AnsiColor, Color, Style};
use serde_json::{Value, json};

use logdive_core::{LogEntry, Result};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Output format chosen by the user via `--format`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable, colored when stdout is a TTY.
    Pretty,
    /// Newline-delimited JSON, one `LogEntry` per line.
    Json,
}

/// Render a slice of entries to stdout in the requested format.
///
/// Empty result sets produce no stdout output. For `Pretty`, a
/// "No matches." hint is written to stderr; for `Json`, silence is
/// pipeline-correct (an empty NDJSON stream is still valid NDJSON).
pub fn render(entries: &[LogEntry], format: OutputFormat) -> Result<()> {
    if entries.is_empty() {
        if matches!(format, OutputFormat::Pretty) {
            eprintln!("No matches.");
        }
        return Ok(());
    }

    match format {
        OutputFormat::Pretty => render_pretty(entries),
        OutputFormat::Json => render_json(entries),
    }
}

// ---------------------------------------------------------------------------
// Pretty
// ---------------------------------------------------------------------------

fn render_pretty(entries: &[LogEntry]) -> Result<()> {
    // `anstream::AutoStream::auto` inspects the underlying stream and strips
    // ANSI when it's not a terminal or when NO_COLOR is set. We still want
    // to know whether colors are actually enabled so that our formatting
    // layer can skip even building style sequences on a pipe (a trivial
    // perf win, but more importantly keeps the logic symmetric).
    let stdout = io::stdout();
    let tty = stdout.is_terminal();
    let mut out = AutoStream::auto(stdout.lock());

    for entry in entries {
        let line = format_pretty_line(entry, tty);
        match writeln!(out, "{line}") {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }

    match out.flush() {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Build the rendered line for a single entry. Extracted from the write
/// loop so it's directly unit-testable without a pipe on the other end.
fn format_pretty_line(entry: &LogEntry, color: bool) -> String {
    let ts = entry.timestamp.as_deref().unwrap_or("-");
    let level = entry.level.as_deref().unwrap_or("-");

    let level_styled = if color {
        let style = level_style(level);
        format!("{style}{level:<5}{style:#}", level = level.to_uppercase())
    } else {
        format!("{:<5}", level.to_uppercase())
    };

    let ts_styled = if color {
        let dim = Style::new().dimmed();
        format!("{dim}{ts}{dim:#}")
    } else {
        ts.to_string()
    };

    let tag_styled = match entry.tag.as_deref() {
        Some(t) if !t.is_empty() => {
            if color {
                let cyan = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
                format!("  {cyan}[{t}]{cyan:#}")
            } else {
                format!("  [{t}]")
            }
        }
        _ => String::new(),
    };

    let msg = entry.message.as_deref().unwrap_or("");

    let fields_styled = if entry.fields.is_empty() {
        String::new()
    } else {
        let rendered = format_fields(&entry.fields);
        if color {
            let dim = Style::new().dimmed();
            format!("  {dim}{{{rendered}}}{dim:#}")
        } else {
            format!("  {{{rendered}}}")
        }
    };

    format!("{ts_styled}  {level_styled}{tag_styled}  {msg}{fields_styled}")
}

/// Map log level to a display style.
///
/// Case-insensitive match on the common level names. Unknown levels fall
/// through to default styling so we never surprise users with a color on
/// a level they've defined.
fn level_style(level: &str) -> Style {
    match level.to_ascii_lowercase().as_str() {
        "error" | "err" | "fatal" => Style::new()
            .fg_color(Some(Color::Ansi(AnsiColor::BrightRed)))
            .bold(),
        "warn" | "warning" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        "info" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
        "debug" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
        "trace" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
        _ => Style::new(),
    }
}

/// Render the `fields` map as `k=v, k=v, ...`. Keys are iterated in their
/// stored (insertion) order — `serde_json::Map` preserves insertion order
/// when the `preserve_order` feature is disabled, which is the default and
/// matches what the parser produces.
fn format_fields(fields: &serde_json::Map<String, Value>) -> String {
    let mut parts = Vec::with_capacity(fields.len());
    for (k, v) in fields {
        parts.push(format!("{k}={}", format_field_value(v)));
    }
    parts.join(", ")
}

/// Compact rendering of a `serde_json::Value` for the pretty line's
/// trailing `{...}`. Strings are emitted unquoted when simple; complex
/// values fall back to their canonical JSON form to stay lossless.
fn format_field_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            // Unquote simple ASCII strings that don't contain characters
            // which would confuse the `k=v, k=v` grammar. Otherwise fall
            // back to JSON encoding (which re-quotes and escapes).
            if is_simple_token(s) {
                s.clone()
            } else {
                Value::String(s.clone()).to_string()
            }
        }
        // Objects and arrays stay as compact JSON — any other choice is lossy.
        _ => v.to_string(),
    }
}

fn is_simple_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

fn render_json(entries: &[LogEntry]) -> Result<()> {
    // Not using AutoStream here — JSON output should never be colored,
    // and raw stdout with line buffering is the canonical NDJSON writer.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for entry in entries {
        let value = entry_to_json(entry);
        // `to_string` on `serde_json::Value` is infallible.
        let line = value.to_string();
        match writeln!(out, "{line}") {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }

    match out.flush() {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Serialize a `LogEntry` to JSON with a stable shape. `tag` is always
/// present (possibly null). `fields` is always an object. `raw` is
/// included so downstream consumers can re-ingest or audit the original
/// line — cheap since it's already stored in-memory.
fn entry_to_json(entry: &LogEntry) -> Value {
    json!({
        "timestamp": entry.timestamp,
        "level":     entry.level,
        "message":   entry.message,
        "tag":       entry.tag,
        "fields":    Value::Object(entry.fields.clone()),
        "raw":       entry.raw,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_entry() -> LogEntry {
        let raw = r#"{"timestamp":"2026-04-22T14:03:21Z","level":"error","message":"boom"}"#;
        let mut e = LogEntry::new(raw.to_string());
        e.timestamp = Some("2026-04-22T14:03:21Z".to_string());
        e.level = Some("error".to_string());
        e.message = Some("boom".to_string());
        e
    }

    #[test]
    fn pretty_line_contains_core_columns_when_color_disabled() {
        let e = make_entry();
        let line = format_pretty_line(&e, false);
        assert!(line.contains("2026-04-22T14:03:21Z"));
        assert!(line.contains("ERROR"));
        assert!(line.contains("boom"));
        // No tag → no brackets.
        assert!(!line.contains('['));
        // No fields → no braces.
        assert!(!line.contains('{'));
    }

    #[test]
    fn pretty_line_includes_tag_when_present() {
        let mut e = make_entry();
        e.tag = Some("payments".to_string());
        let line = format_pretty_line(&e, false);
        assert!(line.contains("[payments]"));
    }

    #[test]
    fn pretty_line_includes_fields_block_when_non_empty() {
        let mut e = make_entry();
        e.fields.insert("retry".to_string(), json!(3));
        e.fields.insert("user_id".to_string(), json!(42));
        let line = format_pretty_line(&e, false);
        assert!(line.contains("retry=3"));
        assert!(line.contains("user_id=42"));
        assert!(line.contains('{') && line.contains('}'));
    }

    #[test]
    fn pretty_line_handles_missing_timestamp_and_level() {
        let mut e = LogEntry::new("".to_string());
        e.message = Some("orphan".to_string());
        let line = format_pretty_line(&e, false);
        assert!(line.contains('-'));
        assert!(line.contains("orphan"));
    }

    #[test]
    fn pretty_line_uppercases_level() {
        let mut e = make_entry();
        e.level = Some("warn".to_string());
        let line = format_pretty_line(&e, false);
        assert!(line.contains("WARN"));
    }

    #[test]
    fn pretty_line_with_color_contains_ansi_sequences() {
        let e = make_entry();
        let colored = format_pretty_line(&e, true);
        let plain = format_pretty_line(&e, false);
        assert_ne!(colored, plain);
        assert!(colored.contains('\x1b'), "expected ANSI escape sequence");
    }

    #[test]
    fn field_value_simple_string_unquoted() {
        assert_eq!(format_field_value(&json!("payments")), "payments");
    }

    #[test]
    fn field_value_string_with_spaces_gets_quoted() {
        assert_eq!(format_field_value(&json!("has spaces")), "\"has spaces\"");
    }

    #[test]
    fn field_value_number_and_bool_and_null() {
        assert_eq!(format_field_value(&json!(42)), "42");
        assert_eq!(format_field_value(&json!(true)), "true");
        assert_eq!(format_field_value(&json!(null)), "null");
    }

    #[test]
    fn field_value_nested_object_falls_back_to_json() {
        let v = json!({"nested": "value"});
        assert_eq!(format_field_value(&v), r#"{"nested":"value"}"#);
    }

    #[test]
    fn entry_to_json_is_valid_ndjson_and_round_trips_shape() {
        let mut e = make_entry();
        e.tag = Some("api".to_string());
        e.fields.insert("k".to_string(), json!("v"));

        let v = entry_to_json(&e);
        let obj = v.as_object().expect("top-level object");
        assert_eq!(obj.get("timestamp").unwrap(), "2026-04-22T14:03:21Z");
        assert_eq!(obj.get("level").unwrap(), "error");
        assert_eq!(obj.get("message").unwrap(), "boom");
        assert_eq!(obj.get("tag").unwrap(), "api");
        assert_eq!(obj.get("fields").unwrap().get("k").unwrap(), "v");
        assert!(obj.get("raw").is_some());
    }

    #[test]
    fn entry_to_json_emits_null_when_fields_absent() {
        let e = LogEntry::new(String::new());
        let v = entry_to_json(&e);
        assert!(v.get("timestamp").unwrap().is_null());
        assert!(v.get("level").unwrap().is_null());
        assert!(v.get("message").unwrap().is_null());
        assert!(v.get("tag").unwrap().is_null());
        // fields still an empty object, not null.
        assert!(v.get("fields").unwrap().is_object());
    }
}
