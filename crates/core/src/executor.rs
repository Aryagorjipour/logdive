//! Query executor: translate a [`QueryNode`] into parameterized SQL, run
//! it against the index, and reconstruct [`LogEntry`] values from the
//! result rows.
//!
//! This module is the bridge between the query AST and the SQLite schema.
//! It never mixes user-controlled strings into SQL text — every literal
//! value is bound as a parameter. The one exception is JSON extraction
//! paths like `$.service`, which embed the field name directly because
//! SQLite parameters aren't allowed inside `json_extract` path expressions;
//! safety there comes from the field name having passed
//! `validate_field_name`'s strict regex in the parser, which we
//! defensively re-check at the executor boundary.
//!
//! # Disjunction (OR) shape
//!
//! v0.2.0 introduced OR. The AST is `QueryNode::Or(Vec<AndGroup>)` where
//! each [`AndGroup`] is a conjunction of clauses. The SQL we emit
//! parenthesizes each AND-group and joins them with ` OR `:
//!
//! ```sql
//! WHERE (level = ? AND json_extract(fields, '$.service') = ?)
//!    OR (level = ?)
//! ```
//!
//! For queries with no OR (a single AND-group), the parens are still
//! emitted — the alternative is a special-case branch in the SQL
//! builder that adds maintenance cost without performance benefit.
//! SQLite's planner ignores redundant parens.
//!
//! # Timestamp handling
//!
//! Timestamps are compared as TEXT, which works correctly for any ISO-8601
//! format because those sort lexicographically in chronological order when
//! all components are fixed-width. Ingested timestamps that aren't ISO-8601
//! shaped will compare incorrectly against `last`/`since` bounds — a known
//! limitation of accepting arbitrary timestamp strings at ingestion time.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde_json::{Map, Value};

use crate::entry::LogEntry;
use crate::error::{LogdiveError, Result};
use crate::query::{AndGroup, Clause, Duration, QueryNode, QueryValue};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Execute a parsed query against the index and return matching entries.
///
/// `limit` caps the result set size; pass `None` for no limit. Results are
/// ordered by `timestamp DESC, id DESC` (newest first, with row id as
/// stable tiebreaker for identical timestamps).
pub fn execute(
    query: &QueryNode,
    conn: &Connection,
    limit: Option<usize>,
) -> Result<Vec<LogEntry>> {
    let (sql, binds) = build_sql(query, limit, Utc::now())?;
    run(conn, &sql, &binds)
}

/// Variant of [`execute`] that uses a caller-supplied "now" value.
///
/// Exposed for testing so time-range clauses produce deterministic bounds.
pub fn execute_at(
    query: &QueryNode,
    conn: &Connection,
    limit: Option<usize>,
    now: DateTime<Utc>,
) -> Result<Vec<LogEntry>> {
    let (sql, binds) = build_sql(query, limit, now)?;
    run(conn, &sql, &binds)
}

// ---------------------------------------------------------------------------
// SQL generation
// ---------------------------------------------------------------------------

/// Intermediate representation of a bindable value, kept as an owned
/// `SqlValue` so `params_from_iter` can consume them without lifetime
/// gymnastics.
type Bind = SqlValue;

fn build_sql(
    query: &QueryNode,
    limit: Option<usize>,
    now: DateTime<Utc>,
) -> Result<(String, Vec<Bind>)> {
    let QueryNode::Or(groups) = query;

    // The parser guarantees at least one AND-group, and each AND-group has
    // at least one clause. Treat both invariants defensively here so a bug
    // upstream produces a recognizable runtime shape rather than a SQL
    // syntax error from an empty `WHERE` clause.
    let mut group_sqls: Vec<String> = Vec::with_capacity(groups.len());
    let mut binds: Vec<Bind> = Vec::new();

    for group in groups {
        let (group_sql, mut group_binds) = translate_and_group(group, now)?;
        group_sqls.push(group_sql);
        binds.append(&mut group_binds);
    }

    let where_sql = if group_sqls.is_empty() {
        // Defensive: should be unreachable given the parser contract.
        "1=1".to_string()
    } else {
        // ` OR ` between groups, each already parenthesized.
        group_sqls.join(" OR ")
    };

    let mut sql = format!(
        "SELECT timestamp, level, message, tag, fields, raw \
         FROM log_entries \
         WHERE {where_sql} \
         ORDER BY timestamp DESC, id DESC"
    );
    if let Some(n) = limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    Ok((sql, binds))
}

