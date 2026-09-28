use serde_json::{Value, json};
use std::collections::HashSet;
use trakt_mcp::trakt::catalog::{self, Call};

#[test]
fn focused_reads_use_supported_catalog_paths_and_reject_invalid_inputs() {
    use trakt_mcp::mcp::focused;
    let examples = [
        (
            "trakt_discover_lists",
            json!({"view":"popular","genres":"science-fiction","page":2,"limit":20}),
            "getListsPopular",
            "/lists/popular",
        ),
        (
            "trakt_get_list_items",
            json!({"owner":"me","list_id":"77","media_type":"movie","page":1}),
            "getUsersListsListItemsMovie",
            "/users/me/lists/77/items/movie",
        ),
        (
            "trakt_get_calendar",
            json!({"target":"my","media_type":"show","start_date":"2026-09-27","days":7}),
            "getCalendarsShows",
            "/calendars/my/shows/2026-09-27/7",
        ),
    ];
    for (name, args, operation_id, path) in examples {
        let call = focused::prepare(name, args).unwrap();
        assert_eq!(call.operation_id, operation_id);
        let prepared =
            catalog::prepare(catalog::find(&call.operation_id).unwrap(), &call, false).unwrap();
        assert_eq!(prepared.path, path);
    }
    for (name, args) in [
        ("trakt_discover_lists", json!({"query":"arbitrary title"})),
        (
            "trakt_get_list_items",
            json!({"owner":"../other","list_id":"77","media_type":"movie"}),
        ),
        (
            "trakt_get_list_items",
            json!({"owner":"me","list_id":"77","media_type":"season"}),
        ),
        (
            "trakt_get_calendar",
            json!({"target":"my","media_type":"movie","start_date":"2026-02-30","days":7}),
        ),
        (
            "trakt_get_calendar",
            json!({"target":"all","media_type":"show","start_date":"2026-09-27","days":32}),
        ),
    ] {
        assert!(focused::prepare(name, args).is_err(), "{name}");
    }
}

#[test]
fn focused_writes_prepare_only_concrete_private_or_owned_list_changes() {
    use trakt_mcp::mcp::focused;
    let create = focused::prepare(
        "trakt_create_list",
        json!({"name":"Rainy Sunday","confirmed":true}),
    )
    .unwrap();
    assert_eq!(create.operation_id, "postUsersListsCreate");
    let request =
        catalog::prepare(catalog::find(&create.operation_id).unwrap(), &create, true).unwrap();
    assert_eq!(request.path, "/users/me/lists");
    assert_eq!(request.body.unwrap()["privacy"], "private");

    for (name, operation_id, path) in [
        (
            "trakt_add_list_items",
            "postUsersListsListAdd",
            "/users/me/lists/77/items",
        ),
        (
            "trakt_remove_list_items",
            "postUsersListsListRemove",
            "/users/me/lists/77/items/remove",
        ),
    ] {
        let call = focused::prepare(name, json!({"list_id":"77","items":[{"media_type":"movie","trakt_id":123},{"media_type":"show","trakt_id":456}],"confirmed":true})).unwrap();
        assert_eq!(call.operation_id, operation_id);
        let prepared =
            catalog::prepare(catalog::find(&call.operation_id).unwrap(), &call, true).unwrap();
        assert_eq!(prepared.path, path);
        assert_eq!(
            prepared.body.unwrap(),
            json!({"movies":[{"ids":{"trakt":123}}],"shows":[{"ids":{"trakt":456}}]})
        );
    }
    for (name, args) in [
        ("trakt_create_list", json!({"name":"Rainy Sunday"})),
        ("trakt_create_list", json!({"name":" ","confirmed":true})),
        (
            "trakt_create_list",
            json!({"name":"Rainy Sunday","privacy":"friends","confirmed":true}),
        ),
        (
            "trakt_add_list_items",
            json!({"list_id":"77","items":[],"confirmed":true}),
        ),
        (
            "trakt_add_list_items",
            json!({"list_id":"77","items":[{"media_type":"movie","trakt_id":0}],"confirmed":true}),
        ),
        (
            "trakt_add_list_items",
            json!({"list_id":"77","items":[{"media_type":"movie","trakt_id":123},{"media_type":"movie","trakt_id":123}],"confirmed":true}),
        ),
        (
            "trakt_remove_list_items",
            json!({"list_id":"../other","items":[{"media_type":"movie","trakt_id":123}],"confirmed":true}),
        ),
        (
            "trakt_remove_list_items",
            json!({"list_id":"77","items":[{"media_type":"movie","trakt_id":123}],"confirmed":false}),
        ),
    ] {
        assert!(focused::prepare(name, args).is_err(), "{name}");
    }
}

