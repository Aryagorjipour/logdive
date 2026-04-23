//! Query language: tokenizer, AST, and recursive descent parser.
//!
//! Implements the grammar from the project doc's "Notes → Query language
//! grammar (v1)" section verbatim. AND-only per the 2026-04-19 decisions
//! log entry — OR is deferred to v2.
//!
//! This module owns *only* the parse step: `&str → QueryNode`. Translating
//! a `QueryNode` into SQL and binding parameters is milestone 4's executor.
//! Resolving relative time ranges like `last 2h` against wall-clock time
//! is also the executor's job; the AST just carries the raw spec.
//!
//! # Grammar (from the doc, reproduced for reference)
//!
//! ```text
//! query     := clause (AND clause)*
//! clause    := field OP value
//!            | field CONTAINS string
//!            | TIME_RANGE
//! field     := [a-zA-Z_][a-zA-Z0-9_.]*
//! OP        := "=" | "!=" | ">" | "<"
//! value     := string | number | bool
//! string    := '"' .* '"' | bare_word
//! TIME_RANGE := "last" duration | "since" datetime
//! duration  := number ("m" | "h" | "d")
//! ```

use std::fmt;

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// The top-level query: one or more clauses joined by AND.
///
/// A query with a single clause parses as `And(vec![clause])` so the
/// executor has exactly one code path.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryNode {
    And(Vec<Clause>),
}

/// A single clause — the atomic unit a query is built from.
#[derive(Debug, Clone, PartialEq)]
pub enum Clause {
    /// `field OP value` — e.g. `level = error`, `req_id > 100`.
    Compare {
        field: String,
        op: CompareOp,
        value: QueryValue,
    },
    /// `field CONTAINS string` — substring match on a string column.
    Contains { field: String, value: String },
    /// `last <N><unit>` — relative time range ending at query time.
    LastDuration(Duration),
    /// `since <datetime>` — absolute time range starting at the given moment.
    /// The string is opaque at the parse layer; the executor uses chrono to
    /// resolve it (which allows us to accept multiple formats without
    /// teaching the grammar about any particular one).
    SinceDatetime(String),
}

/// Comparison operator for `field OP value` clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    NotEq,
    Gt,
    Lt,
}

impl fmt::Display for CompareOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CompareOp::Eq => "=",
            CompareOp::NotEq => "!=",
            CompareOp::Gt => ">",
            CompareOp::Lt => "<",
        })
    }
}

/// A literal value appearing on the right-hand side of a comparison.
///
/// The type distinction matters because milestone 4's executor binds
/// numbers and booleans with their native SQLite types so numeric
/// comparison (`req_id > 100`) uses proper ordering rather than lexical.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
}

/// A relative duration parsed from `last <N><unit>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration {
    pub amount: u64,
    pub unit: DurationUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Minutes,
    Hours,
    Days,
}

