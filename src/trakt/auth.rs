use super::client::{Client, upstream_error};
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::{Method, Storage};
#[derive(Serialize, Deserialize, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub created_at: u64,
}
#[derive(Serialize, Deserialize)]
pub struct Pending {
    pub code: String,
    pub expires_at: u64,
    pub next_poll: u64,
    pub interval: u64,
}
pub fn now() -> u64 {
    worker::Date::now().as_millis() / 1000
}
pub fn device_status(status: u16) -> &'static str {
    match status {
        400 => "authorization_pending",
        404 => "invalid_device_code",
        409 => "already_used",
        410 => "expired_token",
        418 => "access_denied",
        429 => "slow_down",
        _ => "trakt_upstream_error",
    }
}
pub async fn get_optional<T: serde::de::DeserializeOwned>(
    storage: &mut Storage,
    key: &str,
) -> Result<Option<T>> {
    match storage.get(key).await {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.to_string().contains("No such value") => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub async fn request_device_code(c: &Client<'_>, storage: &mut Storage) -> Result<Value> {
    if let Some(p) = get_optional::<Pending>(storage, "pending").await?
        && p.expires_at > now()
    {
        return Err(ApiError::new(409, "login_already_pending"));
    }
    let id = c.env.secret("TRAKT_CLIENT_ID")?.to_string();
    let r = c
        .send(
            "/oauth/device/code",
            Method::Post,
            &[],
            None,
            Some(&json!({"client_id":id})),
        )
        .await?;
    if r.status != 200 {
        return Err(upstream_error(&r));
    }
    let code = r.data["device_code"]
        .as_str()
        .ok_or(ApiError::new(502, "invalid_trakt_response"))?;
    let expires = r.data["expires_in"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 3600)
        .ok_or(ApiError::new(502, "invalid_trakt_response"))?;
    let interval = r.data["interval"].as_u64().unwrap_or(5).clamp(1, 60);
    let user_code = r.data["user_code"]
        .as_str()
        .ok_or(ApiError::new(502, "invalid_trakt_response"))?;
    storage
        .put(
            "pending",
            Pending {
                code: code.into(),
                expires_at: now() + expires,
                next_poll: now() + interval,
                interval,
            },
        )
        .await?;
    Ok(
        json!({"device_code":code,"user_code":user_code,"verification_url":"https://trakt.tv/activate","expires_in":expires,"interval":interval,"instructions":format!("Visit https://trakt.tv/activate and enter code {user_code}. Then return here to finish connecting.")}),
    )
}
pub async fn save(c: &Client<'_>, storage: &mut Storage, id: &str, t: &Tokens) -> Result<()> {
    if t.access_token.is_empty() || t.refresh_token.is_empty() || t.expires_in == 0 {
        return Err(ApiError::new(502, "invalid_trakt_token"));
    }
    storage.put("trakt_tokens", t).await?;
    // Durable storage is authoritative; KV failure must not lose a rotated refresh token.
    if let Ok(kv) = c.env.kv("TRAKT_SESSIONS")
        && let Ok(p) = kv.put(&format!("user:{id}:tokens"), t)
    {
        let _ = p.expiration_ttl(30 * 86400).execute().await;
    }
    Ok(())
}
pub async fn poll_device_token(
    c: &Client<'_>,
    storage: &mut Storage,
    id: &str,
    code: &str,
) -> Result<Value> {
    let mut p = get_optional::<Pending>(storage, "pending")
        .await?
        .ok_or(ApiError::new(404, "invalid_device_code"))?;
    if p.code != code {
        return Err(ApiError::new(404, "invalid_device_code"));
    }
    if now() >= p.expires_at {
        storage.delete("pending").await?;
        return Err(ApiError::new(410, "expired_token"));
    }
    if now() < p.next_poll {
        let mut e = ApiError::new(429, "slow_down");
        e.retry_after = Some(p.next_poll - now());
        return Err(e);
    }
    p.next_poll = now() + p.interval;
    storage.put("pending", &p).await?;
    let body = json!({"code":code,"client_id":c.env.secret("TRAKT_CLIENT_ID")?.to_string(),"client_secret":c.env.secret("TRAKT_CLIENT_SECRET")?.to_string()});
    let r = c
        .send("/oauth/device/token", Method::Post, &[], None, Some(&body))
        .await?;
    if r.status == 200 {
        let t: Tokens = serde_json::from_value(r.data)
            .map_err(|_| ApiError::new(502, "invalid_trakt_token"))?;
        save(c, storage, id, &t).await?;
        storage.delete("pending").await?;
        return Ok(json!({"status":"connected"}));
    }
    if matches!(r.status, 404 | 409 | 410 | 418) {
        storage.delete("pending").await?;
    }
    if r.status == 429 {
        p.interval += 5;
        p.next_poll = now() + p.interval;
        storage.put("pending", &p).await?;
    }
    let mut e = ApiError::new(
        if matches!(r.status, 400 | 404 | 409 | 410 | 418 | 429) {
            r.status
        } else {
            502
        },
        device_status(r.status),
    );
    if matches!(r.status, 400 | 429) {
        e.retry_after = Some(p.interval);
    }
    Err(e)
}
pub async fn access_token(c: &Client<'_>, storage: &mut Storage, id: &str) -> Result<String> {
    let t = get_optional::<Tokens>(storage, "trakt_tokens")
        .await?
        .ok_or(ApiError::new(401, "trakt_login_required"))?;
    if t.created_at.saturating_add(t.expires_in) > now() + 60 {
        return Ok(t.access_token);
    }
    let body = json!({"refresh_token":t.refresh_token,"client_id":c.env.secret("TRAKT_CLIENT_ID")?.to_string(),"client_secret":c.env.secret("TRAKT_CLIENT_SECRET")?.to_string(),"redirect_uri":c.env.var("TRAKT_REDIRECT_URI")?.to_string(),"grant_type":"refresh_token"});
    let r = c
        .send("/oauth/token", Method::Post, &[], None, Some(&body))
        .await?;
    if r.status != 200 {
        if matches!(r.status, 400 | 401) {
            return Err(ApiError::new(401, "trakt_login_required"));
        }
        return Err(upstream_error(&r));
    }
    let t: Tokens =
        serde_json::from_value(r.data).map_err(|_| ApiError::new(502, "invalid_trakt_token"))?;
    save(c, storage, id, &t).await?;
    Ok(t.access_token)
}