#[test]
fn focused_list_text_limits_count_unicode_characters() {
    use trakt_mcp::mcp::focused;
    let name = "界".repeat(100);
    let description = "é".repeat(1000);
    let call = focused::prepare(
        "trakt_create_list",
        json!({"name":name,"description":description,"confirmed":true}),
    )
    .unwrap();
    let body = call.body.unwrap();
    assert_eq!(body["name"].as_str().unwrap().chars().count(), 100);
    assert_eq!(body["description"].as_str().unwrap().chars().count(), 1000);
    assert!(
        focused::prepare(
            "trakt_create_list",
            json!({"name":"界".repeat(101),"confirmed":true})
        )
        .is_err()
    );
    assert!(
        focused::prepare(
            "trakt_create_list",
            json!({"name":"Valid","description":"é".repeat(1001),"confirmed":true})
        )
        .is_err()
    );
}

fn call(value: Value) -> Call {
    serde_json::from_value(value).unwrap()
}

#[test]
fn bundled_operation_ids_paths_and_schemas_are_consistent() {
    let mut ids = HashSet::new();
    assert!(!catalog::operations().is_empty());
    for op in catalog::operations() {
        let id = op["operation_id"].as_str().unwrap();
        assert!(
            !id.is_empty() && ids.insert(id),
            "duplicate operation: {id}"
        );
        let path = op["path"].as_str().unwrap();
        assert!(
            path.starts_with('/') && !path.starts_with("//"),
            "{id}: fixed relative path"
        );
        assert!(
            op["source_url"]
                .as_str()
                .unwrap()
                .starts_with("https://docs.trakt.tv/reference/")
        );
        assert!(matches!(
            op["method"].as_str(),
            Some("GET" | "POST" | "PUT" | "PATCH" | "DELETE")
        ));
        let parameters = op["parameters"].as_array().unwrap();
        let mut names = HashSet::new();
        for parameter in parameters {
            let place = parameter["in"].as_str().unwrap();
            let name = parameter["name"].as_str().unwrap();
            assert!(
                names.insert((place, name)),
                "{id}: duplicate {place} parameter {name}"
            );
            jsonschema::validator_for(&parameter["schema"])
                .unwrap_or_else(|e| panic!("{id}.{name}: {e}"));
        }
        for placeholder in path.split('{').skip(1) {
            let name = placeholder.split('}').next().unwrap();
            assert!(
                names.contains(&("path", name)),
                "{id}: missing path schema for {name}"
            );
        }
        for parameter in parameters.iter().filter(|p| p["in"] == "path") {
            assert!(path.contains(&format!("{{{}}}", parameter["name"].as_str().unwrap())));
        }
        jsonschema::validator_for(&catalog::input_schema(op))
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

#[test]
fn discovery_pages_cover_every_operation_without_duplicates() {
    let mut ids = HashSet::new();
    let mut page = 1;
    loop {
        let found = catalog::discover(json!({"page":page,"limit":17})).unwrap();
        assert_eq!(
            found["pagination"]["item_count"],
            catalog::operations().len()
        );
        for row in found["data"].as_array().unwrap() {
            assert!(ids.insert(row["operation_id"].as_str().unwrap().to_owned()));
        }
        if found["pagination"]["has_more"] == false {
            assert!(found["pagination"]["next_page"].is_null());
            break;
        }
        page = found["pagination"]["next_page"].as_u64().unwrap();
    }
    assert_eq!(ids.len(), catalog::operations().len());
    for invalid in [
        json!({"page":0}),
        json!({"limit":101}),
        json!({"url":"https://evil.test"}),
    ] {
        assert!(catalog::discover(invalid).is_err());
    }
    assert!(catalog::find("not-an-operation").is_err());
    assert!(catalog::describe(json!({"operation_id":"not-an-operation"})).is_err());
}

#[test]
fn calendar_input_is_typed_and_renders_only_safe_path_segments() {
    let op = catalog::find("getCalendarsMovies").unwrap();
    let base = json!({"operation_id":"getCalendarsMovies","path_params":{"target":"my","start_date":"2026-09-27","days":7}});
    let prepared = catalog::prepare(op, &call(base.clone()), false).unwrap();
    assert_eq!(prepared.path, "/calendars/my/movies/2026-09-27/7");
    for (key, value) in [
        ("target", json!("someone-else")),
        ("days", json!("7")),
        ("start_date", json!("../oauth/token")),
        ("start_date", json!("%2e%2e%2fadmin")),
        ("start_date", json!("https://evil.test")),
        ("start_date", json!("day?token=x")),
        ("start_date", json!("x\nAuthorization: token")),
        ("start_date", json!("..")),
    ] {
        let mut bad = base.clone();
        bad["path_params"][key] = value;
        assert!(catalog::prepare(op, &call(bad), false).is_err(), "{key}");
    }
    for extra in [
        json!({"headers":{"Authorization":"bad"}}),
        json!({"url":"https://evil.test"}),
        json!({"method":"POST"}),
    ] {
        let mut bad = base.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(serde_json::from_value::<Call>(bad).is_err());
    }
    let mut bad = base.clone();
    bad["query_params"] = json!({"access_token":"bad"});
    assert!(catalog::prepare(op, &call(bad), false).is_err());
    let mut bad = base;
    bad["body"] = json!({"name":"must not post"});
    assert!(catalog::prepare(op, &call(bad), false).is_err());
}

#[test]
fn writes_require_the_write_tool_confirmation_and_valid_body() {
    let op = catalog::find("postUsersListsCreate").unwrap();
    let base = json!({"operation_id":"postUsersListsCreate","path_params":{"id":"me"},"body":{"name":"Fixture","privacy":"private"},"confirmed":true});
    let prepared = catalog::prepare(op, &call(base.clone()), true).unwrap();
    assert_eq!(prepared.path, "/users/me/lists");
    assert_eq!(prepared.body.unwrap()["privacy"], "private");
    assert_eq!(
        catalog::prepare(op, &call(base.clone()), false)
            .err()
            .unwrap()
            .code,
        "operation_requires_write_tool"
    );
    for confirmed in [Value::Null, json!(false)] {
        let mut bad = base.clone();
        bad["confirmed"] = confirmed;
        assert_eq!(
            catalog::prepare(op, &call(bad), true).err().unwrap().code,
            "confirmation_required"
        );
    }
    for body in [
        json!({}),
        json!({"name":7}),
        json!({"name":"Fixture","unrecognized":true}),
    ] {
        let mut bad = base.clone();
        bad["body"] = body;
        assert!(catalog::prepare(op, &call(bad), true).is_err());
    }
    let read = catalog::find("getCalendarsMovies").unwrap();
    assert_eq!(
        catalog::prepare(read, &call(base), true)
            .err()
            .unwrap()
            .code,
        "operation_requires_read_tool"
    );
}

#[test]
fn unsupported_operations_cannot_be_executed() {
    // An unavailable entry must fail before parameter or confirmation handling.
    let mut op = catalog::find("postUsersListsCreate").unwrap().clone();
    for status in ["managed_auth", "first_party_only", "unsupported"] {
        op["status"] = json!(status);
        assert_eq!(
            catalog::prepare(&op, &call(json!({"operation_id":op["operation_id"]})), true)
                .err()
                .unwrap()
                .code,
            "operation_unavailable"
        );
    }
    for op in catalog::operations()
        .iter()
        .filter(|op| op["status"] != "supported")
    {
        assert_eq!(
            catalog::prepare(op, &call(json!({"operation_id":op["operation_id"]})), true)
                .err()
                .unwrap()
                .code,
            "operation_unavailable"
        );
    }
}

#[test]
fn generic_pagination_has_explicit_limits_and_rejects_invalid_values() {
    let op = catalog::find("getListsPopular").unwrap();
    let prepared =
        catalog::prepare(op, &call(json!({"operation_id":"getListsPopular"})), false).unwrap();
    assert!(
        prepared
            .query
            .contains(&("page".to_owned(), "1".to_owned()))
    );
    assert!(
        prepared
            .query
            .contains(&("limit".to_owned(), "100".to_owned()))
    );
    for query in [
        json!({"page":0}),
        json!({"page":4294967296u64}),
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"limit":"100"}),
        json!({"unknown":true}),
    ] {
        assert!(
            catalog::prepare(
                op,
                &call(json!({"operation_id":"getListsPopular","query_params":query})),
                false
            )
            .is_err()
        );
    }
}

