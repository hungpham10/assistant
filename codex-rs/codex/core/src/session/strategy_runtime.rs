//! Strategy-harness runtime glue for `/plan <strategy>`.
//!
//! The pure model lives in the `codex-strategy` crate; this module owns the
//! session-facing concerns: strategy discovery (project → user → built-in),
//! the reserved `strategy` additional-context commands, and the bounded
//! guidance fragment injected into the model context on step changes.

use codex_config::types::PlanStrategiesConfig;
use codex_protocol::protocol::StrategyStepEvent;
use codex_strategy::AdvanceOutcome;
use codex_strategy::BacktrackTarget;
use codex_strategy::StrategyModel;
use codex_strategy::StrategyTuning;
use codex_strategy::parse_strategy;
use std::path::Path;
use std::path::PathBuf;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

/// Records the turn's user text into the per-turn strategy scratch.
pub(crate) async fn record_turn_user_text(sess: &Session, turn_context: &TurnContext, text: &str) {
    let Some(turn_state) = sess
        .input_queue
        .turn_state_for_sub_id(&sess.active_turn, turn_context.sub_id.as_str())
        .await
    else {
        return;
    };
    turn_state.lock().await.strategy_scratch.set_user_text(text);
}

/// Records a dispatched tool call into the per-turn strategy scratch.
pub(crate) async fn record_turn_tool_call(
    sess: &Session,
    turn_context: &TurnContext,
    call_id: &str,
    tool_key: String,
) {
    let Some(turn_state) = sess
        .input_queue
        .turn_state_for_sub_id(&sess.active_turn, turn_context.sub_id.as_str())
        .await
    else {
        return;
    };
    turn_state
        .lock()
        .await
        .strategy_scratch
        .record_tool_call(call_id, tool_key);
}

/// Records a resolved tool output into the per-turn strategy scratch.
pub(crate) async fn record_turn_tool_output(
    sess: &Session,
    turn_context: &TurnContext,
    call_id: &str,
    errored: bool,
    body: &str,
) {
    let Some(turn_state) = sess
        .input_queue
        .turn_state_for_sub_id(&sess.active_turn, turn_context.sub_id.as_str())
        .await
    else {
        return;
    };
    turn_state
        .lock()
        .await
        .strategy_scratch
        .record_tool_output(call_id, errored, body);
}

/// Reserved key in `Op::UserInput.additional_context` carrying strategy
/// commands (kind `Application`).
pub(crate) const STRATEGY_CONTEXT_KEY: &str = "strategy";

/// Public name of the reserved `strategy` additional-context key.
pub fn strategy_context_key() -> &'static str {
    STRATEGY_CONTEXT_KEY
}

/// Default project-relative directory scanned for strategy files.
pub(crate) const DEFAULT_STRATEGIES_DIR: &str = ".assistant/strategies";

/// A command transported through the reserved additional-context key.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StrategyCommand {
    /// Activate (or switch) the session strategy by name.
    Activate(String),
    /// Backtrack to a named node, or to the origin when `None`.
    Back(Option<String>),
    /// Boost the preferred tools for the current step.
    Prefer(Vec<String>),
}

/// Parses the reserved-key value: `activate:<name>`, `back:<node|origin>`,
/// `prefer:<tool[,tool…]>`. A bare value is treated as `activate:<value>`.
pub(crate) fn parse_strategy_command(value: &str) -> Option<StrategyCommand> {
    let value = value.trim();
    let (kind, rest) = match value.split_once(':') {
        Some((kind, rest)) => (kind.trim(), rest.trim()),
        None => ("activate", value),
    };
    match kind {
        "activate" => {
            let name = rest.trim();
            (!name.is_empty()).then(|| StrategyCommand::Activate(name.to_string()))
        }
        "back" => match rest {
            "" | "origin" => Some(StrategyCommand::Back(None)),
            node => Some(StrategyCommand::Back(Some(node.to_string()))),
        },
        "prefer" => {
            let tools: Vec<String> = rest
                .split(',')
                .map(str::trim)
                .filter(|tool| !tool.is_empty())
                .map(str::to_string)
                .collect();
            (!tools.is_empty()).then(|| StrategyCommand::Prefer(tools))
        }
        _ => None,
    }
}

