use std::sync::Arc;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_enrich::ToolFamily;
use codex_enrich::VerbSpec;
use codex_protocol::models::ResponseInputItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolExecutorFuture;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value as JsonValue;

/// Exposes a single family verb as a model-callable tool.
///
/// Dispatching the tool runs the verb's deterministic implementation through
/// `ToolFamily::run_verb`; the model only sees the JSON result (with large
/// payloads reduced to artifact metadata once artifact sinks are wired in).
pub(crate) struct FamilyVerbHandler {
    pub(crate) family: Arc<dyn ToolFamily>,
    pub(crate) verb_spec: VerbSpec,
}

impl FamilyVerbHandler {
    fn namespace_description(&self) -> String {
        format!(
            "Deterministic verbs provided by the `{}` family.",
            self.family.namespace()
        )
    }
}

struct FamilyVerbOutput {
    output: JsonValue,
}

impl ToolOutput for FamilyVerbOutput {
    fn log_preview(&self) -> String {
        self.output.to_string()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        FunctionToolOutput::from_text(
            serde_json::to_string(&self.output).unwrap_or_else(|_| "{}".to_string()),
            Some(true),
        )
        .to_response_item(call_id, payload)
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        self.output.clone()
    }
}

impl ToolExecutor<ToolInvocation> for FamilyVerbHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(self.family.namespace(), &self.verb_spec.verb)
    }

    fn spec(&self) -> ToolSpec {
        let parameters = serde_json::from_value::<JsonSchema>(self.verb_spec.parameters.clone())
            .unwrap_or_default();
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: self.family.namespace().to_string(),
            description: self.namespace_description(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: self.verb_spec.verb.clone(),
                description: self.verb_spec.description.clone(),
                strict: false,
                defer_loading: Some(self.verb_spec.defer_loading),
                parameters,
                output_schema: None,
            })],
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolPayload::Function { arguments, .. } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(format!(
                    "{}.{} handler received unsupported payload",
                    self.family.namespace(),
                    self.verb_spec.verb
                )));
            };

            let args: JsonValue = if arguments.trim().is_empty() {
                JsonValue::Null
            } else {
                serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "invalid arguments for `{}.{}`: {err}",
                        self.family.namespace(),
                        self.verb_spec.verb
                    ))
                })?
            };

            let output = self
                .family
                .run_verb(&self.verb_spec.verb, &args)
                .await
                .map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "verb `{}.{}` failed: {err:#}",
                        self.family.namespace(),
                        self.verb_spec.verb
                    ))
                })?;

            Ok(boxed_tool_output(FamilyVerbOutput { output }))
        })
    }
}

impl CoreToolRuntime for FamilyVerbHandler {}