#[test]
fn optional_nullable_queries_are_omitted_without_relaxing_unknown_keys_or_paths() {
    let op = catalog::find("getMoviesTrending").unwrap();
    let prepared = catalog::prepare(
        op,
        &call(json!({"operation_id":"getMoviesTrending","query_params":{"extended":null}})),
        false,
    )
    .unwrap();
    assert!(!prepared.query.iter().any(|(name, _)| name == "extended"));
    assert!(
        catalog::prepare(
            op,
            &call(json!({"operation_id":"getMoviesTrending","query_params":{"unknown":null}})),
            false,
        )
        .is_err()
    );
    let op = catalog::find("getCalendarsMovies").unwrap();
    assert!(catalog::prepare(
        op,
        &call(json!({"operation_id":"getCalendarsMovies","path_params":{"target":"my","start_date":null,"days":7}})),
        false,
    ).is_err());
}

#[test]
fn optional_request_bodies_remain_absent_in_schema_and_prepared_request() {
    let op = catalog::find("postCommentsLike").unwrap();
    let schema = catalog::input_schema(op);
    assert!(
        !schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("body"))
    );
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&json!({
                "path_params":{"id":"123"},"query_params":{}
            }))
    );
    let omitted = call(
        json!({"operation_id":"postCommentsLike","path_params":{"id":"123"},"confirmed":true}),
    );
    assert!(omitted.body.is_none());
    let prepared = catalog::prepare(op, &omitted, true).unwrap();
    assert_eq!(prepared.path, "/comments/123/like");
    assert!(
        prepared.body.is_none(),
        "omission must not become an empty JSON object"
    );
    // This operation's unrestricted schema accepts explicit JSON null, which
    // remains present instead of being silently converted into omission.
    let explicit_null = call(
        json!({"operation_id":"postCommentsLike","path_params":{"id":"123"},"confirmed":true,"body":null}),
    );
    assert_eq!(explicit_null.body, Some(Value::Null));
    assert_eq!(
        catalog::prepare(op, &explicit_null, true).unwrap().body,
        Some(Value::Null)
    );
}

