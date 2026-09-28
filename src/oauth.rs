use crate::config;
use crate::{
    error::{ApiError, Result},
    security,
    trakt::{
        auth::{self, get_optional, now},
        client::Client,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::{Env, Storage};
#[derive(Serialize, Deserialize)]
pub struct Registration {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub secret_hash: Option<String>,
    pub token_endpoint_auth_method: String,
}
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub access_hash: String,
    pub access_expires: u64,
    pub refresh_hash: Option<String>,
    pub refresh_expires: u64,
    pub client_id: Option<String>,
    pub resource: String,
    #[serde(default = "read_scope")]
    pub scope: String,
}
#[derive(Serialize, Deserialize)]
pub struct Flow {
    pub client_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub challenge: Option<String>,
    pub ticket_hash: String,
    pub expires: u64,
    pub device_code: Option<String>,
    pub code_hash: Option<String>,
    pub resource: String,
}
pub fn read_scope() -> String {
    "trakt:read".into()
}
pub fn requested_scope(value: Option<&str>) -> Result<String> {
    let value = value.unwrap_or("trakt:read trakt:write");
    let scopes: Vec<_> = value.split_whitespace().collect();
    if scopes.is_empty()
        || scopes
            .iter()
            .any(|s| !matches!(*s, "trakt:read" | "trakt:write"))
    {
        return Err(ApiError::new(400, "invalid_scope"));
    }
    if !scopes.contains(&"trakt:read") {
        return Err(ApiError::new(400, "invalid_scope"));
    }
    Ok(if scopes.contains(&"trakt:write") {
        "trakt:read trakt:write"
    } else {
        "trakt:read"
    }
    .into())
}
pub async fn require_write(storage: &mut Storage) -> Result<()> {
    let session: Session = storage
        .get("session")
        .await?
        .ok_or(ApiError::new(401, "invalid_token"))?;
    if !session.scope.split_whitespace().any(|s| s == "trakt:write") {
        return Err(ApiError::new(403, "write_authorization_required"));
    }
    Ok(())
}
pub fn metadata(base: &str) -> Value {
    json!({"issuer":base,"authorization_endpoint":format!("{base}/oauth/authorize"),"token_endpoint":format!("{base}/oauth/token"),"registration_endpoint":format!("{base}/oauth/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none","client_secret_post"],"code_challenge_methods_supported":["S256"],"scopes_supported":["trakt:read","trakt:write"]})
}
pub fn resource_metadata(base: &str) -> Value {
    let resource = format!("{base}/");
    json!({"resource":resource,"authorization_servers":[base],"scopes_supported":["trakt:read","trakt:write"],"bearer_methods_supported":["header"]})
}
pub async fn register(storage: &mut Storage, v: Value) -> Result<Value> {
    let redirects = v["redirect_uris"]
        .as_array()
        .ok_or(ApiError::new(400, "invalid_redirect_uri"))?;
    if redirects.is_empty()
        || redirects.len() > 10
        || redirects
            .iter()
            .any(|u| !u.as_str().is_some_and(security::valid_redirect))
    {
        return Err(ApiError::new(400, "invalid_redirect_uri"));
    }
    let method = v["token_endpoint_auth_method"].as_str().unwrap_or("none");
    if !matches!(method, "none" | "client_secret_post") {
        return Err(ApiError::new(400, "invalid_client_metadata"));
    }
    let name = v["client_name"].as_str().unwrap_or("MCP client");
    if name.len() > 100 {
        return Err(ApiError::new(400, "invalid_client_metadata"));
    }
    let id = v["_id"].as_str().ok_or(ApiError::new(500, "missing_id"))?;
    let secret = (method == "client_secret_post").then(security::random);
    let r = Registration {
        client_id: id.into(),
        client_name: name.into(),
        redirect_uris: redirects
            .iter()
            .map(|u| u.as_str().unwrap().into())
            .collect(),
        secret_hash: secret.as_deref().map(security::hash),
        token_endpoint_auth_method: method.into(),
    };
    storage.put("registration", &r).await?;
    let mut out = json!({"client_id":id,"client_name":name,"redirect_uris":r.redirect_uris,"grant_types":["authorization_code","refresh_token"],"response_types":["code"],"token_endpoint_auth_method":method,"client_id_issued_at":now()});
    if let Some(s) = secret {
        out["client_secret"] = json!(s);
        out["client_secret_expires_at"] = json!(0);
    }
    Ok(out)
}
pub async fn create(
    storage: &mut Storage,
    id: &str,
    client_id: Option<String>,
    resource: String,
) -> Result<String> {
    let token = format!("{id}.{}", security::random());
    storage
        .put(
            "session",
            Session {
                id: id.into(),
                access_hash: security::hash(&token),
                access_expires: now() + 900,
                refresh_hash: None,
                refresh_expires: 0,
                client_id,
                resource,
                scope: read_scope(),
            },
        )
        .await?;
    Ok(token)
}
pub async fn authenticate(storage: &mut Storage, token: &str) -> Result<Session> {
    let s = get_optional::<Session>(storage, "session")
        .await?
        .ok_or(ApiError::new(401, "invalid_token"))?;
    if s.access_expires <= now() || !security::equal(&s.access_hash, &security::hash(token)) {
        return Err(ApiError::new(401, "invalid_token"));
    }
    Ok(s)
}
pub async fn issue(storage: &mut Storage) -> Result<Value> {
    let mut s: Session = storage
        .get("session")
        .await?
        .ok_or(ApiError::new(401, "invalid_token"))?;
    let access = format!("{}.{}", s.id, security::random());
    let refresh = format!("{}.{}", s.id, security::random());
    s.access_hash = security::hash(&access);
    s.access_expires = now() + 3600;
    if let Some(old) = &s.refresh_hash {
        let mut used = get_optional::<Vec<String>>(storage, "used_refreshes")
            .await?
            .unwrap_or_default();
        if used.len() >= 1024 {
            return Err(ApiError::new(401, "reauthorization_required"));
        }
        used.push(old.clone());
        storage.put("used_refreshes", used).await?;
    }
    s.refresh_hash = Some(security::hash(&refresh));
    s.refresh_expires = now() + 30 * 86400;
    storage.put("session", &s).await?;
    Ok(
        json!({"access_token":access,"token_type":"Bearer","expires_in":3600,"refresh_token":refresh,"scope":s.scope}),
    )
}
pub async fn begin(env: &Env, storage: &mut Storage, v: Value) -> Result<Value> {
    let resource = config::resource(env)?;
    let r: Registration = serde_json::from_value(v["registration"].clone())?;
    let q = &v["params"];
    let get = |key: &str| q[key].as_str().unwrap_or("");
    if get("response_type") != "code" || !r.redirect_uris.iter().any(|s| s == get("redirect_uri")) {
        return Err(ApiError::new(400, "invalid_authorization_request"));
    }
    let challenge = if get("code_challenge").is_empty() {
        if r.secret_hash.is_none() {
            return Err(ApiError::new(400, "pkce_required"));
        }
        None
    } else {
        if get("code_challenge_method") != "S256"
            || get("code_challenge").len() != 43
            || !get("code_challenge")
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(ApiError::new(400, "invalid_code_challenge"));
        }
        Some(get("code_challenge").into())
    };
    if !get("resource").is_empty() && get("resource") != resource {
        return Err(ApiError::new(400, "invalid_target"));
    }
    let scope = requested_scope(q["scope"].as_str())?;
    let id = v["_id"].as_str().ok_or(ApiError::new(500, "missing_id"))?;
    let ticket = security::random();
    create(storage, id, Some(r.client_id.clone()), resource.clone()).await?;
    let mut session: Session = storage
        .get("session")
        .await?
        .ok_or(ApiError::new(500, "missing_session"))?;
    session.scope = scope.clone();
    storage.put("session", &session).await?;
    let f = Flow {
        client_id: r.client_id,
        redirect_uri: get("redirect_uri").into(),
        state: get("state").into(),
        challenge,
        ticket_hash: security::hash(&ticket),
        expires: now() + 900,
        device_code: None,
        code_hash: None,
        resource: resource.clone(),
    };
    storage.put("flow", &f).await?;
    Ok(
        json!({"session_id":id,"ticket":ticket,"client_name":r.client_name,"redirect_uri":f.redirect_uri,"scope":scope}),
    )
}
pub async fn browser_step(env: &Env, storage: &mut Storage, id: &str, v: Value) -> Result<Value> {
    let mut f = get_optional::<Flow>(storage, "flow")
        .await?
        .ok_or(ApiError::new(400, "invalid_flow"))?;
    if f.expires <= now()
        || !security::equal(
            &f.ticket_hash,
            &security::hash(v["ticket"].as_str().unwrap_or("")),
        )
    {
        return Err(ApiError::new(400, "invalid_flow"));
    }
    if !matches!(v["action"].as_str(), Some("start" | "poll")) {
        return Err(ApiError::new(400, "invalid_action"));
    }
    let c = Client { env };
    if v["action"] == "start" {
        if f.device_code.is_some() {
            return Err(ApiError::new(409, "login_already_pending"));
        }
        let d = auth::request_device_code(&c, storage).await?;
        f.device_code = d["device_code"].as_str().map(String::from);
        storage.put("flow", &f).await?;
        return Ok(d);
    }
    if f.code_hash.is_some() {
        return Err(ApiError::new(409, "flow_already_completed"));
    }
    let code = f
        .device_code
        .as_deref()
        .ok_or(ApiError::new(400, "login_not_started"))?;
    auth::poll_device_token(&c, storage, id, code).await?;
    let code = format!("{id}.{}", security::random());
    f.code_hash = Some(security::hash(&code));
    f.expires = now() + 120;
    let mut redirect = worker::Url::parse(&f.redirect_uri)
        .map_err(|_| ApiError::new(500, "invalid_redirect_uri"))?;
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &f.state);
    storage.put("flow", &f).await?;
    Ok(json!({"redirect":redirect.as_str()}))
}
pub async fn exchange(env: &Env, storage: &mut Storage, v: Value) -> Result<Value> {
    let q = &v["params"];
    let r: Option<Registration> = serde_json::from_value(v["registration"].clone())?;
    let mut s: Session = get_optional(storage, "session")
        .await?
        .ok_or(ApiError::new(400, "invalid_grant"))?;
    if let Some(expected) = &s.client_id {
        let r = r.ok_or(ApiError::new(401, "invalid_client"))?;
        if &r.client_id != expected || q["client_id"].as_str() != Some(expected) {
            return Err(ApiError::new(401, "invalid_client"));
        }
        if let Some(hash) = r.secret_hash
            && !security::equal(
                &hash,
                &security::hash(q["client_secret"].as_str().unwrap_or("")),
            )
        {
            return Err(ApiError::new(401, "invalid_client"));
        }
    } else if q["client_id"].as_str().is_some_and(|s| !s.is_empty()) {
        return Err(ApiError::new(401, "invalid_client"));
    }
    if q["resource"].as_str().is_some_and(|r| r != s.resource) {
        return Err(ApiError::new(400, "invalid_target"));
    }
    match q["grant_type"].as_str() {
        Some("authorization_code") => {
            let f: Flow = get_optional(storage, "flow")
                .await?
                .ok_or(ApiError::new(400, "invalid_grant"))?;
            if f.expires <= now()
                || f.code_hash.as_ref().is_none_or(|h| {
                    !security::equal(h, &security::hash(q["code"].as_str().unwrap_or("")))
                })
                || q["redirect_uri"].as_str() != Some(&f.redirect_uri)
            {
                return Err(ApiError::new(400, "invalid_grant"));
            }
            if let Some(challenge) = f.challenge {
                let verifier = q["code_verifier"].as_str().unwrap_or("");
                if !security::valid_verifier(verifier)
                    || !security::equal(&challenge, &security::pkce(verifier))
                {
                    return Err(ApiError::new(400, "invalid_grant"));
                }
            }
            storage.delete("flow").await?;
        }
        Some("refresh_token") => {
            if let Some(scope) = q["scope"].as_str() {
                let scope = requested_scope(Some(scope))?;
                if scope
                    .split_whitespace()
                    .any(|permission| !s.scope.split_whitespace().any(|old| old == permission))
                {
                    return Err(ApiError::new(400, "invalid_scope"));
                }
                s.scope = scope;
            }
            let presented = security::hash(q["refresh_token"].as_str().unwrap_or(""));
            let used = get_optional::<Vec<String>>(storage, "used_refreshes")
                .await?
                .unwrap_or_default();
            if used.iter().any(|h| security::equal(h, &presented)) {
                storage.delete_all().await?;
                env.kv("TRAKT_SESSIONS")?
                    .delete(&format!("user:{}:tokens", s.id))
                    .await?;
                return Err(ApiError::new(400, "invalid_grant"));
            }
            if s.refresh_expires <= now()
                || s.refresh_hash.as_ref().is_none_or(|h| {
                    !security::equal(
                        h,
                        &security::hash(q["refresh_token"].as_str().unwrap_or("")),
                    )
                })
            {
                return Err(ApiError::new(400, "invalid_grant"));
            }
        }
        _ => return Err(ApiError::new(400, "unsupported_grant_type")),
    }
    storage.put("session", &s).await?;
    issue(storage).await
}
pub fn login_page(v: &Value) -> String {
    let field = |k: &str| security::escape(v[k].as_str().unwrap_or(""));
    let permissions = if v["scope"]
        .as_str()
        .unwrap_or("trakt:read")
        .split_whitespace()
        .any(|s| s == "trakt:write")
    {
        "read your Trakt data and make changes you request, including lists, ratings, watch history, comments, and other account actions"
    } else {
        "read your Trakt data without making account changes"
    };
    format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Connect Trakt</title><link rel="stylesheet" href="/login.css"><main><p>TRAKT MCP</p><h1>Connect your Trakt account</h1><p><strong>{}</strong> is requesting permission to {}. Your credentials stay on this server.</p><p>After authorization you will return to <code>{}</code>.</p><form id="connect" data-session="{}" data-ticket="{}"><button type="submit">Connect Trakt</button></form><section id="status" aria-live="polite"></section><p>You can close this page to cancel.</p></main><script src="/login.js" defer></script></html>"#,
        field("client_name"),
        permissions,
        field("redirect_uri"),
        field("session_id"),
        field("ticket")
    )
}
pub const LOGIN_JS: &str = r#"const form=document.querySelector('#connect'), status=document.querySelector('#status');let interval=5,expires=0;
async function step(action){const response=await fetch('/oauth/complete',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({session_id:form.dataset.session,ticket:form.dataset.ticket,action})});return [response,await response.json()];}
async function poll(){if(Date.now()>expires){status.append(document.createTextNode(' Code expired. Close this page and reconnect.'));return;}try{const [r,d]=await step('poll');if(d.redirect){location.assign(d.redirect);return;}if(d.error==='authorization_pending'||d.error==='slow_down'){setTimeout(poll,(d.retry_after||interval)*1000);return;}status.append(document.createTextNode(' '+(d.error||'Connection failed.')));}catch{status.append(document.createTextNode(' Network error. Reconnect to retry.'));}}
form.addEventListener('submit',async e=>{e.preventDefault();form.querySelector('button').disabled=true;try{const [r,d]=await step('start');if(!r.ok){status.textContent=d.error;return;}interval=d.interval;expires=Date.now()+d.expires_in*1000;const p=document.createElement('p');p.textContent='Enter this code: ';const code=document.createElement('strong');code.textContent=d.user_code;p.append(code);const a=document.createElement('a');a.href='https://trakt.tv/activate';a.target='_blank';a.rel='noopener noreferrer';a.textContent='Open Trakt activation';status.replaceChildren(p,a);setTimeout(poll,interval*1000);}catch{status.textContent='Network error. Reconnect to retry.'}});"#;
pub const LOGIN_CSS: &str = "body{font:18px/1.6 system-ui;background:#101419;color:#ecf1f5;margin:0;padding:8vh 24px}main{max-width:640px;margin:auto}h1{line-height:1.15;font-size:40px}button,a{display:inline-block;background:#ef4444;color:white;border:0;border-radius:8px;padding:12px 20px;font:inherit;cursor:pointer}code{overflow-wrap:anywhere;font-size:14px}strong{color:#fff}#status strong{font-size:32px;letter-spacing:4px}button:disabled{opacity:.5}";

#[cfg(test)]
mod scope_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn old_sessions_are_read_only_and_consent_scopes_are_explicit() {
        let legacy: Session = serde_json::from_value(json!({
            "id":"fixture", "access_hash":"hash", "access_expires":123,
            "refresh_hash":null,"refresh_expires":456,"client_id":null,
            "resource":"https://plugin.example.test"
        }))
        .unwrap();
        assert_eq!(legacy.scope, "trakt:read");
        assert_eq!(requested_scope(None).unwrap(), "trakt:read trakt:write");
        assert_eq!(requested_scope(Some("trakt:read")).unwrap(), "trakt:read");
        for scope in ["", "trakt:write", "trakt:read admin", "trakt:read_write"] {
            assert_eq!(
                requested_scope(Some(scope)).err().unwrap().code,
                "invalid_scope"
            );
        }
        let write_page = login_page(&json!({"scope":"trakt:read trakt:write"}));
        assert!(write_page.contains("make changes you request"));
        assert!(write_page.contains("lists, ratings, watch history, comments"));
        let read_page = login_page(&json!({"scope":"trakt:read"}));
        assert!(read_page.contains("without making account changes"));
        assert!(!read_page.contains("make changes you request"));
    }
}
