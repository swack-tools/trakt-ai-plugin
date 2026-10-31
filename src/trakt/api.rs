use super::{
    auth::Conn,
    catalog::{self, Call},
    client::{Client, response_pagination, upstream_error},
};
use crate::{
    error::{ApiError, Result},
    grant, oauth,
};
use serde_json::{Value, json};
use std::time::Duration;
use worker::{Delay, Method, Storage};

/// Wait out the connected user's write pacing. `op` is read, reserve, or mark.
pub async fn pace(c: &Client<'_>, conn: &Conn, op: &str) -> Result<()> {
    let Some(grant_id) = &conn.grant else {
        return Ok(());
    };
    let reply = grant::request(c.env, grant_id, "/_grant/pace", json!({"op":op})).await?;
    if let Some(wait) = reply["wait_ms"].as_u64().filter(|wait| *wait > 0) {
        Delay::from(Duration::from_millis(wait)).await;
    }
    Ok(())
}

/// A pacing failure must not turn an applied write into an apparent failure.
async fn pace_after_write(c: &Client<'_>, conn: &Conn) {
    if let Some(grant_id) = &conn.grant {
        let _ = grant::request(c.env, grant_id, "/_grant/pace", json!({"op":"mark"})).await;
    }
}

pub async fn execute_call(
    c: &Client<'_>,
    storage: &mut Storage,
    conn: &Conn,
    call: &Call,
    write: bool,
) -> Result<Value> {
    let op = catalog::find(&call.operation_id)?;
    let request = catalog::prepare(op, call, write)?;
    if write {
        oauth::require_write(storage).await?;
    }
    // Even public reads require a fully connected session; never accept a
    // user-supplied token or allow temporary device credentials to call the API.
    let token = super::auth::access_token(c, conn).await?;
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
    pace(c, conn, if write { "reserve" } else { "read" }).await?;
    let sent = c
        .send(
            &request.path,
            method,
            &query,
            Some(&token),
            request.body.as_ref(),
        )
        .await;
    if write {
        // The next request waits a second from when this write finished.
        pace_after_write(c, conn).await;
    }
    let r = sent?;
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
