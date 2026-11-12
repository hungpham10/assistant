use pretty_assertions::assert_eq;

use crate::cond::StrategyCondition;
use crate::cond::ToolResultKind;
use crate::doc::BacktrackTarget;
use crate::doc::StrategyParseError;
use crate::doc::builtin_strategy_source;
use crate::doc::builtin_strategy_sources;
use crate::doc::parse_strategy;

const VALID: &str = r#"
[strategy]
name = "explore-code"
description = "Map the call graph"

[origin]
entrypoint-flow = 0.6
sample-impact = 0.4

[node.entrypoint-flow]
guidance = "Trace from entrypoints."
tools = { "codegraph.flow" = 1.6, "shell" = 0.4 }
budget = 5
token_budget = 15000
backtrack_to = "origin"

[[node.entrypoint-flow.edges]]
to = "judge"
when = "tool_result:codegraph.trace_full_flow:nonempty"
likelihood = 0.8

[[node.entrypoint-flow.edges]]
to = "sample-impact"
likelihood = 0.4

[node.sample-impact]
guidance = "Sample symbols, expand via impact."
budget = 1
backtrack_to = "entrypoint-flow"

[node.judge]
guidance = "Summarize."
terminal = true
"#;

#[test]
fn parses_and_normalizes_valid_doc() {
    let doc = parse_strategy(VALID).expect("valid doc");
    assert_eq!(doc.meta.name, "explore-code");
    assert_eq!(doc.meta.description.as_deref(), Some("Map the call graph"));

    // π is normalized to sum to 1 in authored order.
    let origin_sum: f32 = doc.origin.iter().map(|(_, w)| w).sum();
    assert!((origin_sum - 1.0).abs() < 1e-6, "origin must normalize");
    assert_eq!(doc.origin[0].0, "entrypoint-flow");

    let flow = doc.node("entrypoint-flow").expect("node exists");
    assert_eq!(flow.budget, 5);
    assert_eq!(flow.token_budget, Some(15000));
    assert_eq!(flow.backtrack_to, BacktrackTarget::Origin);
    assert_eq!(flow.tools.get("codegraph.flow"), Some(&1.6));

    // Edge likelihoods within a node normalize to sum to 1.
    let row_sum: f32 = flow.edges.iter().map(|e| e.likelihood).sum();
    assert!((row_sum - 1.0).abs() < 1e-6, "A row must normalize");
    assert_eq!(
        flow.edges[0].when,
        Some(StrategyCondition::ToolResult {
            tool: "codegraph.trace_full_flow".into(),
            outcome: ToolResultKind::NonEmpty,
        })
    );

    let judge = doc.node("judge").expect("node exists");
    assert!(judge.terminal);
    assert_eq!(
        judge.backtrack_to,
        BacktrackTarget::Origin,
        "default backtrack is origin"
    );
    assert_eq!(judge.budget, 4, "default budget applies");
}

#[test]
fn default_likelihood_fills_missing_field() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.a]
"#;
    let doc = parse_strategy(source).expect("parses");
    let node = doc.node("a").expect("node");
    assert_eq!(node.edges.len(), 0, "no edges authored");
}

#[test]
fn rejects_unknown_edge_target() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.a]
[[node.a.edges]]
to = "ghost"
"#;
    assert_eq!(
        parse_strategy(source),
        Err(StrategyParseError::Node(
            "a".into(),
            "edge target `ghost` does not match any node".into()
        ))
    );
}

#[test]
fn rejects_unknown_origin_branch() {
    let source = r#"
[strategy]
name = "s"
[origin]
ghost = 1.0
[node.a]
"#;
    assert_eq!(
        parse_strategy(source),
        Err(StrategyParseError::Origin(
            "branch `ghost` does not match any node".into()
        ))
    );
}

#[test]
fn rejects_unknown_backtrack_target() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.a]
backtrack_to = "ghost"
"#;
    assert_eq!(
        parse_strategy(source),
        Err(StrategyParseError::Node(
            "a".into(),
            "backtrack target `ghost` does not match any node".into()
        ))
    );
}

#[test]
fn rejects_reserved_node_id_origin() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.origin]
"#;
    assert!(parse_strategy(source).is_err());
}

#[test]
fn rejects_empty_name_and_no_nodes() {
    let source = r#"
[strategy]
name = ""
[origin]
a = 1.0
[node.a]
"#;
    assert!(parse_strategy(source).is_err());

    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
"#;
    assert!(parse_strategy(source).is_err());
}

#[test]
fn rejects_non_positive_weights() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = -1.0
[node.a]
"#;
    assert!(parse_strategy(source).is_err());

    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.a]
tools = { "shell" = 0.0 }
"#;
    assert!(parse_strategy(source).is_err());
}

#[test]
fn rejects_invalid_condition_in_edge() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.b]
[node.a]
[[node.a.edges]]
to = "b"
when = "tool_result:shell:bogus"
"#;
    let err = parse_strategy(source).expect_err("bad condition must fail");
    assert!(err.to_string().contains("unknown result kind `bogus`"));
}

#[test]
fn rejects_unknown_top_level_tables() {
    let source = r#"
[strategy]
name = "s"
[origin]
a = 1.0
[node.a]
[bogus]
x = 1
"#;
    assert!(parse_strategy(source).is_err());
}

#[test]
fn built_in_strategies_parse() {
    let sources = builtin_strategy_sources();
    assert_eq!(sources.len(), 2);
    for (name, source) in sources {
        let doc = parse_strategy(source).expect("built-ins must parse");
        assert_eq!(doc.meta.name, name);
        assert!(!doc.nodes.is_empty());
    }
}

#[test]
fn builtin_lookup_by_name() {
    assert!(builtin_strategy_source("explore-code").is_some());
    assert!(builtin_strategy_source("write-code").is_some());
    assert!(builtin_strategy_source("nope").is_none());
}
