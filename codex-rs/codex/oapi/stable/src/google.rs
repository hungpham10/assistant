use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

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
