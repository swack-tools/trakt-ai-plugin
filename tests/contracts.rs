use serde_json::json;
use trakt_mcp::{
    mcp::protocol,
    trakt::{auth::device_status, client::Query},
};
#[test]
fn device_errors_are_distinct() {
    assert_eq!(device_status(400), "authorization_pending");
    assert_eq!(device_status(404), "invalid_device_code");
    assert_eq!(device_status(410), "expired_token");
    assert_eq!(device_status(418), "access_denied");
    assert_eq!(device_status(429), "slow_down");
}
#[test]
fn invalid_ranges_and_types_are_rejected() {
    assert!(Query::parse(json!({"limit": 101})).is_err());
    assert!(Query::parse(json!({"page": 0})).is_err());
    assert!(Query::parse(json!({"media_type":"../users"})).is_err());
    assert!(Query::parse(json!({"years":"2025-2020"})).is_err());
    assert!(Query::parse(json!({"surprise":true})).is_err());
}
#[test]
fn jsonrpc_validates_envelopes_and_notifications() {
    assert!(protocol::validate(&json!({"jsonrpc":"2.0","method":"ping"})).is_ok());
    assert!(protocol::validate(&json!({"jsonrpc":"2.0","method":"ping","id":{}})).is_err());
    assert!(protocol::validate(&json!({"method":"ping","id":1})).is_err());
}
#[test]
fn pkce_matches_rfc7636_vector() {
    assert_eq!(
        trakt_mcp::security::pkce("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert!(!trakt_mcp::security::valid_verifier("short"));
}
#[test]
fn bearer_tokens_require_both_routing_id_and_secret() {
    assert!(trakt_mcp::security::token_id("known-session").is_err());
    assert!(
        trakt_mcp::security::token_id(&format!("{}.{}", "a".repeat(32), "b".repeat(64))).is_ok()
    );
}
