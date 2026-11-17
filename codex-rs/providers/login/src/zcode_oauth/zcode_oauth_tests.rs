//! Unit tests for ZCode token data helpers.

use super::*;

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
