use super::{
    catalog::{self, Call},
    client::{Client, response_pagination, upstream_error},
};
use crate::{
    error::{ApiError, Result},
    oauth,
};
use serde_json::{Value, json};
use worker::{Method, Storage};

pub async fn execute(
    c: &Client<'_>,
    storage: &mut Storage,
    session_id: &str,
    args: Value,
    write: bool,
) -> Result<Value> {
    let call: Call =
        serde_json::from_value(args).map_err(|_| ApiError::new(400, "invalid_api_parameters"))?;
    let op = catalog::find(&call.operation_id)?;
    let request = catalog::prepare(op, &call, write)?;
    if write {
        oauth::require_write(storage).await?;
    }
    // Even public reads require a fully connected session; never accept a
    // user-supplied token or allow temporary device credentials to call the API.
    let token = super::auth::access_token(c, storage, session_id).await?;
    let method = match op["method"].as_str() {
        Some("GET") => Method::Get,
        Some("POST") => Method::Post,
        Some("PUT") => Method::Put,
        Some("PATCH") => Method::Patch,
        Some("DELETE") => Method::Delete,
        _ => return Err(ApiError::new(400, "operation_unavailable")),
    };
    let query: Vec<_> = request
        .query
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    let r = c
        .send(
            &request.path,
            method,
            &query,
            Some(&token),
            request.body.as_ref(),
        )
        .await?;
    if !(200..300).contains(&r.status) {
        return Err(upstream_error(&r));
    }
    let requested = request
        .query
        .iter()
        .find(|(k, _)| k == "page")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(1);
    let length = r
        .data
        .as_array()
        .map(Vec::len)
        .or_else(|| r.data.get("data").and_then(Value::as_array).map(Vec::len))
        .or_else(|| r.data.as_object().map(|o| o.len()))
        .unwrap_or(0);
    // Unpaginated operations (especially completed writes) must not turn a
    // successful mutation into an apparent failure due to unrelated headers.
    let pagination = if !write && op["pagination"]["supported"] == true {
        response_pagination(&r.headers, requested, length)?
    } else {
        Value::Null
    };
    Ok(
        json!({"operation_id":call.operation_id,"status":r.status,"data":r.data,"pagination":pagination}),
    )
}
