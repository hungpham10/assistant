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
    tool_name: String,
}

impl CodegraphHandler {
    pub fn new(service: Arc<CodegraphService>) -> Self {
        Self {
            service,
            tool_name: TOOL_NAME.to_string(),
        }
    }

    pub fn new_named(service: Arc<CodegraphService>, name: &str) -> Self {
        Self {
            service,
            tool_name: name.to_string(),
        }
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
        ToolName::namespaced(NAMESPACE, self.tool_name.clone())
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: NAMESPACE.to_string(),
            description: "Codegraph search over the local symbol/call graph. \
                Lifecycle: begin(path) -> search(action, ...) -> end(). \
                Indexing is host-only (codex codegraph index / /index)."
                .to_string(),
            tools: vec![
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: TOOL_NAME.to_string(),
                    description: "Call codegraph verbs (begin/search/end/status).".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: parameters_schema(),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "search_symbol".to_string(),
                    description: "Search for symbols by name/pattern.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "query".to_string(),
                                JsonSchema::string(Some("search query".to_string())),
                            );
                            props.insert(
                                "mode".to_string(),
                                JsonSchema::string(Some(
                                    "match mode: contains|prefix|suffix|exact|semantic|hybrid"
                                        .to_string(),
                                )),
                            );
                            props.insert(
                                "kind".to_string(),
                                JsonSchema::string(Some("symbol kind filter".to_string())),
                            );
                            props.insert(
                                "limit".to_string(),
                                JsonSchema::integer(Some("max results".to_string())),
                            );
                            props.insert(
                                "offset".to_string(),
                                JsonSchema::integer(Some("pagination offset".to_string())),
                            );
                            props
                        },
                        Some(vec!["query".to_string()]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "code_search".to_string(),
                    description: "Search for code symbols.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "query".to_string(),
                                JsonSchema::string(Some("search query".to_string())),
                            );
                            props.insert(
                                "mode".to_string(),
                                JsonSchema::string(Some(
                                    "match mode: contains|prefix|suffix|exact|semantic|hybrid"
                                        .to_string(),
                                )),
                            );
                            props.insert(
                                "kind".to_string(),
                                JsonSchema::string(Some("symbol kind filter".to_string())),
                            );
                            props.insert(
                                "limit".to_string(),
                                JsonSchema::integer(Some("max results".to_string())),
                            );
                            props.insert(
                                "offset".to_string(),
                                JsonSchema::integer(Some("pagination offset".to_string())),
                            );
                            props
                        },
                        Some(vec!["query".to_string()]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "callers".to_string(),
                    description: "Find callers of a symbol.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some(
                                    "symbol name (if id not provided)".to_string(),
                                )),
                            );
                            props.insert(
                                "depth".to_string(),
                                JsonSchema::integer(Some("call depth".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "callees".to_string(),
                    description: "Find callees of a symbol.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some("symbol name".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "context".to_string(),
                    description: "Get context for a symbol or topic.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "query".to_string(),
                                JsonSchema::string(Some("query".to_string())),
                            );
                            props.insert(
                                "depth".to_string(),
                                JsonSchema::integer(Some("depth".to_string())),
                            );
                            props.insert("include_source".to_string(), JsonSchema::boolean(None));
                            props.insert(
                                "limit".to_string(),
                                JsonSchema::integer(Some("limit".to_string())),
                            );
                            props
                        },
                        Some(vec!["query".to_string()]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "files".to_string(),
                    description: "List files in the indexed codebase.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "prefix".to_string(),
                                JsonSchema::string(Some("path prefix filter".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "flow".to_string(),
                    description: "Get flow/call chain for a symbol.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some("symbol name".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "trace_full_flow".to_string(),
                    description: "Trace full flow from a symbol.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some("symbol name".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "impact".to_string(),
                    description: "Find impact radius of changing a symbol.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some("symbol name".to_string())),
                            );
                            props.insert(
                                "max_depth".to_string(),
                                JsonSchema::integer(Some("max depth".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "symbol".to_string(),
                    description: "Get symbol details.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "id".to_string(),
                                JsonSchema::integer(Some("symbol id".to_string())),
                            );
                            props.insert(
                                "name".to_string(),
                                JsonSchema::string(Some("symbol name".to_string())),
                            );
                            props
                        },
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "stats".to_string(),
                    description: "Get codegraph statistics.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::new(),
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "code_stats".to_string(),
                    description: "Get code statistics.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::new(),
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "status".to_string(),
                    description: "Get session status.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::new(),
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "doc_search".to_string(),
                    description: "Search documents.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "query".to_string(),
                                JsonSchema::string(Some("search query".to_string())),
                            );
                            props.insert(
                                "limit".to_string(),
                                JsonSchema::integer(Some("limit".to_string())),
                            );
                            props
                        },
                        Some(vec!["query".to_string()]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "doc_list".to_string(),
                    description: "List documents.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::new(),
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "doc_stats".to_string(),
                    description: "Get document statistics.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::new(),
                        Some(vec![]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "init".to_string(),
                    description: "Bind to a workspace.".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        {
                            let mut props = BTreeMap::new();
                            props.insert(
                                "path".to_string(),
                                JsonSchema::string(Some("workspace path".to_string())),
                            );
                            props
                        },
                        Some(vec!["path".to_string()]),
                        Some(AdditionalProperties::Boolean(false)),
                    ),
                    output_schema: None,
                }),
            ],
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        let tool_name = self.tool_name.clone();
        Box::pin(async move {
            let ToolPayload::Function { arguments, .. } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "codegraph handler received unsupported payload".to_string(),
                ));
            };
            let mut args: JsonValue = if arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "invalid arguments for `codegraph.{tool_name}`: {err}"
                    ))
                })?
            };
            if !args.is_object() {
                args = json!({});
            }
            match tool_name.as_str() {
                "call" => {
                    let verb = args
                        .get("verb")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let output = match verb.as_str() {
                        "begin" | "init" => {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                            CodegraphService::begin(path).await
                        }
                        "search" => {
                            let action = args
                                .get("action")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            CodegraphService::search(&action, &args).await
                        }
                        "end" => CodegraphService::end().await,
                        "status" => CodegraphService::status().await,
                        other => format!("error: unknown verb \"{other}\""),
                    };
                    Ok(boxed_tool_output(CodegraphOutput(output)))
                }
                "search_symbol" | "code_search" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("search_symbol"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("search_symbol", &args).await,
                    )))
                }
                "callers" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("callers"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("callers", &args).await,
                    )))
                }
                "callees" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("callees"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("callees", &args).await,
                    )))
                }
                "context" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("context"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("context", &args).await,
                    )))
                }
                "files" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("files"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("files", &args).await,
                    )))
                }
                "flow" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("flow"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("flow", &args).await,
                    )))
                }
                "trace_full_flow" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("trace_full_flow"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("trace_full_flow", &args).await,
                    )))
                }
                "impact" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("impact"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("impact", &args).await,
                    )))
                }
                "symbol" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("symbol"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("symbol", &args).await,
                    )))
                }
                "stats" | "code_stats" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("stats"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("stats", &args).await,
                    )))
                }
                "status" => Ok(boxed_tool_output(CodegraphOutput(
                    CodegraphService::status().await,
                ))),
                "doc_search" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("doc_search"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("doc_search", &args).await,
                    )))
                }
                "doc_list" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("doc_list"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("doc_list", &args).await,
                    )))
                }
                "doc_stats" => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), json!("doc_stats"));
                    }
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::search("doc_stats", &args).await,
                    )))
                }
                "init" => {
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    Ok(boxed_tool_output(CodegraphOutput(
                        CodegraphService::begin(path).await,
                    )))
                }
                other => Ok(boxed_tool_output(CodegraphOutput(format!(
                    "unknown codegraph tool: {other}"
                )))),
            }
        })
    }
}

impl CoreToolRuntime for CodegraphHandler {}
