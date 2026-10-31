//! The reviewed tool spec maps every tool to supported operations and fixed inputs.
use serde_json::{Value, json};
use trakt_mcp::{
    mcp::{focused, protocol, spec},
    trakt::catalog,
};

fn document() -> Value {
    serde_json::from_str(spec::SOURCE).unwrap()
}

fn entry<'a>(document: &'a mut Value, name: &str) -> &'a mut Value {
    document["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == name)
        .unwrap()
}

fn expect_problem(document: &Value, text: &str) {
    let problems = spec::problems(document);
    assert!(
        problems.iter().any(|problem| problem.contains(text)),
        "expected {text:?} in {problems:?}"
    );
}

#[test]
fn the_committed_spec_has_no_problems() {
    assert_eq!(spec::problems(&document()), Vec::<String>::new());
    assert_eq!(spec::tools().len(), 37);
}

#[test]
fn a_missing_or_unsupported_operation_fails() {
    let mut d = document();
    entry(&mut d, "trakt_get_stats")["operation_id"] = json!("notAnOperation");
    expect_problem(&d, "missing operation notAnOperation");

    let mut d = document();
    entry(&mut d, "trakt_get_title")["operations"]["map"]["movie"] = json!("notAnOperation");
    expect_problem(&d, "missing operation notAnOperation");

    let mut d = document();
    entry(&mut d, "trakt_add_history")["operation_id"] = json!("postOauthRevoke");
    expect_problem(&d, "non-supported operation postOauthRevoke");

    let mut d = document();
    entry(&mut d, "trakt_add_history")["operation_id"] = json!("putUsersAvatar");
    expect_problem(&d, "non-supported operation putUsersAvatar");
}

#[test]
fn a_write_without_required_confirmed_const_true_fails() {
    for name in ["trakt_add_ratings", "trakt_delete_list"] {
        let mut d = document();
        entry(&mut d, name)["inputs"]
            .as_object_mut()
            .unwrap()
            .remove("confirmed");
        expect_problem(&d, "confirmed const true");

        let mut d = document();
        entry(&mut d, name)["inputs"]["confirmed"] = json!({"type":"boolean"});
        expect_problem(&d, "confirmed const true");

        let mut d = document();
        entry(&mut d, name)["required"]
            .as_array_mut()
            .unwrap()
            .retain(|input| input != "confirmed");
        expect_problem(&d, "confirmed const true");
    }
}

#[test]
fn missing_annotations_and_titles_fail() {
    for hint in [
        "readOnlyHint",
        "destructiveHint",
        "idempotentHint",
        "openWorldHint",
    ] {
        let mut d = document();
        entry(&mut d, "trakt_search")["annotations"]
            .as_object_mut()
            .unwrap()
            .remove(hint);
        expect_problem(&d, &format!("missing annotation {hint}"));
    }
    let mut d = document();
    entry(&mut d, "trakt_search")["annotations"] = Value::Null;
    expect_problem(&d, "missing annotation readOnlyHint");
    let mut d = document();
    entry(&mut d, "trakt_search")["title"] = json!("");
    expect_problem(&d, "missing title");
    let mut d = document();
    entry(&mut d, "trakt_add_ratings")["annotations"]["destructiveHint"] = json!(false);
    expect_problem(&d, "annotations contradict");
}

#[test]
fn free_form_and_nullable_inputs_fail() {
    for input in ["path", "method", "operation_id", "body"] {
        let mut d = document();
        entry(&mut d, "trakt_search")["inputs"][input] = json!({"type":"string"});
        expect_problem(&d, &format!("free-form input {input}"));
    }
    for schema in [
        json!({"anyOf":[{"type":"string","enum":["a"]},{"type":"null"}]}),
        json!({"type":["string","null"]}),
        json!({"enum":["a",null]}),
        json!({"anyOf":[{},{"type":"null"}]}),
        json!({}),
    ] {
        let mut d = document();
        entry(&mut d, "trakt_get_trending")["inputs"]["extra"] = schema;
        expect_problem(&d, "null or an unconstrained value");
    }
}

#[test]
fn a_read_cannot_map_to_a_write_operation() {
    let mut d = document();
    entry(&mut d, "trakt_get_stats")["operation_id"] = json!("postSyncHistoryAdd");
    expect_problem(&d, "method contradicts");
}

