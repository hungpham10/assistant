use std::path::PathBuf;

use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::GoogleTokenData;
use super::build_authorize_url;
use super::load_google_oauth_tokens;
use super::save_google_oauth_tokens;
use crate::auth::AuthDotJson;
use crate::auth::AuthKeyringBackendKind;

fn sample_tokens() -> GoogleTokenData {
    GoogleTokenData::from_token_response(
        "access-token".to_string(),
        Some("refresh-token".to_string()),
        /*expires_in*/ 3600,
        /*email*/ None,
    )
}

#[test]
fn authorize_url_contains_google_endpoints_and_pkce() {
    let url = build_authorize_url(
        "client-id",
        "http://127.0.0.1:12345",
        "code-challenge",
        "state",
    );

    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert!(url.contains("client_id=client-id"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A12345"));
    assert!(url.contains("code_challenge=code-challenge"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("access_type=offline"));
}

#[test]
fn needs_refresh_is_false_for_fresh_tokens() {
    let tokens = sample_tokens();
    assert!(!tokens.needs_refresh());
}

#[test]
fn needs_refresh_is_true_for_expired_tokens() {
    let mut tokens = sample_tokens();
    tokens.expires_at = 0;
    assert!(tokens.needs_refresh());
}

#[test]
fn refresh_grant_preserves_missing_refresh_token_from_response() {
    let tokens = GoogleTokenData::from_token_response(
        "new-access".to_string(),
        /*refresh_token*/ None,
        /*expires_in*/ 1000,
        /*email*/ None,
    );
    assert_eq!(tokens.refresh_token, String::new());
}

#[tokio::test]
async fn tokens_round_trip_through_auth_json() {
    let codex_home = tempdir().expect("create temporary codex home");
    let codex_home: PathBuf = codex_home.path().to_path_buf();
    let mode = AuthCredentialsStoreMode::File;
    let keyring_backend_kind = AuthKeyringBackendKind::default();

    save_google_oauth_tokens(&codex_home, sample_tokens(), mode, keyring_backend_kind)
        .expect("save google tokens");

    let loaded = load_google_oauth_tokens(&codex_home, mode, keyring_backend_kind)
        .expect("load google tokens");
    assert_eq!(loaded, Some(sample_tokens()));
}

#[tokio::test]
async fn saving_tokens_preserves_existing_auth_fields() {
    let codex_home = tempdir().expect("create temporary codex home");
    let codex_home: PathBuf = codex_home.path().to_path_buf();
    let mode = AuthCredentialsStoreMode::File;
    let keyring_backend_kind = AuthKeyringBackendKind::default();

    let existing = AuthDotJson {
        openai_api_key: Some("sk-existing".to_string()),
        ..AuthDotJson::default()
    };
    crate::auth::save_auth(&codex_home, &existing, mode, keyring_backend_kind)
        .expect("save existing auth");

    save_google_oauth_tokens(&codex_home, sample_tokens(), mode, keyring_backend_kind)
        .expect("save google tokens");

    let auth = crate::auth::load_auth_dot_json(&codex_home, mode, keyring_backend_kind)
        .expect("load auth")
        .expect("auth present");
    assert_eq!(auth.openai_api_key.as_deref(), Some("sk-existing"));
    assert_eq!(auth.google_oauth, Some(sample_tokens()));
}

#[tokio::test]
async fn saving_api_key_auth_preserves_google_tokens() {
    let codex_home = tempdir().expect("create temporary codex home");
    let codex_home: PathBuf = codex_home.path().to_path_buf();
    let mode = AuthCredentialsStoreMode::File;
    let keyring_backend_kind = AuthKeyringBackendKind::default();

    save_google_oauth_tokens(&codex_home, sample_tokens(), mode, keyring_backend_kind)
        .expect("save google tokens");

    crate::auth::login_with_api_key(&codex_home, "sk-new", mode, keyring_backend_kind)
        .expect("save api key auth");

    let auth = crate::auth::load_auth_dot_json(&codex_home, mode, keyring_backend_kind)
        .expect("load auth")
        .expect("auth present");
    assert_eq!(auth.openai_api_key.as_deref(), Some("sk-new"));
    assert_eq!(auth.google_oauth, Some(sample_tokens()));
}
