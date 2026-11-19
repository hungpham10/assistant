//! Code Assist (free "Login with Google" / oauth-personal) backend.
//!
//! Mirrors Gemini CLI's `code_assist/setup.ts`: load the user's Code Assist
//! profile and onboard to resolve the `cloudaicompanionProject` id needed by
//! the `v1internal:streamGenerateContent` endpoint.

use std::io;
use std::time::Duration;

use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use serde_json::json;

pub const CODE_ASSIST_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
pub const CODE_ASSIST_API_VERSION: &str = "v1internal";

/// Version string gemini-cli advertises in its User-Agent header.
const GEMINI_CLI_VERSION: &str = "0.1.5";

/// User-Agent that mirrors gemini-cli's `get_user_agent()`; Google may use
/// this header to recognize the CLI client.
fn gemini_cli_user_agent() -> String {
    format!(
        "GeminiCLI/{GEMINI_CLI_VERSION} ({}; {})",
        std::env::consts::OS.to_uppercase(),
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else {
            "unknown"
        }
    )
}

/// Client metadata mirroring gemini-cli's `get_client_metadata()`; the Code
/// Assist discovery endpoints use it to identify the calling IDE/plugin.
fn client_metadata(project_id: Option<&str>) -> serde_json::Value {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "DARWIN_ARM64",
        ("macos", "x86_64") => "DARWIN_AMD64",
        ("linux", "aarch64") => "LINUX_ARM64",
        ("linux", "x86_64") => "LINUX_AMD64",
        ("windows", _) => "WINDOWS_AMD64",
        _ => "PLATFORM_UNSPECIFIED",
    };
    let mut metadata = serde_json::json!({
        "ideType": "IDE_UNSPECIFIED",
        "platform": platform,
        "pluginType": "GEMINI",
    });
    if let Some(project) = project_id.filter(|project| !project.is_empty()) {
        metadata["duetProject"] = serde_json::Value::String(project.to_string());
    }
    metadata
}

/// Effective Code Assist base URL, overridable via env for testing/proxies.
pub fn code_assist_base_url() -> String {
    std::env::var(super::CODE_ASSIST_ENDPOINT_OVERRIDE_ENV_VAR)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| CODE_ASSIST_ENDPOINT.to_string())
}

fn method_url(method: &str) -> String {
    format!(
        "{}/{}:{method}",
        code_assist_base_url(),
        CODE_ASSIST_API_VERSION
    )
}

/// POST a JSON payload to a Code Assist method with the bearer token.
async fn ca_post(
    token: &str,
    method: &str,
    body: serde_json::Value,
) -> io::Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let resp = client
        .post(method_url(method))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::USER_AGENT, gemini_cli_user_agent())
        .json(&body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(io::Error::other)?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "Code Assist {method} returned status {status}: {text}"
        )));
    }
    serde_json::from_str(&text).map_err(|err| {
        io::Error::other(format!(
            "Code Assist {method} returned invalid JSON: {err}: {text}"
        ))
    })
}

/// GET an LRO operation until it completes, returning the final response.
async fn ca_get_operation(token: &str, name: &str) -> io::Result<serde_json::Value> {
    let url = format!(
        "{}/{}/{}",
        code_assist_base_url(),
        CODE_ASSIST_API_VERSION,
        name
    );
    let client = reqwest::Client::new();
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let resp = client
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, gemini_cli_user_agent())
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(io::Error::other)?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(io::Error::other(format!(
                "Code Assist getOperation returned status {status}: {text}"
            )));
        }
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|err| {
            io::Error::other(format!("Code Assist getOperation invalid JSON: {err}"))
        })?;
        if value.get("done").and_then(serde_json::Value::as_bool) == Some(true) {
            return Ok(value);
        }
    }
    Err(io::Error::other(
        "Code Assist onboarding operation timed out",
    ))
}