/// Translate one AND-group into a parenthesized SQL fragment and the
/// associated bind values, in clause-declaration order.
///
/// Always parenthesizes — including the single-clause case. Uniformity in
/// the SQL emitter outweighs prettiness in the rare query-debugger view.
fn translate_and_group(group: &AndGroup, now: DateTime<Utc>) -> Result<(String, Vec<Bind>)> {
    let mut clause_sqls: Vec<String> = Vec::with_capacity(group.clauses.len());
    let mut binds: Vec<Bind> = Vec::new();

    for clause in &group.clauses {
        let (sql, mut clause_binds) = translate_clause(clause, now)?;
        clause_sqls.push(sql);
        binds.append(&mut clause_binds);
    }

    let inner = if clause_sqls.is_empty() {
        // Defensive: parser guarantees non-empty AND-groups.
        "1=1".to_string()
    } else {
        clause_sqls.join(" AND ")
    };
    Ok((format!("({inner})"), binds))
}

fn translate_clause(clause: &Clause, now: DateTime<Utc>) -> Result<(String, Vec<Bind>)> {
    match clause {
        Clause::Compare { field, op, value } => {
            let column_expr = column_for_field(field)?;
            let sql = format!("{column_expr} {op} ?");
            Ok((sql, vec![value_to_bind(value)]))
        }
        Clause::Contains { field, value } => {
            let column_expr = column_for_field(field)?;
            // Escape SQL LIKE metacharacters (%, _, \) so a user searching
            // for a literal '%' doesn't accidentally wildcard the world.
            let escaped = escape_like(value);
            let pattern = format!("%{escaped}%");
            let sql = format!("{column_expr} LIKE ? ESCAPE '\\'");
            Ok((sql, vec![SqlValue::Text(pattern)]))
        }
        Clause::LastDuration(d) => {
            let cutoff = compute_last_cutoff(*d, now);
            Ok((
                "timestamp >= ?".to_string(),
                vec![SqlValue::Text(cutoff.to_rfc3339())],
            ))
        }
        Clause::SinceDatetime(s) => {
            let dt = parse_datetime(s)?;
            Ok((
                "timestamp >= ?".to_string(),
                vec![SqlValue::Text(dt.to_rfc3339())],
            ))
        }
    }
}

/// Return the SQL expression that references a given query field.
///
/// Known fields resolve to indexed columns. Unknown fields resolve to a
/// `json_extract(fields, '$.<field>')` expression — which is why the
/// field name must survive `validate_field_name`'s regex *and* the
/// defensive check here.
fn column_for_field(field: &str) -> Result<String> {
    if LogEntry::KNOWN_KEYS.contains(&field) {
        Ok(field.to_string())
    } else {
        if !is_safe_json_path_segment(field) {
            return Err(LogdiveError::UnsafeFieldName(field.to_string()));
        }
        Ok(format!("json_extract(fields, '$.{field}')"))
    }
}

/// Defensive: the parser's `validate_field_name` already enforces this,
/// but we re-check at the SQL boundary so the trust model is obvious
/// from inside this module. Allowed: letters, digits, `_`, `.`.
fn is_safe_json_path_segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn value_to_bind(v: &QueryValue) -> Bind {
    match v {
        QueryValue::String(s) => SqlValue::Text(s.clone()),
        QueryValue::Integer(n) => SqlValue::Integer(*n),
        QueryValue::Float(f) => SqlValue::Real(*f),
        QueryValue::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
    }
}

