//! ZCode (Z.AI) OAuth login and token management.
//!
//! Mirrors the ZCode CLI's native polling flow (contract confirmed from the
//! ZCode desktop app runtime): request an authorization URL from the ZCode
//! broker with a client-generated bearer poll token, open it in the browser,
//! then poll until the user signs in, exchange the OAuth token for the Z.AI
//! business access token, and resolve the Coding Plan project access token
//! used for inference.
//!
//! Contracts (verified against the live broker):
//! - `POST {base}/api/v1/oauth/cli/init`
//!   headers: `Authorization: Bearer <clientToken>`, `Content-Type: application/json`
//!   body: `{"provider":"zai"}`
//!   → `{code, msg, data:{authorize_url, flow_id, poll_token, expires_at, poll_interval_sec}}`
//! - `GET {base}/api/v1/oauth/cli/poll/{flow_id}`
//!   headers: `Authorization: Bearer <data.poll_token>` → `{code, msg, data}`
//! - `POST https://api.z.ai/api/auth/z/login` `{token}` → `{access_token}` (business token)
//! - `GET https://api.z.ai/api/biz/customer/getCustomerInfo` → organizations/projects

use std::io;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use codex_config::types::AuthCredentialsStoreMode;
use serde::Deserialize;
use serde::Serialize;

pub mod login;

pub use login::ZcodeLoginOptions;
pub use login::login_with_zcode_oauth;

/// ZCode broker that mediates the OAuth flow (it owns the app secret).
pub const ZCODE_BASE_URL: &str = "https://zcode.z.ai";
pub const ZCODE_OAUTH_INIT_PATH: &str = "/api/v1/oauth/cli/init";
/// Poll path prefix; the flow id is appended: `/api/v1/oauth/cli/poll/{flow_id}`.
pub const ZCODE_OAUTH_POLL_PATH: &str = "/api/v1/oauth/cli/poll/";
/// OAuth provider identifier sent in the init body.
pub const ZCODE_OAUTH_PROVIDER: &str = "zai";

/// Z.AI API used for the business-token exchange and project resolution.
pub const ZAI_BASE_URL: &str = "https://api.z.ai";
pub const ZAI_LOGIN_EXCHANGE_PATH: &str = "/api/auth/z/login";
pub const ZAI_CUSTOMER_INFO_PATH: &str = "/api/biz/customer/getCustomerInfo";
/// Project API-key namespace: `/api/biz/v1/organization/{org}/projects/{project}/api_keys`.
pub const ZAI_PROJECT_API_KEYS_PATH: &str = "/api/biz/v1/organization";
/// Name of the Coding Plan API key the ZCode desktop creates for a project.
const ZCODE_API_KEY_NAME: &str = "zcode-api-key";
/// `usageScene` enum value for the Coding Plan (STANDARD is 2).
const USAGE_SCENE_CODING_PLAN: i64 = 1;
/// Client identity the broker expects on the access-token request.
pub const ZCODE_CLIENT_VERSION: &str = "3.18.2";

/// OpenAI-compatible Coding Plan endpoint used for inference.
pub const ZAI_CODING_CHAT_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";

pub const ZCODE_BASE_URL_OVERRIDE_ENV_VAR: &str = "CODEX_ZCODE_BASE_URL";
pub const ZAI_BASE_URL_OVERRIDE_ENV_VAR: &str = "CODEX_ZAI_BASE_URL";

/// Refresh the project access token when it expires within this window
/// (5 minutes). Project tokens are short-lived (about one hour).
const PROJECT_TOKEN_REFRESH_WINDOW_SECS: i64 = 300;

/// ZCode/Z.AI tokens persisted in `auth.json` under the `zcode_oauth` key.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ZcodeTokenData {
    /// ZCode service JWT (used for Start Plan flows; also the poll identity).
    pub zcode_jwt: String,
    /// Z.AI business access token (`zaiBusinessAccessToken`).
    pub zai_business_token: String,
    /// Coding Plan project access token used as the inference Bearer key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_token: Option<String>,
    /// Unix seconds at which the project token expires.
    #[serde(default)]
    pub project_token_expires_at: i64,
    /// Z.AI project id resolved during login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Z.AI organization that owns `project_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Unix seconds at which the tokens were last refreshed.
    pub last_refresh: i64,
}

