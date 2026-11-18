//! Unit tests for ZCode token data helpers and auth resolution.

use std::net::SocketAddr;
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use codex_config::types::AuthCredentialsStoreMode;
use serial_test::serial;

use super::*;
use crate::auth::AuthKeyringBackendKind;

fn tokens_with_project_token(expires_in: i64) -> ZcodeTokenData {
    ZcodeTokenData {
        zcode_jwt: "zcode-jwt".to_string(),
        zai_business_token: "business-token".to_string(),
        project_token: Some("project-token".to_string()),
        project_token_expires_at: Utc::now().timestamp() + expires_in,
        project_id: Some("project-1".to_string()),
        email: None,
        last_refresh: Utc::now().timestamp(),
    }
}

fn save_tokens(codex_home: &Path, tokens: ZcodeTokenData) {
    save_zcode_oauth_tokens(
        codex_home,
        tokens,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .expect("save zcode tokens");
}

fn load_tokens(codex_home: &Path) -> ZcodeTokenData {
    load_zcode_oauth_tokens(
        codex_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .expect("load zcode tokens")
    .expect("zcode tokens present")
}

/// Starts a tolerant mock server that answers every request with `status` and
/// `body`, used to drive `issue_project_token` in refresh tests.
fn start_mock_server(status: u16, body: serde_json::Value) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let server = tiny_http::Server::from_listener(listener, None).expect("mock server");
    thread::spawn(move || {
        while let Ok(req) = server.recv() {
            let data = serde_json::to_vec(&body).expect("mock body");
            let response = tiny_http::Response::from_data(data)
                .with_status_code(tiny_http::StatusCode(status))
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .expect("header bytes"),
                );
            let _ = req.respond(response);
        }
    });
    addr
}

#[test]
fn project_token_refresh_not_needed_when_fresh() {
    let tokens = tokens_with_project_token(3600);
    assert!(!tokens.needs_project_token_refresh());
}

#[test]
fn project_token_refresh_needed_when_expired() {
    let tokens = tokens_with_project_token(-10);
    assert!(tokens.needs_project_token_refresh());
}

#[test]
fn project_token_refresh_needed_when_missing() {
    let mut tokens = tokens_with_project_token(3600);
    tokens.project_token = None;
    assert!(tokens.needs_project_token_refresh());
}

#[test]
fn unwrap_envelope_passes_data_through() {
    let value = serde_json::json!({ "code": 0, "msg": "ok", "data": { "a": 1 } });
    let unwrapped = unwrap_envelope(value, "test").expect("unwrap");
    assert_eq!(
        unwrapped.get("a").and_then(serde_json::Value::as_i64),
        Some(1)
    );
}

#[test]
fn unwrap_envelope_errors_on_nonzero_code() {
    let value = serde_json::json!({ "code": 401, "msg": "unauthorized" });
    let err = unwrap_envelope(value, "test").expect_err("should fail");
    assert!(err.to_string().contains("unauthorized"));
}

#[tokio::test]
async fn resolve_bearer_prefers_fresh_project_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    save_tokens(dir.path(), tokens_with_project_token(3600));

    let token = resolve_zcode_bearer_token_with_store(
        dir.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .await
    .expect("resolve bearer");

    assert_eq!(token.as_deref(), Some("project-token"));
}

#[tokio::test]
async fn resolve_bearer_returns_none_without_tokens() {
    let dir = tempfile::tempdir().expect("tempdir");

    let token = resolve_zcode_bearer_token_with_store(
        dir.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .await
    .expect("resolve bearer");

    assert_eq!(token, None);
}

#[tokio::test]
#[serial]
async fn resolve_bearer_falls_back_to_business_token_when_project_issue_fails() {
    // Mock a failing project-token issuance so the refresh keeps the stored
    // tokens and resolution falls back to the business token.
    let addr = start_mock_server(500, serde_json::json!({ "code": 500, "msg": "boom" }));
    unsafe {
        std::env::set_var("CODEX_ZAI_BASE_URL", format!("http://{addr}"));
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let mut tokens = tokens_with_project_token(-10);
    tokens.project_token = None;
    save_tokens(dir.path(), tokens);

    let token = resolve_zcode_bearer_token_with_store(
        dir.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .await
    .expect("resolve bearer");

    unsafe {
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }
    assert_eq!(token.as_deref(), Some("business-token"));
}

#[tokio::test]
#[serial]
async fn refresh_project_token_persists_new_token() {
    let addr = start_mock_server(
        200,
        serde_json::json!({
            "code": 0,
            "msg": "ok",
            "data": { "accessToken": "fresh-token", "expiresIn": 3600 }
        }),
    );
    unsafe {
        std::env::set_var("CODEX_ZAI_BASE_URL", format!("http://{addr}"));
    }

    let dir = tempfile::tempdir().expect("tempdir");
    save_tokens(dir.path(), tokens_with_project_token(-10));

    let token = resolve_zcode_bearer_token_with_store(
        dir.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .await
    .expect("resolve bearer");

    unsafe {
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }

    assert_eq!(token.as_deref(), Some("fresh-token"));
    let persisted = load_tokens(dir.path());
    assert_eq!(persisted.project_token.as_deref(), Some("fresh-token"));
    assert!(persisted.project_token_expires_at > Utc::now().timestamp());
    assert!(!persisted.needs_project_token_refresh());
}

#[tokio::test]
#[serial]
async fn refresh_project_token_keeps_old_token_on_error() {
    let addr = start_mock_server(500, serde_json::json!({ "code": 500, "msg": "boom" }));
    unsafe {
        std::env::set_var("CODEX_ZAI_BASE_URL", format!("http://{addr}"));
    }

    let dir = tempfile::tempdir().expect("tempdir");
    save_tokens(dir.path(), tokens_with_project_token(-10));

    let token = resolve_zcode_bearer_token_with_store(
        dir.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )
    .await
    .expect("resolve bearer");

    unsafe {
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }

    assert_eq!(token.as_deref(), Some("project-token"));
    let persisted = load_tokens(dir.path());
    assert_eq!(persisted.project_token.as_deref(), Some("project-token"));
}