impl DurationUnit {
    /// Total seconds for one unit. The executor multiplies by `amount` to
    /// compute the cutoff timestamp against `now`.
    pub fn seconds(self) -> i64 {
        match self {
            DurationUnit::Minutes => 60,
            DurationUnit::Hours => 60 * 60,
            DurationUnit::Days => 24 * 60 * 60,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Parse error with a byte offset into the original input.
///
/// Byte offsets (rather than line/column) are sufficient because queries
/// are single-line. The CLI's milestone 7 pretty printer can slice the
/// original input around `position` to render a caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryParseError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "query parse error at position {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for QueryParseError {}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A bare identifier — could be a field name, a bare-word value, or a
    /// keyword depending on position. We resolve keywords at parse time
    /// rather than at tokenization time because "last" used as a field name
    /// (in the unlikely event a log has a field literally called "last")
    /// should still work in `CONTAINS` contexts.
    Ident(String),
    /// A double-quoted string, with the quotes stripped.
    QuotedString(String),
    /// A literal number — stored as text so the parser can decide whether
    /// it's an integer or float.
    Number(String),
    Eq,
    NotEq,
    Gt,
    Lt,
}

#[derive(Debug, Clone)]
struct SpannedToken {
    token: Token,
    position: usize,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Return true if `b` is allowed *inside* an identifier (but not necessarily
/// as the first byte). Matches the grammar's field rule plus the extra
/// characters needed for bare-word values and datetime literals: `-` for
/// hyphenated values like `x-request-id`, `:` for colon-separated values
/// like time components, and `.` for both dotted field names and float-like
/// version strings in values.
fn is_ident_continuation(b: u8) -> bool {
    b == b'_' || b == b'.' || b == b'-' || b == b':' || b.is_ascii_alphanumeric()
}

/// Split the input into a stream of tokens with byte-offset positions.
///
/// Whitespace is skipped. Unrecognized bytes produce a `QueryParseError`
/// pointing at the offending character.
fn tokenize(input: &str) -> Result<Vec<SpannedToken>, QueryParseError> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();

    while i < bytes.len() {
        let c = bytes[i];

        // Whitespace.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Operators — order matters: check `!=` before `!` would-be, and
        // both before single `<`/`>`/`=`.
        if c == b'!' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                out.push(SpannedToken {
                    token: Token::NotEq,
                    position: i,
                });
                i += 2;
                continue;
            }
            return Err(QueryParseError {
                position: i,
                message: "unexpected '!' — did you mean '!='?".to_string(),
            });
        }
        if c == b'=' {
            out.push(SpannedToken {
                token: Token::Eq,
                position: i,
            });
            i += 1;
            continue;
        }
        if c == b'>' {
            out.push(SpannedToken {
                token: Token::Gt,
                position: i,
            });
            i += 1;
            continue;
        }
        if c == b'<' {
            out.push(SpannedToken {
                token: Token::Lt,
                position: i,
            });
            i += 1;
            continue;
        }

        // Quoted string.
        if c == b'"' {
            let start = i;
            i += 1; // consume opening quote
            let content_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                // No escape handling in v1 — the grammar is `'"' .* '"'`
                // and real log-query users don't embed quotes in values.
                // If this becomes a pain we add escape handling in v2.
                i += 1;
            }
            if i >= bytes.len() {
                return Err(QueryParseError {
                    position: start,
                    message: "unterminated quoted string".to_string(),
                });
            }
            let s = std::str::from_utf8(&bytes[content_start..i])
                .expect("input is &str, slice is UTF-8")
                .to_string();
            i += 1; // consume closing quote
            out.push(SpannedToken {
                token: Token::QuotedString(s),
                position: start,
            });
            continue;
        }

        // Digit-led token.
        //
        // Two possibilities:
        //  - Pure-digit run (with optional fractional part) → Token::Number.
        //    Example: `100`, `1.5`.
        //  - Digit-led run that contains `-` or `:` → Token::Ident. This
        //    supports bare datetime literals like `2024-01-01T10:00:00Z`
        //    after `since`, per the 2026-04-22 decision to let bare dates
        //    tokenize as identifiers. Colon is included for completeness
        //    so time-of-day literals don't need quoting either.
        //
        // The disambiguation happens at the first non-digit, non-dot byte:
        // if that byte is `-` or `:`, we promote the whole run (and keep
        // consuming continuation bytes) to an Ident. Otherwise we stop at
        // the end of the numeric run and emit a Number.
        if c.is_ascii_digit() {
            let start = i;
            let mut saw_dot = false;

            // First phase: consume digits and at most one dot (only when
            // the dot is followed by a digit, preserving the existing
            // `1.5` behaviour). We peek at the next byte after each dot
            // to decide.
            while i < bytes.len() && (bytes[i].is_ascii_digit() || (bytes[i] == b'.' && !saw_dot)) {
                if bytes[i] == b'.' {
                    if i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
                        break;
                    }
                    saw_dot = true;
                }
                i += 1;
            }

            // Second phase: if the next byte indicates this digit-led run
            // is actually an ident (datetime, dotted version string,
            // alphanumeric suffix, etc.), keep consuming all ident-
            // continuation bytes and emit an Ident.
            //
            // Promotion triggers:
            //   `-` or `:` — datetime literals (`2024-01-01`, `10:30`)
            //   `.`        — dotted strings beyond one fractional part
            //                (`1.2.3`, which can't be a valid Number)
            //   letter     — alphanumeric suffixes (`3beta`, `v1rc2`)
            //
            // Note: the first phase stops at a *second* dot because the
            // `!saw_dot` guard fires, leaving `bytes[i]` on that second
            // dot — hence `.` being a valid trigger here.
            // Second phase: if the next byte indicates this digit-led run
            // is actually an ident (datetime, multi-dot version string),
            // keep consuming all ident-continuation bytes and emit Ident.
            //
            // Promotion triggers:
            //   `-` or `:` — datetime literals (`2024-01-01`, `10:30`)
            //   `.`        — dotted strings beyond one fractional part
            //                (`1.2.3`); the first phase stops at the
            //                second dot due to its `!saw_dot` guard,
            //                leaving `bytes[i]` on that second dot.
            //
            // Letters are intentionally NOT a promotion trigger: `30m`
            // must tokenize as Number("30") + Ident("m") so the parser's
            // `last <N><unit>` rule works. Users who want digit-led
            // values with letter suffixes (`3beta`) must quote them.
            if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b':' || bytes[i] == b'.') {
                while i < bytes.len() && is_ident_continuation(bytes[i]) {
                    i += 1;
                }
                let s = std::str::from_utf8(&bytes[start..i])
                    .expect("input is &str, slice is UTF-8")
                    .to_string();
                out.push(SpannedToken {
                    token: Token::Ident(s),
                    position: start,
                });
                continue;
            }