impl ZcodeTokenData {
    /// Returns true when the project token expires within the refresh window
    /// (or is missing entirely, meaning it must be (re)issued once).
    pub fn needs_project_token_refresh(&self) -> bool {
        match self.project_token.as_deref() {
            Some(token) if !token.trim().is_empty() => {
                self.project_token_expires_at - Utc::now().timestamp()
                    <= PROJECT_TOKEN_REFRESH_WINDOW_SECS
            }
            _ => true,
        }
    }
}

/// The token exchange endpoint, overridable for tests/proxies.
fn zcode_base_url() -> String {
    std::env::var(ZCODE_BASE_URL_OVERRIDE_ENV_VAR)
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| ZCODE_BASE_URL.to_string())
}

/// The Z.AI API base URL, overridable for tests/proxies.
fn zai_base_url() -> String {
    std::env::var(ZAI_BASE_URL_OVERRIDE_ENV_VAR)
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| ZAI_BASE_URL.to_string())
}

/// POST JSON to an endpoint and parse the `{code, msg, data}` envelope.
pub(crate) async fn post_json(
    url: &str,
    body: serde_json::Value,
    bearer: Option<&str>,
) -> io::Result<serde_json::Value> {
    let mut request = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(60));
    if let Some(bearer) = bearer {
        request = request.header(reqwest::header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let resp = request.send().await.map_err(io::Error::other)?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "POST {url} returned status {status}: {text}"
        )));
    }
    serde_json::from_str(&text)
        .map_err(|err| io::Error::other(format!("POST {url} returned invalid JSON: {err}: {text}")))
}

/// GET an endpoint with a Bearer token and parse JSON.
pub(crate) async fn get_json(url: &str, bearer: &str) -> io::Result<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {bearer}"))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(io::Error::other)?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "GET {url} returned status {status}: {text}"
        )));
    }
    serde_json::from_str(&text)
        .map_err(|err| io::Error::other(format!("GET {url} returned invalid JSON: {err}: {text}")))
}

/// Unwraps the `{code, msg, data}` envelope used by the Z.AI APIs.
///
/// The Z.AI business API reports success as either `0` or `200` (numeric or
/// string), so all four encodings are accepted; anything else is an error.
pub(crate) fn unwrap_envelope(
    value: serde_json::Value,
    context: &str,
) -> io::Result<serde_json::Value> {
    let code = value.get("code");
    let code_is_success = match code {
        None => true,
        Some(code) => {
            if let Some(code) = code.as_i64() {
                code == 0 || code == 200
            } else if let Some(code) = code.as_str() {
                code == "0" || code == "200"
            } else {
                false
            }
        }
    };
    if !code_is_success {
        let msg = value
            .get("msg")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
        let code = code.map(ToString::to_string).unwrap_or_default();
        return Err(io::Error::other(format!(
            "{context} failed (code {code}): {msg}"
        )));
    }
    Ok(value.get("data").cloned().unwrap_or(value))
}

/// Exchanges the OAuth access token from the poll response for the Z.AI
/// business access token (`zaiBusinessAccessToken`).
pub(crate) async fn exchange_business_token(oauth_access_token: &str) -> io::Result<String> {
    let url = format!("{}{ZAI_LOGIN_EXCHANGE_PATH}", zai_base_url());
    let raw = post_json(
        &url,
        serde_json::json!({ "token": oauth_access_token }),
        /*bearer*/ None,
    )
    .await?;
    let data = unwrap_envelope(raw.clone(), "z/login")?;
    let access_token = data
        .get("access_token")
        .or_else(|| raw.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            io::Error::other(format!(
                "Z.AI login did not return a business access token: {raw}"
            ))
        })?;
    Ok(access_token.to_string())
}

/// Z.AI organization/project pair that owns the Coding Plan API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectScope {
    pub organization_id: String,
    pub project_id: String,
}

