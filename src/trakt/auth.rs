use super::client::{Client, upstream_error};
use crate::{
    error::{ApiError, Result},
    grant, retention, seal, security,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::{Method, State, Storage};
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
    Ok(storage.get(key).await?)
}
pub async fn request_device_code(c: &Client<'_>, storage: &mut Storage) -> Result<Value> {
    // A new login abandons any tokens still waiting from an earlier one.
    drop_pending(storage).await?;
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
pub fn check(t: &Tokens) -> Result<()> {
    if t.access_token.is_empty() || t.refresh_token.is_empty() || t.expires_in == 0 {
        return Err(ApiError::new(502, "invalid_trakt_token"));
    }
    Ok(())
}
pub async fn poll_device_token(
    c: &Client<'_>,
    storage: &mut Storage,
    code: &str,
) -> Result<Tokens> {
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
        check(&t)?;
        storage.delete("pending").await?;
        return Ok(t);
    }
    if matches!(r.status, 404 | 409 | 410 | 418) {
        storage.delete("pending").await?;
    }
    let told = r
        .headers
        .get("Retry-After")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u64>().ok());
    if r.status == 429 {
        p.interval += 5;
        p.next_poll = now() + p.interval.max(told.unwrap_or(0));
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
    if r.status == 400 {
        e.retry_after = Some(p.interval);
    }
    if r.status == 429 {
        e.retry_after = Some(p.next_poll - now());
    }
    Err(e)
}
/// Exchange a browser authorization code. PKCE proves the request, so no client
/// secret is sent. A 429 leaves the code unused, so the caller can try it again.
pub async fn exchange_code(
    c: &Client<'_>,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Tokens> {
    let body = json!({"code":code,"client_id":c.env.secret("TRAKT_CLIENT_ID")?.to_string(),"redirect_uri":redirect_uri,"grant_type":"authorization_code","code_verifier":verifier});
    let r = c
        .send("/oauth/token", Method::Post, &[], None, Some(&body))
        .await?;
    if r.status != 200 {
        if matches!(r.status, 400 | 401) {
            return Err(ApiError::new(400, "trakt_authorization_failed"));
        }
        return Err(upstream_error(&r));
    }
    let t: Tokens =
        serde_json::from_value(r.data).map_err(|_| ApiError::new(502, "invalid_trakt_token"))?;
    check(&t)?;
    Ok(t)
}
/// The connection a request runs on, with its key while the request lasts.
pub struct Conn {
    pub id: String,
    pub key: Option<seal::Key>,
    pub grant: Option<String>,
}

pub async fn access_token(c: &Client<'_>, conn: &Conn) -> Result<String> {
    let (Some(grant_id), Some(key)) = (&conn.grant, &conn.key) else {
        return Err(ApiError::new(401, "trakt_login_required"));
    };
    let reply = grant::request(
        c.env,
        grant_id,
        "/_grant/access",
        json!({"k":grant::wire_key(key)}),
    )
    .await
    .map_err(|e| {
        // Another login replaced this key; the user must reconnect or link again.
        if e.code == "connection_key_invalid" {
            ApiError::new(401, "trakt_login_required")
        } else {
            e
        }
    })?;
    reply["access_token"]
        .as_str()
        .map(String::from)
        .ok_or(ApiError::new(502, "invalid_trakt_token"))
}

const PENDING_LOGIN: &str = "pending_login";
const PENDING_SOURCE: &str = "pending_login_source";
/// How long new Trakt tokens may wait, sealed under the Worker secret, for login to end.
const PENDING_LOGIN_SECONDS: u64 = 15 * 60;

pub async fn drop_pending(storage: &Storage) -> Result<()> {
    storage.delete(PENDING_LOGIN).await?;
    storage.delete(PENDING_SOURCE).await?;
    storage.delete(retention::PENDING_EXPIRES).await?;
    Ok(())
}

/// A device code is spent once Trakt issues tokens, so keep them until the login ends.
/// `source` names that login, so a later login can never finish with these tokens.
pub async fn park(c: &Client<'_>, state: &State, id: &str, source: &str, t: &Tokens) -> Result<()> {
    let keys = seal::Keys::from_env(c.env)?;
    let storage = state.storage();
    storage
        .put(
            PENDING_LOGIN,
            keys.seal_pending(id, &serde_json::to_string(t)?)?,
        )
        .await?;
    storage.put(PENDING_SOURCE, security::hash(source)).await?;
    storage
        .put(retention::PENDING_EXPIRES, now() + PENDING_LOGIN_SECONDS)
        .await?;
    retention::rearm(state).await
}

pub async fn parked(
    c: &Client<'_>,
    storage: &Storage,
    id: &str,
    source: &str,
) -> Result<Option<Tokens>> {
    let Some(sealed) = storage.get::<String>(PENDING_LOGIN).await? else {
        return Ok(None);
    };
    let stored = storage
        .get::<String>(PENDING_SOURCE)
        .await?
        .unwrap_or_default();
    if !security::equal(&stored, &security::hash(source)) {
        return Ok(None);
    }
    let expires = storage
        .get::<u64>(retention::PENDING_EXPIRES)
        .await?
        .unwrap_or(0);
    let plain = seal::Keys::from_env(c.env)?.open_pending(id, &sealed, now());
    match plain.filter(|_| expires > now()) {
        Some(text) => Ok(Some(serde_json::from_str(&text)?)),
        None => {
            drop_pending(storage).await?;
            Ok(None)
        }
    }
}

/// Key the grant by a keyed hash of the Trakt user. A failure leaves the login parked.
pub async fn grant_for(c: &Client<'_>, t: &Tokens) -> Result<String> {
    let r = c
        .send(
            "/users/settings",
            Method::Get,
            &[],
            Some(&t.access_token),
            None,
        )
        .await?;
    if r.status != 200 {
        return Err(upstream_error(&r));
    }
    let ids = &r.data["user"]["ids"];
    let user = ids["trakt"]
        .as_u64()
        .map(|id| id.to_string())
        .or_else(|| ids["slug"].as_str().map(String::from))
        .filter(|user| !user.is_empty())
        .ok_or(ApiError::new(502, "invalid_trakt_response"))?;
    seal::Keys::from_env(c.env)?.grant_id(&user)
}

/// Store the parked tokens as this user's grant under `key`, replacing any older one.
pub async fn store_grant(
    c: &Client<'_>,
    storage: &Storage,
    conn_id: &str,
    grant_id: &str,
    tokens: &Tokens,
    key: &seal::Key,
) -> Result<()> {
    let sealed = seal::seal_grant(key, grant_id, &serde_json::to_string(tokens)?)?;
    grant::request(
        c.env,
        grant_id,
        "/_grant/put",
        json!({"sealed":sealed,"connection":conn_id,"k":grant::wire_key(key)}),
    )
    .await?;
    drop_pending(storage).await
}