            let s = std::str::from_utf8(&bytes[start..i])
                .expect("ascii digits are UTF-8")
                .to_string();
            out.push(SpannedToken {
                token: Token::Number(s),
                position: start,
            });
            continue;
        }

        // Identifier / bare word: starts with letter or underscore,
        // continues per `is_ident_continuation`. Hyphen and colon are
        // allowed inside so bare-word values like `x-request-id` and
        // colon-separated fragments work; `validate_field_name` later
        // enforces the stricter field-name subset.
        if c == b'_' || c.is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && is_ident_continuation(bytes[i]) {
                i += 1;
            }
            let s = std::str::from_utf8(&bytes[start..i])
                .expect("input is &str, slice is UTF-8")
                .to_string();
            out.push(SpannedToken {
                token: Token::Ident(s),
                position: start,
            });
            continue;
        }

        return Err(QueryParseError {
            position: i,
            message: format!("unexpected character {:?}", c as char),
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a query string into a `QueryNode`.
///
/// This is the only public entry point. Implements the grammar from the
/// project doc top-down via recursive descent, with AND chaining at the
/// outermost level.
pub fn parse(input: &str) -> Result<QueryNode, QueryParseError> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(QueryParseError {
            position: 0,
            message: "empty query".to_string(),
        });
    }

    let mut p = Parser {
        tokens: &tokens,
        cursor: 0,
    };
    let mut clauses = Vec::new();
    clauses.push(p.parse_clause()?);

    while let Some(tok) = p.peek() {
        // AND is a keyword stored as an Ident. Case-insensitive.
        match &tok.token {
            Token::Ident(s) if s.eq_ignore_ascii_case("and") => {
                p.advance();
                clauses.push(p.parse_clause()?);
            }
            Token::Ident(s) if s.eq_ignore_ascii_case("or") => {
                // Specific, actionable error per the doc's emphasis on good messages.
                return Err(QueryParseError {
                    position: tok.position,
                    message: "OR is not supported in v1; only AND. See project doc decisions log."
                        .to_string(),
                });
            }
            _ => {
                return Err(QueryParseError {
                    position: tok.position,
                    message: "expected 'AND' between clauses".to_string(),
                });
            }
        }
    }

    Ok(QueryNode::And(clauses))
}

