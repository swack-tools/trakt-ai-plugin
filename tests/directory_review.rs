//! Requirements from the Anthropic and OpenAI directory reviews.
use serde_json::Value;
use trakt_mcp::{error, mcp::protocol, security};

fn tools() -> Vec<Value> {
    protocol::tools()["tools"].as_array().unwrap().clone()
}

fn find(name: &str) -> Value {
    tools().into_iter().find(|t| t["name"] == name).unwrap()
}

#[test]
fn loopback_redirects_match_on_any_port() {
    // RFC 8252 section 7.3: native clients pick an ephemeral port at request time.
    for (registered, requested) in [
        (
            "http://127.0.0.1:3000/callback",
            "http://127.0.0.1:54321/callback",
        ),
        (
            "http://127.0.0.1/callback",
            "http://127.0.0.1:8080/callback",
        ),
        (
            "http://localhost:3000/callback",
            "http://localhost:4000/callback",
        ),
        ("http://[::1]:3000/callback", "http://[::1]:4000/callback"),
        (
            "https://claude.ai/api/mcp/auth_callback",
            "https://claude.ai/api/mcp/auth_callback",
        ),
    ] {
        assert!(
            security::redirect_matches(registered, requested),
            "{requested}"
        );
    }
    for (registered, requested) in [
        (
            "http://127.0.0.1:3000/callback",
            "http://127.0.0.1:3000/other",
        ),
        (
            "http://127.0.0.1:3000/callback",
            "http://localhost:3000/callback",
        ),
        (
            "http://127.0.0.1:3000/callback",
            "http://127.0.0.1:3000/callback?x=1",
        ),
        (
            "https://app.example.test/cb",
            "https://app.example.test:8443/cb",
        ),
        (
            "https://app.example.test/cb",
            "https://evil.example.test/cb",
        ),
        ("http://127.0.0.1:3000/callback", "not a url"),
    ] {
        assert!(
            !security::redirect_matches(registered, requested),
            "{requested}"
        );
    }
}

#[test]
fn anthropic_egress_shares_a_larger_registration_bucket() {
    // Hosted Claude registers clients from 160.79.104.0/21 for every user.
    for ip in ["160.79.104.0", "160.79.107.12", "160.79.111.255"] {
        assert_eq!(
            security::rate_bucket(ip),
            ("anthropic".to_string(), 600),
            "{ip}"
        );
    }
    for ip in [
        "160.79.103.255",
        "160.79.112.0",
        "203.0.113.9",
        "2001:db8::1",
        "local",
    ] {
        let (key, limit) = security::rate_bucket(ip);
        assert_eq!(limit, 20, "{ip}");
        assert_eq!(key, format!("ip:{}", security::hash(ip)));
    }
}