/// Session-scoped strategy runtime.
#[derive(Debug)]
pub(crate) struct StrategyRuntime {
    model: StrategyModel,
    /// Source label the strategy was loaded from (for logging/status).
    source: String,
    /// Node whose guidance was last injected into the model context; the
    /// fragment is re-emitted only when this changes.
    last_injected_node: Option<String>,
    max_guidance_bytes: usize,
}

impl StrategyRuntime {
    /// Loads a strategy by name and activates it.
    pub(crate) fn activate(
        name: &str,
        config: &PlanStrategiesConfig,
        cwd: &Path,
        codex_home: &Path,
    ) -> Result<Self, String> {
        let (source, source_text) = discover_strategy_source(name, config, cwd, codex_home)
            .ok_or_else(|| format!("strategy `{name}` not found"))?;
        let doc =
            parse_strategy(&source_text).map_err(|err| format!("strategy `{name}`: {err}"))?;
        let model = StrategyModel::new(doc, tuning_from_config(config));
        let mut runtime = Self {
            model,
            source,
            last_injected_node: None,
            max_guidance_bytes: config
                .max_guidance_bytes
                .unwrap_or(DEFAULT_MAX_GUIDANCE_BYTES),
        };
        runtime.model.enter();
        Ok(runtime)
    }

    pub(crate) fn model(&self) -> &StrategyModel {
        &self.model
    }

    pub(crate) fn model_mut(&mut self) -> &mut StrategyModel {
        &mut self.model
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    /// Guidance fragment for the model context. Emits only when the active
    /// node changed (or on first call), keeping the context incremental and
    /// bounded.
    pub(crate) fn guidance_fragment(&mut self) -> Option<String> {
        if self.model.phase() != codex_strategy::Phase::Active {
            return None;
        }
        let node = self.model.current_node()?.to_string();
        if self.last_injected_node.as_ref() == Some(&node) {
            return None;
        }
        self.last_injected_node = Some(node.clone());
        Some(render_guidance(&self.model, &node, self.max_guidance_bytes))
    }
}

const DEFAULT_MAX_GUIDANCE_BYTES: usize = 8192;

fn tuning_from_config(config: &PlanStrategiesConfig) -> StrategyTuning {
    let default = StrategyTuning::default();
    StrategyTuning {
        learning_rate: config.learning_rate.unwrap_or(default.learning_rate),
        explore_epsilon: config.explore_epsilon.unwrap_or(default.explore_epsilon),
        strong_prior_weight: config
            .strong_prior_weight
            .unwrap_or(default.strong_prior_weight),
        max_visits_per_node: default.max_visits_per_node,
        max_transitions: default.max_transitions,
        default_token_budget: default.default_token_budget,
    }
}

/// Renders the bounded developer fragment for a step.
fn render_guidance(model: &StrategyModel, node: &str, max_bytes: usize) -> String {
    let doc = model.doc();
    let guidance = doc
        .node(node)
        .map(|n| n.guidance.as_str())
        .unwrap_or_default();
    let mut guidance = guidance.trim().to_string();
    if guidance.len() > max_bytes {
        // Truncate at a char boundary; `String::truncate` panics otherwise.
        let mut end = max_bytes;
        while end > 0 && !guidance.is_char_boundary(end) {
            end -= 1;
        }
        guidance.truncate(end);
    }
    let description = doc.meta.description.as_deref().unwrap_or_default();
    format!(
        "<STRATEGY>\n\
         Strategy `{name}` — step `{node}` (the strategy is a suggestion, not a mandate).\n\
         {description_line}\
         {guidance}\n\
         When a step is clearly done or clearly wrong, end your message with a one-line marker: \
         `<step id=\"{node}\" status=\"done\" next=\"<node>\"/>` or \
         `<step id=\"{node}\" status=\"wrong\" back=\"<node|origin>\"/>`.\n\
         </STRATEGY>",
        name = doc.meta.name,
        description_line = if description.is_empty() {
            String::new()
        } else {
            format!("{description}\n")
        },
    )
}

/// Discovery order: project directory (walked up to the project root),
/// user directory (`<codex_home>/strategies`), then built-ins. Returns the
/// source label and TOML text.
pub(crate) fn discover_strategy_source(
    name: &str,
    config: &PlanStrategiesConfig,
    cwd: &Path,
    codex_home: &Path,
) -> Option<(String, String)> {
    let sanitized = sanitize_strategy_name(name)?;
    let dir_name = config
        .dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
        .unwrap_or(DEFAULT_STRATEGIES_DIR);

    for dir in walk_up_for_strategies_dirs(cwd, dir_name) {
        let candidate = dir.join(format!("{sanitized}.toml"));
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            return Some((candidate.display().to_string(), text));
        }
    }
    let user_candidate = codex_home
        .join("strategies")
        .join(format!("{sanitized}.toml"));
    if let Ok(text) = std::fs::read_to_string(&user_candidate) {
        return Some((user_candidate.display().to_string(), text));
    }
    codex_strategy::builtin_strategy_source(&sanitized)
        .map(|text| (format!("builtin:{sanitized}"), text.to_string()))
}

