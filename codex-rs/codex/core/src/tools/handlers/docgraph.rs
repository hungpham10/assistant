#[cfg(feature = "codegraph")]
/// `docgraph.*` tools — document graph operations.

#[cfg(feature = "codegraph")]
use crate::function_tool::FunctionCallError;
#[cfg(feature = "codegraph")]
use crate::tools::codegraph::service::CodegraphService;
#[cfg(feature = "codegraph")]
use crate::tools::context::FunctionToolOutput;
#[cfg(feature = "codegraph")]
use crate::tools::context::ToolInvocation;
#[cfg(feature = "codegraph")]
use crate::tools::context::ToolOutput;
#[cfg(feature = "codegraph")]
use crate::tools::context::ToolPayload;
#[cfg(feature = "codegraph")]
use crate::tools::context::boxed_tool_output;
#[cfg(feature = "codegraph")]
use crate::tools::registry::CoreToolRuntime;
#[cfg(feature = "codegraph")]
use crate::tools::registry::ToolExecutor;
#[cfg(feature = "codegraph")]
use codex_protocol::models::ResponseInputItem;
#[cfg(feature = "codegraph")]
use codex_tools::AdditionalProperties;
#[cfg(feature = "codegraph")]
use codex_tools::JsonSchema;
#[cfg(feature = "codegraph")]
use codex_tools::ResponsesApiNamespace;
#[cfg(feature = "codegraph")]
use codex_tools::ResponsesApiNamespaceTool;
#[cfg(feature = "codegraph")]
use codex_tools::ResponsesApiTool;
#[cfg(feature = "codegraph")]
use codex_tools::ToolName;
#[cfg(feature = "codegraph")]
use codex_tools::ToolSpec;
#[cfg(feature = "codegraph")]
use serde_json::Value as JsonValue;
#[cfg(feature = "codegraph")]
use serde_json::json;
#[cfg(feature = "codegraph")]
use std::collections::BTreeMap;
#[cfg(feature = "codegraph")]
use std::sync::Arc;

#[cfg(feature = "codegraph")]
const NAMESPACE: &str = "docgraph";

#[cfg(feature = "codegraph")]
struct DocgraphOutput(String);

#[cfg(feature = "codegraph")]
impl ToolOutput for DocgraphOutput {
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

#[cfg(feature = "codegraph")]
pub struct DocgraphHandler {
    #[allow(dead_code)]
    service: Arc<CodegraphService>,
    tool_name: String,
}

#[cfg(feature = "codegraph")]
impl DocgraphHandler {
    pub fn new(service: Arc<CodegraphService>) -> Self {
        Self {
            service,
            tool_name: "search".to_string(),
        }
    }

    pub fn new_named(service: Arc<CodegraphService>, name: &str) -> Self {
        Self {
            service,
            tool_name: name.to_string(),
        }
    }
}

#[cfg(feature = "codegraph")]
impl ToolExecutor<ToolInvocation> for DocgraphHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(NAMESPACE, self.tool_name.clone())
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: NAMESPACE.to_string(),
            description: "Document graph operations (search, list, ingest, stats).".to_string(),
            tools: vec![
                ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "search".to_string(),
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
                    name: "list".to_string(),
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
                    name: "stats".to_string(),
                    description: "Document statistics.".to_string(),
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
                    "docgraph handler received unsupported payload".to_string(),
                ));
            };
            let mut args: JsonValue = if arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "invalid arguments for `docgraph.{tool_name}`: {err}"
                    ))
                })?
            };
            if !args.is_object() {
                args = json!({});
            }
            let Some(args_obj) = args.as_object_mut() else {
                return Err(FunctionCallError::RespondToModel(
                    "docgraph arguments must be a JSON object".to_string(),
                ));
            };
            match tool_name.as_str() {
                "search" => {
                    args_obj.insert("action".to_string(), json!("doc_search"));
                    Ok(boxed_tool_output(DocgraphOutput(
                        CodegraphService::search("doc_search", &args).await,
                    )))
                }
                "list" => {
                    args_obj.insert("action".to_string(), json!("doc_list"));
                    Ok(boxed_tool_output(DocgraphOutput(
                        CodegraphService::search("doc_list", &args).await,
                    )))
                }
                "stats" => {
                    args_obj.insert("action".to_string(), json!("doc_stats"));
                    Ok(boxed_tool_output(DocgraphOutput(
                        CodegraphService::search("doc_stats", &args).await,
                    )))
                }
                "init" => {
                    let path = args_obj
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    Ok(boxed_tool_output(DocgraphOutput(
                        CodegraphService::begin(path).await,
                    )))
                }
                other => Ok(boxed_tool_output(DocgraphOutput(format!(
                    "unknown docgraph tool: {other}"
                )))),
            }
        })
    }
}

#[cfg(feature = "codegraph")]
impl CoreToolRuntime for DocgraphHandler {}