/// Resolves the organization and project that hold the Coding Plan API key.
///
/// Mirrors the desktop selection: personal accounts prefer the organization
/// named `默认机构` and the project named `默认项目`, skipping shared
/// (`projectType == "2"`) projects, and otherwise take the first available.
pub(crate) async fn resolve_project_scope(
    business_token: &str,
) -> io::Result<Option<ProjectScope>> {
    let url = format!("{}{ZAI_CUSTOMER_INFO_PATH}", zai_base_url());
    let raw = get_json(&url, business_token).await?;
    let value = unwrap_envelope(raw, "getCustomerInfo")?;
    let Some(organizations) = value
        .get("organizations")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(None);
    };

    let organizations: Vec<&serde_json::Value> = organizations
        .iter()
        .filter(|org| !org_id(org).is_empty())
        .collect();
    let organization = organizations
        .iter()
        .copied()
        .find(|org| {
            org.get("organizationName")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("默认机构"))
        })
        .or_else(|| organizations.first().copied());
    let Some(organization) = organization else {
        return Ok(None);
    };

    let projects: Vec<&serde_json::Value> = organization
        .get("projects")
        .and_then(serde_json::Value::as_array)
        .map(|projects| {
            projects
                .iter()
                .filter(|project| !project_id(project).is_empty())
                .filter(|project| {
                    project
                        .get("projectType")
                        .map(project_type_is_shared)
                        .is_some_and(|shared| !shared)
                })
                .collect()
        })
        .unwrap_or_default();
    let project = projects
        .iter()
        .copied()
        .find(|project| {
            project
                .get("projectName")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("默认项目"))
        })
        .or_else(|| projects.first().copied());
    let Some(project) = project else {
        return Ok(None);
    };

    Ok(Some(ProjectScope {
        organization_id: org_id(organization),
        project_id: project_id(project),
    }))
}

/// `projectType` is documented as a string but the API has also returned
/// numbers; treat both encodings as the same value.
fn project_type_is_shared(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value.trim() == "2",
        serde_json::Value::Number(value) => value.as_i64() == Some(2),
        _ => false,
    }
}

fn org_id(organization: &serde_json::Value) -> String {
    organization
        .get("organizationId")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn project_id(project: &serde_json::Value) -> String {
    project
        .get("projectId")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

/// URL of the project's API-key collection.
fn project_api_keys_url(base: &str, scope: &ProjectScope) -> String {
    format!(
        "{base}{ZAI_PROJECT_API_KEYS_PATH}/{}/projects/{}/api_keys",
        urlencoding::encode(&scope.organization_id),
        urlencoding::encode(&scope.project_id)
    )
}

/// Returns the project's Coding Plan API key id, creating the key when the
/// desktop-issued `zcode-api-key` does not exist yet.
async fn ensure_project_api_key(business_token: &str, api_keys_url: &str) -> io::Result<String> {
    let raw = get_json(api_keys_url, business_token).await?;
    let existing = unwrap_envelope(raw, "api_keys")?;
    if let Some(key_id) = existing
        .as_array()
        .and_then(|keys| {
            keys.iter().find(|key| {
                key.get("name").and_then(serde_json::Value::as_str) == Some(ZCODE_API_KEY_NAME)
            })
        })
        .and_then(|key| key.get("apiKey"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|key_id| !key_id.is_empty())
    {
        return Ok(key_id.to_string());
    }

    let raw = post_json(
        api_keys_url,
        serde_json::json!({
            "name": ZCODE_API_KEY_NAME,
            "usageScene": USAGE_SCENE_CODING_PLAN,
        }),
        Some(business_token),
    )
    .await?;
    let created = unwrap_envelope(raw, "api_keys create")?;
    created
        .get("apiKey")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|key_id| !key_id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            io::Error::other(format!(
                "api_keys create response is missing apiKey: {created}"
            ))
        })
}

/// Issues a Coding Plan project access token for the business account.
///
/// The desktop exchanges the project's `zcode-api-key` id for a short-lived
/// project access token; that token is what the Coding Plan endpoint accepts.
/// Returns `(token, expires_at_unix_secs)`.
pub(crate) async fn issue_project_token(
    business_token: &str,
    scope: &ProjectScope,
) -> io::Result<(String, i64)> {
    let api_keys_url = project_api_keys_url(&zai_base_url(), scope);
    let api_key_id = ensure_project_api_key(business_token, &api_keys_url).await?;
    let url = format!(
        "{api_keys_url}/{}/access_tokens",
        urlencoding::encode(&api_key_id)
    );
    let raw = post_json(
        &url,
        serde_json::json!({
            "clientType": "zcode",
            "clientVersion": ZCODE_CLIENT_VERSION,
        }),
        Some(business_token),
    )
    .await?;
    let value = unwrap_envelope(raw, "access_tokens")?;
    let token = value
        .get("accessToken")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            io::Error::other(format!(
                "access_tokens response is missing accessToken: {value}"
            ))
        })?;
    let now = Utc::now().timestamp();
    let expires_in = value
        .get("expiresIn")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(18000);
    let expires_at = value
        .get("expiresAt")
        .and_then(serde_json::Value::as_i64)
        .filter(|expires_at| *expires_at > now)
        .unwrap_or(now + expires_in);
    Ok((token.to_string(), expires_at))
}