/// Lists strategy names available to a session: project strategies first
/// (walking up from `cwd`), then user-level, then built-ins. Duplicates are
/// removed keeping the first occurrence. Used by the TUI to decide whether
/// `/plan <name>` names a strategy or is plain plan text.
pub fn available_strategy_names(
    config: &PlanStrategiesConfig,
    cwd: &Path,
    codex_home: &Path,
) -> Vec<String> {
    let dir_name = config
        .dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
        .unwrap_or(DEFAULT_STRATEGIES_DIR);
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    };
    for dir in walk_up_for_strategies_dirs(cwd, dir_name) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml")
                    && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
                {
                    push(stem);
                }
            }
        }
    }
    let user_dir = codex_home.join("strategies");
    if let Ok(entries) = std::fs::read_dir(&user_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml")
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                push(stem);
            }
        }
    }
    for (builtin, _) in codex_strategy::builtin_strategy_sources() {
        push(builtin);
    }
    names
}

/// Restricts strategy names to a safe file-stem charset.
fn sanitize_strategy_name(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(name)
}

/// Walks from `cwd` up to (and including) the project root (`.git` marker),
/// collecting every `<dir>/<strategies_dir>` that exists.
fn walk_up_for_strategies_dirs(cwd: &Path, strategies_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let candidate = dir.join(strategies_dir);
        if candidate.is_dir() {
            dirs.push(candidate);
        }
        if dir.join(".git").exists() {
            break;
        }
        current = dir.parent();
    }
    dirs
}

/// Resolves a manual backtrack command target.
pub(crate) fn backtrack_target(node: Option<String>) -> BacktrackTarget {
    match node {
        None => BacktrackTarget::Origin,
        Some(node) => BacktrackTarget::Node(node),
    }
}

/// Snapshot of the active strategy state for one turn, consumed by tool
/// selection (Jev) and spec planning.
#[derive(Debug, Clone)]
pub(crate) struct StrategyTurnContext {
    pub(crate) name: String,
    pub(crate) node: String,
    pub(crate) guidance: String,
    pub(crate) tool_weights: std::collections::BTreeMap<String, f32>,
    pub(crate) strong_prior_tools: Vec<String>,
}

