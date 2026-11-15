//! Google OAuth login and token management for the Gemini provider.
//!
//! Stores Google OAuth tokens alongside other credentials in `auth.json`
//! (`google_oauth` field) so the `google` model provider can fall back to
//! OAuth Bearer authentication when no `GOOGLE_API_KEY` is configured.

mod login_server;

use std::io;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use codex_config::types::AuthCredentialsStoreMode;
use serde::Deserialize;
use serde::Serialize;

pub use login_server::GoogleLoginOptions;
pub use login_server::GoogleLoginServer;
pub use login_server::login_with_google_oauth;
pub use login_server::run_google_login_server;

/// Public OAuth client ID reused from the Gemini CLI (installed-app client).
///
/// Google treats installed-app clients as public: the token endpoint accepts
/// requests without a `client_secret`. A secret can optionally be supplied
/// via `CODEX_GOOGLE_OAUTH_CLIENT_SECRET` for private clients.
pub const GOOGLE_OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
pub const GOOGLE_OAUTH_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
];

pub const GOOGLE_OAUTH_CLIENT_ID_OVERRIDE_ENV_VAR: &str = "CODEX_GOOGLE_OAUTH_CLIENT_ID";
pub const GOOGLE_OAUTH_CLIENT_SECRET_OVERRIDE_ENV_VAR: &str = "CODEX_GOOGLE_OAUTH_CLIENT_SECRET";
pub const GOOGLE_OAUTH_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "CODEX_GOOGLE_OAUTH_TOKEN_URL";

/// Refresh the access token when it expires within this window (5 minutes).
const GOOGLE_TOKEN_REFRESH_WINDOW_SECS: i64 = 300;

/// Google OAuth tokens persisted in `auth.json` under the `google_oauth` key.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct GoogleTokenData {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds at which the access token expires.
    pub expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Unix seconds at which the tokens were last refreshed.
    pub last_refresh: i64,
}

impl GoogleTokenData {
    /// Returns true when the access token expires within the refresh window.
    pub fn needs_refresh(&self) -> bool {
        self.expires_at - Utc::now().timestamp() <= GOOGLE_TOKEN_REFRESH_WINDOW_SECS
    }

    pub fn from_token_response(
        access_token: String,
        refresh_token: Option<String>,
        expires_in: i64,
        email: Option<String>,
    ) -> Self {
        let now = Utc::now().timestamp();
        Self {
            access_token,
            refresh_token: refresh_token.unwrap_or_default(),
            expires_at: now + expires_in,
            email,
            last_refresh: now,
        }
    }
}

/// Exchange payload returned by Google's token endpoint.
#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

fn client_id() -> String {
    std::env::var(GOOGLE_OAUTH_CLIENT_ID_OVERRIDE_ENV_VAR)
        .ok()
        .filter(|client_id| !client_id.trim().is_empty())
        .unwrap_or_else(|| GOOGLE_OAUTH_CLIENT_ID.to_string())
}

/// Optional client secret; installed-app public clients omit it entirely.
pub(crate) fn client_secret() -> Option<String> {
    std::env::var(GOOGLE_OAUTH_CLIENT_SECRET_OVERRIDE_ENV_VAR)
        .ok()
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
}

/// Appends the optional `client_secret` form parameter when configured.
fn append_client_secret(body: &mut String, client_secret: Option<&str>) {
    if let Some(secret) = client_secret {
        body.push_str("&client_secret=");
        body.push_str(&urlencoding::encode(secret));
    }
}

fn token_url() -> String {
    std::env::var(GOOGLE_OAUTH_TOKEN_URL_OVERRIDE_ENV_VAR)
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| GOOGLE_OAUTH_TOKEN_URL.to_string())
}

pub(crate) fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let scopes = GOOGLE_OAUTH_SCOPES.join(" ");
    let query = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", &scopes),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    let qs = query
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{GOOGLE_OAUTH_AUTH_URL}?{qs}")
}