#[test]
fn every_catalog_tool_input_reaches_one_of_its_operations() {
    for tool in spec::tools()
        .iter()
        .filter(|tool| tool.handler == spec::Handler::Catalog)
    {
        let mut operations: Vec<String> = tool.operation_id.iter().cloned().collect();
        if let Some(map) = &tool.operations {
            operations.extend(
                map.map
                    .values()
                    .map(|entry| spec::mapped(entry).0.unwrap().to_string()),
            );
        }
        let selector = tool.operations.as_ref().map(|o| o.selector.clone());
        for input in tool.inputs.keys() {
            if input == "confirmed"
                || Some(input) == selector.as_ref()
                || (input == "items" && tool.body.items)
                || tool.body.fields.contains(input)
            {
                continue;
            }
            let target = tool
                .bind
                .get(input)
                .and_then(Value::as_str)
                .unwrap_or(input);
            assert!(
                operations.iter().any(|id| {
                    catalog::find(id).unwrap()["parameters"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|parameter| parameter["name"] == target)
                }),
                "{}: {input} reaches no operation",
                tool.name
            );
        }
    }
}

fn prepared(name: &str, args: Value) -> (String, catalog::Prepared) {
    let call = focused::prepare(name, args).unwrap_or_else(|e| panic!("{name}: {}", e.code));
    let write = spec::find(name).unwrap().kind == spec::Kind::Write;
    let op = catalog::find(&call.operation_id).unwrap();
    let request = catalog::prepare(op, &call, write)
        .unwrap_or_else(|e| panic!("{name} {}: {}", call.operation_id, e.code));
    (call.operation_id, request)
}

#[test]
fn every_read_builds_a_request_the_catalog_accepts() {
    for (name, args, path) in [
        (
            "trakt_get_title",
            json!({"media_type":"movie","id":"arrival-2016"}),
            "/movies/arrival-2016",
        ),
        (
            "trakt_get_title",
            json!({"media_type":"episode","id":"700","season":1,"episode":2}),
            "/shows/700/seasons/1/episodes/2",
        ),
        (
            "trakt_get_people",
            json!({"media_type":"show","id":"700"}),
            "/shows/700/people",
        ),
        (
            "trakt_get_related",
            json!({"media_type":"movie","id":"1"}),
            "/movies/1/related",
        ),
        (
            "trakt_get_seasons",
            json!({"id":"700"}),
            "/shows/700/seasons",
        ),
        (
            "trakt_lookup_id",
            json!({"id_type":"imdb","id":"tt2543164","media_type":"movie"}),
            "/search/imdb/tt2543164",
        ),
        (
            "trakt_get_trending",
            json!({"media_type":"all","ignore_watched":true}),
            "/media/trending",
        ),
        (
            "trakt_get_popular",
            json!({"media_type":"show"}),
            "/shows/popular",
        ),
        (
            "trakt_get_anticipated",
            json!({"media_type":"movie","genres":"drama","years":"2020-2026"}),
            "/movies/anticipated",
        ),
        (
            "trakt_get_show_progress",
            json!({"id":"700","specials":true}),
            "/shows/700/progress/watched",
        ),
        (
            "trakt_get_up_next",
            json!({"sort_how":"desc"}),
            "/sync/progress/up_next",
        ),
        ("trakt_get_stats", json!({}), "/users/me/stats"),
        (
            "trakt_get_ratings",
            json!({"media_type":"movie"}),
            "/users/me/ratings/movies",
        ),
        (
            "trakt_get_ratings",
            json!({"media_type":"season","rating":"8"}),
            "/users/me/ratings/seasons/8",
        ),
        (
            "trakt_get_collection",
            json!({"media_type":"movie","page":2}),
            "/sync/collection/movies",
        ),
        (
            "trakt_get_collection",
            json!({"media_type":"show"}),
            "/sync/collection/shows",
        ),
        (
            "trakt_get_watchlist",
            json!({"media_type":"movie"}),
            "/sync/watchlist/movies/rank/asc",
        ),
        (
            "trakt_get_watchlist",
            json!({"media_type":"all","sort_by":"added","sort_how":"desc"}),
            "/sync/watchlist/all/added/desc",
        ),
        ("trakt_get_my_lists", json!({}), "/users/me/lists"),
        (
            "trakt_get_list_items",
            json!({"owner":"me","list_id":"77","media_type":"all","sort_by":"added"}),
            "/users/me/lists/77/items/movie,show,season,episode",
        ),
    ] {
        assert_eq!(prepared(name, args.clone()).1.path, path, "{name} {args}");
    }
    let (_, seasons) = prepared("trakt_get_seasons", json!({"id":"700"}));
    assert!(
        seasons
            .query
            .contains(&("extended".to_owned(), "episodes".to_owned())),
        "seasons must include episode IDs"
    );
    for (kind, path) in [
        ("shows", "shows"),
        ("new_shows", "shows/new"),
        ("premieres", "shows/premieres"),
        ("finales", "shows/finales"),
        ("movies", "movies"),
        ("streaming", "streaming"),
        ("dvd", "dvd"),
    ] {
        for target in ["my", "all"] {
            assert_eq!(
                prepared(
                    "trakt_get_calendar",
                    json!({"kind":kind,"target":target,"start_date":"2026-10-05","days":7})
                )
                .1
                .path,
                format!("/calendars/{target}/{path}/2026-10-05/7")
            );
        }
    }
}

