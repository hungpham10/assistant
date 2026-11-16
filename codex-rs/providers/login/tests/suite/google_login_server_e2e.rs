#![allow(clippy::unwrap_used)]
use std::io::Read;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::GoogleLoginOptions;
use codex_login::GoogleTokenData;
use codex_login::load_google_oauth_tokens;
use codex_login::run_google_login_server;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use url::Url;

/// Mock Google token endpoint accepting authorization-code exchanges.
fn start_mock_token_endpoint() -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tiny_http::Server::from_listener(listener, None).unwrap();

    let handle = thread::spawn(move || {
        while let Ok(mut req) = server.recv() {
            let url = req.url().to_string();
            if url.starts_with("/token") {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                assert!(body.contains("grant_type=authorization_code"));
                assert!(body.contains("code=abc"));
                assert!(body.contains("code_verifier="));
                let tokens = serde_json::json!({
                    "access_token": "ga-123",
                    "refresh_token": "gr-123",
                    "expires_in": 3600,
                });
                let data = serde_json::to_vec(&tokens).unwrap();
                let mut resp = tiny_http::Response::from_data(data);
                resp.add_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .expect("header bytes should be valid"),
                );
                let _ = req.respond(resp);
            } else {
                let _ = req
                    .respond(tiny_http::Response::from_string("not found").with_status_code(404));
            }
        }
    });

    (addr, handle)
}

#[tokio::test]
async fn end_to_end_google_login_flow_persists_google_oauth_tokens() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let (token_addr, token_handle) = start_mock_token_endpoint();
    let token_url = format!("http://{}:{}/token", token_addr.ip(), token_addr.port());
    // SAFETY(test): the override only affects this test process and is scoped
    // to this test's runtime; the default endpoint is restored after.
    unsafe {
        std::env::set_var("CODEX_GOOGLE_OAUTH_TOKEN_URL", &token_url);
    }

    let tmp = tempdir()?;
    let codex_home = tmp.path().to_path_buf();

    let opts = GoogleLoginOptions {
        codex_home: codex_home.clone(),
        client_id: codex_login::GOOGLE_OAUTH_CLIENT_ID.to_string(),
        client_secret: None,
        open_browser: false,
        auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        auth_keyring_backend_kind: AuthKeyringBackendKind::Direct,
    };

    let server = run_google_login_server(opts)?;
    // Extract the generated state from the auth URL so the callback passes
    // validation.
    let auth_url = Url::parse(&server.auth_url)?;
    let state = auth_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .expect("auth url contains state");
    let login_port = server.actual_port;

    let handle = tokio::spawn(async move { server.block_until_done().await });

    // Simulate the browser redirect (loopback redirect URI has no path).
    let callback_url = format!("http://127.0.0.1:{login_port}/?code=abc&state={state}");
    let response = reqwest::get(&callback_url).await?;
    assert_eq!(response.status(), 200);

    tokio::time::timeout(Duration::from_secs(10), handle).await??; // errors if login failed

    let stored = load_google_oauth_tokens(
        &codex_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )?
    .expect("google oauth tokens persisted");
    assert_eq!(
        stored,
        GoogleTokenData {
            access_token: "ga-123".to_string(),
            refresh_token: "gr-123".to_string(),
            expires_at: stored.expires_at,
            email: None,
            last_refresh: stored.last_refresh,
            project_id: None,
        }
    );
    assert!(stored.expires_at > stored.last_refresh);

    unsafe {
        std::env::remove_var("CODEX_GOOGLE_OAUTH_TOKEN_URL");
    }
    // Keep the mock server thread alive until assertions finish; dropping the
    // handle would detach it silently.
    drop(token_handle);
    Ok(())
}