/// Reads the active strategy snapshot for a session, if steering is live.
pub(crate) async fn strategy_context_for_turn(sess: &Session) -> Option<StrategyTurnContext> {
    let state = sess.state.lock().await;
    let model = &state.strategy.as_ref()?.model;
    if model.phase() != codex_strategy::Phase::Active {
        return None;
    }
    let node = model.current_node()?.to_string();
    Some(StrategyTurnContext {
        name: model.doc().meta.name.clone(),
        guidance: model.guidance()?.to_string(),
        node,
        tool_weights: model.tool_weights().cloned().unwrap_or_default(),
        strong_prior_tools: model.strong_prior_tools(),
    })
}

/// Applies a strategy command transported through the reserved
/// additional-context key, returning the step event to surface.
pub(crate) async fn apply_strategy_command(
    sess: &Session,
    turn_context: &TurnContext,
    command: StrategyCommand,
) -> Option<StrategyStepEvent> {
    if !turn_context.config.plan_strategies.enabled {
        return Some(StrategyStepEvent {
            strategy: String::new(),
            node: None,
            phase: "stuck".into(),
            detail: Some("plan_strategies is disabled in config".into()),
        });
    }
    match command {
        StrategyCommand::Activate(name) => {
            let cwd = turn_context
                .environments
                .primary()
                .and_then(|environment| environment.cwd().to_abs_path().ok());
            let codex_home = turn_context.config.codex_home.to_path_buf();
            let result = match cwd {
                Some(cwd) => StrategyRuntime::activate(
                    &name,
                    &turn_context.config.plan_strategies,
                    cwd.as_path(),
                    &codex_home,
                ),
                None => Err("no working directory available".into()),
            };
            match result {
                Ok(runtime) => {
                    let event = strategy_step_event(
                        runtime.model(),
                        &AdvanceOutcome::Advanced {
                            from: "origin".into(),
                            to: runtime.model().current_node().unwrap_or_default().into(),
                            reason: codex_strategy::TransitionReason::StartBranch,
                        },
                    );
                    sess.state.lock().await.strategy = Some(runtime);
                    tracing::info!(strategy = %name, "strategy harness activated");
                    Some(event)
                }
                Err(err) => Some(StrategyStepEvent {
                    strategy: name,
                    node: None,
                    phase: "stuck".into(),
                    detail: Some(err),
                }),
            }
        }
        StrategyCommand::Back(target) => {
            let mut state = sess.state.lock().await;
            let Some(runtime) = state.strategy.as_mut() else {
                return None;
            };
            let outcome = runtime
                .model_mut()
                .manual_backtrack(&backtrack_target(target));
            Some(strategy_step_event(runtime.model(), &outcome))
        }
        StrategyCommand::Prefer(tools) => {
            let mut state = sess.state.lock().await;
            let Some(runtime) = state.strategy.as_mut() else {
                return None;
            };
            runtime.model_mut().prefer_tools(&tools);
            let joined = tools.join(", ");
            let mut event = strategy_step_event(
                runtime.model(),
                &AdvanceOutcome::Stayed {
                    node: runtime.model().current_node().unwrap_or_default().into(),
                },
            );
            event.detail = Some(format!("preferred tools: {joined}"));
            Some(event)
        }
    }
}

/// Observes a completed turn and advances the strategy walk, returning the
/// step event to surface.
pub(crate) async fn observe_turn_completion(
    sess: &Session,
    scratch: StrategyTurnScratch,
    tokens_used: u64,
    duration_ms: u64,
    last_agent_message: Option<&str>,
) -> Option<StrategyStepEvent> {
    let mut observation = scratch.into_observation();
    observation.tokens_used = tokens_used;
    observation.duration_ms = duration_ms;
    if let Some(message) = last_agent_message {
        observation.marker = codex_strategy::StepMarker::parse(message);
    }
    let mut state = sess.state.lock().await;
    let runtime = state.strategy.as_mut()?;
    let outcome = runtime.model_mut().observe_turn(&observation);
    Some(strategy_step_event(runtime.model(), &outcome))
}