struct Parser<'a> {
    tokens: &'a [SpannedToken],
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a SpannedToken> {
        self.tokens.get(self.cursor)
    }

    fn advance(&mut self) -> Option<&'a SpannedToken> {
        let t = self.tokens.get(self.cursor);
        if t.is_some() {
            self.cursor += 1;
        }
        t
    }

    /// Position to attribute to an error when the tokens are exhausted.
    fn end_position(&self) -> usize {
        self.tokens
            .last()
            .map(|t| t.position + token_len(&t.token))
            .unwrap_or(0)
    }

    fn parse_clause(&mut self) -> Result<Clause, QueryParseError> {
        let tok = self.peek().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected a clause, got end of input".to_string(),
        })?;

        // Time-range clauses are keyword-led.
        if let Token::Ident(s) = &tok.token {
            if s.eq_ignore_ascii_case("last") {
                self.advance();
                return self.parse_last_duration();
            }
            if s.eq_ignore_ascii_case("since") {
                self.advance();
                return self.parse_since_datetime();
            }
        }

        // Otherwise: field-led clause (compare or contains).
        self.parse_field_led_clause()
    }

    fn parse_last_duration(&mut self) -> Result<Clause, QueryParseError> {
        let num_tok = self.advance().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected a number after 'last'".to_string(),
        })?;
        let num_str = match &num_tok.token {
            Token::Number(s) => s,
            _ => {
                return Err(QueryParseError {
                    position: num_tok.position,
                    message: "expected a number after 'last'".to_string(),
                });
            }
        };
        if num_str.contains('.') {
            return Err(QueryParseError {
                position: num_tok.position,
                message: "duration amount must be a whole number".to_string(),
            });
        }
        let amount: u64 = num_str.parse().map_err(|_| QueryParseError {
            position: num_tok.position,
            message: format!("invalid duration amount {num_str:?}"),
        })?;

        let unit_tok = self.advance().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected a duration unit ('m', 'h', or 'd') after the number".to_string(),
        })?;
        let unit_str = match &unit_tok.token {
            Token::Ident(s) => s,
            _ => {
                return Err(QueryParseError {
                    position: unit_tok.position,
                    message: "expected a duration unit ('m', 'h', or 'd')".to_string(),
                });
            }
        };
        let unit = match unit_str.as_str() {
            "m" => DurationUnit::Minutes,
            "h" => DurationUnit::Hours,
            "d" => DurationUnit::Days,
            other => {
                return Err(QueryParseError {
                    position: unit_tok.position,
                    message: format!("unknown duration unit {other:?}, expected 'm', 'h', or 'd'"),
                });
            }
        };

        Ok(Clause::LastDuration(Duration { amount, unit }))
    }

    fn parse_since_datetime(&mut self) -> Result<Clause, QueryParseError> {
        let tok = self.advance().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected a datetime after 'since'".to_string(),
        })?;
        let dt = match &tok.token {
            Token::QuotedString(s) => s.clone(),
            Token::Ident(s) => s.clone(),
            Token::Number(s) => s.clone(),
            _ => {
                return Err(QueryParseError {
                    position: tok.position,
                    message: "expected a datetime after 'since'".to_string(),
                });
            }
        };
        Ok(Clause::SinceDatetime(dt))
    }

    fn parse_field_led_clause(&mut self) -> Result<Clause, QueryParseError> {
        let field_tok = self.advance().expect("caller peeked a token");
        let field = match &field_tok.token {
            Token::Ident(s) => s.clone(),
            _ => {
                return Err(QueryParseError {
                    position: field_tok.position,
                    message: "expected a field name".to_string(),
                });
            }
        };
        validate_field_name(&field, field_tok.position)?;

        let op_tok = self.advance().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected an operator after the field name".to_string(),
        })?;

        // CONTAINS is a keyword stored as Ident.
        if let Token::Ident(s) = &op_tok.token {
            if s.eq_ignore_ascii_case("contains") {
                let val_tok = self.advance().ok_or_else(|| QueryParseError {
                    position: self.end_position(),
                    message: "expected a string after 'contains'".to_string(),
                })?;
                let s = match &val_tok.token {
                    Token::QuotedString(s) => s.clone(),
                    Token::Ident(s) => s.clone(),
                    _ => {
                        return Err(QueryParseError {
                            position: val_tok.position,
                            message: "'contains' requires a string value".to_string(),
                        });
                    }
                };
                return Ok(Clause::Contains { field, value: s });
            }
        }

        let op = match &op_tok.token {
            Token::Eq => CompareOp::Eq,
            Token::NotEq => CompareOp::NotEq,
            Token::Gt => CompareOp::Gt,
            Token::Lt => CompareOp::Lt,
            _ => {
                return Err(QueryParseError {
                    position: op_tok.position,
                    message: "expected one of =, !=, >, <, or 'contains'".to_string(),
                });
            }
        };

        let val_tok = self.advance().ok_or_else(|| QueryParseError {
            position: self.end_position(),
            message: "expected a value after the operator".to_string(),
        })?;
        let value = token_to_query_value(val_tok)?;

        Ok(Clause::Compare { field, op, value })
    }
}