#[derive(Deserialize)]
struct Tier {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    is_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssistResponse {
    #[serde(default)]
    current_tier: Option<Tier>,
    #[serde(default)]
    allowed_tiers: Vec<Tier>,
    #[serde(default)]
    cloudaicompanion_project: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnboardOperation {
    #[serde(default)]
    done: bool,
    #[serde(default)]
    name: Option<String>,
    response: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BucketInfo {
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetrieveUserQuotaResponse {
    #[serde(default)]
    buckets: Option<Vec<BucketInfo>>,
}

fn project_from(value: Option<&serde_json::Value>) -> Option<String> {
    let v = value?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    v.get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn pick_tier(res: &LoadCodeAssistResponse) -> Option<&Tier> {
    if let Some(tier) = &res.current_tier
        && tier.id.is_some()
    {
        return Some(tier);
    }
    res.allowed_tiers.iter().find(|t| t.is_default)
}

/// Resolve the Code Assist project id for the current Google account.
///
/// Flow mirrors Gemini CLI `setupUser`: `loadCodeAssist` then `onboardUser`
pub async fn ensure_code_assist_project(
    tokens: crate::google_oauth::GoogleTokenData,
    codex_home: &std::path::Path,
    auth_credentials_store_mode: crate::AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<(GoogleProjectResult, crate::google_oauth::GoogleTokenData)> {
    if let Some(project) = &tokens.project_id {
        return Ok((
            GoogleProjectResult {
                project_id: project.clone(),
                tier: None,
            },
            tokens,
        ));
    }
    // Mirror gemini-cli's project priority: an explicit `GOOGLE_CLOUD_PROJECT`
    // wins over discovery.
    if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
        let project = project.trim().to_string();
        if !project.is_empty() {
            let mut persisted = tokens.clone();
            persisted.project_id = Some(project.clone());
            crate::google_oauth::persist_tokens_with_project(
                codex_home,
                persisted.clone(),
                auth_credentials_store_mode,
                keyring_backend_kind,
            )?;
            return Ok((
                GoogleProjectResult {
                    project_id: project,
                    tier: None,
                },
                persisted,
            ));
        }
    }
    let token = &tokens.access_token;
    let load: LoadCodeAssistResponse = serde_json::from_value(
        ca_post(
            token,
            "loadCodeAssist",
            json!({"metadata": client_metadata(None)}),
        )
        .await?,
    )
    .map_err(|err| io::Error::other(format!("Code Assist loadCodeAssist parse failed: {err}")))?;

    let existing = project_from(load.cloudaicompanion_project.as_ref());
    let tier = pick_tier(&load);
    let tier_id = tier.and_then(|t| t.id.clone());
    let tier_name = tier.and_then(|t| t.name.clone());

    let (project, _tier_id, tier_name) = if let Some(project) = existing {
        (project, tier_id, tier_name)
    } else {
        // onboardUser: free tier onboards without an explicit project.
        let body = match &tier_id {
            Some(id) if id == "free-tier" => {
                json!({ "tierId": id, "metadata": client_metadata(None) })
            }
            Some(id) => json!({ "tierId": id, "metadata": client_metadata(None) }),
            None => json!({ "metadata": client_metadata(None) }),
        };
        let op: OnboardOperation =
            serde_json::from_value(ca_post(token, "onboardUser", body).await?).map_err(|err| {
                io::Error::other(format!("Code Assist onboardUser parse failed: {err}"))
            })?;
        let mut op = op;
        if !op.done {
            let name = op.name.clone().ok_or_else(|| {
                io::Error::other("Code Assist onboardUser returned no operation name")
            })?;
            let finished = ca_get_operation(token, &name).await?;
            op = serde_json::from_value(finished).map_err(|err| {
                io::Error::other(format!("Code Assist operation parse failed: {err}"))
            })?;
        }
        let project = op
            .response
            .as_ref()
            .and_then(|r| project_from(r.get("cloudaicompanionProject")))
            .ok_or_else(|| {
                io::Error::other(
                    "Code Assist onboarding did not return a cloudaicompanionProject; \
                     set GOOGLE_CLOUD_PROJECT or use a Google API key instead",
                )
            })?;
        (project, tier_id, tier_name)
    };

    // Cache the project on the stored token data for future sessions.
    let mut persisted = tokens.clone();
    persisted.project_id = Some(project.clone());
    crate::google_oauth::persist_tokens_with_project(
        codex_home,
        persisted.clone(),
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?;

    Ok((
        GoogleProjectResult {
            project_id: project,
            tier: tier_name,
        },
        persisted,
    ))
}

/// Returns the distinct model ids tracked by Code Assist user-quota buckets
/// for the given project, if the backend reports them.
///
/// `retrieveUserQuota` is best-effort: some accounts/tiers return no buckets,
/// in which case an empty list is returned.
pub async fn fetch_quota_model_ids(token: &str, project_id: &str) -> io::Result<Vec<String>> {
    let body = serde_json::json!({ "project": project_id });
    let res: RetrieveUserQuotaResponse =
        serde_json::from_value(ca_post(token, "retrieveUserQuota", body).await?).map_err(
            |err| io::Error::other(format!("Code Assist retrieveUserQuota parse failed: {err}")),
        )?;
    let mut ids: Vec<String> = res
        .buckets
        .unwrap_or_default()
        .into_iter()
        .filter_map(|bucket| bucket.model_id)
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Resolved Code Assist onboarding data.
#[derive(Debug, Clone)]
pub struct GoogleProjectResult {
    pub project_id: String,
    pub tier: Option<String>,
}
