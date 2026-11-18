//! ZCode (Z.AI) OAuth login and token management.
//!
//! Mirrors the ZCode CLI's native polling flow (contract confirmed from the
//! ZCode desktop app runtime): request an authorization URL from the ZCode
//! broker with a client-generated bearer poll token, open it in the browser,
//! then poll until the user signs in, exchange the OAuth token for the Z.AI
//! business access token, and resolve the Coding Plan project access token
//! used for inference.
//!
//! Contracts:
//! - `POST {base}/api/v1/oauth/cli/init`
//!   headers: `Authorization: Bearer <pollToken>`, `Content-Type: application/json`
//!   body: `{"provider":"zai"}` → `{authorize_url, flow_id, expires_at, poll_interval_sec}`
//! - `GET {base}/api/v1/oauth/cli/poll/{flow_id}`
//!   headers: `Authorization: Bearer <pollToken>` → `{status, token, zai:{access_token}}`
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

/// Unwraps the `{code, msg, data}` envelope used by the Z.AI business API.
pub(crate) fn unwrap_envelope(
    value: serde_json::Value,
    context: &str,
) -> io::Result<serde_json::Value> {
    let code = value.get("code").and_then(serde_json::Value::as_i64);
    if let Some(code) = code.filter(|code| *code != 0) {
        let msg = value
            .get("msg")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
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
    let value = post_json(
        &url,
        serde_json::json!({ "token": oauth_access_token }),
        /*bearer*/ None,
    )
    .await?;
    let access_token = value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| io::Error::other("Z.AI login did not return a business access token."))?;
    Ok(access_token.to_string())
}

/// Resolves the Z.AI project id for the business account (best effort).
pub(crate) async fn resolve_project_id(business_token: &str) -> io::Result<Option<String>> {
    let url = format!("{}{ZAI_CUSTOMER_INFO_PATH}", zai_base_url());
    let value = get_json(&url, business_token).await?;
    let value = unwrap_envelope(value, "getCustomerInfo")?;
    // organizations[0].projects[0].projectId
    let project_id = value
        .get("organizations")
        .and_then(serde_json::Value::as_array)
        .and_then(|orgs| orgs.first())
        .and_then(|org| org.get("projects"))
        .and_then(serde_json::Value::as_array)
        .and_then(|projects| projects.first())
        .and_then(|project| project.get("projectId"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(project_id)
}

/// Issues a Coding Plan project access token for the business account.
///
/// Returns `(token, expires_at_unix_secs)`.
pub(crate) async fn issue_project_token(
    business_token: &str,
    project_id: Option<&str>,
) -> io::Result<(String, i64)> {
    let base = zai_base_url();
    let url = match project_id.filter(|id| !id.trim().is_empty()) {
        Some(project_id) => format!("{base}/api/biz/customer/project/{project_id}/access_tokens"),
        None => format!("{base}/api/biz/customer/access_tokens"),
    };
    let value = get_json(&url, business_token).await?;
    let value = unwrap_envelope(value, "access_tokens")?;
    let token = value
        .get("accessToken")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| io::Error::other("access_tokens response is missing accessToken"))?;
    let now = Utc::now().timestamp();
    let expires_in = value
        .get("expiresIn")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(3600);
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
    match issue_project_token(&tokens.zai_business_token, tokens.project_id.as_deref()).await {
        Ok((project_token, expires_at)) => {
            let mut persisted = tokens;
            persisted.project_token = Some(project_token);
            persisted.project_token_expires_at = expires_at;
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
