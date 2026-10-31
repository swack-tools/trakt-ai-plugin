use super::spec;
use crate::error::ApiError;
use serde_json::{Value, json};

/// Largest JSON text a result may carry. With the structured copy the result stays
/// under claude.ai's ~150,000-character limit, and the text alone stays under Claude
/// Code's default 25,000-token limit.
pub const MAX_TEXT_CHARS: usize = 60_000;

/// Keep text results usable by existing clients while exposing typed data. A result
/// over the limit fails explicitly instead of being cut, so partial data can never
/// look like a complete page. A write has already reached Trakt by this point, so its
/// outcome names the operation and upstream status and says not to repeat it.
pub fn success(tool: &str, data: Value) -> Value {
    let text = data.to_string();
    if text.len() > MAX_TEXT_CHARS {
        if changes_data(tool) {
            let mut outcome = ApiError::new(413, "write_result_too_large").value();
            outcome["operation_id"] = data["operation_id"].clone();
            outcome["status"] = data["status"].clone();
            return json!({"content":[{"type":"text","text":outcome.to_string()}],"isError":true});
        }
        return failure(&ApiError::new(413, "tool_result_too_large"));
    }
    json!({"content":[{"type":"text","text":text}],"structuredContent":{
        "schema_version":"1","tool":tool,"result":data},"isError":false})
}

fn changes_data(tool: &str) -> bool {
    spec::find(tool).is_some_and(|tool| tool.kind == spec::Kind::Write)
}

pub fn failure(error: &ApiError) -> Value {
    json!({"content":[{"type":"text","text":error.value().to_string()}],"isError":true})
}

pub fn schema(tool: &str) -> Value {
    let paged = json!({"type":"object","required":["data","pagination"],
        "properties":{"data":{},"pagination":{}},"additionalProperties":true});
    let generic = json!({"type":"object","required":["operation_id","status","data","pagination"],
        "properties":{"operation_id":{"type":"string"},"status":{"type":"integer"},
        "data":{},"pagination":{}},"additionalProperties":true});
    let output = spec::find(tool)
        .unwrap_or_else(|| panic!("Output schema requested for unregistered tool: {tool}"))
        .output;
    let result = match output {
        spec::Output::Operation => generic,
        spec::Output::LoginRequest => {
            json!({"type":"object","required":["device_code","user_code","verification_url","expires_in","interval","instructions"],
            "properties":{"device_code":{"type":"string"},"user_code":{"type":"string"},
            "verification_url":{"type":"string"},"expires_in":{"type":"integer"},
            "interval":{"type":"integer"},"instructions":{"type":"string"}},"additionalProperties":false})
        }
        spec::Output::LoginConfirm => json!({"type":"object","required":["status"],
            "properties":{"status":{"const":"connected"}},"additionalProperties":false}),
        spec::Output::Watched => json!({"anyOf":[paged.clone(),
            {"type":"object","required":["movies","shows"],
            "properties":{"movies":paged.clone(),"shows":paged.clone()},"additionalProperties":false}]}),
        spec::Output::Page => paged.clone(),
        spec::Output::Search => {
            json!({"type":"object","required":["data","pagination","filters_applied_to_page"],
            "properties":{"data":{},"pagination":{},"filters_applied_to_page":{"type":"boolean"}},
            "additionalProperties":false})
        }
    };
    json!({"type":"object","required":["schema_version","tool","result"],
        "properties":{"schema_version":{"const":"1"},"tool":{"const":tool},"result":result},
        "additionalProperties":false})
}
