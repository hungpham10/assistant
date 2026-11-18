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
        organization_id: Some("org-1".to_string()),
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

/// Starts a mock Z.AI business API that serves the project-key flow used by
/// `issue_project_token`: `getCustomerInfo`, the project `api_keys`
/// collection, key creation, and the `access_tokens` exchange.
///
/// `existing_key` controls whether `GET api_keys` already returns the desktop
/// `zcode-api-key` (otherwise the create path is exercised).
fn start_project_api_mock(existing_key: bool, status: u16) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let server = tiny_http::Server::from_listener(listener, None).expect("mock server");
    thread::spawn(move || {
        while let Ok(req) = server.recv() {
            let path = req.url().to_string();
            let method = req.method().to_string();
            let body = if status != 200 {
                serde_json::json!({ "code": 500, "msg": "boom" })
            } else if path.ends_with("/access_tokens") {
                serde_json::json!({
                    "code": 200,
                    "msg": "Operation successful",
                    "data": {
                        "accessToken": "fresh-token",
                        "tokenType": "Bearer",
                        "expiresIn": 18000,
                        "expiresAt": Utc::now().timestamp() + 18000,
                    }
                })
            } else if path.contains("/api_keys") {
                if method == "POST" {
                    serde_json::json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": { "name": ZCODE_API_KEY_NAME, "apiKey": "created-key-id" }
                    })
                } else if existing_key {
                    serde_json::json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": [{ "name": ZCODE_API_KEY_NAME, "apiKey": "key-id" }]
                    })
                } else {
                    serde_json::json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": [{ "name": "other-key", "apiKey": "other-id" }]
                    })
                }
            } else if path.ends_with("/getCustomerInfo") {
                serde_json::json!({
                    "code": 200,
                    "msg": "Operation successful",
                    "data": {
                        "organizations": [{
                            "organizationId": "org-1",
                            "organizationName": "默认机构",
                            "projects": [{
                                "projectId": "project-1",
                                "projectName": "默认项目",
                                "projectType": 1,
                            }]
                        }]
                    }
                })
            } else {
                serde_json::json!({ "code": 404, "msg": "unexpected path" })
            };
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
    let addr = start_project_api_mock(/*existing_key*/ true, /*status*/ 500);
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
    let addr = start_project_api_mock(/*existing_key*/ true, /*status*/ 200);
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
    let addr = start_project_api_mock(/*existing_key*/ true, /*status*/ 500);
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

#[test]
fn unwrap_envelope_accepts_200_and_string_codes() {
    for code in [
        serde_json::json!(0),
        serde_json::json!(200),
        serde_json::json!("0"),
        serde_json::json!("200"),
    ] {
        let value = serde_json::json!({ "code": code, "msg": "Operation successful", "data": { "ok": true } });
        let unwrapped = unwrap_envelope(value, "test").expect("200-style codes are success");
        assert_eq!(
            unwrapped.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}

#[tokio::test]
#[serial]
async fn resolve_project_scope_prefers_default_org_and_project() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let server = tiny_http::Server::from_listener(listener, None).expect("mock server");
    thread::spawn(move || {
        while let Ok(req) = server.recv() {
            let body = serde_json::json!({
                "code": 200,
                "msg": "Operation successful",
                "data": {
                    "organizations": [
                        {
                            "organizationId": "org-other",
                            "organizationName": "Acme",
                            "projects": [
                                { "projectId": "proj-shared", "projectName": "Team", "projectType": "2" }
                            ]
                        },
                        {
                            "organizationId": "org-default",
                            "organizationName": "默认机构",
                            "projects": [
                                { "projectId": "proj-default", "projectName": "默认项目", "projectType": 1 }
                            ]
                        }
                    ]
                }
            });
            let data = serde_json::to_vec(&body).expect("mock body");
            let response = tiny_http::Response::from_data(data)
                .with_status_code(tiny_http::StatusCode(200))
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .expect("header bytes"),
                );
            let _ = req.respond(response);
        }
    });
    unsafe {
        std::env::set_var("CODEX_ZAI_BASE_URL", format!("http://{addr}"));
    }

    let scope = resolve_project_scope("business-token")
        .await
        .expect("resolve scope");

    unsafe {
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }

    assert_eq!(
        scope,
        Some(ProjectScope {
            organization_id: "org-default".to_string(),
            project_id: "proj-default".to_string(),
        })
    );
}

#[tokio::test]
#[serial]
async fn issue_project_token_creates_the_coding_plan_key_when_missing() {
    // No `zcode-api-key` yet, so the create path runs and its key id is used
    // for the access-token exchange.
    let addr = start_project_api_mock(/*existing_key*/ false, /*status*/ 200);
    unsafe {
        std::env::set_var("CODEX_ZAI_BASE_URL", format!("http://{addr}"));
    }

    let scope = ProjectScope {
        organization_id: "org-1".to_string(),
        project_id: "project-1".to_string(),
    };
    let (token, expires_at) = issue_project_token("business-token", &scope)
        .await
        .expect("issue project token");

    unsafe {
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }

    assert_eq!(token, "fresh-token");
    assert!(expires_at > Utc::now().timestamp());
}
