#![allow(clippy::unwrap_used)]
use std::io::Read;
use std::net::SocketAddr;
use std::net::TcpListener;
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
/// - POST /api/v1/oauth/cli/init            → authorize_url + flow_id
/// - POST /api/v1/oauth/cli/poll/flow       → pending, then ready
/// - POST /api/auth/z/login                 → business access token
/// - GET  /api/biz/customer/getCustomerInfo → project id envelope
/// - GET  /api/biz/customer/project/<id>/access_tokens → project token
fn start_mock_zcode_server() -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tiny_http::Server::from_listener(listener, None).unwrap();

    let handle = thread::spawn(move || {
        let mut poll_count = 0u32;
        while let Ok(mut req) = server.recv() {
            let url = req.url().to_string();
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let response_json = match url.as_str() {
                "/api/v1/oauth/cli/init" => {
                    json!({
                        "authorize_url": format!(
                            "https://zcode.z.ai/oauth/authorize?flow=flow-123"
                        ),
                        "flow_id": "flow-123",
                        "poll_interval_sec": 1,
                    })
                }
                "/api/v1/oauth/cli/poll/flow" => {
                    poll_count += 1;
                    if body.contains("flow-123") && poll_count >= 1 {
                        json!({
                            "status": "ready",
                            "token": "zcode-jwt-123",
                            "user": {"email": "user@example.com"},
                            "zai": {"access_token": "zai-oauth-123"},
                        })
                    } else {
                        json!({"status": "pending"})
                    }
                }
                "/api/auth/z/login" => {
                    assert!(body.contains("\"token\":\"zai-oauth-123\""));
                    json!({"access_token": "zai-business-123"})
                }
                "/api/biz/customer/getCustomerInfo" => {
                    json!({
                        "code": 0,
                        "msg": "ok",
                        "data": {
                            "organizations": [
                                {"projects": [{"projectId": "proj-1"}]}
                            ]
                        }
                    })
                }
                "/api/biz/customer/project/proj-1/access_tokens" => {
                    json!({
                        "code": 0,
                        "msg": "ok",
                        "data": {"accessToken": "proj-token-123", "expiresIn": 3600}
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
    let (addr, handle) = start_mock_zcode_server();
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

    let tokens = load_zcode_oauth_tokens(
        &codex_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::Direct,
    )?
    .expect("zcode tokens persisted");

    assert_eq!(tokens.zcode_jwt, "zcode-jwt-123");
    assert_eq!(tokens.zai_business_token, "zai-business-123");
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
