use super::{output, spec};
use serde_json::{Value, json};
pub fn validate(v: &Value) -> std::result::Result<(), &'static str> {
    if !v.is_object()
        || v["jsonrpc"] != "2.0"
        || !v["method"].is_string()
        || v.get("id")
            .is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64())
        || v.get("params").is_some_and(|p| !p.is_object())
    {
        return Err("Invalid Request");
    }
    Ok(())
}
/// Protocol revisions this server negotiates, newest first.
pub const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// A missing header means the client predates it; any other value must be one we speak.
pub fn supported_version_header(header: Option<&str>) -> bool {
    header.is_none_or(|v| VERSIONS.contains(&v))
}
pub fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
pub fn tools() -> Value {
    let tools: Vec<Value> = spec::tools()
        .iter()
        .map(|tool| {
            let mut annotations = tool.annotations.clone();
            annotations["title"] = json!(tool.title);
            json!({"name":tool.name,"title":tool.title,"description":tool.description,
                "inputSchema":tool.input_schema(),"outputSchema":output::schema(&tool.name),
                "annotations":annotations})
        })
        .collect();
    json!({"tools":tools})
}