/// Frames the model state as a surfaceable step event.
fn strategy_step_event(model: &StrategyModel, outcome: &AdvanceOutcome) -> StrategyStepEvent {
    let (phase, detail) = match outcome {
        AdvanceOutcome::Stayed { node } => ("active".to_string(), format!("stayed on `{node}`")),
        AdvanceOutcome::Advanced { to, reason, .. } => (
            format_phase(model),
            format!("advanced to `{to}` ({reason:?})"),
        ),
        AdvanceOutcome::Backtracked { to, reason, .. } => (
            format_phase(model),
            format!("backtracked to `{to}` ({reason:?})"),
        ),
        AdvanceOutcome::Complete { node } => (
            "complete".to_string(),
            format!("terminal step `{node}` reached"),
        ),
        AdvanceOutcome::Stuck => (
            "stuck".to_string(),
            "loop guard fired; strategy steering stopped".to_string(),
        ),
    };
    StrategyStepEvent {
        strategy: model.doc().meta.name.clone(),
        node: model.current_node().map(str::to_string),
        phase,
        detail: Some(detail),
    }
}

fn format_phase(model: &StrategyModel) -> String {
    match model.phase() {
        codex_strategy::Phase::Active => "active".to_string(),
        codex_strategy::Phase::Complete => "complete".to_string(),
        codex_strategy::Phase::Stuck => "stuck".to_string(),
    }
}

/// Per-turn facts the strategy harness consumes at turn completion.
#[derive(Default)]
pub(crate) struct StrategyTurnScratch {
    /// Qualified tool keys dispatched during the turn (`namespace:tool`,
    /// `server/tool`, or bare builtin names).
    pub(crate) tools_called: Vec<String>,
    /// Classified tool results gathered when outputs resolve.
    pub(crate) tool_results: Vec<codex_strategy::ToolOutcome>,
    /// Lowercased user text that started the turn (capped).
    pub(crate) user_text: Option<String>,
    /// `call_id → qualified tool key` for outputs resolved later in the turn.
    pub(crate) call_names: std::collections::HashMap<String, String>,
}

impl StrategyTurnScratch {
    /// Caps the user text kept for `user_text_contains` conditions.
    pub(crate) const MAX_USER_TEXT_CHARS: usize = 2000;

    pub(crate) fn record_tool_call(&mut self, call_id: &str, tool_key: String) {
        self.tools_called.push(tool_key.clone());
        self.call_names.insert(call_id.to_string(), tool_key);
    }

    pub(crate) fn set_user_text(&mut self, text: &str) {
        let mut text = text.trim().to_lowercase();
        if text.len() > Self::MAX_USER_TEXT_CHARS {
            let mut end = Self::MAX_USER_TEXT_CHARS;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        self.user_text = Some(text);
    }

    /// Classifies a resolved tool output for the recorded call.
    pub(crate) fn record_tool_output(&mut self, call_id: &str, errored: bool, body: &str) {
        let Some(tool) = self.call_names.get(call_id).cloned() else {
            return;
        };
        let kind = if errored {
            codex_strategy::ToolResultKind::Error
        } else if body.trim().is_empty() {
            codex_strategy::ToolResultKind::Empty
        } else {
            codex_strategy::ToolResultKind::NonEmpty
        };
        self.tool_results
            .push(codex_strategy::ToolOutcome::new(tool, kind));
    }

    /// Converts the scratch into a crate observation for `observe_turn`.
    pub(crate) fn into_observation(self) -> codex_strategy::TurnObservation {
        codex_strategy::TurnObservation {
            tools_called: self.tools_called,
            tool_results: self.tool_results,
            tokens_used: 0,
            duration_ms: 0,
            user_text: self.user_text,
            marker: None,
        }
    }
}

#[cfg(test)]
#[path = "strategy_runtime_tests.rs"]
mod tests;
