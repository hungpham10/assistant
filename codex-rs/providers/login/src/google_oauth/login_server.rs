//! Local OAuth callback server for Google (Gemini provider) sign-in.
//!
//! Mirrors the structure of the OpenAI login server (`crate::server`) but
//! speaks Google's OAuth endpoints: `accounts.google.com` for authorization
//! and `oauth2.googleapis.com/token` for code exchange and refresh.

use std::io;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::thread;

use base64::Engine;
use codex_config::types::AuthCredentialsStoreMode;
use codex_utils_template::Template;
use rand::RngCore;
use tiny_http::Header;
use tiny_http::Response;
use tiny_http::Server;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::google_oauth::GoogleTokenData;
use crate::google_oauth::build_authorize_url;
use crate::google_oauth::client_id;
use crate::google_oauth::client_secret;
use crate::google_oauth::exchange_google_code;
use crate::google_oauth::save_google_oauth_tokens;
use crate::pkce::generate_pkce;

static GOOGLE_LOGIN_ERROR_PAGE_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    Template::parse(include_str!("../assets/error.html"))
        .unwrap_or_else(|err| panic!("login error page template must parse: {err}"))
});

const GOOGLE_LOGIN_SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Sign-in complete</title></head>
<body style="font-family: sans-serif; text-align: center; margin-top: 4rem;">
<h1>You are signed in with Google</h1>
<p>Codex can now use Gemini without an API key. You can close this window.</p>
</body>
</html>
"#;

/// Options for launching the Google login callback server.
#[derive(Debug, Clone)]
pub struct GoogleLoginOptions {
    pub codex_home: PathBuf,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub open_browser: bool,
    pub auth_credentials_store_mode: AuthCredentialsStoreMode,
    pub auth_keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
}

impl GoogleLoginOptions {
    /// Creates options using the Gemini CLI client credentials and current env.
    pub fn new(
        codex_home: PathBuf,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
        auth_keyring_backend_kind: crate::auth::AuthKeyringBackendKind,
    ) -> Self {
        Self {
            codex_home,
            client_id: client_id(),
            client_secret: client_secret(),
            open_browser: true,
            auth_credentials_store_mode,
            auth_keyring_backend_kind,
        }
    }
}

/// Handle for a running Google login callback server.
pub struct GoogleLoginServer {
    pub auth_url: String,
    pub actual_port: u16,
    server_handle: tokio::task::JoinHandle<io::Result<()>>,
    shutdown_handle: ShutdownHandle,
}

impl GoogleLoginServer {
    /// Waits for the login callback loop to finish.
    pub async fn block_until_done(self) -> io::Result<()> {
        self.server_handle
            .await
            .map_err(|err| io::Error::other(format!("login server thread panicked: {err:?}")))?
    }

    /// Requests shutdown of the callback server.
    pub fn cancel(&self) {
        self.shutdown_handle.shutdown();
    }
}

/// Handle used to signal the login server loop to exit.
#[derive(Clone, Debug)]
pub struct ShutdownHandle {
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl ShutdownHandle {
    /// Signals the login loop to terminate.
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }
}

