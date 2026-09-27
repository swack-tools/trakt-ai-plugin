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
pub fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
pub fn tools() -> Value {
    let query = json!({"media_type":{"type":"string","enum":["movie","show","movies","shows","all"]},"query":{"type":"string","minLength":1,"maxLength":500},"genres":{"type":"string","description":"Comma separated genre slugs."},"years":{"type":"string","description":"A year or inclusive YYYY-YYYY range."},"limit":{"type":"integer","minimum":1,"maximum":100},"page":{"type":"integer","minimum":1,"maximum":10000}});
    let tool = |name: &str,
                description: &str,
                properties: Value,
                required: Vec<&str>,
                read_only: bool| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only,"destructiveHint":false,"openWorldHint":true}});
    let select = |keys: &[&str]| {
        Value::Object(
            keys.iter()
                .map(|k| ((*k).into(), query[*k].clone()))
                .collect(),
        )
    };
    json!({"tools":[
      tool("trakt_request_login","Reconnect your own Trakt account. Displays the activation URL and explicit user code. A connection is normally created during browser OAuth login.",json!({}),vec![],false),
      tool("trakt_confirm_login","Confirm a device code after the user activates it. Respect the returned polling interval. Trakt tokens stay private.",json!({"device_code":{"type":"string","minLength":1,"maxLength":512}}),vec!["device_code"],false),
      tool("trakt_get_watched_history","Read watched movies/shows, including full metadata such as genres and release dates. This is the watched summary, not individual play events.",select(&["media_type"]),vec![],true),
      tool("trakt_get_recommendations","Get Trakt's personalized movie/show recommendations based on its viewing/preferences signals, filtered by genre and year. Select movie or show; limit 1-100.",select(&["media_type","genres","years","limit"]),vec![],true),
      tool("trakt_search","Search movies/shows. Genre/year filters apply to the returned upstream page; pagination metadata describes the unfiltered search.",query,vec!["query"],true)
    ]})
}