#[test]
fn inputs_outside_the_selected_operation_are_rejected() {
    for (name, args) in [
        (
            "trakt_get_title",
            json!({"media_type":"movie","id":"1","season":1}),
        ),
        (
            "trakt_get_ratings",
            json!({"media_type":"movie","rating":"8"}),
        ),
        (
            "trakt_get_collection",
            json!({"media_type":"show","page":2}),
        ),
        ("trakt_get_stats", json!({"id":"someone-else"})),
        (
            "trakt_get_title",
            json!({"media_type":"movie","id":"../oauth/token"}),
        ),
        (
            "trakt_get_title",
            json!({"media_type":"movie","id":"1","extended":null}),
        ),
        ("trakt_get_title", json!({"media_type":null,"id":"1"})),
        (
            "trakt_get_trending",
            json!({"media_type":"movie","url":"https://evil.test"}),
        ),
        ("trakt_get_watchlist", json!({"media_type":"person"})),
    ] {
        let rejected = focused::prepare(name, args.clone()).and_then(|call| {
            catalog::prepare(catalog::find(&call.operation_id)?, &call, false).map(|_| ())
        });
        assert!(rejected.is_err(), "{name} accepted {args}");
    }
}

#[test]
fn every_item_write_accepts_each_media_type() {
    for (name, extra, path) in [
        ("trakt_add_history", json!({}), "/sync/history"),
        ("trakt_remove_history", json!({}), "/sync/history/remove"),
        ("trakt_add_ratings", json!({"rating":8}), "/sync/ratings"),
        ("trakt_remove_ratings", json!({}), "/sync/ratings/remove"),
        ("trakt_add_collection", json!({}), "/sync/collection"),
        (
            "trakt_remove_collection",
            json!({}),
            "/sync/collection/remove",
        ),
        ("trakt_add_watchlist", json!({}), "/sync/watchlist"),
        (
            "trakt_remove_watchlist",
            json!({}),
            "/sync/watchlist/remove",
        ),
    ] {
        for (media, key) in [
            ("movie", "movies"),
            ("show", "shows"),
            ("season", "seasons"),
            ("episode", "episodes"),
        ] {
            let mut item = json!({"media_type":media,"trakt_id":123});
            item.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let (_, request) = prepared(name, json!({"items":[item],"confirmed":true}));
            assert_eq!(request.path, path);
            let mut expected = json!({"ids":{"trakt":123}});
            expected
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                request.body.unwrap(),
                json!({key:[expected]}),
                "{name} {media}"
            );
        }
    }
    for name in ["trakt_add_list_items", "trakt_remove_list_items"] {
        for media in ["movie", "show", "season", "episode"] {
            prepared(
                name,
                json!({"list_id":"77","items":[{"media_type":media,"trakt_id":1}],"confirmed":true}),
            );
        }
    }
}