/// Starts the local callback server and returns the Google auth URL.
pub fn run_google_login_server(opts: GoogleLoginOptions) -> io::Result<GoogleLoginServer> {
    let pkce = generate_pkce();
    let state = generate_state();

    let server = Server::http("127.0.0.1:0").map_err(io::Error::other)?;
    let actual_port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrInUse, "Unable to determine server port")
        })?;
    let server = Arc::new(server);

    // Google allows arbitrary ports for loopback-IP redirect URIs on
    // installed-app clients.
    let redirect_uri = format!("http://127.0.0.1:{actual_port}");
    let auth_url =
        build_authorize_url(&opts.client_id, &redirect_uri, &pkce.code_challenge, &state);

    if opts.open_browser {
        let _ = webbrowser::open(&auth_url);
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<tiny_http::Request>(16);
    let recv_server = Arc::clone(&server);
    thread::spawn(move || {
        while let Ok(request) = recv_server.recv() {
            if tx.blocking_send(request).is_err() {
                break;
            }
        }
    });

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let server_handle = {
        let opts = opts.clone();
        let redirect_uri = redirect_uri.clone();
        let shutdown_notify = shutdown_notify.clone();
        tokio::spawn(async move {
            let result = loop {
                tokio::select! {
                    _ = shutdown_notify.notified() => {
                        break Err(io::Error::other("Login was not completed"));
                    }
                    maybe_req = rx.recv() => {
                        let Some(req) = maybe_req else {
                            break Err(io::Error::other("Login was not completed"));
                        };

                        let url_raw = req.url().to_string();
                        let outcome = process_request(
                            &url_raw,
                            &opts,
                            &redirect_uri,
                            &pkce.code_verifier,
                            &state,
                        )
                        .await;

                        let exit_result = match outcome {
                            Ok(response) => {
                                let _ = tokio::task::spawn_blocking(move || {
                                    req.respond(response)
                                })
                                .await;
                                // A successful callback is terminal: the
                                // tokens were persisted, so stop the loop.
                                Some(Ok(()))
                            }
                            Err(err) => {
                                let body = render_login_error_page(&err.to_string());
                                let _ = tokio::task::spawn_blocking(move || {
                                    let _ = req.respond(
                                        Response::from_string(body)
                                            .with_status_code(tiny_http::StatusCode(400)),
                                    );
                                })
                                .await;
                                Some(Err(err))
                            }
                        };

                        if let Some(result) = exit_result {
                            break result;
                        }
                    }
                }
            };

            server.unblock();
            result
        })
    };

    Ok(GoogleLoginServer {
        auth_url,
        actual_port,
        server_handle,
        shutdown_handle: ShutdownHandle { shutdown_notify },
    })
}

/// Runs the full interactive Google login flow and persists the tokens.
pub async fn login_with_google_oauth(opts: GoogleLoginOptions) -> io::Result<()> {
    let server = run_google_login_server(opts)?;
    eprintln!(
        "Starting local login server on http://127.0.0.1:{}.\nIf your browser did not open, navigate to this URL to authenticate:\n\n{}",
        server.actual_port, server.auth_url
    );
    server.block_until_done().await
}

async fn process_request(
    url_raw: &str,
    opts: &GoogleLoginOptions,
    redirect_uri: &str,
    code_verifier: &str,
    state: &str,
) -> io::Result<Response<Cursor<Vec<u8>>>> {
    let parsed_url = url::Url::parse(&format!("http://localhost{url_raw}")).map_err(|err| {
        warn!(error = %err, "google login callback url parse failure");
        io::Error::other(format!("URL parse error: {err}"))
    })?;

    let params: std::collections::HashMap<String, String> =
        parsed_url.query_pairs().into_owned().collect();
    let state_valid = params.get("state").map(String::as_str) == Some(state);
    info!(
        path = %parsed_url.path(),
        has_code = params.contains_key("code"),
        state_valid,
        "received google login callback"
    );
    if !state_valid {
        warn!("google login callback state mismatch");
        return Err(io::Error::other("State mismatch"));
    }
    if let Some(error_code) = params.get("error") {
        let description = params
            .get("error_description")
            .map(String::as_str)
            .unwrap_or(error_code);
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Sign-in failed: {description}"),
        ));
    }
    let code = params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or_else(|| io::Error::other("Missing authorization code"))?;

    let tokens: GoogleTokenData = exchange_google_code(
        &opts.client_id,
        opts.client_secret.as_deref(),
        redirect_uri,
        code_verifier,
        &code,
    )
    .await
    .inspect_err(|err| {
        error!(error = %err, "google oauth token exchange failed");
    })?;
    save_google_oauth_tokens(
        &opts.codex_home,
        tokens,
        opts.auth_credentials_store_mode,
        opts.auth_keyring_backend_kind,
    )
    .map_err(|err| {
        error!(error = %err, "failed to persist google oauth tokens");
        io::Error::other(format!("Persist error: {err}"))
    })?;

    Ok(
        Response::from_string(GOOGLE_LOGIN_SUCCESS_HTML).with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                .expect("content type header is valid"),
        ),
    )
}

fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn render_login_error_page(message: &str) -> String {
    let escaped = html_escape(message);
    GOOGLE_LOGIN_ERROR_PAGE_TEMPLATE
        .render([
            ("error_title", "Sign-in could not be completed".to_string()),
            ("error_message", escaped.clone()),
            ("error_code", "google_sign_in_failed".to_string()),
            ("error_description", escaped),
            (
                "error_help",
                "Return to Codex to retry, or check your Google account permissions.".to_string(),
            ),
        ])
        .unwrap_or_else(|err| panic!("login error page template must render: {err}"))
}

/// Escapes error strings before inserting them into HTML.
fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
