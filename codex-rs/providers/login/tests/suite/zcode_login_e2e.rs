#![allow(clippy::unwrap_used)]
use std::net::SocketAddr;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use anyhow::Result;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::ZcodeLoginOptions;
use codex_login::load_zcode_oauth_tokens;
use serde_json::json;

/// Serializes tests that override the ZCode/Z.AI base-URL environment
/// variables (they are process-global).
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Mock ZCode broker + Z.AI API server covering the full login chain:
/// - POST /api/v1/oauth/cli/init      (Authorization: Bearer <pollToken>,
///   body {"provider":"zai"})           → authorize_url + flow_id + expires_at
/// - GET  /api/v1/oauth/cli/poll/{id} (Authorization: Bearer <pollToken>) → ready
/// - POST /api/auth/z/login           → business access token
/// - GET  /api/biz/customer/getCustomerInfo → project id envelope
/// - GET  /api/biz/customer/project/<id>/access_tokens → project token
fn start_mock_zcode_server(
    poll_token: Arc<Mutex<Option<String>>>,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tiny_http::Server::from_listener(listener, None).unwrap();

    let handle = thread::spawn(move || {
        let mut poll_count = 0u32;
        while let Ok(mut req) = server.recv() {
            let url = req.url().to_string();
            let method = req.method().to_string();
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);

            // Capture the bearer poll token from the init request so the poll
            // request can be asserted to reuse the exact same token.
            if url == "/api/v1/oauth/cli/init" && method == "POST" {
                if let Some(header) = req
                    .headers()
                    .iter()
                    .find(|h| h.field.to_string().eq_ignore_ascii_case("authorization"))
                {
                    let _ = poll_token.lock().unwrap().insert(header.value.to_string());
                }
            }

            let response_json = match (url.as_str(), method.as_str()) {
                ("/api/v1/oauth/cli/init", "POST") => {
                    assert!(body.contains("\"provider\":\"zai\""), "init body: {body}");
                    json!({
                        "code": 0,
                        "msg": "",
                        "data": {
                            "authorize_url": "https://zcode.z.ai/oauth/authorize?flow=flow-123",
                            "flow_id": "flow-123",
                            "poll_token": "server-poll-token",
                            "expires_at": chrono::Utc::now().timestamp() + 3600,
                            "poll_interval_sec": 1,
                            "server_time": chrono::Utc::now().timestamp(),
                        },
                    })
                }
                ("/api/v1/oauth/cli/poll/flow-123", "GET") => {
                    // The broker issues its own poll token during init and
                    // rejects polls that reuse the caller-supplied token.
                    let actual = req
                        .headers()
                        .iter()
                        .find(|h| h.field.to_string().eq_ignore_ascii_case("authorization"))
                        .map(|h| h.value.to_string());
                    assert_eq!(
                        actual.as_deref(),
                        Some("Bearer server-poll-token"),
                        "poll request must use the broker-issued poll token"
                    );
                    poll_count += 1;
                    if poll_count >= 1 {
                        json!({
                            "code": 0,
                            "msg": "",
                            "data": {
                                "status": "ready",
                                "token": "zcode-jwt-123",
                                "user": {"user_id": "fixture-user"},
                                "zai": {"access_token": "zai-oauth-123"},
                            },
                        })
                    } else {
                        json!({"code": 0, "msg": "", "data": {"status": "pending"}})
                    }
                }
                ("/api/auth/z/login", "POST") => {
                    assert!(body.contains("\"token\":\"zai-oauth-123\""));
                    json!({"code": 200, "msg": "Operation successful", "data": {"access_token": "zai-business-123"}})
                }
                ("/api/biz/customer/getCustomerInfo", "GET") => {
                    json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": {
                            "organizations": [{
                                "organizationId": "org-1",
                                "organizationName": "默认机构",
                                "projects": [{
                                    "projectId": "proj-1",
                                    "projectName": "默认项目",
                                    "projectType": 1,
                                }],
                            }]
                        }
                    })
                }
                ("/api/biz/v1/organization/org-1/projects/proj-1/api_keys", "GET") => {
                    json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": [{"name": "zcode-api-key", "apiKey": "key-id-1"}]
                    })
                }
                (
                    "/api/biz/v1/organization/org-1/projects/proj-1/api_keys/key-id-1/access_tokens",
                    "POST",
                ) => {
                    assert!(body.contains("\"clientType\":\"zcode\""));
                    json!({
                        "code": 200,
                        "msg": "Operation successful",
                        "data": {
                            "accessToken": "proj-token-123",
                            "tokenType": "Bearer",
                            "expiresIn": 18000,
                        }
                    })
                }
                _ => json!({"code": 404, "msg": "not found"}),
            };
            let data = serde_json::to_vec(&response_json).unwrap();
            let mut resp = tiny_http::Response::from_data(data);
            resp.add_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .expect("header bytes should be valid"),
            );
            let _ = req.respond(resp);
        }
    });

    (addr, handle)
}

#[tokio::test]
async fn end_to_end_zcode_login_flow_persists_zcode_oauth_tokens() -> Result<()> {
    let poll_token: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (addr, _handle) = start_mock_zcode_server(Arc::clone(&poll_token));
    let base = format!("http://{addr}");
    let _env_guard = ENV_LOCK.lock().unwrap();
    // SAFETY(test): the overrides only affect this test process; they are
    // restored after the login flow completes.
    unsafe {
        std::env::set_var("CODEX_ZCODE_BASE_URL", &base);
        std::env::set_var("CODEX_ZAI_BASE_URL", &base);
    }

    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path().to_path_buf();

    let opts = ZcodeLoginOptions {
        codex_home: codex_home.clone(),
        open_browser: false,
        auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        auth_keyring_backend_kind: AuthKeyringBackendKind::Direct,
    };

    codex_login::login_with_zcode_oauth(opts).await?;

    // The init request must carry a bearer poll token (64 hex chars).
    let captured = poll_token.lock().unwrap().clone();
    let Some(header_value) = captured.as_deref() else {
        panic!("zcode cli/init did not carry an Authorization bearer poll token");
    };
    assert!(
        header_value.starts_with("Bearer "),
        "init Authorization header: {header_value}"
    );
    let token = &header_value["Bearer ".len()..];
    assert_eq!(
        token.len(),
        64,
        "poll token should be 32 bytes hex: {token}"
    );

    let tokens = load_zcode_oauth_tokens(
        &codex_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )?
    .expect("zcode tokens persisted");

    assert_eq!(tokens.zcode_jwt, "zcode-jwt-123");
    assert_eq!(tokens.zai_business_token, "zai-business-123");
    assert_eq!(tokens.organization_id.as_deref(), Some("org-1"));
    assert_eq!(tokens.project_id.as_deref(), Some("proj-1"));
    assert_eq!(tokens.project_token.as_deref(), Some("proj-token-123"));
    assert!(tokens.project_token_expires_at > chrono::Utc::now().timestamp());
    assert!(!tokens.needs_project_token_refresh());

    unsafe {
        std::env::remove_var("CODEX_ZCODE_BASE_URL");
        std::env::remove_var("CODEX_ZAI_BASE_URL");
    }
    Ok(())
}