/// Enforce the grammar's field regex: `[a-zA-Z_][a-zA-Z0-9_.]*`.
///
/// The tokenizer is more permissive (it allows `-` and `:` inside idents
/// so that bare-word *values* like `x-request-id` and datetime literals
/// tokenize cleanly). We re-validate here because a field name is a
/// stricter subset.
fn validate_field_name(s: &str, position: usize) -> Result<(), QueryParseError> {
    let mut chars = s.chars();
    let first = chars.next().ok_or_else(|| QueryParseError {
        position,
        message: "empty field name".to_string(),
    })?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(QueryParseError {
            position,
            message: format!("invalid field name {s:?}: must start with a letter or underscore"),
        });
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            return Err(QueryParseError {
                position,
                message: format!(
                    "invalid field name {s:?}: only letters, digits, underscores, and dots are allowed"
                ),
            });
        }
    }
    Ok(())
}

fn token_to_query_value(tok: &SpannedToken) -> Result<QueryValue, QueryParseError> {
    match &tok.token {
        Token::QuotedString(s) => Ok(QueryValue::String(s.clone())),
        Token::Number(s) => {
            if s.contains('.') {
                let f: f64 = s.parse().map_err(|_| QueryParseError {
                    position: tok.position,
                    message: format!("invalid number {s:?}"),
                })?;
                Ok(QueryValue::Float(f))
            } else {
                let n: i64 = s.parse().map_err(|_| QueryParseError {
                    position: tok.position,
                    message: format!("invalid integer {s:?}"),
                })?;
                Ok(QueryValue::Integer(n))
            }
        }
        Token::Ident(s) => {
            // Booleans as bare words.
            if s.eq_ignore_ascii_case("true") {
                Ok(QueryValue::Bool(true))
            } else if s.eq_ignore_ascii_case("false") {
                Ok(QueryValue::Bool(false))
            } else {
                Ok(QueryValue::String(s.clone()))
            }
        }
        _ => Err(QueryParseError {
            position: tok.position,
            message: "expected a value (string, number, or boolean)".to_string(),
        }),
    }
}

