use pretty_assertions::assert_eq;

use crate::cond::ToolResultKind;
use crate::observe::MarkerStatus;
use crate::observe::StepMarker;
use crate::observe::ToolOutcome;
use crate::observe::TurnObservation;

#[test]
fn parses_self_closing_marker() {
    let text = "worked through the trace.\n<step id=\"trace\" status=\"done\" next=\"judge\"/>";
    let marker = StepMarker::parse(text).expect("marker should parse");
    assert_eq!(
        marker,
        StepMarker {
            node: Some("trace".into()),
            status: MarkerStatus::Done,
            next: Some("judge".into()),
            back_to: None,
            note: None,
        }
    );
}

#[test]
fn parses_wrong_marker_with_back_target() {
    let text = "<step id='orient' status='wrong' back='bind' note='symbol not found'></step>";
    let marker = StepMarker::parse(text).expect("marker should parse");
    assert_eq!(marker.status, MarkerStatus::Wrong);
    assert_eq!(marker.back_to.as_deref(), Some("bind"));
    assert_eq!(marker.note.as_deref(), Some("symbol not found"));
}

#[test]
fn defaults_status_to_done() {
    let marker = StepMarker::parse("<step/>").expect("marker should parse");
    assert_eq!(marker.status, MarkerStatus::Done);
    assert_eq!(marker.node, None);
}

#[test]
fn returns_none_without_marker() {
    assert_eq!(StepMarker::parse("plain assistant text, no marker"), None);
    assert_eq!(StepMarker::parse(""), None);
    assert_eq!(StepMarker::parse("<step unclosed"), None);
}

#[test]
fn ignores_unknown_status_values() {
    let marker = StepMarker::parse("<step status=\"bogus\"/>").expect("marker should parse");
    assert_eq!(
        marker.status,
        MarkerStatus::Done,
        "unknown status falls back to done"
    );
}

#[test]
fn first_marker_wins() {
    let text = "<step id=\"a\" status=\"wrong\"/> later <step id=\"b\" status=\"done\"/>";
    let marker = StepMarker::parse(text).expect("marker should parse");
    assert_eq!(marker.node.as_deref(), Some("a"));
    assert_eq!(marker.status, MarkerStatus::Wrong);
}

#[test]
fn observation_defaults() {
    let obs = TurnObservation::default();
    assert_eq!(
        obs,
        TurnObservation {
            tools_called: Vec::new(),
            tool_results: Vec::new(),
            tokens_used: 0,
            duration_ms: 0,
            user_text: None,
            marker: None,
        }
    );
}

#[test]
fn tool_outcome_constructor() {
    let outcome = ToolOutcome::new("codegraph.impact", ToolResultKind::NonEmpty);
    assert_eq!(
        outcome,
        ToolOutcome {
            tool: "codegraph.impact".into(),
            kind: ToolResultKind::NonEmpty,
        }
    );
}
