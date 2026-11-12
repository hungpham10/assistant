use pretty_assertions::assert_eq;

use super::StrategyCommand;
use super::parse_strategy_command;
use codex_config::types::PlanStrategiesConfig;
use codex_strategy::Phase;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn config() -> PlanStrategiesConfig {
    PlanStrategiesConfig::default()
}

#[test]
fn parses_strategy_commands() {
    assert_eq!(
        parse_strategy_command("activate:explore-code"),
        Some(StrategyCommand::Activate("explore-code".into()))
    );
    assert_eq!(
        parse_strategy_command("explore-code"),
        Some(StrategyCommand::Activate("explore-code".into())),
        "bare values activate"
    );
    assert_eq!(
        parse_strategy_command("back:"),
        Some(StrategyCommand::Back(None))
    );
    assert_eq!(
        parse_strategy_command("back:origin"),
        Some(StrategyCommand::Back(None))
    );
    assert_eq!(
        parse_strategy_command("back:sample-impact"),
        Some(StrategyCommand::Back(Some("sample-impact".into())))
    );
    assert_eq!(
        parse_strategy_command("prefer: codegraph.impact , shell"),
        Some(StrategyCommand::Prefer(vec![
            "codegraph.impact".into(),
            "shell".into()
        ]))
    );
    assert_eq!(parse_strategy_command("activate:"), None);
    assert_eq!(parse_strategy_command("prefer:"), None);
    assert_eq!(parse_strategy_command("bogus:x"), None);
}

#[test]
fn activates_builtin_strategy() {
    let tmp = tempdir().expect("tempdir");
    let runtime =
        super::StrategyRuntime::activate("explore-code", &config(), tmp.path(), tmp.path())
            .expect("builtin strategy activates");
    assert_eq!(runtime.source(), "builtin:explore-code");
    assert_eq!(runtime.model().phase(), Phase::Active);
    assert!(runtime.model().current_node().is_some());
    assert!(
        runtime.model().guidance().is_some(),
        "built-in explore strategy starts with guidance"
    );
}

#[test]
fn activation_fails_for_unknown_strategy() {
    let tmp = tempdir().expect("tempdir");
    let err = super::StrategyRuntime::activate("nope", &config(), tmp.path(), tmp.path())
        .expect_err("unknown strategy must fail");
    assert!(err.contains("not found"), "unexpected error: {err}");
}

#[test]
fn project_strategy_file_shadows_builtin() {
    let tmp = tempdir().expect("tempdir");
    let strategies = tmp.path().join(".assistant/strategies");
    fs::create_dir_all(&strategies).expect("mkdir");
    fs::write(
        strategies.join("explore-code.toml"),
        r#"
[strategy]
name = "explore-code"
description = "project override"
[origin]
mine = 1.0
[node.mine]
guidance = "project guidance"
budget = 2
"#,
    )
    .expect("write strategy");

    let runtime = super::StrategyRuntime::activate(
        "explore-code",
        &config(),
        tmp.path(),
        tempdir().expect("home").path(),
    )
    .expect("project strategy activates");
    assert!(
        runtime.source().contains(".assistant/strategies"),
        "project file wins, got {}",
        runtime.source()
    );
    assert!(runtime.source().ends_with("explore-code.toml"));
}

#[test]
fn walk_up_finds_strategies_dir_and_stops_at_git_root() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("repo");
    let nested = root.join("a/b");
    fs::create_dir_all(root.join(".git")).expect("mkdir");
    fs::create_dir_all(root.join(".assistant/strategies")).expect("mkdir");
    fs::write(root.join(".assistant/strategies/x.toml"), "x").expect("write");

    let dirs = super::walk_up_for_strategies_dirs(&nested, ".assistant/strategies");
    assert_eq!(dirs.len(), 1, "stops at the .git root: {dirs:?}");
    assert!(dirs[0].ends_with(".assistant/strategies"));
}

#[test]
fn sanitize_rejects_path_traversal() {
    assert!(
        super::discover_strategy_source("../etc/passwd", &config(), Path::new("/"), Path::new("/"))
            .is_none()
    );
    assert!(
        super::discover_strategy_source("", &config(), Path::new("/"), Path::new("/")).is_none()
    );
    assert!(
        super::discover_strategy_source("has space", &config(), Path::new("/"), Path::new("/"))
            .is_none()
    );
}

#[test]
fn user_home_strategy_is_found() {
    let home = tempdir().expect("home");
    let strategies = home.path().join("strategies");
    fs::create_dir_all(&strategies).expect("mkdir");
    fs::write(
        strategies.join("custom.toml"),
        r#"
[strategy]
name = "custom"
[origin]
solo = 1.0
[node.solo]
guidance = "solo guidance"
"#,
    )
    .expect("write");

    let cwd = tempdir().expect("cwd");
    let found = super::discover_strategy_source("custom", &config(), cwd.path(), home.path())
        .expect("user strategy found");
    assert!(found.0.ends_with("strategies/custom.toml"));
}

#[test]
fn guidance_fragment_emits_once_per_node() {
    let tmp = tempdir().expect("tempdir");
    let mut runtime =
        super::StrategyRuntime::activate("explore-code", &config(), tmp.path(), tmp.path())
            .expect("activates");
    let first = runtime.guidance_fragment().expect("first emission");
    assert!(first.contains("<STRATEGY>"));
    assert!(first.contains("step `"));
    assert!(
        runtime.guidance_fragment().is_none(),
        "no re-emission for the same node"
    );

    // Force a transition by sending a done marker toward an existing node.
    let obs = codex_strategy::TurnObservation {
        marker: Some(codex_strategy::StepMarker {
            node: None,
            status: codex_strategy::MarkerStatus::Done,
            next: Some("sample-impact".into()),
            back_to: None,
            note: None,
        }),
        ..codex_strategy::TurnObservation::default()
    };
    let _ = runtime.model_mut().observe_turn(&obs);
    let second = runtime
        .guidance_fragment()
        .expect("emits after node change");
    assert!(second.contains("sample-impact"));
}

#[test]
fn guidance_is_truncated_to_budget() {
    let tmp = tempdir().expect("tempdir");
    let mut cfg = config();
    cfg.max_guidance_bytes = Some(16);
    let mut runtime =
        super::StrategyRuntime::activate("explore-code", &cfg, tmp.path(), tmp.path())
            .expect("activates");
    let fragment = runtime.guidance_fragment().expect("emission");
    assert!(
        fragment.contains("Start from entry"),
        "guidance truncated to 16 bytes: {fragment}"
    );
    assert!(
        !fragment.contains("Start from entrypoints (main"),
        "full guidance must not survive the cap"
    );
}

#[test]
fn backtrack_target_mapping() {
    assert_eq!(
        super::backtrack_target(None),
        codex_strategy::BacktrackTarget::Origin
    );
    assert_eq!(
        super::backtrack_target(Some("b".into())),
        codex_strategy::BacktrackTarget::Node("b".into())
    );
}
