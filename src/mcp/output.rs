use crate::error::ApiError;
use serde_json::{Value, json};

/// Largest JSON text a result may carry. With the structured copy the result stays
/// under claude.ai's ~150,000-character limit, and the text alone stays under Claude
/// Code's default 25,000-token limit.
pub const MAX_TEXT_CHARS: usize = 60_000;

/// Keep text results usable by existing clients while exposing typed data. A result
/// over the limit fails explicitly instead of being cut, so partial data can never
/// look like a complete page.
pub fn success(tool: &str, data: Value) -> Value {
    let text = data.to_string();
    if text.len() > MAX_TEXT_CHARS {
        return failure(&ApiError::new(413, "tool_result_too_large"));
    }
    json!({"content":[{"type":"text","text":text}],"structuredContent":{
        "schema_version":"1","tool":tool,"result":data},"isError":false})
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
    let result = match tool {
        "trakt_list_operations" => paged.clone(),
        "trakt_get_operation" => json!({"type":"object","required":["operation_id"],
            "properties":{"operation_id":{"type":"string"}},"additionalProperties":true}),
        "trakt_api_read"
        | "trakt_api_write"
        | "trakt_discover_lists"
        | "trakt_get_list_items"
        | "trakt_get_calendar"
        | "trakt_create_list"
        | "trakt_add_list_items"
        | "trakt_remove_list_items" => generic,
        "trakt_request_login" => {
            json!({"type":"object","required":["device_code","user_code","verification_url","expires_in","interval","instructions"],
            "properties":{"device_code":{"type":"string"},"user_code":{"type":"string"},
            "verification_url":{"type":"string"},"expires_in":{"type":"integer"},
            "interval":{"type":"integer"},"instructions":{"type":"string"}},"additionalProperties":false})
        }
        "trakt_confirm_login" => json!({"type":"object","required":["status"],
            "properties":{"status":{"const":"connected"}},"additionalProperties":false}),
        "trakt_get_watched_history" => json!({"anyOf":[paged.clone(),
            {"type":"object","required":["movies","shows"],
            "properties":{"movies":paged.clone(),"shows":paged.clone()},"additionalProperties":false}]}),
        "trakt_get_recommendations" => paged.clone(),
        "trakt_search" => {
            json!({"type":"object","required":["data","pagination","filters_applied_to_page"],
            "properties":{"data":{},"pagination":{},"filters_applied_to_page":{"type":"boolean"}},
            "additionalProperties":false})
        }
        _ => panic!("Output schema requested for unregistered tool: {tool}"),
    };
    json!({"type":"object","required":["schema_version","tool","result"],
        "properties":{"schema_version":{"const":"1"},"tool":{"const":tool},"result":result},
        "additionalProperties":false})
}
