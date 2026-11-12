use pretty_assertions::assert_eq;

use crate::cond::ConditionParseError;
use crate::cond::StrategyCondition;
use crate::cond::ToolResultKind;

#[test]
fn parses_all_condition_kinds() {
    assert_eq!(
        StrategyCondition::parse("always"),
        Ok(StrategyCondition::Always)
    );
    assert_eq!(
        StrategyCondition::parse("tool_called:shell"),
        Ok(StrategyCondition::ToolCalled("shell".into()))
    );
    assert_eq!(
        StrategyCondition::parse("tool_result:codegraph.trace_full_flow:nonempty"),
        Ok(StrategyCondition::ToolResult {
            tool: "codegraph.trace_full_flow".into(),
            outcome: ToolResultKind::NonEmpty,
        })
    );
    assert_eq!(
        StrategyCondition::parse("tool_result:codegraph.search_symbol:error"),
        Ok(StrategyCondition::ToolResult {
            tool: "codegraph.search_symbol".into(),
            outcome: ToolResultKind::Error,
        })
    );
    assert_eq!(
        StrategyCondition::parse("turn_count_ge:3"),
        Ok(StrategyCondition::TurnCountGe(3))
    );
    assert_eq!(
        StrategyCondition::parse("user_text_contains:go back"),
        Ok(StrategyCondition::UserTextContains("go back".into()))
    );
}

#[test]
fn trims_whitespace_around_parts() {
    assert_eq!(
        StrategyCondition::parse("  tool_called: codegraph.impact  "),
        Ok(StrategyCondition::ToolCalled("codegraph.impact".into()))
    );
}

#[test]
fn display_round_trips_parse() {
    let raw = "tool_result:codegraph.trace_full_flow:nonempty";
    let parsed = StrategyCondition::parse(raw).expect("condition should parse");
    assert_eq!(parsed.to_string(), raw);
    assert_eq!(
        StrategyCondition::parse(&parsed.to_string()),
        Ok(parsed),
        "display output must re-parse"
    );
}

#[test]
fn rejects_unknown_kind() {
    let err = StrategyCondition::parse("magic:thing").expect_err("unknown kind must fail");
    assert_eq!(
        err,
        ConditionParseError::new(
            "magic:thing",
            "unknown condition kind `magic`; expected one of always, tool_called, tool_result, turn_count_ge, user_text_contains"
        )
    );
}

#[test]
fn rejects_empty_arguments() {
    assert!(StrategyCondition::parse("tool_called:").is_err());
    assert!(StrategyCondition::parse("tool_result:shell").is_err());
    assert!(StrategyCondition::parse("user_text_contains: ").is_err());
}

#[test]
fn rejects_unknown_result_kind() {
    let err =
        StrategyCondition::parse("tool_result:shell:sometimes").expect_err("bad kind must fail");
    assert!(err.to_string().contains("unknown result kind `sometimes`"));
}

#[test]
fn rejects_non_numeric_turn_count() {
    assert!(StrategyCondition::parse("turn_count_ge:many").is_err());
    assert!(
        StrategyCondition::parse("turn_count_ge:0").is_ok(),
        "zero is a valid lower bound"
    );
}

#[test]
fn always_requires_no_argument() {
    assert!(StrategyCondition::parse("always:extra").is_err());
}
