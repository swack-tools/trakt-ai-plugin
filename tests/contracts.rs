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

#[test]
fn tool_metadata_distinguishes_account_reads_catalog_and_auth_changes() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 9);
    for tool in tools {
        assert!(tool["title"].as_str().unwrap().len() > 5);
    }
    let find = |name| tools.iter().find(|tool| tool["name"] == name).unwrap();
    assert_eq!(
        find("trakt_get_watched_history")["annotations"]["openWorldHint"],
        false
    );
    assert_eq!(find("trakt_search")["annotations"]["openWorldHint"], true);
    assert_eq!(
        find("trakt_confirm_login")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        find("trakt_confirm_login")["annotations"]["destructiveHint"],
        true
    );
    assert_eq!(
        find("trakt_request_login")["annotations"]["readOnlyHint"],
        false
    );
    for name in [
        "trakt_list_operations",
        "trakt_get_operation",
        "trakt_api_read",
    ] {
        assert_eq!(find(name)["annotations"]["readOnlyHint"], true);
        assert_eq!(find(name)["annotations"]["destructiveHint"], false);
    }
    assert_eq!(
        find("trakt_list_operations")["annotations"]["openWorldHint"],
        false
    );
    assert_eq!(
        find("trakt_get_operation")["annotations"]["openWorldHint"],
        false
    );
    assert_eq!(find("trakt_api_read")["annotations"]["openWorldHint"], true);
    assert_eq!(
        find("trakt_api_write")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        find("trakt_api_write")["annotations"]["destructiveHint"],
        true
    );
    assert_eq!(
        find("trakt_api_write")["annotations"]["openWorldHint"],
        true
    );
    assert_eq!(
        find("trakt_api_write")["inputSchema"]["properties"]["confirmed"]["const"],
        true
    );
    assert!(
        find("trakt_api_write")["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("confirmed"))
    );
    for tool in tools {
        jsonschema::validator_for(&tool["inputSchema"])
            .expect("Every advertised tool has a valid schema");
    }
    assert!(
        !find("trakt_get_recommendations")["inputSchema"]["properties"]["media_type"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("all"))
    );
}

#[test]
fn pagination_follows_actual_limits_and_rejects_unreliable_metadata() {
    use trakt_mcp::trakt::client::pagination;
    let first = pagination([Some(1), Some(7), Some(40), Some(251)], 1, 40).unwrap();
    assert_eq!(first["next_page"], 2);
    assert_eq!(first["has_more"], true);
    let last = pagination([Some(7), Some(7), Some(40), Some(251)], 7, 11).unwrap();
    assert_eq!(last["has_more"], false);
    assert!(last["next_page"].is_null());
    assert_eq!(
        pagination([Some(1), Some(0), Some(100), Some(0)], 1, 0).unwrap()["has_more"],
        false
    );
    assert!(pagination([None; 4], 1, 1).unwrap()["has_more"].is_null());
    for (headers, requested, length) in [
        ([Some(1), Some(3), None, Some(251)], 1, 100),
        ([Some(1), Some(3), Some(100), Some(251)], 2, 100),
        ([Some(1), Some(3), Some(100), Some(251)], 1, 0),
        ([Some(1), Some(3), Some(0), Some(251)], 1, 1),
        ([Some(1), Some(3), Some(40), Some(251)], 1, 100),
    ] {
        assert!(pagination(headers, requested, length).is_err());
    }
}

#[test]
fn watched_modes_and_large_page_numbers_are_validated() {
    assert!(Query::parse(json!({"mode":"recent","page":1,"limit":100})).is_ok());
    assert!(Query::parse(json!({"mode":"all","page":10001})).is_ok());
    assert!(Query::parse(json!({"mode":"everything"})).is_err());
    assert!(Query::parse(json!({"page":4294967296u64})).is_err());
}

#[test]
fn compact_watched_rows_preserve_identity_genres_and_watch_events() {
    let mut row = json!({"id":12,"watched_at":"2026-01-01T00:00:00Z","action":"watch",
        "plays":2,"movie":{"title":"Fixture","year":2020,"ids":{"trakt":123},
        "genres":["drama"],"runtime":100,"overview":"large plot","images":{"poster":"image"}},
        "seasons":[{"number":1}],"private_unneeded_field":"discard"});
    trakt_mcp::trakt::sync::compact(&mut row);
    assert_eq!(row["id"], 12);
    assert_eq!(row["plays"], 2);
    assert_eq!(row["movie"]["ids"]["trakt"], 123);
    assert_eq!(row["movie"]["genres"], json!(["drama"]));
    assert!(row["movie"].get("overview").is_none());
    assert!(row["movie"].get("images").is_none());
    assert!(row.get("private_unneeded_field").is_none());
    assert!(Query::parse(json!({"detail":"compact"})).is_ok());
    assert!(Query::parse(json!({"detail":"anything"})).is_err());
}

#[test]
fn pagination_never_reports_completion_for_contradictory_or_truncated_totals() {
    use trakt_mcp::trakt::client::pagination;
    // A single advertised page cannot contain 250 items at a limit of 100.
    assert!(pagination([Some(1), Some(1), Some(100), Some(250)], 1, 100).is_err());
    // Even a consistent page count is insufficient if the last page is cut short.
    assert!(pagination([Some(3), Some(3), Some(100), Some(250)], 3, 20).is_err());
    assert!(pagination([Some(3), Some(3), Some(100), Some(250)], 3, 49).is_err());
    let complete = pagination([Some(3), Some(3), Some(100), Some(250)], 3, 50).unwrap();
    assert_eq!(complete["has_more"], false);
    assert!(complete["next_page"].is_null());
    // Hostile metadata must be rejected without overflowing multiplication.
    assert!(
        pagination(
            [Some(u64::MAX), Some(u64::MAX), Some(2), Some(u64::MAX)],
            u64::MAX,
            1
        )
        .is_err()
    );
}

#[test]
fn pagination_rejects_short_nonfinal_pages_before_completion() {
    use trakt_mcp::trakt::client::pagination;
    for page in [1, 2] {
        assert!(pagination([Some(page), Some(3), Some(100), Some(250)], page, 20).is_err());
        assert!(pagination([Some(page), Some(3), Some(100), Some(250)], page, 99).is_err());
        assert_eq!(
            pagination([Some(page), Some(3), Some(100), Some(250)], page, 100).unwrap()["has_more"],
            true
        );
    }
    assert_eq!(
        pagination([Some(3), Some(3), Some(100), Some(250)], 3, 50).unwrap()["has_more"],
        false
    );
}
