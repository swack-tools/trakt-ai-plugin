//! The reviewed tool contract in api/trakt/tools.json. Tools expose fixed inputs and
//! run only their mapped catalog operations; callers never choose a path or method.
use crate::trakt::catalog;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

pub const SOURCE: &str = include_str!("../../api/trakt/tools.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub kind: Kind,
    #[serde(default)]
    pub handler: Handler,
    pub operation_id: Option<String>,
    pub operations: Option<Operations>,
    #[serde(default)]
    pub fixed: Fixed,
    #[serde(default)]
    pub bind: Map<String, Value>,
    #[serde(default)]
    pub defaults: Map<String, Value>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub ownership: bool,
    pub inputs: Map<String, Value>,
    pub required: Vec<String>,
    pub annotations: Value,
    pub output: Output,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Read,
    Write,
    Login,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Handler {
    #[default]
    Catalog,
    Search,
    Watched,
    Recommendations,
    LoginRequest,
    LoginConfirm,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    Operation,
    Page,
    Search,
    Watched,
    LoginRequest,
    LoginConfirm,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operations {
    pub selector: String,
    pub default: Option<String>,
    pub map: Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Fixed {
    #[serde(default)]
    pub path: Map<String, Value>,
    #[serde(default)]
    pub query: Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Body {
    #[serde(default)]
    pub items: bool,
    #[serde(default)]
    pub fields: Vec<String>,
}

pub fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let document: Value = serde_json::from_str(SOURCE).expect("the tool spec is valid JSON");
        serde_json::from_value(document["tools"].clone()).expect("the tool spec is validated in CI")
    })
}

pub fn find(name: &str) -> Option<&'static Tool> {
    tools().iter().find(|tool| tool.name == name)
}

impl Tool {
    pub fn input_schema(&self) -> Value {
        json!({"type":"object","properties":self.inputs,"required":self.required,
            "additionalProperties":false})
    }
}

/// One mapped operation plus the path and query values it fixes.
pub fn mapped(entry: &Value) -> (Option<&str>, Map<String, Value>, Map<String, Value>) {
    if let Some(id) = entry.as_str() {
        return (Some(id), Map::new(), Map::new());
    }
    let group = |key: &str| entry[key].as_object().cloned().unwrap_or_default();
    (
        entry["operation_id"].as_str(),
        group("path"),
        group("query"),
    )
}

fn operation_ids(tool: &Value) -> Vec<String> {
    let mut ids: Vec<String> = tool["operation_id"]
        .as_str()
        .map(String::from)
        .into_iter()
        .collect();
    if let Some(map) = tool["operations"]["map"].as_object() {
        ids.extend(
            map.values()
                .filter_map(|entry| mapped(entry).0.map(String::from)),
        );
    }
    ids
}

/// A nullable or empty branch would let a caller send an unconstrained value.
fn loose(schema: &Value) -> bool {
    match schema {
        Value::Object(object) => {
            object.is_empty()
                || object.get("type") == Some(&json!("null"))
                || object.get("type").is_some_and(Value::is_array)
                || object.get("enum").is_some_and(|values| {
                    values
                        .as_array()
                        .is_some_and(|values| values.iter().any(Value::is_null))
                })
                || object.iter().any(|(key, value)| {
                    !matches!(key.as_str(), "properties" | "const" | "default") && loose(value)
                })
                || object
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|properties| properties.values().any(loose))
        }
        Value::Array(values) => values.iter().any(loose),
        _ => false,
    }
}

/// Every defect the reviewed spec must not contain. Empty means the spec is sound.
pub fn problems(document: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let empty = Vec::new();
    for tool in document["tools"].as_array().unwrap_or(&empty) {
        let name = tool["name"].as_str().unwrap_or("<unnamed>");
        let mut report = |problem: String| found.push(format!("{name}: {problem}"));
        let kind = tool["kind"].as_str().unwrap_or("");
        let annotations = &tool["annotations"];
        for hint in [
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        ] {
            if !annotations[hint].is_boolean() {
                report(format!("missing annotation {hint}"));
            }
        }
        if tool["title"].as_str().is_none_or(str::is_empty) {
            report("missing title".into());
        }
        let is_read = kind == "read";
        if (kind == "read" || kind == "write")
            && (annotations["readOnlyHint"] != json!(is_read)
                || annotations["destructiveHint"] != json!(!is_read))
        {
            report("annotations contradict the tool's effect".into());
        }
        let inputs = &tool["inputs"];
        if kind == "write"
            && (inputs["confirmed"]["const"] != json!(true)
                || !tool["required"]
                    .as_array()
                    .is_some_and(|required| required.contains(&json!("confirmed"))))
        {
            report("write lacks a required confirmed const true".into());
        }
        if kind != "write" && !inputs["confirmed"].is_null() {
            report("only a write takes confirmed".into());
        }
        for forbidden in [
            "path",
            "method",
            "url",
            "headers",
            "operation_id",
            "path_params",
            "query_params",
            "body",
        ] {
            if !inputs[forbidden].is_null() {
                report(format!("free-form input {forbidden}"));
            }
        }
        if inputs
            .as_object()
            .is_some_and(|inputs| inputs.values().any(loose))
        {
            report("input accepts null or an unconstrained value".into());
        }
        let ids = operation_ids(tool);
        if kind == "login" {
            if !ids.is_empty() {
                report("login uses managed authentication, not catalog operations".into());
            }
            continue;
        }
        if ids.is_empty() {
            report("no mapped operation".into());
        }
        for id in ids {
            match catalog::find(&id) {
                Err(_) => report(format!("maps to missing operation {id}")),
                Ok(op) => {
                    if op["status"] != "supported" {
                        report(format!("maps to non-supported operation {id}"));
                    }
                    if (op["method"] == "GET") != is_read {
                        report(format!("{id} method contradicts the tool kind"));
                    }
                }
            }
        }
    }
    found
}
