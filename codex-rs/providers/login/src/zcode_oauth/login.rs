//! Interactive ZCode login flow (native polling).
//!
//! Unlike the Google flow there is no local callback server: the ZCode
//! broker returns an authorization URL and a flow id, and the CLI polls the
//! broker until the user completes sign-in in the browser.

use std::io;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;

use crate::zcode_oauth::ZCODE_OAUTH_INIT_PATH;
use crate::zcode_oauth::ZCODE_OAUTH_POLL_PATH;
use crate::zcode_oauth::ZcodeTokenData;
use crate::zcode_oauth::exchange_business_token;
use crate::zcode_oauth::issue_project_token;
use crate::zcode_oauth::post_json;
use crate::zcode_oauth::resolve_project_id;
use crate::zcode_oauth::save_zcode_oauth_tokens;
use crate::zcode_oauth::unwrap_envelope;
use crate::zcode_oauth::zcode_base_url;

/// Options for the ZCode login flow.
#[derive(Debug, Clone)]
pub struct ZcodeLoginOptions {
    pub codex_home: std::path::PathBuf,
    pub open_browser: bool,
    pub auth_credentials_store_mode: codex_config::types::AuthCredentialsStoreMode,
    pub auth_keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
}

impl ZcodeLoginOptions {
    /// Creates options for the current environment.
    pub fn new(
        codex_home: std::path::PathBuf,
        auth_credentials_store_mode: codex_config::types::AuthCredentialsStoreMode,
        auth_keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
    ) -> Self {
        Self {
            codex_home,
            open_browser: true,
            auth_credentials_store_mode,
            auth_keyring_backend_kind,
        }
    }
}

/// Response from the broker's `cli/init` endpoint.
#[derive(Debug, Deserialize)]
struct ZcodeInitResponse {
    authorize_url: String,
    flow_id: String,
    #[serde(default)]
    poll_interval_sec: Option<u64>,
}

/// Response from the broker's `cli/poll/flow` endpoint.
#[derive(Debug, Deserialize)]
struct ZcodePollResponse {
    status: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    zai: Option<serde_json::Value>,
}

/// Maximum time to wait for the user to complete sign-in in the browser.
const LOGIN_TIMEOUT_SECS: u64 = 15 * 60;

/// Runs the interactive ZCode login flow and persists the tokens.
pub async fn login_with_zcode_oauth(opts: ZcodeLoginOptions) -> io::Result<()> {
    let base = zcode_base_url();

    // 1. Request an authorization URL from the broker.
    let init: ZcodeInitResponse = serde_json::from_value(
        post_json(
            &format!("{base}{ZCODE_OAUTH_INIT_PATH}"),
            serde_json::json!({}),
            /*bearer*/ None,
        )
        .await?,
    )
    .map_err(|err| io::Error::other(format!("zcode cli/init parse failed: {err}")))?;

    eprintln!(
        "Open this URL in your browser to sign in with Z.AI:\n\n{}",
        init.authorize_url
    );
    if opts.open_browser {
        let _ = webbrowser::open(&init.authorize_url);
    }

    // 2. Poll until the user completes sign-in.
    let poll_interval = Duration::from_secs(init.poll_interval_sec.unwrap_or(1).max(1));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(LOGIN_TIMEOUT_SECS);
    let poll_url = format!("{base}{ZCODE_OAUTH_POLL_PATH}");
    let poll: ZcodePollResponse = loop {
        tokio::time::sleep(poll_interval).await;
        let value = post_json(
            &poll_url,
            serde_json::json!({ "flow_id": init.flow_id }),
            /*bearer*/ None,
        )
        .await?;
        let value = unwrap_envelope(value, "cli/poll/flow")?;
        let poll: ZcodePollResponse = serde_json::from_value(value)
            .map_err(|err| io::Error::other(format!("zcode cli/poll/flow parse failed: {err}")))?;
        match poll.status.as_str() {
            "ready" => break poll,
            "pending" => {}
            other => {
                return Err(io::Error::other(format!(
                    "zcode login flow returned unexpected status: {other}"
                )));
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::other(
                "zcode login flow timed out waiting for browser sign-in",
            ));
        }
    };

    // 3. Extract the ZCode JWT and Z.AI OAuth access token.
    let zcode_jwt = poll
        .token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| io::Error::other("zcode login flow did not return a token"))?;
    let oauth_access_token = poll
        .zai
        .as_ref()
        .and_then(|zai| zai.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| io::Error::other("zcode login flow did not return a Z.AI access token"))?
        .to_string();

    // 4. Exchange the OAuth token for the Z.AI business token.
    let zai_business_token = exchange_business_token(&oauth_access_token).await?;

    // 5. Resolve the project and issue a Coding Plan project token.
    let project_id = resolve_project_id(&zai_business_token)
        .await
        .unwrap_or(None);
    let (project_token, project_token_expires_at) =
        match issue_project_token(&zai_business_token, project_id.as_deref()).await {
            Ok((token, expires_at)) => (Some(token), expires_at),
            Err(err) => {
                tracing::warn!("could not issue a project access token: {err}");
                (None, 0)
            }
        };

    let tokens = ZcodeTokenData {
        zcode_jwt,
        zai_business_token,
        project_token,
        project_token_expires_at,
        project_id,
        email: None,
        last_refresh: Utc::now().timestamp(),
    };
    save_zcode_oauth_tokens(
        &opts.codex_home,
        tokens,
        opts.auth_credentials_store_mode,
        opts.auth_keyring_backend_kind,
    )
}
