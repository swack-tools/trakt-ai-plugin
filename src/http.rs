use crate::{
    error::{ApiError, Result},
    oauth, security,
};
use futures::StreamExt;
use serde_json::{Map, Value, json};
use worker::*;
pub fn query(req: &Request) -> Result<Value> {
    let mut out = Map::new();
    for (k, v) in req.url()?.query_pairs() {
        if out.contains_key(k.as_ref()) {
            return Err(ApiError::new(400, "duplicate_parameter"));
        }
        let value = if matches!(k.as_ref(), "page" | "limit") {
            json!(
                v.parse::<u32>()
                    .map_err(|_| ApiError::new(400, "invalid_pagination"))?
            )
        } else {
            json!(v)
        };
        out.insert(k.into_owned(), value);
    }
    Ok(Value::Object(out))
}
async fn call(env: &Env, id: &str, path: &str, value: Value) -> Result<Response> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(value.to_string().into()));
    let req = Request::new_with_init(&format!("https://internal{path}"), &init)?;
    Ok(env
        .durable_object("TRAKT_COORDINATOR")?
        .id_from_name(id)?
        .get_stub()?
        .fetch_with_request(req)
        .await?)
}
async fn call_json(env: &Env, id: &str, path: &str, value: Value) -> Result<Value> {
    let mut r = call(env, id, path, value).await?;
    if r.status_code() >= 400 {
        return Err(ApiError::new(r.status_code(), "invalid_oauth_request"));
    }
    Ok(r.json().await?)
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
}
async fn registration(env: &Env, id: &str) -> Result<Value> {
    if !valid_id(id) {
        return Err(ApiError::new(400, "invalid_client"));
    }
    call_json(env, &format!("client:{id}"), "/_client", json!({})).await
}
/// Fetch a Client ID Metadata Document: no redirects, a short timeout, and a 5 KiB
/// cap, as the draft recommends against server-side request abuse.
async fn client_metadata(client_id: &str) -> Result<Value> {
    let invalid = || ApiError::new(400, "invalid_client_metadata");
    oauth::check_metadata_url(client_id)?;
    let headers = Headers::new();
    headers.set("Accept", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get)
        .with_headers(headers)
        .with_redirect(RequestRedirect::Manual);
    let req = Request::new_with_init(client_id, &init)?;
    let mut res = Fetch::Request(req)
        .send_with_signal(&AbortSignal::from(
            worker::worker_sys::web_sys::AbortSignal::timeout_with_u32(5000),
        ))
        .await
        .map_err(|_| invalid())?;
    if res.status_code() != 200 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    let mut stream = res.stream().map_err(|_| invalid())?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| invalid())?;
        if bytes.len() + chunk.len() > 5 * 1024 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    Ok(serde_json::to_value(oauth::metadata_registration(
        client_id, &document,
    )?)?)
}
async fn rate(env: &Env, req: &Request) -> Result<()> {
    let ip = req
        .headers()
        .get("CF-Connecting-IP")?
        .unwrap_or("local".into());
    let (bucket, limit) = security::rate_bucket(&ip);
    let r = call(
        env,
        &format!("rate:{bucket}"),
        "/_rate",
        json!({"limit": limit}),
    )
    .await?;
    if r.status_code() != 200 {
        return Err(ApiError::new(429, "rate_limited"));
    }
    Ok(())
}
async fn form(req: &mut Request) -> Result<Value> {
    let content = req.headers().get("Content-Type")?.unwrap_or_default();
    if content.starts_with("application/json") {
        return Ok(req.json::<Value>().await?);
    }
    if !content.starts_with("application/x-www-form-urlencoded") {
        return Err(ApiError::new(415, "unsupported_media_type"));
    }
    let text = req.text().await?;
    let fake = Request::new(&format!("https://internal/?{text}"), Method::Get)?;
    query(&fake)
}
pub async fn route(mut req: Request, env: Env) -> Result<Response> {
    let base = crate::config::base(&env)?;
    let path = req.path();
    let method = req.method();
    if method == Method::Get {
        match path.as_str() {
            "/" => {
                return Ok(Response::from_html(format!(
                    "<!doctype html><title>Trakt MCP</title><h1>Trakt MCP</h1><p>Connect your own Trakt account through your MCP client at <code>{}/mcp</code>.</p><p><a href='/openapi.json'>HTTP API schema</a> · <a href='/data-privacy'>Privacy</a></p>",
                    security::escape(&base)
                ))?);
            }
            "/health" => {
                return Ok(Response::from_json(
                    &json!({"status":"ok","service":"trakt-mcp"}),
                )?);
            }
            "/privacy" => {
                // The maintained policy is a documentation page; keep the old URL working.
                let mut r = Response::empty()?.with_status(308);
                r.headers_mut()
                    .set("Location", &format!("{base}/data-privacy"))?;
                return Ok(r);
            }
            "/openapi.json" => {
                let mut spec: Value = serde_json::from_str(include_str!("../openapi.json"))?;
                spec["servers"] = json!([{ "url": base }]);
                let flow = &mut spec["components"]["securitySchemes"]["oauth"]["flows"]["authorizationCode"];
                flow["authorizationUrl"] = json!(format!("{base}/oauth/authorize"));
                flow["tokenUrl"] = json!(format!("{base}/oauth/token"));
                return Ok(Response::from_json(&spec)?);
            }
            "/.well-known/openai-apps-challenge" => {
                let challenge = env
                    .var("OPENAI_APPS_CHALLENGE")
                    .ok()
                    .map(|v| v.to_string())
                    .filter(|v| !v.is_empty() && v.len() <= 4096 && !v.contains(['\r', '\n']))
                    .ok_or(ApiError::new(404, "not_found"))?;
                let mut response = Response::ok(challenge)?;
                response
                    .headers_mut()
                    .set("Content-Type", "text/plain; charset=utf-8")?;
                return Ok(response);
            }
            "/.well-known/ai-plugin.json" => {
                let contact = env
                    .var("SUPPORT_EMAIL")
                    .ok()
                    .map(|v| v.to_string())
                    .filter(|v| !v.is_empty())
                    .ok_or(ApiError::new(404, "legacy_manifest_not_configured"))?;
                return Ok(Response::from_json(
                    &json!({"schema_version":"v1","name_for_human":"Trakt","name_for_model":"trakt","description_for_human":"Your Trakt history, recommendations and search.","description_for_model":"Read your own Trakt watched history, personalized recommendations, and movie/show search.","auth":{"type":"oauth","client_url":format!("{}/oauth/authorize",base),"scope":"trakt:read","authorization_url":format!("{}/oauth/token",base),"authorization_content_type":"application/x-www-form-urlencoded","verification_tokens":{}},"api":{"type":"openapi","url":format!("{}/openapi.json",base)},"logo_url":format!("{}/logo.svg",base),"contact_email":contact,"legal_info_url":format!("{}/data-privacy",base)}),
                )?);
            }
            "/logo.svg" => {
                let mut r = Response::ok(
                    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect width='64' height='64' rx='12' fill='#ef4444'/><path d='M18 16h28v8H36v26h-8V24H18z' fill='white'/></svg>",
                )?;
                r.headers_mut().set("Content-Type", "image/svg+xml")?;
                return Ok(r);
            }
            "/.well-known/oauth-authorization-server"
            | "/.well-known/oauth-authorization-server/mcp"
            | "/.well-known/oauth-authorization-server/sse" => {
                return Ok(Response::from_json(&oauth::metadata(&base))?);
            }
            "/.well-known/oauth-protected-resource"
            | "/.well-known/oauth-protected-resource/mcp"
            | "/.well-known/oauth-protected-resource/sse" => {
                return Ok(Response::from_json(&oauth::resource_metadata(
                    &base, &path,
                ))?);
            }
            "/login.js" => {
                let mut r = Response::ok(oauth::LOGIN_JS)?;
                r.headers_mut().set("Content-Type", "text/javascript")?;
                return Ok(r);
            }
            "/login.css" => {
                let mut r = Response::ok(oauth::LOGIN_CSS)?;
                r.headers_mut().set("Content-Type", "text/css")?;
                return Ok(r);
            }
            "/oauth/authorize" => {
                rate(&env, &req).await?;
                let q = query(&req)?;
                let client_id = q["client_id"]
                    .as_str()
                    .ok_or(ApiError::new(400, "invalid_client"))?;
                let r = if oauth::is_metadata_client_id(client_id) {
                    client_metadata(client_id).await?
                } else {
                    registration(&env, client_id).await?
                };
                let id = uuid::Uuid::new_v4().simple().to_string();
                let out = call_json(
                    &env,
                    &id,
                    "/_begin",
                    json!({"params":q,"registration":r,"_id":id}),
                )
                .await?;
                return Ok(Response::from_html(oauth::login_page(&out))?);
            }
            _ => {}
        }
    }
    if method == Method::Post {
        match path.as_str() {
            "/oauth/register" => {
                rate(&env, &req).await?;
                let id = uuid::Uuid::new_v4().simple().to_string();
                let mut v = req.json::<Value>().await?;
                if !v.is_object() {
                    return Err(ApiError::new(400, "invalid_client_metadata"));
                }
                v["_id"] = json!(id);
                let response = call(&env, &format!("client:{id}"), "/_register", v).await?;
                return Ok(if response.status_code() == 200 {
                    response.with_status(201)
                } else {
                    response
                });
            }
            "/oauth/complete" => {
                let v = req.json::<Value>().await?;
                let id = v["session_id"]
                    .as_str()
                    .filter(|s| valid_id(s))
                    .ok_or(ApiError::new(400, "invalid_flow"))?
                    .to_string();
                return call(&env, &id, "/_browser", v).await;
            }
            "/oauth/token" => {
                let q = form(&mut req).await?;
                let token = q[if q["grant_type"] == "refresh_token" {
                    "refresh_token"
                } else {
                    "code"
                }]
                .as_str()
                .ok_or(ApiError::new(400, "invalid_grant"))?;
                let id = security::token_id(token)?.to_string();
                let r = match q["client_id"].as_str() {
                    // A metadata-document client is public: the session already binds
                    // its URL, and PKCE proves possession, so nothing is fetched here.
                    Some(id) if oauth::is_metadata_client_id(id) => json!({
                        "client_id":id,"client_name":"","redirect_uris":[],
                        "secret_hash":null,"token_endpoint_auth_method":"none"}),
                    Some(id) if !id.is_empty() => registration(&env, id).await?,
                    _ => Value::Null,
                };
                return call(
                    &env,
                    &id,
                    "/_exchange",
                    json!({"params":q,"registration":r}),
                )
                .await;
            }
            "/auth/device/code" if req.headers().get("Authorization")?.is_none() => {
                rate(&env, &req).await?;
                let id = uuid::Uuid::new_v4().simple().to_string();
                return call(&env, &id, "/_device", json!({"_id":id})).await;
            }
            _ => {}
        }
    }
    let allowed = match path.as_str() {
        "/mcp" | "/messages" | "/auth/device/code" | "/auth/device/token" => method == Method::Post,
        "/sse" | "/sync/watched" | "/recommendations" | "/search" => method == Method::Get,
        "/auth/session" => method == Method::Delete,
        _ => return Err(ApiError::new(404, "not_found")),
    };
    if !allowed {
        return Err(ApiError::new(405, "method_not_allowed"));
    }
    let token = req
        .headers()
        .get("Authorization")?
        .and_then(|v| v.strip_prefix("Bearer ").map(String::from))
        .ok_or(ApiError::new(401, "unauthorized"))?;
    let id = security::token_id(&token)?;
    Ok(env
        .durable_object("TRAKT_COORDINATOR")?
        .id_from_name(id)?
        .get_stub()?
        .fetch_with_request(req)
        .await?)
}
pub async fn bounded(mut req: Request) -> Result<Request> {
    if !matches!(req.method(), Method::Post | Method::Put | Method::Patch) {
        return Ok(req);
    }
    if req
        .headers()
        .get("Content-Length")?
        .and_then(|s| s.parse::<usize>().ok())
        .is_some_and(|n| n > 65536)
    {
        return Err(ApiError::new(413, "request_too_large"));
    }
    let mut stream = req.stream()?;
    let mut bytes = Vec::new();
    while let Some(part) = stream.next().await {
        let part = part?;
        if bytes.len() + part.len() > 65536 {
            return Err(ApiError::new(413, "request_too_large"));
        }
        bytes.extend_from_slice(&part);
    }
    let mut init = RequestInit::new();
    init.with_method(req.method())
        .with_headers(req.headers().clone())
        .with_body(Some(js_sys::Uint8Array::from(bytes.as_slice()).into()));
    Ok(Request::new_with_init(req.url()?.as_str(), &init)?)
}