#[test]
fn write_inputs_keep_their_specific_rules() {
    let (_, request) = prepared(
        "trakt_add_history",
        json!({"items":[{"media_type":"movie","trakt_id":1,"watched_at":"2026-10-01T20:00:00.000Z"}],"confirmed":true}),
    );
    assert_eq!(
        request.body.unwrap()["movies"][0]["watched_at"],
        "2026-10-01T20:00:00.000Z"
    );
    let (_, request) = prepared(
        "trakt_remove_history",
        json!({"history_ids":[1234],"confirmed":true}),
    );
    assert_eq!(request.body.unwrap(), json!({"ids":[1234]}));
    let (_, request) = prepared(
        "trakt_update_list",
        json!({"list_id":"77","name":" Renamed ","privacy":"public","confirmed":true}),
    );
    assert_eq!(request.path, "/users/me/lists/77/");
    assert_eq!(
        request.body.unwrap(),
        json!({"name":"Renamed","privacy":"public"})
    );
    let (_, request) = prepared(
        "trakt_delete_list",
        json!({"list_id":"77","confirmed":true}),
    );
    assert_eq!(request.path, "/users/me/lists/77/");
    assert!(request.body.is_none());
    let (_, request) = prepared(
        "trakt_hide_recommendation",
        json!({"media_type":"show","id":"700","confirmed":true}),
    );
    assert_eq!(request.path, "/recommendations/shows/700");
    let (_, request) = prepared(
        "trakt_create_list",
        json!({"name":"Fixture","display_numbers":true,"sort_how":"desc","confirmed":true}),
    );
    let body = request.body.unwrap();
    assert_eq!(body["privacy"], "private");
    assert_eq!(body["display_numbers"], true);
    for (name, args, code) in [
        (
            "trakt_add_ratings",
            json!({"items":[{"media_type":"movie","trakt_id":1,"rating":8}]}),
            "confirmation_required",
        ),
        (
            "trakt_add_ratings",
            json!({"items":[{"media_type":"movie","trakt_id":1,"rating":11}],"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_add_ratings",
            json!({"items":[{"media_type":"movie","trakt_id":1}],"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_add_ratings",
            json!({"items":[{"media_type":"movie","trakt_id":1,"rating":8},{"media_type":"movie","trakt_id":1,"rating":9}],"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_remove_history",
            json!({"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_add_history",
            json!({"items":[{"media_type":"movie","trakt_id":1,"watched_at":"yesterday"}],"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_add_collection",
            json!({"items":[{"media_type":"movie","trakt_id":1,"rating":8}],"confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_update_list",
            json!({"list_id":"77","confirmed":true}),
            "invalid_parameters",
        ),
        (
            "trakt_delete_list",
            json!({"list_id":"77","confirmed":false}),
            "confirmation_required",
        ),
        (
            "trakt_hide_recommendation",
            json!({"media_type":"movie","id":"1"}),
            "confirmation_required",
        ),
    ] {
        assert_eq!(
            focused::prepare(name, args.clone()).err().map(|e| e.code),
            Some(code),
            "{name} {args}"
        );
    }
}

#[test]
fn list_changes_verify_ownership() {
    for tool in spec::tools() {
        assert_eq!(
            tool.ownership,
            matches!(
                tool.name.as_str(),
                "trakt_update_list"
                    | "trakt_delete_list"
                    | "trakt_add_list_items"
                    | "trakt_remove_list_items"
            ),
            "{}",
            tool.name
        );
    }
}

#[test]
fn operations_acting_on_the_connected_user_require_a_token() {
    for op in catalog::operations() {
        if op["status"] == "supported"
            && (op["method"] != "GET" || op["path"].as_str().unwrap().starts_with("/sync/"))
        {
            assert_eq!(op["auth"], "required", "{}", op["operation_id"]);
        }
    }
}

/// The eval runner reads this mirror; set UPDATE_EVAL_TOOLS=1 to regenerate it.
#[test]
fn eval_tool_mocks_mirror_the_advertised_tools() {
    let mut expected = protocol::tools();
    for tool in expected["tools"].as_array_mut().unwrap() {
        tool.as_object_mut().unwrap().remove("outputSchema");
    }
    let path = "evals/mocks/trakt/_tools.json";
    if std::env::var_os("UPDATE_EVAL_TOOLS").is_some() {
        let text = serde_json::to_string_pretty(&expected).unwrap() + "\n";
        std::fs::write(path, text).unwrap();
    }
    let actual: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(actual, expected);
}
