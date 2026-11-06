//! `codegraph.call` agent tool — verb-style entry point over the codegraph
//! session lifecycle (`begin` → `search` → `end`, plus `status`).
//!
//! There is deliberately **no `index` verb**: indexing is a host action
//! (`codex codegraph index` / TUI `/index`); the agent only searches an
//! already-built index.

use crate::function_tool::FunctionCallError;
use crate::tools::codegraph::service::CodegraphService;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::ResponseInputItem;
use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value as JsonValue;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

const NAMESPACE: &str = "codegraph";
const TOOL_NAME: &str = "call";

pub struct CodegraphHandler {
    // Service is stateless (process-global session); kept so registration can
    // pass an `Arc` like other handlers.
    #[allow(dead_code)]
    service: Arc<CodegraphService>,
}

impl CodegraphHandler {
    pub fn new(service: Arc<CodegraphService>) -> Self {
        Self { service }
    }
}

struct CodegraphOutput(String);

impl ToolOutput for CodegraphOutput {
    fn log_preview(&self) -> String {
        self.0.clone()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        FunctionToolOutput::from_text(self.0.clone(), Some(true)).to_response_item(call_id, payload)
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        json!({})
    }
}

fn prop(schema: JsonSchema) -> BTreeMap<String, JsonSchema> {
    BTreeMap::from([("verb".to_string(), schema)])
}

/// JSON schema for the 4 verbs (`oneOf` by `verb`). No `index` verb —
/// indexing is host-only.
fn parameters_schema() -> JsonSchema {
    let verb_enum = |verb: &str| {
        JsonSchema::string_enum(
            vec![JsonValue::String(verb.to_string())],
            Some(format!("must be \"{verb}\"")),
        )
    };
    let mut begin_props = prop(verb_enum("begin"));
    begin_props.insert(
        "path".to_string(),
        JsonSchema::string(Some("absolute workspace path to bind".to_string())),
    );
    let mut search_props = prop(verb_enum("search"));
    search_props.insert(
        "action".to_string(),
        JsonSchema::string_enum(
            [
                "search_symbol",
                "symbol",
                "resolve",
                "callers",
                "callees",
                "impact",
                "flow",
                "search_flow",
                "references",
                "list_by_kind",
                "files",
                "stats",
                "doc_search",
                "doc_list",
                "doc_stats",
            ]
            .into_iter()
            .map(|s| JsonValue::String(s.to_string()))
            .collect(),
            Some("query verb to run".to_string()),
        ),
    );
    search_props.insert(
        "query".to_string(),
        JsonSchema::string(Some("search query / name / pattern".to_string())),
    );
    search_props.insert(
        "pattern".to_string(),
        JsonSchema::string(Some("flow pattern, e.g. \"LOOP, save\"".to_string())),
    );
    search_props.insert(
        "id".to_string(),
        JsonSchema::integer(Some("symbol id".to_string())),
    );
    search_props.insert(
        "name".to_string(),
        JsonSchema::string(Some("exact symbol name (resolve)".to_string())),
    );
    search_props.insert(
        "kind".to_string(),
        JsonSchema::string(Some(
            "symbol kind: function|method|class|interface|enum|variable|constant|parameter|field|module|file|config"
                .to_string(),
        )),
    );
    search_props.insert(
        "mode".to_string(),
        JsonSchema::string(Some(
            "match mode: contains|prefix|suffix|exact|semantic|hybrid".to_string(),
        )),
    );
    search_props.insert(
        "depth".to_string(),
        JsonSchema::integer(Some("callers depth (max 100 hops)".to_string())),
    );
    search_props.insert(
        "max_depth".to_string(),
        JsonSchema::integer(Some("impact max depth".to_string())),
    );
    search_props.insert(
        "limit".to_string(),
        JsonSchema::integer(Some("max results (default 30, max 100)".to_string())),
    );
    search_props.insert(
        "offset".to_string(),
        JsonSchema::integer(Some("pagination offset".to_string())),
    );
    search_props.insert(
        "prefix".to_string(),
        JsonSchema::string(Some("file path prefix filter".to_string())),
    );
    JsonSchema::one_of(
        vec![
            JsonSchema::object(
                begin_props,
                Some(vec!["verb".to_string(), "path".to_string()]),
                Some(AdditionalProperties::Boolean(false)),
            ),
            JsonSchema::object(
                search_props,
                Some(vec!["verb".to_string(), "action".to_string()]),
                Some(AdditionalProperties::Boolean(false)),
            ),
            JsonSchema::object(
                prop(verb_enum("end")),
                Some(vec!["verb".to_string()]),
                Some(AdditionalProperties::Boolean(false)),
            ),
            JsonSchema::object(
                prop(verb_enum("status")),
                Some(vec!["verb".to_string()]),
                Some(AdditionalProperties::Boolean(false)),
            ),
        ],
        Some(
            "codegraph session verbs: begin(path) -> search(action, ...) -> end(); \
             status reports the bound workspace. Always begin before search."
                .to_string(),
        ),
    )
}

impl ToolExecutor<ToolInvocation> for CodegraphHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(NAMESPACE, TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: NAMESPACE.to_string(),
            description: "Codegraph search over the local symbol/call graph. \
                Lifecycle: begin(path) -> search(action, ...) -> end(). \
                Indexing is host-only (codex codegraph index / /index)."
                .to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: TOOL_NAME.to_string(),
                description: "Call codegraph verbs (begin/search/end/status).".to_string(),
                strict: false,
                defer_loading: None,
                parameters: parameters_schema(),
                output_schema: None,
            })],
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolPayload::Function { arguments, .. } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "codegraph handler received unsupported payload".to_string(),
                ));
            };
            let args: JsonValue = if arguments.trim().is_empty() {
                JsonValue::Null
            } else {
                serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "invalid arguments for `codegraph.call`: {err}"
                    ))
                })?
            };
            let verb = args
                .get("verb")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let output = match verb {
                "begin" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    CodegraphService::begin(path).await
                }
                "search" => {
                    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    CodegraphService::search(action, &args).await
                }
                "end" => CodegraphService::end().await,
                "status" => CodegraphService::status().await,
                "index" | "deinit" => format!(
                    "error: verb \"{verb}\" is host-only — run `codex codegraph index` \
                     (or the TUI /index command) instead; codegraph.call only supports \
                     begin/search/end/status"
                ),
                other => format!(
                    "error: unknown verb \"{other}\" — codegraph.call verbs: begin, search, end, status"
                ),
            };
            Ok(boxed_tool_output(CodegraphOutput(output)))
        })
    }
}

impl CoreToolRuntime for CodegraphHandler {}
