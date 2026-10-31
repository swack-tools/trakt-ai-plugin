//! One encrypted Trakt grant per Trakt user, held in that user's Durable Object.
//! The object's request lock serializes single-use Trakt refreshes.
use crate::{
    error::{ApiError, Result},
    retention, seal,
    trakt::{
        auth::{self, Tokens, now},
        client::{Client, upstream_error},
    },
};
use serde_json::{Value, json};
use worker::{Env, Method, State, Storage};

const GRANT: &str = "grant";
const CONNECTIONS: &str = "connections";
const PACED_UNTIL: &str = "paced_until";
const BACKOFF_UNTIL: &str = "backoff_until";
/// Trakt's authenticated POST, PUT, and DELETE limit is one per second per user.
const PACE_MILLIS: u64 = 1000;
/// Used when a 429 carries no Retry-After, which Trakt's edge limiting can omit.
pub const DEFAULT_BACKOFF_SECONDS: u64 = 60;

fn wire(e: ApiError) -> Value {
    json!({"error":e.code,"status":e.status,"retry_after":e.retry_after})
}

fn key_from(v: &Value) -> Option<seal::Key> {
    seal::from_hex(v["k"].as_str()?)?.try_into().ok()
}

async fn open(storage: &Storage, id: &str, v: &Value) -> Result<Tokens> {
    let sealed: String = storage
        .get(GRANT)
        .await?
        .ok_or(ApiError::new(401, "trakt_login_required"))?;
    let key = key_from(v).ok_or(ApiError::new(401, "connection_key_invalid"))?;
    let plain =
        seal::open_grant(&key, id, &sealed).ok_or(ApiError::new(401, "connection_key_invalid"))?;
    serde_json::from_str(&plain).map_err(|_| ApiError::new(500, "invalid_trakt_token"))
}

async fn connections(storage: &Storage) -> Result<Vec<String>> {
    Ok(storage.get(CONNECTIONS).await?.unwrap_or_default())
}

async fn add_connection(storage: &Storage, connection: &str) -> Result<()> {
    let mut all = connections(storage).await?;
    if !all.iter().any(|c| c == connection) {
        all.push(connection.into());
        storage.put(CONNECTIONS, all).await?;
    }
    Ok(())
}

/// Refresh only near expiry. A 429 means the single-use token was not consumed, so
/// the stored grant stays valid; the caller gets the wait and nothing retries here.
async fn access(env: &Env, storage: &Storage, id: &str, v: &Value) -> Result<Value> {
    let tokens = open(storage, id, v).await?;
    if tokens.created_at.saturating_add(tokens.expires_in) > now() + 60 {
        return Ok(json!({"access_token":tokens.access_token}));
    }
    if let Some(until) = storage.get::<u64>(BACKOFF_UNTIL).await?
        && until > now()
    {
        let mut e = ApiError::new(429, "trakt_rate_limited");
        e.retry_after = Some(until - now());
        return Err(e);
    }
    let c = Client { env };
    let body = json!({"refresh_token":tokens.refresh_token,"client_id":env.secret("TRAKT_CLIENT_ID")?.to_string(),"client_secret":env.secret("TRAKT_CLIENT_SECRET")?.to_string(),"redirect_uri":env.var("TRAKT_REDIRECT_URI")?.to_string(),"grant_type":"refresh_token"});
    let r = c
        .send("/oauth/token", Method::Post, &[], None, Some(&body))
        .await?;
    if r.status == 429 {
        let e = upstream_error(&r);
        let wait = e.retry_after.unwrap_or(DEFAULT_BACKOFF_SECONDS);
        storage.put(BACKOFF_UNTIL, now() + wait).await?;
        return Err(e);
    }
    if r.status != 200 {
        if matches!(r.status, 400 | 401) {
            return Err(ApiError::new(401, "trakt_login_required"));
        }
        return Err(upstream_error(&r));
    }
    let fresh: Tokens =
        serde_json::from_value(r.data).map_err(|_| ApiError::new(502, "invalid_trakt_token"))?;
    auth::check(&fresh)?;
    let key = key_from(v).ok_or(ApiError::new(401, "connection_key_invalid"))?;
    let plain = serde_json::to_string(&fresh)?;
    storage
        .put(GRANT, seal::seal_grant(&key, id, &plain)?)
        .await?;
    storage.delete(BACKOFF_UNTIL).await?;
    Ok(json!({"access_token":fresh.access_token}))
}