fn token_len(t: &Token) -> usize {
    match t {
        Token::Ident(s) | Token::Number(s) => s.len(),
        Token::QuotedString(s) => s.len() + 2, // approximate, for error positioning only
        Token::Eq | Token::Gt | Token::Lt => 1,
        Token::NotEq => 2,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn and_of(clauses: Vec<Clause>) -> QueryNode {
        QueryNode::And(clauses)
    }

    fn cmp(field: &str, op: CompareOp, value: QueryValue) -> Clause {
        Clause::Compare {
            field: field.to_string(),
            op,
            value,
        }
    }

    // --- Each operator parses correctly ---

    #[test]
    fn eq_operator() {
        assert_eq!(
            parse("level=error").unwrap(),
            and_of(vec![cmp(
                "level",
                CompareOp::Eq,
                QueryValue::String("error".into())
            )])
        );
    }

    #[test]
    fn not_eq_operator() {
        assert_eq!(
            parse("level!=info").unwrap(),
            and_of(vec![cmp(
                "level",
                CompareOp::NotEq,
                QueryValue::String("info".into())
            )])
        );
    }

    #[test]
    fn gt_operator_with_integer() {
        assert_eq!(
            parse("req_id > 100").unwrap(),
            and_of(vec![cmp("req_id", CompareOp::Gt, QueryValue::Integer(100))])
        );
    }

    #[test]
    fn lt_operator_with_float() {
        assert_eq!(
            parse("duration < 1.5").unwrap(),
            and_of(vec![cmp("duration", CompareOp::Lt, QueryValue::Float(1.5))])
        );
    }

    #[test]
    fn contains_operator_with_quoted_string() {
        assert_eq!(
            parse(r#"message contains "database timeout""#).unwrap(),
            and_of(vec![Clause::Contains {
                field: "message".into(),
                value: "database timeout".into(),
            }])
        );
    }

    #[test]
    fn contains_operator_with_bare_word() {
        assert_eq!(
            parse("message contains timeout").unwrap(),
            and_of(vec![Clause::Contains {
                field: "message".into(),
                value: "timeout".into(),
            }])
        );
    }

    #[test]
    fn contains_is_case_insensitive() {
        assert_eq!(
            parse("message CONTAINS boom").unwrap(),
            and_of(vec![Clause::Contains {
                field: "message".into(),
                value: "boom".into(),
            }])
        );
    }

    #[test]
    fn boolean_value() {
        assert_eq!(
            parse("ok=true").unwrap(),
            and_of(vec![cmp("ok", CompareOp::Eq, QueryValue::Bool(true))])
        );
        assert_eq!(
            parse("ok=FALSE").unwrap(),
            and_of(vec![cmp("ok", CompareOp::Eq, QueryValue::Bool(false))])
        );
    }

    #[test]
    fn quoted_string_value_preserves_spaces() {
        assert_eq!(
            parse(r#"service="payments gateway""#).unwrap(),
            and_of(vec![cmp(
                "service",
                CompareOp::Eq,
                QueryValue::String("payments gateway".into())
            )])
        );
    }

    #[test]
    fn dotted_field_name_for_nested_json() {
        assert_eq!(
            parse("user.id=42").unwrap(),
            and_of(vec![cmp("user.id", CompareOp::Eq, QueryValue::Integer(42))])
        );
    }

    // --- Time ranges ---

    #[test]
    fn last_minutes() {
        assert_eq!(
            parse("last 30m").unwrap(),
            and_of(vec![Clause::LastDuration(Duration {
                amount: 30,
                unit: DurationUnit::Minutes
            })])
        );
    }

    #[test]
    fn last_hours() {
        assert_eq!(
            parse("last 2h").unwrap(),
            and_of(vec![Clause::LastDuration(Duration {
                amount: 2,
                unit: DurationUnit::Hours
            })])
        );
    }

    #[test]
    fn last_days() {
        assert_eq!(
            parse("last 7d").unwrap(),
            and_of(vec![Clause::LastDuration(Duration {
                amount: 7,
                unit: DurationUnit::Days
            })])
        );
    }

    #[test]
    fn since_datetime_is_opaque_string() {
        assert_eq!(
            parse("since 2024-01-01").unwrap(),
            and_of(vec![Clause::SinceDatetime("2024-01-01".into())])
        );
    }

    #[test]
    fn since_datetime_can_be_quoted() {
        assert_eq!(
            parse(r#"since "2024-01-01T10:00:00Z""#).unwrap(),
            and_of(vec![Clause::SinceDatetime("2024-01-01T10:00:00Z".into())])
        );
    }

    #[test]
    fn since_datetime_bare_with_time_component_parses() {
        // Regression: digit-led tokens containing `-` or `:` must tokenize
        // as Ident, not blow up mid-number.
        assert_eq!(
            parse("since 2024-01-01T10:00:00Z").unwrap(),
            and_of(vec![Clause::SinceDatetime("2024-01-01T10:00:00Z".into())])
        );
    }

    #[test]
    fn since_datetime_bare_followed_by_and_clause() {
        // The datetime must terminate at whitespace so the AND chain still works.
        assert_eq!(
            parse("since 2024-01-01 AND level=error").unwrap(),
            and_of(vec![
                Clause::SinceDatetime("2024-01-01".into()),
                cmp("level", CompareOp::Eq, QueryValue::String("error".into())),
            ])
        );
    }

    // --- AND chaining ---

    #[test]
    fn two_clauses_with_and() {
        assert_eq!(
            parse("level=error AND service=payments").unwrap(),
            and_of(vec![
                cmp("level", CompareOp::Eq, QueryValue::String("error".into())),
                cmp(
                    "service",
                    CompareOp::Eq,
                    QueryValue::String("payments".into())
                ),
            ])
        );
    }

    #[test]
    fn and_is_case_insensitive() {
        assert_eq!(
            parse("level=error and service=payments").unwrap(),
            and_of(vec![
                cmp("level", CompareOp::Eq, QueryValue::String("error".into())),
                cmp(
                    "service",
                    CompareOp::Eq,
                    QueryValue::String("payments".into())
                ),
            ])
        );
    }

    #[test]
    fn three_clauses_with_time_range() {
        assert_eq!(
            parse("tag=api AND level=error AND last 30m").unwrap(),
            and_of(vec![
                cmp("tag", CompareOp::Eq, QueryValue::String("api".into())),
                cmp("level", CompareOp::Eq, QueryValue::String("error".into())),
                Clause::LastDuration(Duration {
                    amount: 30,
                    unit: DurationUnit::Minutes
                }),
            ])
        );
    }

    // --- Error cases: invalid input produces descriptive messages ---

    #[test]
    fn empty_query_is_an_error() {
        let err = parse("").unwrap_err();
        assert_eq!(err.position, 0);
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn whitespace_only_query_is_an_error() {
        let err = parse("   ").unwrap_err();
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn missing_value_after_operator() {
        let err = parse("level=").unwrap_err();
        assert!(err.message.contains("value"));
    }

    #[test]
    fn missing_operator_after_field() {
        let err = parse("level").unwrap_err();
        assert!(err.message.contains("operator"));
    }

    #[test]
    fn unknown_duration_unit_names_the_unit() {
        let err = parse("last 5y").unwrap_err();
        assert!(err.message.contains("unit"));
        assert!(err.message.contains("\"y\""));
    }

    #[test]
    fn fractional_duration_rejected() {
        let err = parse("last 1.5h").unwrap_err();
        assert!(err.message.contains("whole number"));
    }

    #[test]
    fn or_operator_suggests_v2_deferral() {
        let err = parse("level=error OR level=warn").unwrap_err();
        assert!(err.message.contains("OR"));
        assert!(err.message.contains("AND"));
    }

    #[test]
    fn bang_without_equals_is_actionable() {
        let err = parse("level!error").unwrap_err();
        assert!(err.message.contains("!="));
    }

    #[test]
    fn unterminated_quoted_string_points_at_opening_quote() {
        let input = r#"service="oops"#;
        let err = parse(input).unwrap_err();
        assert_eq!(err.position, input.find('"').unwrap());
        assert!(err.message.contains("unterminated"));
    }

    #[test]
    fn contains_with_number_is_rejected() {
        // The grammar says `field CONTAINS string` — a bare number is not a string.
        let err = parse("message contains 42").unwrap_err();
        assert!(err.message.contains("string"));
    }

    #[test]
    fn invalid_field_name_starting_with_digit() {
        // The tokenizer turns `3foo` into a Number followed by an Ident,
        // so the parser sees a number in field position and complains.
        let err = parse("3foo=x").unwrap_err();
        assert!(err.message.contains("field"));
    }

    #[test]
    fn missing_and_between_clauses_is_actionable() {
        let err = parse("level=error service=payments").unwrap_err();
        assert!(err.message.contains("AND"));
    }

    #[test]
    fn last_without_number() {
        let err = parse("last h").unwrap_err();
        assert!(err.message.contains("number"));
    }

    #[test]
    fn last_without_unit() {
        let err = parse("last 30").unwrap_err();
        assert!(err.message.contains("unit"));
    }

    // --- Sanity checks on tokenizer edge cases ---

    #[test]
    fn tokens_survive_around_operators_with_no_spaces() {
        assert_eq!(
            parse("level=error").unwrap(),
            parse("level = error").unwrap()
        );
        assert_eq!(parse("req_id!=5").unwrap(), parse("req_id != 5").unwrap());
    }

    #[test]
    fn hyphenated_bare_word_value_parses() {
        assert_eq!(
            parse("request_id=x-request-1").unwrap(),
            and_of(vec![cmp(
                "request_id",
                CompareOp::Eq,
                QueryValue::String("x-request-1".into())
            )])
        );
    }

    #[test]
    fn digit_led_value_with_hyphen_is_string_not_number() {
        // `version=1.2.3-beta` — regression guard: this should be a string
        // value, not a parse error from trying to be a number.
        assert_eq!(
            parse("version=1.2.3-beta").unwrap(),
            and_of(vec![cmp(
                "version",
                CompareOp::Eq,
                QueryValue::String("1.2.3-beta".into())
            )])
        );
    }

    #[test]
    fn dotted_version_string_is_not_a_number() {
        // `version=1.2.3` — more than one dot means it can't be a float.
        // Must tokenize as a single Ident/String, not a Number followed
        // by an unexpected `.`.
        assert_eq!(
            parse("version=1.2.3").unwrap(),
            and_of(vec![cmp(
                "version",
                CompareOp::Eq,
                QueryValue::String("1.2.3".into())
            )])
        );
    }

    #[test]
    fn pure_digit_run_is_still_a_number() {
        // Belt-and-braces: the digit-promotion logic must not accidentally
        // turn `100` into an Ident.
        match &parse("req_id=100").unwrap() {
            QueryNode::And(clauses) => match &clauses[0] {
                Clause::Compare {
                    value: QueryValue::Integer(n),
                    ..
                } => assert_eq!(*n, 100),
                other => panic!("expected Integer value, got {other:?}"),
            },
        }
    }
}