/// Load stored ZCode tokens without applying refreshes.
pub fn load_zcode_oauth_tokens(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<Option<ZcodeTokenData>> {
    Ok(crate::auth::load_auth_dot_json(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    .and_then(|auth| auth.zcode_oauth))
}

/// Persist ZCode tokens while preserving all other stored auth fields.
pub(crate) fn save_zcode_oauth_tokens(
    codex_home: &Path,
    tokens: ZcodeTokenData,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<()> {
    let mut auth_dot_json = crate::auth::load_auth_dot_json(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    .unwrap_or_default();
    auth_dot_json.zcode_oauth = Some(tokens);
    crate::auth::save_auth(
        codex_home,
        &auth_dot_json,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )
}

/// Re-issues the Coding Plan project access token when it is missing or
/// about to expire, persisting the updated token data (best effort: on
/// failure the stored tokens are returned unchanged and logged).
async fn refresh_project_token(
    tokens: ZcodeTokenData,
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<ZcodeTokenData> {
    if !tokens.needs_project_token_refresh() {
        return Ok(tokens);
    }
    let scope = match project_scope_from_tokens(&tokens) {
        Some(scope) => Some(scope),
        None => resolve_project_scope(&tokens.zai_business_token)
            .await
            .unwrap_or(None),
    };
    let Some(scope) = scope else {
        tracing::warn!("cannot refresh the ZCode project token: no project resolved");
        return Ok(tokens);
    };
    match issue_project_token(&tokens.zai_business_token, &scope).await {
        Ok((project_token, expires_at)) => {
            let mut persisted = tokens;
            persisted.project_token = Some(project_token);
            persisted.project_token_expires_at = expires_at;
            persisted.organization_id = Some(scope.organization_id);
            persisted.project_id = Some(scope.project_id);
            persisted.last_refresh = Utc::now().timestamp();
            save_zcode_oauth_tokens(
                codex_home,
                persisted.clone(),
                auth_credentials_store_mode,
                keyring_backend_kind,
            )?;
            Ok(persisted)
        }
        Err(err) => {
            tracing::warn!("failed to refresh ZCode project token: {err}");
            Ok(tokens)
        }
    }
}

/// Builds the project scope from already-stored ids, when both are present.
fn project_scope_from_tokens(tokens: &ZcodeTokenData) -> Option<ProjectScope> {
    let organization_id = tokens.organization_id.as_deref()?.trim();
    let project_id = tokens.project_id.as_deref()?.trim();
    if organization_id.is_empty() || project_id.is_empty() {
        return None;
    }
    Some(ProjectScope {
        organization_id: organization_id.to_string(),
        project_id: project_id.to_string(),
    })
}

/// Re-issues the Coding Plan project access token when it is missing or
/// about to expire, persisting the updated token data.
///
/// Returns `Ok(None)` when no tokens are stored.
pub async fn refresh_zcode_project_token_if_needed(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<Option<ZcodeTokenData>> {
    let Some(tokens) = load_zcode_oauth_tokens(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    else {
        return Ok(None);
    };
    refresh_project_token(
        tokens,
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )
    .await
    .map(Some)
}

/// Resolves the Bearer token for ZCode-backed inference: the Coding Plan
/// project token when available (refreshed on demand), otherwise the Z.AI
/// business token.
///
/// Tokens are read via `Auto` storage so credentials saved under any store
/// mode (File, Keyring, Auto) resolve consistently. Returns `Ok(None)` when
/// no ZCode tokens are stored.
pub async fn resolve_zcode_bearer_token(codex_home: &Path) -> io::Result<Option<String>> {
    resolve_zcode_bearer_token_with_store(
        codex_home,
        AuthCredentialsStoreMode::Auto,
        crate::auth::AuthKeyringBackendKind::default(),
    )
    .await
}

/// Same as [`resolve_zcode_bearer_token`] but with an explicit credential
/// store, so tests can use `File`/`Direct` instead of the keyring.
pub(crate) async fn resolve_zcode_bearer_token_with_store(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<Option<String>> {
    let Some(tokens) = load_zcode_oauth_tokens(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    else {
        return Ok(None);
    };
    let tokens = refresh_project_token(
        tokens,
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )
    .await?;
    Ok(tokens
        .project_token
        .clone()
        .filter(|token| !token.trim().is_empty())
        .or(Some(tokens.zai_business_token)))
}

#[cfg(test)]
#[path = "zcode_oauth_tests.rs"]
mod tests;
