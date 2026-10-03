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
    json!({"issuer":base,"authorization_endpoint":format!("{base}/oauth/authorize"),"token_endpoint":format!("{base}/oauth/token"),"registration_endpoint":format!("{base}/oauth/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none","client_secret_post"],"code_challenge_methods_supported":["S256"],"scopes_supported":["trakt:read","trakt:write"],"client_id_metadata_document_supported":true})
}
/// A Client ID Metadata Document client identifies itself with an HTTPS URL
/// (draft-ietf-oauth-client-id-metadata-document); registered clients use hex IDs.
pub fn is_metadata_client_id(client_id: &str) -> bool {
    client_id.starts_with("https://")
}
/// Turn a fetched Client ID Metadata Document into a public registration. Only the
/// host that served the document is trusted for display, since its contents are
/// self-asserted.
/// A metadata URL must be a canonical HTTPS URL on a public host name with a path.
/// It is checked before anything is fetched from it.
pub fn check_metadata_url(url: &str) -> Result<String> {
    let invalid = || ApiError::new(400, "invalid_client_metadata");
    let parsed = worker::Url::parse(url).map_err(|_| invalid())?;
    let host = parsed.host_str().unwrap_or("");
    if url.len() > 2048
        || parsed.as_str() != url
        || parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.path() == "/"
        || !host.contains('.')
        || host.ends_with(".localhost")
        || host.starts_with('[')
        || host.parse::<std::net::Ipv4Addr>().is_ok()
    {
        return Err(invalid());
    }
    Ok(host.into())
}
pub fn metadata_registration(url: &str, document: &Value) -> Result<Registration> {
    let invalid = || ApiError::new(400, "invalid_client_metadata");
    let host = check_metadata_url(url)?;
    let doc = document.as_object().ok_or_else(invalid)?;
    let redirects = doc
        .get("redirect_uris")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if doc.get("client_id").and_then(Value::as_str) != Some(url)
        || doc.contains_key("client_secret")
        || doc.contains_key("client_secret_expires_at")
        || doc
            .get("token_endpoint_auth_method")
            .is_some_and(|m| m != "none")
        || redirects.is_empty()
        || redirects.len() > 10
        || redirects
            .iter()
            .any(|u| !u.as_str().is_some_and(security::valid_redirect))
    {
        return Err(invalid());
    }
    Ok(Registration {
        client_id: url.into(),
        client_name: host,
        redirect_uris: redirects
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        secret_hash: None,
        token_endpoint_auth_method: "none".into(),
    })
}
/// Each transport's metadata names the exact URL a user enters. The origin document
/// keeps the shared `{base}/` resource that earlier clients negotiated.
pub fn resource_metadata(base: &str, path: &str) -> Value {
    let resource = match path.rsplit_once('/') {
        Some((_, transport @ ("mcp" | "sse"))) => format!("{base}/{transport}"),
        _ => format!("{base}/"),
    };
    json!({"resource":resource,"authorization_servers":[base],"scopes_supported":["trakt:read","trakt:write"],"bearer_methods_supported":["header"]})
}
pub fn resource_metadata_url(base: &str, request_path: &str) -> String {
    let suffix = match request_path {
        "/mcp" => "/mcp",
        "/sse" | "/messages" => "/sse",
        _ => "",
    };
    format!("{base}/.well-known/oauth-protected-resource{suffix}")
}
pub fn accepted_resource(base: &str, resource: &str) -> bool {
    ["/", "/mcp", "/sse"]
        .iter()
        .any(|path| resource.strip_prefix(base) == Some(path))
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
    let base = config::base(env)?;
    let r: Registration = serde_json::from_value(v["registration"].clone())?;
    let q = &v["params"];
    let get = |key: &str| q[key].as_str().unwrap_or("");
    if get("response_type") != "code"
        || !r
            .redirect_uris
            .iter()
            .any(|s| security::redirect_matches(s, get("redirect_uri")))
    {
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
    let resource = match get("resource") {
        "" => config::resource(env)?,
        requested if accepted_resource(&base, requested) => requested.into(),
        _ => return Err(ApiError::new(400, "invalid_target")),
    };
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
    let base = config::base(env)?;
    if q["resource"]
        .as_str()
        .is_some_and(|r| !accepted_resource(&base, r))
    {
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
    // A local redirect can be claimed by any process on the device (MCP spec).
    let local = worker::Url::parse(v["redirect_uri"].as_str().unwrap_or("")).is_ok_and(|u| {
        u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    });
    format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Connect Trakt</title><link rel="stylesheet" href="/login.css"><main><p>TRAKT MCP</p><h1>Connect your Trakt account</h1><p><strong>{}</strong> is requesting permission to {}. Your credentials stay on this server.</p><p>After authorization you will return to <code>{}</code>.</p>{}<form id="connect" data-session="{}" data-ticket="{}"><button type="submit">Connect Trakt</button></form><section id="status" aria-live="polite"></section><p>You can close this page to cancel.</p></main><script src="/login.js" defer></script></html>"#,
        field("client_name"),
        permissions,
        field("redirect_uri"),
        if local {
            "<p><strong>This app receives the authorization on this device.</strong> Continue only if you started this connection from an app on this computer.</p>"
        } else {
            ""
        },
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

    #[test]
    fn transport_metadata_names_the_exact_url_users_enter() {
        let base = "https://plugin.example.test";
        let resource = |path| resource_metadata(base, path)["resource"].clone();
        assert_eq!(
            resource("/.well-known/oauth-protected-resource/mcp"),
            json!("https://plugin.example.test/mcp")
        );
        assert_eq!(
            resource("/.well-known/oauth-protected-resource/sse"),
            json!("https://plugin.example.test/sse")
        );
        // The origin document predates per-transport metadata and still covers both.
        assert_eq!(
            resource("/.well-known/oauth-protected-resource"),
            json!("https://plugin.example.test/")
        );
        assert_eq!(
            resource_metadata_url(base, "/mcp"),
            "https://plugin.example.test/.well-known/oauth-protected-resource/mcp"
        );
        for path in ["/sse", "/messages"] {
            assert_eq!(
                resource_metadata_url(base, path),
                "https://plugin.example.test/.well-known/oauth-protected-resource/sse"
            );
        }
        assert_eq!(
            resource_metadata_url(base, "/sync/watched"),
            "https://plugin.example.test/.well-known/oauth-protected-resource"
        );
    }

    fn claude_code_document() -> Value {
        // Published at https://claude.ai/oauth/claude-code-client-metadata.
        json!({"client_id":"https://claude.ai/oauth/claude-code-client-metadata","client_name":"Claude Code","client_uri":"https://claude.ai","redirect_uris":["http://localhost/callback","http://127.0.0.1/callback"],"grant_types":["authorization_code","refresh_token"],"response_types":["code"],"token_endpoint_auth_method":"none"})
    }

    #[test]
    fn metadata_advertises_client_id_metadata_documents_for_public_clients() {
        let m = metadata("https://plugin.example.test");
        assert_eq!(m["client_id_metadata_document_supported"], true);
        assert!(
            m["token_endpoint_auth_methods_supported"]
                .as_array()
                .unwrap()
                .contains(&json!("none"))
        );
    }

    #[test]
    fn a_client_id_metadata_document_becomes_a_public_registration() {
        let url = "https://claude.ai/oauth/claude-code-client-metadata";
        assert!(is_metadata_client_id(url));
        assert!(check_metadata_url(url).is_ok());
        assert!(!is_metadata_client_id(&"a".repeat(32)));
        let r = metadata_registration(url, &claude_code_document()).unwrap();
        assert_eq!(r.client_id, url);
        // The document is self-asserted, so consent names the host that serves it.
        assert_eq!(r.client_name, "claude.ai");
        assert_eq!(
            r.redirect_uris,
            ["http://localhost/callback", "http://127.0.0.1/callback"]
        );
        assert!(r.secret_hash.is_none());
        assert_eq!(r.token_endpoint_auth_method, "none");
    }

    #[test]
    fn client_id_metadata_documents_are_validated_before_use() {
        let url = "https://claude.ai/oauth/claude-code-client-metadata";
        let with = |key: &str, value: Value| {
            let mut d = claude_code_document();
            d[key] = value;
            d
        };
        for doc in [
            with("client_id", json!("https://claude.ai/oauth/other")),
            with("token_endpoint_auth_method", json!("client_secret_post")),
            with("token_endpoint_auth_method", json!("private_key_jwt")),
            with("client_secret", json!("leaked")),
            with("redirect_uris", json!([])),
            with("redirect_uris", json!(["javascript:alert(1)"])),
            with("redirect_uris", json!(vec!["https://claude.ai/cb"; 11])),
            json!([]),
        ] {
            assert_eq!(
                metadata_registration(url, &doc).err().unwrap().code,
                "invalid_client_metadata",
                "{doc}"
            );
        }
        for bad in [
            "http://claude.ai/oauth/metadata",
            "https://claude.ai",
            "https://claude.ai/",
            "https://claude.ai/oauth/metadata#x",
            "https://user:pass@claude.ai/oauth/metadata",
            "https://203.0.113.9/metadata",
            "https://localhost/metadata",
            "https://claude.ai/oauth/../metadata",
        ] {
            // URLs are refused before anything is fetched from them.
            assert!(check_metadata_url(bad).is_err(), "{bad}");
            let mut doc = claude_code_document();
            doc["client_id"] = json!(bad);
            assert!(metadata_registration(bad, &doc).is_err(), "{bad}");
        }
    }

    #[test]
    fn consent_warns_when_the_redirect_stays_on_this_device() {
        let local = login_page(
            &json!({"client_name":"claude.ai","redirect_uri":"http://localhost:3118/callback"}),
        );
        assert!(local.contains("on this device"));
        let hosted = login_page(
            &json!({"client_name":"claude.ai","redirect_uri":"https://claude.ai/api/mcp/auth_callback"}),
        );
        assert!(!hosted.contains("on this device"));
    }

    #[test]
    fn every_advertised_resource_is_accepted_and_others_are_not() {
        let base = "https://plugin.example.test";
        for r in [
            "https://plugin.example.test/",
            "https://plugin.example.test/mcp",
            "https://plugin.example.test/sse",
        ] {
            assert!(accepted_resource(base, r), "{r}");
        }
        for r in [
            "https://plugin.example.test",
            "https://plugin.example.test/mcp/",
            "https://other.test/mcp",
            "https://plugin.example.test/messages",
        ] {
            assert!(!accepted_resource(base, r), "{r}");
        }
    }
}
