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
    let query = json!({"media_type":{"type":"string","enum":["movie","show","movies","shows","all"]},"query":{"type":"string","minLength":1,"maxLength":500},"genres":{"type":"string","maxLength":200,"description":"Comma separated genre slugs."},"years":{"type":"string","pattern":"^[0-9]{4}(-[0-9]{4})?$","description":"A year or inclusive YYYY-YYYY range."},"limit":{"type":"integer","minimum":1,"maximum":100},"page":{"type":"integer","minimum":1,"maximum":4294967295u32}});
    let tool = |name: &str,
                title: &str,
                description: &str,
                properties: Value,
                required: Vec<&str>,
                read_only: bool,
                open_world: bool,
                destructive: bool| json!({"name":name,"title":title,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only,"destructiveHint":destructive,"openWorldHint":open_world}});
    let select = |keys: &[&str]| {
        Value::Object(
            keys.iter()
                .map(|k| ((*k).into(), query[*k].clone()))
                .collect(),
        )
    };
    let mut recommendation = select(&["media_type", "genres", "years", "limit"]);
    recommendation["media_type"]["enum"] = json!(["movie", "show", "movies", "shows"]);
    let mut watched = select(&["media_type", "page", "limit"]);
    watched["page"]["default"] = json!(1);
    watched["limit"]["default"] = json!(100);
    watched["mode"] = json!({"type":"string","enum":["all","recent"],"default":"all"});
    watched["detail"] = json!({"type":"string","enum":["compact","full"],"default":"compact"});
    json!({"tools":[
      tool("trakt_request_login","Connect your Trakt account","Reconnect your own Trakt account. Displays the activation URL and explicit user code. A connection is normally created during browser OAuth login.",json!({}),vec![],false,true,false),
      tool("trakt_confirm_login","Complete Trakt connection","Confirm a device code after the user activates it. Respect the returned polling interval. Trakt tokens stay private. This replaces the current connection authorization on success.",json!({"device_code":{"type":"string","minLength":1,"maxLength":512}}),vec!["device_code"],false,true,true),
      tool("trakt_get_watched_history","Read watched movies and history","Read one page (default 100). mode=all returns watched summaries, not play events. detail=compact (default) keeps identifiers, genres, dates and play counts for reliable traversal; detail=full includes all upstream metadata. For ALL watched movies or recommendations based on full viewing history, follow pagination.next_page until has_more=false BEFORE recommending; do not stop at 100. Use each media type separately when traversing movies and shows. mode=recent returns chronological watch events newest first, including repeat watches; collect only the latest 100 events unless another count is requested. Follow actual pagination limits; missing/failed pages mean incomplete coverage.",watched,vec![],true,false,false),
      tool("trakt_get_recommendations","Get personalized recommendations","Get Trakt's personalized movie/show recommendations based on its viewing/preferences signals, filtered by genre and year. Select movie or show; limit 1-100.",recommendation,vec![],true,true,false),
      tool("trakt_search","Find a movie or show","Search movies/shows. Genre/year filters apply to the returned upstream page; pagination metadata describes the unfiltered search. For explicitly requested all results, follow pagination.next_page through every page, even if local filters empty a page; otherwise fetch only relevant pages.",query,vec!["query"],true,true,false)
    ]})
}
