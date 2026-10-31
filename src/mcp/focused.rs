//! Turn one tool's validated inputs into its mapped catalog call.
use super::spec::{self, Kind, Tool};
use crate::{
    error::{ApiError, Result},
    trakt::catalog::{self, Call},
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

fn invalid() -> ApiError {
    ApiError::new(400, "invalid_parameters")
}

/// Group media items as the Trakt sync and list bodies expect them.
fn media_items(items: &[Value], body: &mut Map<String, Value>) -> Result<()> {
    let mut seen = HashSet::new();
    for item in items {
        let media = item["media_type"].as_str().ok_or_else(invalid)?;
        let id = item["trakt_id"].as_u64().ok_or_else(invalid)?;
        if !seen.insert((media, id)) {
            return Err(invalid());
        }
        let key = match media {
            "movie" => "movies",
            "show" => "shows",
            "season" => "seasons",
            "episode" => "episodes",
            _ => return Err(invalid()),
        };
        let mut encoded = item.as_object().cloned().ok_or_else(invalid)?;
        encoded.remove("media_type");
        encoded.remove("trakt_id");
        encoded.insert("ids".into(), json!({"trakt":id}));
        body.entry(key)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(invalid)?
            .push(Value::Object(encoded));
    }
    Ok(())
}

type Selected<'a> = (&'a str, Map<String, Value>, Map<String, Value>);

fn selected<'a>(tool: &'a Tool, args: &Map<String, Value>) -> Result<Selected<'a>> {
    if let Some(id) = &tool.operation_id {
        return Ok((id, Map::new(), Map::new()));
    }
    let operations = tool.operations.as_ref().ok_or_else(invalid)?;
    let choice = args
        .get(&operations.selector)
        .and_then(Value::as_str)
        .or(operations.default.as_deref())
        .ok_or_else(invalid)?;
    let (id, path, query) = spec::mapped(operations.map.get(choice).ok_or_else(invalid)?);
    Ok((id.ok_or_else(invalid)?, path, query))
}

pub fn prepare(name: &str, args: Value) -> Result<Call> {
    let tool = spec::find(name).ok_or(ApiError::new(404, "unknown_tool"))?;
    let write = tool.kind == Kind::Write;
    if write && args["confirmed"] != json!(true) {
        return Err(ApiError::new(400, "confirmation_required"));
    }
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&tool.input_schema())
        .map_err(|_| ApiError::new(500, "invalid_operation_schema"))?;
    if !validator.is_valid(&args) {
        return Err(invalid());
    }
    let args = args.as_object().ok_or_else(invalid)?;
    let (operation_id, mut path, mut query) = selected(tool, args)?;
    let op = catalog::find(operation_id)?;
    let selector = tool.operations.as_ref().map(|o| o.selector.as_str());
    let declared = |place: &str, name: &str| {
        op["parameters"]
            .as_array()
            .is_some_and(|all| all.iter().any(|p| p["in"] == place && p["name"] == name))
    };
    let mut body = Map::new();
    for (input, value) in args {
        if input == "confirmed" || Some(input.as_str()) == selector {
            continue;
        }
        let target = tool
            .bind
            .get(input)
            .and_then(Value::as_str)
            .unwrap_or(input);
        if input == "items" && tool.body.items {
            media_items(value.as_array().ok_or_else(invalid)?, &mut body)?;
        } else if tool.body.fields.contains(input) {
            let value = match (input.as_str(), value.as_str()) {
                ("name", Some(text)) => json!(text.trim()),
                _ => value.clone(),
            };
            body.insert(target.into(), value);
        } else if declared("path", target) {
            path.insert(target.into(), value.clone());
        } else if declared("query", target) {
            query.insert(target.into(), value.clone());
        } else {
            return Err(invalid());
        }
    }
    for (key, value) in &tool.fixed.path {
        path.insert(key.clone(), value.clone());
    }
    for (key, value) in &tool.fixed.query {
        query.insert(key.clone(), value.clone());
    }
    for (key, value) in &tool.defaults {
        if tool.body.fields.contains(key) {
            body.entry(key.clone()).or_insert_with(|| value.clone());
        } else if declared("path", key) {
            path.entry(key.clone()).or_insert_with(|| value.clone());
        } else if declared("query", key) {
            query.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    let sends_body = tool.body.items || !tool.body.fields.is_empty();
    if sends_body && body.is_empty() {
        return Err(invalid());
    }
    let mut call = json!({"operation_id":operation_id,"path_params":path,"query_params":query});
    if sends_body {
        call["body"] = Value::Object(body);
    }
    if write {
        call["confirmed"] = json!(true);
    }
    serde_json::from_value(call).map_err(|_| invalid())
}