#[test]
fn every_error_code_has_an_actionable_description() {
    let sources = std::fs::read_dir("src")
        .unwrap()
        .chain(std::fs::read_dir("src/mcp").unwrap())
        .chain(std::fs::read_dir("src/trakt").unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"));
    let mut codes = std::collections::BTreeSet::new();
    for path in sources {
        let text = std::fs::read_to_string(&path).unwrap();
        for constructor in ["ApiError::new(", "Self::new("] {
            for part in text.split(constructor).skip(1) {
                let arguments = part.split(')').next().unwrap();
                if let Some(code) = arguments.split('"').nth(1) {
                    codes.insert(code.to_string());
                }
            }
        }
    }
    assert!(codes.len() > 30, "error code scan found too few codes");
    for status in [400, 404, 409, 410, 418, 429, 500] {
        codes.insert(trakt_mcp::trakt::auth::device_status(status).to_string());
    }
    for code in &codes {
        let description = error::describe(code);
        assert_ne!(
            description,
            error::describe("not-a-real-code"),
            "{code} lacks a description"
        );
        assert!(description.ends_with('.'), "{code}");
    }
    let value = error::ApiError::new(400, "invalid_target").value();
    assert_eq!(value["error"], "invalid_target");
    assert!(
        value["error_description"]
            .as_str()
            .unwrap()
            .contains("resource")
    );
}

#[test]
fn every_tool_carries_complete_annotations() {
    for tool in tools() {
        let name = tool["name"].as_str().unwrap();
        let a = &tool["annotations"];
        assert_eq!(a["title"], tool["title"], "{name}");
        for hint in [
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        ] {
            assert!(a[hint].is_boolean(), "{name} {hint}");
        }
        if a["readOnlyHint"] == true {
            assert_eq!(a["destructiveHint"], false, "{name}");
            assert_eq!(a["idempotentHint"], true, "{name}");
        }
    }
}

#[test]
fn every_data_change_is_marked_destructive() {
    // Anthropic: destructiveHint is true for tools that modify or delete data.
    for name in [
        "trakt_create_list",
        "trakt_add_list_items",
        "trakt_remove_list_items",
        "trakt_api_write",
        "trakt_confirm_login",
    ] {
        assert_eq!(find(name)["annotations"]["destructiveHint"], true, "{name}");
    }
    for name in ["trakt_add_list_items", "trakt_remove_list_items"] {
        assert_eq!(find(name)["annotations"]["idempotentHint"], true, "{name}");
    }
    for name in [
        "trakt_create_list",
        "trakt_api_write",
        "trakt_request_login",
        "trakt_confirm_login",
    ] {
        assert_eq!(find(name)["annotations"]["idempotentHint"], false, "{name}");
    }
}

#[test]
fn tool_descriptions_describe_the_tool_instead_of_steering_the_model() {
    for tool in tools() {
        let name = tool["name"].as_str().unwrap();
        let description = tool["description"].as_str().unwrap();
        for phrase in [
            "BEFORE", "do not", "Do not", "never", "Never", "don't", "Don't",
        ] {
            assert!(!description.contains(phrase), "{name}: {phrase}");
        }
    }
    let write = find("trakt_api_write");
    let description = write["description"].as_str().unwrap();
    assert!(description.contains("Trakt API"));
    assert!(description.contains("https://docs.trakt.tv"));
}

#[test]
fn streamable_http_rejects_unsupported_protocol_version_headers() {
    // MCP transports spec: an invalid or unsupported MCP-Protocol-Version gets 400.
    for header in [
        None,
        Some("2025-06-18"),
        Some("2025-03-26"),
        Some("2024-11-05"),
    ] {
        assert!(protocol::supported_version_header(header), "{header:?}");
    }
    for header in ["2099-01-01", "", "latest"] {
        assert!(
            !protocol::supported_version_header(Some(header)),
            "{header}"
        );
    }
}

#[test]
fn tool_results_fit_claude_result_limits() {
    use trakt_mcp::mcp::output;
    // claude.ai accepts about 150,000 characters per result and Claude Code 25,000
    // tokens. The text and structured copies together must stay inside both.
    let small = output::success("trakt_search", serde_json::json!({"data":[1,2,3]}));
    assert_eq!(small["isError"], false);
    assert!(small["structuredContent"].is_object());

    let row = serde_json::json!({"title":"x".repeat(1000)});
    let fits = output::success(
        "trakt_search",
        serde_json::json!({"data":vec![row.clone(); 55]}),
    );
    assert_eq!(fits["isError"], false);
    assert!(fits["content"][0]["text"].as_str().unwrap().len() <= output::MAX_TEXT_CHARS);

    let large = output::success("trakt_search", serde_json::json!({"data":vec![row; 70]}));
    assert_eq!(large["isError"], true);
    assert!(large.get("structuredContent").is_none());
    let text = large["content"][0]["text"].as_str().unwrap();
    let error: Value = serde_json::from_str(text).unwrap();
    assert_eq!(error["error"], "tool_result_too_large");
    assert!(
        text.len() < 1000,
        "an oversized result must not leak partial data"
    );
    assert!(output::MAX_TEXT_CHARS * 2 < 150_000);
}