/// Exchange an authorization code for Google OAuth tokens.
pub(crate) async fn exchange_google_code(
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> io::Result<GoogleTokenData> {
    let mut body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(code_verifier),
    );
    append_client_secret(&mut body, client_secret);
    let resp = reqwest::Client::new()
        .post(token_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(io::Error::other)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(io::Error::other(format!(
            "Google token endpoint returned status {status}: {body}"
        )));
    }
    let tokens: GoogleTokenResponse = resp.json().await.map_err(io::Error::other)?;
    Ok(GoogleTokenData::from_token_response(
        tokens.access_token,
        tokens.refresh_token,
        tokens.expires_in.unwrap_or(3600),
        /*email*/ None,
    ))
}

/// Refresh Google OAuth tokens using a refresh token grant.
pub async fn refresh_google_token(refresh_token: &str) -> io::Result<GoogleTokenData> {
    let mut body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencoding::encode(refresh_token),
        urlencoding::encode(&client_id()),
    );
    append_client_secret(&mut body, client_secret().as_deref());
    let resp = reqwest::Client::new()
        .post(token_url())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(io::Error::other)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(io::Error::other(format!(
            "Google token refresh returned status {status}: {body}"
        )));
    }
    let tokens: GoogleTokenResponse = resp.json().await.map_err(io::Error::other)?;
    Ok(GoogleTokenData::from_token_response(
        tokens.access_token,
        tokens.refresh_token,
        tokens.expires_in.unwrap_or(3600),
        /*email*/ None,
    ))
}

/// Load stored Google OAuth tokens without applying refreshes.
pub fn load_google_oauth_tokens(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<Option<GoogleTokenData>> {
    Ok(crate::auth::load_auth_dot_json(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    .and_then(|auth| auth.google_oauth))
}

/// Persist Google OAuth tokens while preserving all other stored auth fields.
pub(crate) fn save_google_oauth_tokens(
    codex_home: &Path,
    tokens: GoogleTokenData,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<()> {
    let mut auth_dot_json = crate::auth::load_auth_dot_json(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    .unwrap_or_default();
    auth_dot_json.google_oauth = Some(tokens);
    crate::auth::save_auth(
        codex_home,
        &auth_dot_json,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )
}

/// Loads Google OAuth tokens for the given home, refreshing and persisting
/// them when they are expired or about to expire.
///
/// Returns `Ok(None)` when no Google OAuth tokens are stored. Refresh
/// failures are logged and surface the stale access token so callers can
/// still attempt a request that the backend may accept.
pub async fn refresh_google_oauth_tokens_if_needed(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
) -> io::Result<Option<GoogleTokenData>> {
    let Some(tokens) = load_google_oauth_tokens(
        codex_home,
        auth_credentials_store_mode,
        keyring_backend_kind,
    )?
    else {
        return Ok(None);
    };
    if !tokens.needs_refresh() {
        return Ok(Some(tokens));
    }
    if tokens.refresh_token.is_empty() {
        tracing::warn!("google oauth access token expired but no refresh token is stored");
        return Ok(Some(tokens));
    }
    match refresh_google_token(&tokens.refresh_token).await {
        Ok(new_tokens) => {
            let mut persisted = new_tokens.clone();
            if persisted.refresh_token.is_empty() {
                persisted.refresh_token = tokens.refresh_token.clone();
            }
            if persisted.email.is_none() {
                persisted.email = tokens.email.clone();
            }
            save_google_oauth_tokens(
                codex_home,
                persisted,
                auth_credentials_store_mode,
                keyring_backend_kind,
            )?;
            Ok(Some(new_tokens))
        }
        Err(err) => {
            tracing::warn!("failed to refresh google oauth tokens: {err}");
            Ok(Some(tokens))
        }
    }
}

#[cfg(test)]
#[path = "google_oauth_tests.rs"]
mod tests;
