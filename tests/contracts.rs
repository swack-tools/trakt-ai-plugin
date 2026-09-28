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
    let names: std::collections::BTreeSet<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "trakt_search",
            "trakt_get_watched_history",
            "trakt_get_recommendations",
            "trakt_request_login",
            "trakt_confirm_login",
            "trakt_list_operations",
            "trakt_get_operation",
            "trakt_api_read",
            "trakt_api_write",
            "trakt_discover_lists",
            "trakt_get_list_items",
            "trakt_get_calendar",
            "trakt_create_list",
            "trakt_add_list_items",
            "trakt_remove_list_items",
        ]
        .into_iter()
        .collect()
    );
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
fn tool_output_contracts_are_advertised_and_validate_success_envelopes() {
    let definitions = protocol::tools();
    for tool in definitions["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["outputSchema"];
        assert!(schema.is_object(), "{name}: missing output schema");
        let validator = jsonschema::validator_for(schema).unwrap();
        let result = match name {
            "trakt_list_operations" => json!({"data":[],"pagination":{}}),
            "trakt_get_operation" => json!({"operation_id":"getCalendarsMovies"}),
            "trakt_api_read"
            | "trakt_api_write"
            | "trakt_discover_lists"
            | "trakt_get_list_items"
            | "trakt_get_calendar"
            | "trakt_create_list"
            | "trakt_add_list_items"
            | "trakt_remove_list_items" => {
                json!({"operation_id":"getCalendarsMovies","status":200,"data":[],"pagination":null})
            }
            "trakt_request_login" => {
                json!({"device_code":"device","user_code":"ABCD","verification_url":"https://trakt.tv/activate","expires_in":600,"interval":5,"instructions":"Visit Trakt"})
            }
            "trakt_confirm_login" => json!({"status":"connected"}),
            "trakt_get_watched_history" => json!({"data":[],"pagination":{}}),
            "trakt_get_recommendations" => json!({"data":[],"pagination":{}}),
            "trakt_search" => json!({"data":[],"pagination":{},"filters_applied_to_page":false}),
            _ => panic!("unexpected tool {name}"),
        };
        let example = json!({"schema_version":"1","tool":name,"result":result});
        assert!(
            validator.is_valid(&example),
            "{name}: valid envelope rejected"
        );
        assert!(
            !validator.is_valid(&json!({"schema_version":"1","tool":name})),
            "{name}: missing result accepted"
        );
        if name == "trakt_api_read" {
            assert!(
                !validator
                    .is_valid(&json!({"schema_version":"1","tool":name,"result":{"data":[]}})),
                "generic result must identify the operation and status"
            );
        }
    }
}

#[test]
fn focused_read_actions_are_registered_with_read_only_contracts() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    for name in [
        "trakt_discover_lists",
        "trakt_get_list_items",
        "trakt_get_calendar",
    ] {
        let tool = tools.iter().find(|tool| tool["name"] == name).expect(name);
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
        assert!(tool["outputSchema"].is_object());
    }
}

#[test]
fn list_discovery_genre_schema_matches_runtime_validation() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == "trakt_discover_lists")
        .unwrap();
    let validator = jsonschema::validator_for(&tool["inputSchema"]).unwrap();
    for genres in ["science-fiction", "drama,comedy"] {
        assert!(validator.is_valid(&json!({"genres":genres})), "{genres}");
    }
    for genres in ["", "Science-Fiction", "drama comedy", "drama\n"] {
        assert!(!validator.is_valid(&json!({"genres":genres})), "{genres}");
    }
}

#[test]
fn focused_list_page_schemas_match_u32_runtime_bounds() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    for name in ["trakt_discover_lists", "trakt_get_list_items"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        let validator = jsonschema::validator_for(&tool["inputSchema"]).unwrap();
        let mut args = if name == "trakt_get_list_items" {
            json!({"owner":"me","list_id":"77","media_type":"movie"})
        } else {
            json!({})
        };
        args["page"] = json!(4_294_967_295_u64);
        assert!(validator.is_valid(&args), "{name}: u32 maximum rejected");
        args["page"] = json!(4_294_967_296_u64);
        assert!(
            !validator.is_valid(&args),
            "{name}: value beyond u32 accepted"
        );
    }
}

#[test]
fn focused_list_path_schemas_reject_unsafe_segments() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    for (name, key, base) in [
        (
            "trakt_get_list_items",
            "owner",
            json!({"list_id":"77","media_type":"movie"}),
        ),
        (
            "trakt_get_list_items",
            "list_id",
            json!({"owner":"me","media_type":"movie"}),
        ),
        (
            "trakt_add_list_items",
            "list_id",
            json!({"items":[{"media_type":"movie","trakt_id":1}],"confirmed":true}),
        ),
        (
            "trakt_remove_list_items",
            "list_id",
            json!({"items":[{"media_type":"movie","trakt_id":1}],"confirmed":true}),
        ),
    ] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        let validator = jsonschema::validator_for(&tool["inputSchema"]).unwrap();
        for value in ["me", "fixture-77", "List.Name_1"] {
            let mut args = base.clone();
            args[key] = json!(value);
            assert!(validator.is_valid(&args), "{name} {key} {value}");
        }
        for value in [".", "..", "../other", "two words", "é"] {
            let mut args = base.clone();
            args[key] = json!(value);
            assert!(!validator.is_valid(&args), "{name} {key} {value}");
        }
    }
}

#[test]
fn focused_write_schemas_reject_whitespace_names_and_duplicate_items() {
    let definitions = protocol::tools();
    let tools = definitions["tools"].as_array().unwrap();
    let find = |name| tools.iter().find(|tool| tool["name"] == name).unwrap();
    let create = jsonschema::validator_for(&find("trakt_create_list")["inputSchema"]).unwrap();
    assert!(create.is_valid(&json!({"name":"Rainy Sunday","confirmed":true})));
    assert!(!create.is_valid(&json!({"name":"   ","confirmed":true})));
    for name in ["trakt_add_list_items", "trakt_remove_list_items"] {
        let validator = jsonschema::validator_for(&find(name)["inputSchema"]).unwrap();
        let item = json!({"media_type":"movie","trakt_id":123});
        assert!(
            !validator.is_valid(&json!({
                "list_id":"77","items":[item.clone(),item],"confirmed":true
            })),
            "{name}"
        );
    }
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
