use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningControl;
use codex_protocol::openai_models::default_input_modalities;

/// Default priority assigned to models discovered live from the Generative
/// Language API when no explicit ranking is available.
const GOOGLE_LIVE_MODEL_PRIORITY: i32 = 10;

/// Default context window assumed when the API does not report one.
const GOOGLE_DEFAULT_CONTEXT_WINDOW: i64 = 128_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerateContentRequest {
    pub contents: Vec<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GoogleTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<Value>,
    /// BLOCK_NONE safety settings mirroring gemini-cli's defaults; omitted
    /// only when callers explicitly opt out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<Value>>,
}

/// Safety categories gemini-cli sends with every generateContent request
/// (all `BLOCK_NONE`) so responses are not filtered by provider defaults.
pub fn google_default_safety_settings() -> Vec<Value> {
    [
        "HARM_CATEGORY_HARASSMENT",
        "HARM_CATEGORY_HATE_SPEECH",
        "HARM_CATEGORY_SEXUALLY_EXPLICIT",
        "HARM_CATEGORY_DANGEROUS_CONTENT",
        "HARM_CATEGORY_CIVIC_INTEGRITY",
        "HARM_CATEGORY_IMAGE_DANGEROUS_CONTENT",
        "HARM_CATEGORY_IMAGE_HARASSMENT",
        "HARM_CATEGORY_IMAGE_HATE",
        "HARM_CATEGORY_IMAGE_SEXUALLY_EXPLICIT",
        "HARM_CATEGORY_UNSPECIFIED",
        "HARM_CATEGORY_JAILBREAK",
    ]
    .into_iter()
    .map(|category| serde_json::json!({ "category": category, "threshold": "BLOCK_NONE" }))
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<GooglePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum GooglePart {
    Text {
        text: String,
    },
    FunctionCall {
        function_call: GoogleFunctionCall,
    },
    FunctionResponse {
        function_response: GoogleFunctionResponse,
    },
    InlineData {
        inline_data: GoogleInlineData,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionCall {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionResponse {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleTool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_declarations: Option<Vec<GoogleFunctionDeclaration>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerateContentResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<GoogleCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Query params for `GET v1beta/models`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleListModelsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}

/// A single model resource from the Generative Language API (`v1beta/models`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleModel {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_token_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_generation_methods: Option<Vec<String>>,
    /// Full model name returned by the API, e.g. `models/gemini-2.5-flash`.
    pub full_name: Option<String>,
}

/// Paged response from `GET v1beta/models`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleListModelsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<GoogleModel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Extract the model slug from a full resource name like `models/gemini-2.5-flash`.
pub fn model_slug_from_name(name: &str) -> String {
    name.strip_prefix("models/").unwrap_or(name).to_string()
}

/// Returns true if the model reports support for generateContent or streamGenerateContent.
pub fn supports_generate_content(methods: &[String]) -> bool {
    methods
        .iter()
        .any(|m| m == "generateContent" || m == "streamGenerateContent")
}

/// Build the `v1beta/models` path with optional page query params.
pub fn models_path(request: &GoogleListModelsRequest) -> String {
    let mut params = Vec::new();
    if let Some(page_size) = request.page_size {
        params.push(format!("pageSize={page_size}"));
    }
    if let Some(page_token) = &request.page_token
        && !page_token.is_empty()
    {
        params.push(format!("pageToken={page_token}"));
    }
    if params.is_empty() {
        "v1beta/models".to_string()
    } else {
        format!("v1beta/models?{}", params.join("&"))
    }
}

/// Convert live Generative Language API models into `ModelInfo` descriptors.
///
/// Only models that advertise `generateContent`/`streamGenerateContent` are
/// considered usable for the generate-content path; everything else is skipped.
pub fn google_models_to_model_infos(models: &[GoogleModel]) -> Vec<ModelInfo> {
    let mut infos = Vec::new();
    for model in models {
        let methods = model.supported_generation_methods.as_deref().unwrap_or(&[]);
        if !supports_generate_content(methods) {
            continue;
        }
        let slug = model_slug_from_name(&model.name);
        let context_window = model.input_token_limit.or(model.output_token_limit);
        let context_window = context_window
            .filter(|&w| w > 0)
            .unwrap_or(GOOGLE_DEFAULT_CONTEXT_WINDOW);
        infos.push(ModelInfo {
            slug: slug.clone(),
            display_name: model.display_name.clone().unwrap_or_else(|| slug.clone()),
            description: model.description.clone(),
            default_reasoning_level: None,
            supported_reasoning_levels: Vec::new(),
            reasoning_control: ReasoningControl::None,
            shell_type: codex_protocol::openai_models::ConfigShellToolType::Default,
            visibility: ModelVisibility::List,
            supported_in_api: true,
            priority: GOOGLE_LIVE_MODEL_PRIORITY,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            availability_nux: None,
            upgrade: None,
            base_instructions: String::new(),
            model_messages: None,
            include_skills_usage_instructions: false,
            supports_reasoning_summaries: false,
            default_reasoning_summary: ReasoningSummary::Auto,
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            web_search_tool_type: codex_protocol::openai_models::WebSearchToolType::Text,
            truncation_policy: codex_protocol::openai_models::TruncationPolicyConfig::bytes(
                /*limit*/ 10_000,
            ),
            supports_parallel_tool_calls: false,
            supports_image_detail_original: false,
            context_window: Some(context_window),
            max_context_window: Some(context_window),
            auto_compact_token_limit: None,
            comp_hash: None,
            effective_context_window_percent: 95,
            experimental_supported_tools: Vec::new(),
            input_modalities: default_input_modalities(),
            used_fallback_model_metadata: false,
            supports_search_tool: false,
            use_responses_lite: false,
            auto_review_model_override: None,
            tool_mode: None,
            multi_agent_version: None,
        });
    }
    infos
}

/// Code Assist (free "Login with Google") request envelope.
///
/// Wraps a Vertex-style generate content request inside the Code Assist
/// envelope expected by `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAssistGenerateContentRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_prompt_id: Option<String>,
    pub request: GoogleGenerateContentRequest,
}

/// Code Assist response envelope:
/// `{ "response": {...generateContentResponse}, "traceId": "..." }`.
///
/// Kept as public API to document the Code Assist envelope shape. Deserialized
/// in tests (see `code_assist_response_unwraps_envelope`); the SSE layer unwraps
/// `.response` directly via `serde_json::Value`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAssistGenerateContentResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<GoogleGenerateContentResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_slug_from_name_strips_prefix() {
        assert_eq!(
            model_slug_from_name("models/gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
        assert_eq!(model_slug_from_name("gemini-2.5-pro"), "gemini-2.5-pro");
    }

    #[test]
    fn supports_generate_content_filters() {
        assert!(supports_generate_content(&["generateContent".to_string()]));
        assert!(supports_generate_content(&[
            "streamGenerateContent".to_string()
        ]));
        assert!(!supports_generate_content(&["countTokens".to_string()]));
        assert!(!supports_generate_content(&[]));
    }

    #[test]
    fn google_models_to_model_infos_skips_non_generate() {
        let models = vec![
            GoogleModel {
                name: "models/gemini-2.5-flash".to_string(),
                version: None,
                display_name: Some("Gemini 2.5 Flash".to_string()),
                description: Some("flash".to_string()),
                input_token_limit: Some(1_048_576),
                output_token_limit: Some(65_536),
                supported_generation_methods: Some(vec![
                    "generateContent".to_string(),
                    "streamGenerateContent".to_string(),
                ]),
                full_name: None,
            },
            GoogleModel {
                name: "models/text-embedding-004".to_string(),
                version: None,
                display_name: None,
                description: None,
                input_token_limit: None,
                output_token_limit: None,
                supported_generation_methods: Some(vec!["embedContent".to_string()]),
                full_name: None,
            },
        ];
        let infos = google_models_to_model_infos(&models);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].slug, "gemini-2.5-flash");
        assert_eq!(infos[0].display_name, "Gemini 2.5 Flash");
        assert_eq!(infos[0].context_window, Some(1_048_576));
        assert!(infos[0].supported_in_api);
        assert_eq!(infos[0].visibility, ModelVisibility::List);
    }

    #[test]
    fn list_models_response_parses_pagination() {
        let res: GoogleListModelsResponse = serde_json::from_value(serde_json::json!({
            "models": [
                { "name": "models/gemini-2.5-flash" }
            ],
            "nextPageToken": "page-2"
        }))
        .unwrap();
        assert_eq!(res.models.as_deref().unwrap().len(), 1);
        assert_eq!(res.next_page_token.as_deref(), Some("page-2"));
    }

    #[test]
    fn models_path_builds_query() {
        assert_eq!(
            models_path(&GoogleListModelsRequest::default()),
            "v1beta/models"
        );
        assert_eq!(
            models_path(&GoogleListModelsRequest {
                page_size: Some(5),
                page_token: Some("tok".to_string()),
            }),
            "v1beta/models?pageSize=5&pageToken=tok"
        );
    }

    #[test]
    fn code_assist_request_serializes_camel_case() {
        let req = CodeAssistGenerateContentRequest {
            model: "models/gemini-2.5-flash".to_string(),
            project: Some("123456".to_string()),
            user_prompt_id: Some("prompt-1".to_string()),
            request: GoogleGenerateContentRequest {
                contents: vec![],
                tools: None,
                system_instruction: None,
                generation_config: None,
                safety_settings: None,
            },
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "models/gemini-2.5-flash");
        assert_eq!(v["project"], "123456");
        assert_eq!(v["userPromptId"], "prompt-1");
        assert!(v.get("request").is_some());
    }

    #[test]
    fn code_assist_response_unwraps_envelope() {
        let env: CodeAssistGenerateContentResponse = serde_json::from_value(
            serde_json::json!({ "response": { "candidates": [] }, "traceId": "trace-1" }),
        )
        .unwrap();
        assert_eq!(env.trace_id.as_deref(), Some("trace-1"));
        assert!(env.response.is_some());
    }
}