#[test]
fn required_and_typed_request_bodies_reject_omission_or_invalid_null() {
    let required = catalog::find("postUsersListsCreate").unwrap();
    let schema = catalog::input_schema(required);
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("body"))
    );
    assert!(
        !jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&json!({
                "path_params":{"id":"me"},"query_params":{}
            }))
    );
    assert!(
        catalog::prepare(
            required,
            &call(json!({
                "operation_id":"postUsersListsCreate","path_params":{"id":"me"},"confirmed":true
            })),
            true
        )
        .is_err()
    );
    let typed_optional = catalog::find("postNotesCreate").unwrap();
    assert!(
        catalog::prepare(
            typed_optional,
            &call(json!({
                "operation_id":"postNotesCreate","confirmed":true
            })),
            true
        )
        .unwrap()
        .body
        .is_none()
    );
    assert!(
        catalog::prepare(
            typed_optional,
            &call(json!({
                "operation_id":"postNotesCreate","confirmed":true,"body":null
            })),
            true
        )
        .is_err()
    );
}

#[test]
fn get_operations_reject_any_body_including_explicit_json_null() {
    let op = catalog::find("getCalendarsMovies").unwrap();
    for body in [Value::Null, json!({}), json!("unexpected")] {
        let input = call(json!({"operation_id":"getCalendarsMovies",
            "path_params":{"target":"my","start_date":"2026-09-27","days":7},"body":body}));
        assert!(input.body.is_some());
        assert_eq!(
            catalog::prepare(op, &input, false).err().unwrap().code,
            "invalid_api_parameters"
        );
    }
}
