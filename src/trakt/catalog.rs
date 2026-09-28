//! Explicit operation allowlist compiled from the public Trakt API reference.
//! Neither remote content nor caller input can supply URLs, headers or schemas.
use crate::error::{ApiError, Result};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

pub fn catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../api/trakt/catalog.json"))
            .expect("the bundled catalog is validated in CI")
    })
}
pub fn operations() -> &'static [Value] {
    catalog()["operations"]
        .as_array()
        .expect("catalog operations")
        .as_slice()
}
pub fn find(id: &str) -> Result<&'static Value> {
    operations()
        .iter()
        .find(|op| op["operation_id"] == id)
        .ok_or(ApiError::new(400, "unknown_operation"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    query: Option<String>,
    category: Option<String>,
    page: Option<u32>,
    limit: Option<u32>,
}
pub fn discover(args: Value) -> Result<Value> {
    let q: Discovery =
        serde_json::from_value(args).map_err(|_| ApiError::new(400, "invalid_api_parameters"))?;
    if q.query.as_ref().is_some_and(|s| s.len() > 200)
        || q.category.as_ref().is_some_and(|s| s.len() > 100)
    {
        return Err(ApiError::new(400, "invalid_api_parameters"));
    }
    let (page, limit) = (q.page.unwrap_or(1), q.limit.unwrap_or(25));
    if page == 0 || limit == 0 || limit > 100 {
        return Err(ApiError::new(400, "invalid_pagination"));
    }
    let rows: Vec<_> = operations()
        .iter()
        .filter(|op| {
            let text = format!(
                "{} {} {} {}",
                op["operation_id"], op["summary"], op["path"], op["categories"]
            )
            .to_lowercase();
            q.query.as_ref().is_none_or(|s| {
                s.to_lowercase()
                    .split_whitespace()
                    .all(|word| text.contains(word))
            }) && q.category.as_ref().is_none_or(|category| {
                op["categories"].as_array().is_some_and(|categories| {
                    categories
                        .iter()
                        .any(|c| c.as_str().is_some_and(|s| s.eq_ignore_ascii_case(category)))
                })
            })
        })
        .collect();
    let count = rows.len();
    let pages = count.div_ceil(limit as usize);
    let skip = (page as u64 - 1) * limit as u64;
    let data: Vec<_> = rows
        .into_iter()
        .skip(usize::try_from(skip).unwrap_or(usize::MAX))
        .take(limit as usize)
        .map(|op| {
            json!({"operation_id":op["operation_id"],"summary":op["summary"],
            "method":op["method"],"path":op["path"],"categories":op["categories"],
            "status":op["status"],"status_reason":op["status_reason"],
            "tool":if op["method"]=="GET" {"trakt_api_read"} else {"trakt_api_write"}})
        })
        .collect();
    Ok(
        json!({"data":data,"pagination":{"page":page,"limit":limit,"item_count":count,
        "page_count":pages,"has_more":(page as usize)<pages,
        "next_page":if (page as usize)<pages {page.checked_add(1)} else {None}}}),
    )
}
pub fn describe(args: Value) -> Result<Value> {
    if args.as_object().is_none_or(|a| a.len() != 1) {
        return Err(ApiError::new(400, "invalid_api_parameters"));
    }
    let id = args["operation_id"]
        .as_str()
        .ok_or(ApiError::new(400, "invalid_api_parameters"))?;
    let mut op = find(id)?.clone();
    op["input_schema"] = input_schema(&op);
    op["tool"] = json!(if op["method"] == "GET" {
        "trakt_api_read"
    } else {
        "trakt_api_write"
    });
    op["write_scope_required"] = json!(op["method"] != "GET");
    Ok(op)
}
/// The catalog uses JSON Schema, with OpenAPI refs and nullable normalized at generation.
pub fn input_schema(op: &Value) -> Value {
    let mut groups = Map::new();
    for (place, group) in [("path", "path_params"), ("query", "query_params")] {
        let mut properties = Map::new();
        let mut required = Vec::new();
        if let Some(parameters) = op["parameters"].as_array() {
            for p in parameters.iter().filter(|p| p["in"] == place) {
                if let Some(name) = p["name"].as_str() {
                    properties.insert(name.into(), p["schema"].clone());
                    if place == "path" || p["required"] == true {
                        required.push(name);
                    }
                }
            }
        }
        groups.insert(
            group.into(),
            json!({"type":"object","properties":properties,
            "required":required,"additionalProperties":false}),
        );
    }
    let body = &op["request_body"];
    if !body.is_null() {
        groups.insert("body".into(), body["schema"].clone());
    }
    let mut required = vec!["path_params", "query_params"];
    if body["required"] == true {
        required.push("body");
    }
    json!({"type":"object","properties":groups,"required":required,"additionalProperties":false})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub operation_id: String,
    #[serde(default)]
    pub path_params: Map<String, Value>,
    #[serde(default)]
    pub query_params: Map<String, Value>,
    #[serde(default, deserialize_with = "present_body")]
    pub body: Option<Value>,
    pub confirmed: Option<bool>,
}
fn present_body<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
pub struct Prepared {
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<Value>,
}
fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        // Query arrays use the Trakt comma-separated representation.
        Value::Array(values) => values
            .iter()
            .map(scalar)
            .collect::<Result<Vec<_>>>()
            .map(|parts| parts.join(",")),
        _ => Err(ApiError::new(400, "invalid_api_parameters")),
    }
}
pub fn prepare(op: &Value, call: &Call, write: bool) -> Result<Prepared> {
    if op["status"] != "supported" {
        return Err(ApiError::new(400, "operation_unavailable"));
    }
    if write && op["method"] == "GET" {
        return Err(ApiError::new(400, "operation_requires_read_tool"));
    }
    if !write && op["method"] != "GET" {
        return Err(ApiError::new(400, "operation_requires_write_tool"));
    }
    if write && call.confirmed != Some(true) {
        return Err(ApiError::new(400, "confirmation_required"));
    }
    if !write && call.confirmed.is_some() {
        return Err(ApiError::new(400, "invalid_api_parameters"));
    }
    let mut query = call.query_params.clone();
    // OpenAPI uses nullable for optional filters. Null means leave that filter
    // unset; keep unknown keys so strict schema validation still rejects them.
    for parameter in op["parameters"].as_array().into_iter().flatten() {
        if parameter["in"] == "query"
            && parameter["required"] != true
            && let Some(name) = parameter["name"].as_str()
            && query.get(name).is_some_and(Value::is_null)
        {
            query.remove(name);
        }
    }
    if op["pagination"]["supported"] == true {
        // Stable explicit limits avoid upstream default truncation. Never fetch
        // successive pages within one invocation or retry a write automatically.
        query.entry("page").or_insert(json!(1));
        query.entry("limit").or_insert(json!(100));
        if query["page"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > u32::MAX as u64)
            || query["limit"].as_u64().is_none_or(|n| n == 0 || n > 100)
        {
            return Err(ApiError::new(400, "invalid_pagination"));
        }
    }
    let mut input = json!({"path_params":call.path_params,"query_params":query});
    let body = call.body.clone();
    if let Some(body) = &body {
        input["body"] = body.clone();
    }
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&input_schema(op))
        .map_err(|_| ApiError::new(500, "invalid_operation_schema"))?;
    if !validator.is_valid(&input) {
        return Err(ApiError::new(400, "invalid_api_parameters"));
    }
    let mut path = op["path"]
        .as_str()
        .ok_or(ApiError::new(500, "invalid_operation_schema"))?
        .to_string();
    for (name, value) in &call.path_params {
        let value = scalar(value)?;
        // Restrict each value to one harmless path segment. Percent-encoded
        // separators, traversal, control bytes and URL syntax are not accepted.
        if value.is_empty()
            || value.len() > 256
            || matches!(value.as_str(), "." | "..")
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:,".contains(&c))
        {
            return Err(ApiError::new(400, "invalid_api_parameters"));
        }
        path = path.replace(&format!("{{{name}}}"), &value);
    }
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['{', '}', '?', '#'])
        || path.starts_with("/oauth/")
        || path.contains("/../")
    {
        return Err(ApiError::new(400, "invalid_api_parameters"));
    }
    let query = query
        .iter()
        .map(|(k, v)| scalar(v).map(|v| (k.clone(), v)))
        .collect::<Result<_>>()?;
    Ok(Prepared { path, query, body })
}