/// Pre-escape SQL LIKE wildcards (`%`, `_`) and the escape character
/// itself so a user's literal CONTAINS string is matched literally.
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn compute_last_cutoff(d: Duration, now: DateTime<Utc>) -> DateTime<Utc> {
    // `amount` is u64; promote to i64 for chrono. Saturate on the
    // (astronomically unlikely) overflow case.
    let amount_i64 = i64::try_from(d.amount).unwrap_or(i64::MAX);
    let secs = amount_i64.saturating_mul(d.unit.seconds());
    let delta = chrono::Duration::seconds(secs);
    now.checked_sub_signed(delta).unwrap_or_else(|| {
        Utc.timestamp_opt(0, 0)
            .single()
            .expect("unix epoch is valid")
    })
}

/// Accept three datetime formats for `since` clauses:
///   - RFC3339 / ISO-8601 with timezone (e.g. `2024-01-01T10:00:00Z`)
///   - ISO naive datetime (e.g. `2024-01-01 10:00:00` or `2024-01-01T10:00:00`), interpreted as UTC
///   - ISO date (e.g. `2024-01-01`), interpreted as UTC midnight
fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    for fmt in &["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(Utc.from_utc_datetime(&ndt));
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = nd.and_hms_opt(0, 0, 0).expect("00:00:00 is valid");
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    Err(LogdiveError::InvalidDatetime {
        input: s.to_string(),
        reason: "expected RFC3339, `YYYY-MM-DD HH:MM:SS`, or `YYYY-MM-DD`".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

fn run(conn: &Connection, sql: &str, binds: &[Bind]) -> Result<Vec<LogEntry>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(binds.iter()), |row| {
        let timestamp: Option<String> = row.get(0)?;
        let level: Option<String> = row.get(1)?;
        let message: Option<String> = row.get(2)?;
        let tag: Option<String> = row.get(3)?;
        let fields_json: String = row.get(4)?;
        let raw: String = row.get(5)?;
        // We tunnel the raw JSON out; deserialization happens below so the
        // closure's error type stays `rusqlite::Error`.
        Ok((timestamp, level, message, tag, fields_json, raw))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (timestamp, level, message, tag, fields_json, raw) = row?;
        let fields: Map<String, Value> =
            serde_json::from_str(&fields_json).map_err(LogdiveError::CorruptFieldsJson)?;
        out.push(LogEntry {
            timestamp,
            level,
            message,
            tag,
            fields,
            raw,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::Indexer;
    use crate::query::parse;
    use std::collections::HashSet;

    /// Convenience: parse a query string and run it against the given
    /// connection. Panics if parsing fails — tests pass well-formed input.
    fn run_query(conn: &Connection, q: &str) -> Vec<LogEntry> {
        let ast = parse(q).expect("test queries are well-formed");
        execute(&ast, conn, None).expect("execute")
    }

    fn run_query_at(conn: &Connection, q: &str, now: DateTime<Utc>) -> Vec<LogEntry> {
        let ast = parse(q).expect("test queries are well-formed");
        execute_at(&ast, conn, None, now).expect("execute")
    }

    fn make_entry(ts: &str, level: &str, message: &str) -> LogEntry {
        let raw = format!(r#"{{"timestamp":"{ts}","level":"{level}","message":"{message}"}}"#);
        let mut e = LogEntry::new(raw);
        e.timestamp = Some(ts.to_string());
        e.level = Some(level.to_string());
        e.message = Some(message.to_string());
        e
    }

    fn fixture() -> Indexer {
        let mut idx = Indexer::open_in_memory().unwrap();
        let mut a = make_entry("2026-04-20T10:00:00Z", "error", "payment failed");
        a.tag = Some("api".into());
        a.fields
            .insert("service".into(), Value::String("payments".into()));
        a.fields.insert("req_id".into(), Value::from(100));

        let mut b = make_entry("2026-04-20T11:00:00Z", "info", "health check");
        b.tag = Some("api".into());
        b.fields
            .insert("service".into(), Value::String("payments".into()));
        b.fields.insert("req_id".into(), Value::from(200));

        let mut c = make_entry("2026-04-20T12:00:00Z", "error", "timeout on db call");
        c.fields
            .insert("service".into(), Value::String("users".into()));
        c.fields.insert("req_id".into(), Value::from(300));

        idx.insert_batch(&[a, b, c]).unwrap();
        idx
    }

    /// Three-row fixture covering levels error/warn/info across two services.
    /// Used by OR-specific round-trip tests where we want at least one row
    /// per level to exercise multi-group disjunction.
    fn three_level_fixture() -> Indexer {
        let mut idx = Indexer::open_in_memory().unwrap();
        let a = make_entry("2026-04-20T09:00:00Z", "error", "boom");
        let b = make_entry("2026-04-20T10:00:00Z", "warn", "slow query");
        let c = make_entry("2026-04-20T11:00:00Z", "info", "ok");
        idx.insert_batch(&[a, b, c]).unwrap();
        idx
    }

    // -----------------------------------------------------------------
    // SQL generation (inspection) — single AND-group cases (no OR)
    // -----------------------------------------------------------------

    #[test]
    fn compare_on_known_field_binds_value_not_interpolates() {
        let ast = parse("level=error").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        // Always-parenthesized invariant: even single-clause queries are
        // wrapped in parens.
        assert!(sql.contains("WHERE (level = ?)"));
        assert!(!sql.contains("error"));
        assert_eq!(binds.len(), 1);
        match &binds[0] {
            SqlValue::Text(s) => assert_eq!(s, "error"),
            other => panic!("expected text bind, got {other:?}"),
        }
    }

    #[test]
    fn compare_on_unknown_field_uses_json_extract() {
        let ast = parse("service=payments").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(sql.contains("json_extract(fields, '$.service')"));
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn contains_uses_like_with_escape_and_wildcards() {
        let ast = parse(r#"message contains "timeout""#).unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(sql.contains("LIKE ? ESCAPE '\\'"));
        match &binds[0] {
            SqlValue::Text(s) => assert_eq!(s, "%timeout%"),
            other => panic!("expected text bind, got {other:?}"),
        }
    }

    #[test]
    fn contains_escapes_like_metacharacters() {
        let ast = parse(r#"message contains "50%""#).unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        match &binds[0] {
            SqlValue::Text(s) => assert_eq!(s, r"%50\%%"),
            other => panic!("unexpected bind: {other:?}"),
        }
    }

    #[test]
    fn last_duration_produces_timestamp_lower_bound() {
        let ast = parse("last 2h").unwrap();
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let (sql, binds) = build_sql(&ast, None, now).unwrap();
        assert!(sql.contains("timestamp >= ?"));
        match &binds[0] {
            SqlValue::Text(s) => assert!(s.starts_with("2026-04-20T10:00:00")),
            other => panic!("unexpected bind: {other:?}"),
        }
    }

    #[test]
    fn since_accepts_rfc3339() {
        let ast = parse(r#"since "2024-01-01T10:00:00Z""#).unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(sql.contains("timestamp >= ?"));
        match &binds[0] {
            SqlValue::Text(s) => assert!(s.starts_with("2024-01-01T10:00:00")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn since_accepts_bare_date() {
        let ast = parse("since 2024-06-15").unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        match &binds[0] {
            SqlValue::Text(s) => assert!(s.starts_with("2024-06-15T00:00:00")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn since_rejects_garbage() {
        let ast = parse("since not-a-date").unwrap();
        let err = build_sql(&ast, None, Utc::now()).unwrap_err();
        assert!(matches!(err, LogdiveError::InvalidDatetime { .. }));
    }

    #[test]
    fn and_chain_joins_with_and_inside_a_single_group() {
        let ast = parse("level=error AND service=payments").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(sql.contains("level = ?"));
        assert!(sql.contains("json_extract(fields, '$.service') = ?"));
        // AND inside a single parenthesized group, no OR.
        assert!(sql.contains(" AND "));
        assert!(!sql.contains(" OR "));
        // Always-parenthesized invariant.
        assert!(sql.contains("WHERE (level = ? AND json_extract(fields, '$.service') = ?)"));
        assert_eq!(binds.len(), 2);
        match (&binds[0], &binds[1]) {
            (SqlValue::Text(a), SqlValue::Text(b)) => {
                assert_eq!(a, "error");
                assert_eq!(b, "payments");
            }
            other => panic!("unexpected binds: {other:?}"),
        }
    }

    #[test]
    fn integer_binds_as_integer_not_text() {
        let ast = parse("req_id > 100").unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        match &binds[0] {
            SqlValue::Integer(n) => assert_eq!(*n, 100),
            other => panic!("expected integer bind, got {other:?}"),
        }
    }

    #[test]
    fn bool_binds_as_integer_zero_or_one() {
        let ast = parse("ok=true").unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(matches!(binds[0], SqlValue::Integer(1)));

        let ast = parse("ok=false").unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(matches!(binds[0], SqlValue::Integer(0)));
    }

    #[test]
    fn float_binds_as_real() {
        let ast = parse("duration < 1.5").unwrap();
        let (_, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        match &binds[0] {
            SqlValue::Real(f) => assert!((f - 1.5).abs() < 1e-9),
            other => panic!("expected real bind, got {other:?}"),
        }
    }

    #[test]
    fn limit_appends_limit_clause() {
        let ast = parse("level=error").unwrap();
        let (sql, _) = build_sql(&ast, Some(50), Utc::now()).unwrap();
        assert!(sql.ends_with("LIMIT 50"));
    }

    // -----------------------------------------------------------------
    // SQL generation — OR cases (new in v0.2.0)
    // -----------------------------------------------------------------

    #[test]
    fn or_emits_two_parenthesized_groups_joined_by_or() {
        let ast = parse("level=error OR level=warn").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        // Two parenthesized AND-groups joined by ` OR `.
        assert!(sql.contains("WHERE (level = ?) OR (level = ?)"));
        // Bind order matches clause order across the OR boundary.
        match (&binds[0], &binds[1]) {
            (SqlValue::Text(a), SqlValue::Text(b)) => {
                assert_eq!(a, "error");
                assert_eq!(b, "warn");
            }
            other => panic!("unexpected binds: {other:?}"),
        }
    }

    #[test]
    fn or_with_three_groups_joins_with_two_or_keywords() {
        let ast = parse("level=error OR level=warn OR level=fatal").unwrap();
        let (sql, _binds) = build_sql(&ast, None, Utc::now()).unwrap();
        // Exactly two ` OR ` separators between three groups.
        assert_eq!(sql.matches(" OR ").count(), 2);
        // All three values appear bound (we rely on the matches above
        // for the count; spot-check shape here).
        assert!(sql.contains("(level = ?) OR (level = ?) OR (level = ?)"));
    }

    #[test]
    fn or_with_and_inside_each_group_emits_correct_shape() {
        // (level=error AND service=payments) OR (level=warn)
        let ast = parse("level=error AND service=payments OR level=warn").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert!(sql.contains(
            "WHERE (level = ? AND json_extract(fields, '$.service') = ?) OR (level = ?)"
        ));
        // Bind order: error, payments, warn — preserved across OR.
        assert_eq!(binds.len(), 3);
        match (&binds[0], &binds[1], &binds[2]) {
            (SqlValue::Text(a), SqlValue::Text(b), SqlValue::Text(c)) => {
                assert_eq!(a, "error");
                assert_eq!(b, "payments");
                assert_eq!(c, "warn");
            }
            other => panic!("unexpected binds: {other:?}"),
        }
    }

    #[test]
    fn or_with_and_on_both_sides_preserves_bind_ordering() {
        // (a=1 AND b=2) OR (c=3 AND d=4) — exercises bind ordering across
        // OR boundary with mixed integer values.
        let ast = parse("a=1 AND b=2 OR c=3 AND d=4").unwrap();
        let (sql, binds) = build_sql(&ast, None, Utc::now()).unwrap();
        assert_eq!(sql.matches(" OR ").count(), 1);
        assert_eq!(binds.len(), 4);
        match (&binds[0], &binds[1], &binds[2], &binds[3]) {
            (
                SqlValue::Integer(a),
                SqlValue::Integer(b),
                SqlValue::Integer(c),
                SqlValue::Integer(d),
            ) => {
                assert_eq!(*a, 1);
                assert_eq!(*b, 2);
                assert_eq!(*c, 3);
                assert_eq!(*d, 4);
            }
            other => panic!("unexpected binds: {other:?}"),
        }
    }

    #[test]
    fn or_with_mixed_clause_kinds_in_each_group() {
        // level=error OR message contains "timeout" OR last 30m
        // Three groups, each with a different kind of clause.
        let ast = parse(r#"level=error OR message contains "timeout" OR last 30m"#).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let (sql, binds) = build_sql(&ast, None, now).unwrap();
        assert_eq!(sql.matches(" OR ").count(), 2);
        assert_eq!(binds.len(), 3);

        // Group 1: text bind for the level value.
        assert!(matches!(&binds[0], SqlValue::Text(s) if s == "error"));
        // Group 2: text bind with %timeout% LIKE pattern.
        assert!(matches!(&binds[1], SqlValue::Text(s) if s == "%timeout%"));
        // Group 3: text bind with the cutoff timestamp.
        match &binds[2] {
            SqlValue::Text(s) => assert!(s.starts_with("2026-04-20T11:30:00")),
            other => panic!("expected text cutoff, got {other:?}"),
        }
    }

    #[test]
    fn limit_applies_to_or_query_too() {
        let ast = parse("level=error OR level=warn").unwrap();
        let (sql, _) = build_sql(&ast, Some(25), Utc::now()).unwrap();
        assert!(sql.ends_with("LIMIT 25"));
        // LIMIT is outside the WHERE clause.
        assert!(sql.contains(" ORDER BY "));
    }

    // -----------------------------------------------------------------
    // Round-trip: insert, query (without OR), assert results
    // -----------------------------------------------------------------

    #[test]
    fn round_trip_known_field_equality() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level=error");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|e| e.level.as_deref() == Some("error")));
    }

    #[test]
    fn round_trip_unknown_field_via_json_extract() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "service=payments");
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|e| e.fields.get("service") == Some(&Value::String("payments".into())))
        );
    }

    #[test]
    fn round_trip_and_chain() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level=error AND service=payments");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message.as_deref(), Some("payment failed"));
    }

    #[test]
    fn round_trip_contains_substring_match() {
        let idx = fixture();
        let rows = run_query(idx.connection(), r#"message contains "timeout""#);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].message.as_deref().unwrap().contains("timeout"));
    }

    #[test]
    fn round_trip_numeric_comparison_on_json_field() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "req_id > 150");
        assert_eq!(rows.len(), 2);
        let ids: HashSet<i64> = rows
            .iter()
            .map(|e| e.fields.get("req_id").and_then(|v| v.as_i64()).unwrap())
            .collect();
        assert_eq!(ids, HashSet::from([200, 300]));
    }

    #[test]
    fn round_trip_last_duration_uses_now() {
        let idx = fixture();
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 13, 0, 0).unwrap();
        let rows = run_query_at(idx.connection(), "last 3h", now);
        assert_eq!(rows.len(), 3);

        let rows = run_query_at(idx.connection(), "last 70m", now);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp.as_deref(), Some("2026-04-20T12:00:00Z"));
    }

    #[test]
    fn round_trip_since_datetime() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "since 2026-04-20T11:00:00Z");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn round_trip_results_ordered_newest_first() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level=error");
        assert!(rows[0].timestamp > rows[1].timestamp);
    }

    #[test]
    fn round_trip_not_equal_operator() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level!=error");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].level.as_deref(), Some("info"));
    }

    #[test]
    fn round_trip_contains_with_wildcard_character_is_literal() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let a = make_entry("2026-04-20T10:00:00Z", "info", "discount 50% today");
        let b = make_entry("2026-04-20T11:00:00Z", "info", "no special char here");
        idx.insert_batch(&[a, b]).unwrap();

        let rows = run_query(idx.connection(), r#"message contains "50%""#);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].message.as_deref().unwrap().contains("50%"));
    }

    #[test]
    fn round_trip_empty_result_is_empty_vec_not_error() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level=nonsense");
        assert!(rows.is_empty());
    }

    #[test]
    fn round_trip_reconstructs_fields_map() {
        let idx = fixture();
        let rows = run_query(idx.connection(), "level=error AND service=payments");
        assert_eq!(rows.len(), 1);
        let e = &rows[0];
        assert_eq!(
            e.fields.get("service"),
            Some(&Value::String("payments".into()))
        );
        assert_eq!(e.fields.get("req_id").and_then(|v| v.as_i64()), Some(100));
    }

    // -----------------------------------------------------------------
    // Round-trip: OR queries (new in v0.2.0)
    // -----------------------------------------------------------------

    #[test]
    fn round_trip_or_two_groups_returns_union() {
        let idx = three_level_fixture();
        let rows = run_query(idx.connection(), "level=error OR level=warn");
        assert_eq!(rows.len(), 2);

        let levels: HashSet<String> = rows
            .iter()
            .map(|e| e.level.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            levels,
            HashSet::from(["error".to_string(), "warn".to_string()])
        );
    }

    #[test]
    fn round_trip_or_three_groups_returns_full_union() {
        let idx = three_level_fixture();
        let rows = run_query(idx.connection(), "level=error OR level=warn OR level=info");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn round_trip_or_does_not_double_count_overlapping_rows() {
        // A row matched by both groups must appear once (SQL OR is set
        // union over candidate rows in the WHERE evaluation, but each
        // row is yielded once because the FROM is a single table).
        let idx = three_level_fixture();
        // Both clauses match the error row.
        let rows = run_query(
            idx.connection(),
            r#"level=error OR message contains "boom""#,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].level.as_deref(), Some("error"));
    }

    #[test]
    fn round_trip_or_respects_and_precedence() {
        // (level=error AND service=payments) OR level=warn
        // Fixture has one error row with service=payments, one error
        // row with service=users, one info row, no warn rows.
        // Adding a warn row to the fixture so the OR side has a match.
        let mut idx = fixture();
        let mut warn = make_entry("2026-04-20T13:00:00Z", "warn", "almost full");
        warn.fields
            .insert("service".into(), Value::String("orders".into()));
        warn.fields.insert("req_id".into(), Value::from(400));
        idx.insert_batch(&[warn]).unwrap();

        let rows = run_query(
            idx.connection(),
            "level=error AND service=payments OR level=warn",
        );
        // One error/payments row + one warn row = 2.
        assert_eq!(rows.len(), 2);
        let messages: HashSet<String> = rows
            .iter()
            .map(|e| e.message.clone().unwrap_or_default())
            .collect();
        assert!(messages.contains("payment failed"));
        assert!(messages.contains("almost full"));
    }

    #[test]
    fn round_trip_or_with_time_range() {
        // "Errors any time, OR anything in the last hour" is a real-
        // world disjunction shape.
        let idx = fixture();
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 30, 0).unwrap();
        let rows = run_query_at(idx.connection(), "level=error OR last 30m", now);
        // The two error rows + the one row in the last 30m (12:00 row).
        // The 12:00 row is also an error row, so dedup yields 2.
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn round_trip_or_yields_results_ordered_newest_first() {
        // Ordering must be applied across the union, not within each group.
        let idx = three_level_fixture();
        let rows = run_query(idx.connection(), "level=error OR level=info");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].timestamp > rows[1].timestamp);
        // Newest is the info row at 11:00, oldest is the error at 09:00.
        assert_eq!(rows[0].level.as_deref(), Some("info"));
        assert_eq!(rows[1].level.as_deref(), Some("error"));
    }

    #[test]
    fn round_trip_or_with_zero_matches_in_one_group_still_returns_other() {
        let idx = three_level_fixture();
        // First group matches nothing, second matches one row.
        let rows = run_query(idx.connection(), "level=fatal OR level=warn");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].level.as_deref(), Some("warn"));
    }

    // -----------------------------------------------------------------
    // Safety guards
    // -----------------------------------------------------------------

    #[test]
    fn unsafe_field_name_is_rejected_at_executor() {
        let result = column_for_field("service; DROP TABLE log_entries--");
        assert!(matches!(result, Err(LogdiveError::UnsafeFieldName(_))));
    }
}