/// Trakt is told first, but its refusal or a rate limit never keeps the local grant.
async fn revoke(env: &Env, storage: &Storage, id: &str, v: &Value) -> Result<Value> {
    let tokens = match open(storage, id, v).await {
        Ok(tokens) => tokens,
        // A replaced K belongs to an older login; it must not end the newer one.
        Err(e) if e.code == "connection_key_invalid" => return Ok(json!({"stale":true})),
        Err(e) if e.code == "trakt_login_required" => {
            return Ok(json!({"revoked":false,"connections":[]}));
        }
        Err(e) => return Err(e),
    };
    let body = json!({"token":tokens.access_token,"client_id":env.secret("TRAKT_CLIENT_ID")?.to_string(),"client_secret":env.secret("TRAKT_CLIENT_SECRET")?.to_string()});
    let result = Client { env }
        .send("/oauth/revoke", Method::Post, &[], None, Some(&body))
        .await;
    let (revoked, retry_after) = match result {
        Ok(r) if (200..300).contains(&r.status) => (true, None),
        Ok(r) => (false, upstream_error(&r).retry_after),
        Err(_) => (false, None),
    };
    let all = connections(storage).await?;
    storage.delete_all().await?;
    Ok(json!({"revoked":revoked,"retry_after":retry_after,"connections":all}))
}

async fn pace(storage: &Storage, v: &Value) -> Result<Value> {
    let current = worker::Date::now().as_millis();
    let until = storage.get::<u64>(PACED_UNTIL).await?.unwrap_or(0);
    let wait = until.saturating_sub(current);
    match v["op"].as_str() {
        // Atomically claim the next slot, so two connections cannot share one second.
        Some("reserve") => {
            storage
                .put(PACED_UNTIL, until.max(current) + PACE_MILLIS)
                .await?;
        }
        Some("mark") => {
            storage
                .put(PACED_UNTIL, until.max(current + PACE_MILLIS))
                .await?;
        }
        _ => {}
    }
    Ok(json!({"wait_ms":wait}))
}

pub async fn handle(env: &Env, state: &State, path: &str, v: Value) -> Value {
    let storage = state.storage();
    let Some(id) = v["grant"].as_str().map(String::from) else {
        return wire(ApiError::new(500, "missing_id"));
    };
    let result: Result<Value> = async {
        let result = match path {
            "/_grant/exists" => Ok(json!({"exists":storage.get::<String>(GRANT).await?.is_some()})),
            "/_grant/put" => {
                storage
                    .put(GRANT, v["sealed"].as_str().unwrap_or_default())
                    .await?;
                storage.delete(BACKOFF_UNTIL).await?;
                storage
                    .put(
                        CONNECTIONS,
                        vec![v["connection"].as_str().unwrap_or_default()],
                    )
                    .await?;
                Ok(json!({}))
            }
            "/_grant/link" => {
                open(&storage, &id, &v).await?;
                add_connection(&storage, v["connection"].as_str().unwrap_or_default()).await?;
                Ok(json!({}))
            }
            "/_grant/access" => access(env, &storage, &id, &v).await,
            "/_grant/revoke" => return revoke(env, &storage, &id, &v).await,
            "/_grant/leave" => {
                let leaving = v["connection"].as_str().unwrap_or_default();
                let mut all = connections(&storage).await?;
                all.retain(|c| c != leaving);
                storage.put(CONNECTIONS, all).await?;
                Ok(json!({}))
            }
            "/_grant/pace" => pace(&storage, &v).await,
            _ => Err(ApiError::new(404, "not_found")),
        };
        if result.is_ok() || path == "/_grant/access" {
            retention::touch(env, state, false).await?;
        }
        result
    }
    .await;
    result.unwrap_or_else(wire)
}

/// Rebuild a typed error from another Durable Object's reply.
fn from_wire(v: &Value) -> ApiError {
    let code = match v["error"].as_str().unwrap_or_default() {
        "trakt_login_required" => "trakt_login_required",
        "connection_key_invalid" => "connection_key_invalid",
        "trakt_rate_limited" => "trakt_rate_limited",
        "trakt_forbidden" => "trakt_forbidden",
        "trakt_unavailable" => "trakt_unavailable",
        "invalid_trakt_response" => "invalid_trakt_response",
        "invalid_trakt_token" => "invalid_trakt_token",
        "invalid_seal_configuration" => "invalid_seal_configuration",
        "storage_or_runtime_error" => "storage_or_runtime_error",
        _ => "trakt_upstream_error",
    };
    let status = v["status"].as_u64().unwrap_or(502) as u16;
    let mut e = ApiError::new(status, code);
    e.retry_after = v["retry_after"].as_u64();
    e
}

pub async fn request(env: &Env, grant: &str, path: &str, mut body: Value) -> Result<Value> {
    body["grant"] = json!(grant);
    let mut response = crate::http::call(env, &format!("grant:{grant}"), path, body).await?;
    let v: Value = response.json().await?;
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return Err(from_wire(&v));
    }
    Ok(v)
}

pub fn wire_key(key: &seal::Key) -> String {
    seal::to_hex(key)
}
