//! Property-based tests for the logdive query parser.
//!
//! These tests use `proptest` to generate arbitrary inputs and verify
//! structural invariants: the parser never panics, and valid inputs produce
//! the expected AST shape regardless of which concrete values appear.

use logdive_core::{QueryNode, parse_query};
use proptest::prelude::*;

proptest! {
    /// The parser must never panic on any arbitrary string — it must always
    /// return `Ok` or `Err`, never unwind the stack.
    #[test]
    fn prop_parse_arbitrary_string_never_panics(s in ".*") {
        let _ = parse_query(&s);
    }

    /// Any `field=value` query built from a valid identifier and an
    /// alphanumeric value must produce an OR node with exactly one AND-group
    /// containing exactly one clause — when parsing succeeds at all.
    ///
    /// Some identifiers coincide with reserved words (`since`, `last`,
    /// `true`, etc.); the parser may legitimately reject them. The invariant
    /// is: *if* parsing succeeds, the shape is exactly one group, one clause.
    #[test]
    fn prop_valid_equality_parses_to_single_group(
        field in "[a-zA-Z_][a-zA-Z0-9_]{0,15}",
        value in "[a-zA-Z0-9]{1,20}",
    ) {
        let query = format!("{field}={value}");
        if let Ok(ast) = parse_query(&query) {
            let QueryNode::Or(groups) = &ast;
            prop_assert_eq!(groups.len(), 1, "single equality → one OR group");
            prop_assert_eq!(groups[0].clauses.len(), 1, "single equality → one clause");
        }
    }

    /// N disjuncts joined by OR must parse to exactly N AND-groups in the
    /// `QueryNode::Or` vec. The query template uses `level=vN` which never
    /// collides with a reserved word, so every generated query must parse.
    #[test]
    fn prop_n_or_disjuncts_yields_n_groups(n in 1usize..=10usize) {
        let query: String = (0..n)
            .map(|i| format!("level=v{i}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let ast = parse_query(&query).expect("level=vN OR … must always parse");
        let QueryNode::Or(groups) = &ast;
        prop_assert_eq!(groups.len(), n, "OR disjunct count must equal group count");
    }

    /// Quoted values containing arbitrary printable ASCII must not panic the
    /// parser, regardless of any escaping edge case inside the value literal.
    #[test]
    fn prop_quoted_value_with_printable_ascii_never_panics(
        value in "[ -~]{0,64}",
    ) {
        let query = format!(r#"message contains "{value}""#);
        let _ = parse_query(&query);
    }
}
