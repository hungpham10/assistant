use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::ResponseInputItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::codegraph::service::CodegraphService;
use serde_json::Value as JsonValue;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

const NAMESPACE: &str = "codegraph";
const TOOL_NAME: &str = "call";

pub struct CodegraphHandler {
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

impl ToolExecutor<ToolInvocation> for CodegraphHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(NAMESPACE, TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: NAMESPACE.to_string(),
            description: "Codegraph verb-family tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: TOOL_NAME.to_string(),
                description: "Call codegraph verbs (init/search/symbol/files/callers/callees/impact/flow/docs/status).".to_string(),
                strict: false,
                defer_loading: None,
                parameters: JsonSchema::object(
                    BTreeMap::new(),
                    None,
                    Some(false.into()),
                ),
                output_schema: None,
            })],
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            if !matches!(invocation.payload, ToolPayload::Function { .. }) {
                return Err(FunctionCallError::RespondToModel(
                    "codegraph handler received unsupported payload".to_string(),
                ));
            }
            Ok(boxed_tool_output(CodegraphOutput(
                "codegraph: attached (skeleton)".to_string(),
            )))
        })
    }
}

impl CoreToolRuntime for CodegraphHandler {}
