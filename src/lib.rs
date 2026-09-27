mod config;
mod coordinator;
pub mod error;
mod http;
pub mod mcp;
mod oauth;
pub mod security;
pub mod trakt;
pub use coordinator::TraktCoordinator;
use worker::*;
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let base = match config::base(&env) {
        Ok(base) => base,
        Err(e) => return e.response(),
    };
    let origin = req.headers().get("Origin")?;
    if origin.as_deref().is_some_and(|o| {
        o != base
            && !matches!(
                o,
                "https://chatgpt.com" | "https://chat.openai.com" | "https://claude.ai"
            )
    }) {
        return crate::error::ApiError::new(403, "origin_not_allowed").response();
    }
    let method = req.method();
    let mut res = if method == Method::Options {
        Response::empty()?.with_status(204)
    } else {
        match http::bounded(req).await {
            Ok(req) => {
                Router::new()
                    .get_async("/", route_request)
                    .or_else_any_method_async("/*path", route_request)
                    .run(req, env)
                    .await?
            }
            Err(e) => e.response()?,
        }
    };
    let headers = res.headers().clone();
    res = res.with_headers(headers);
    let unauthorized = res.status_code() == 401;
    let h = res.headers_mut();
    if unauthorized {
        h.set(
            "WWW-Authenticate",
            &format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\""),
        )?;
    }
    h.set("Cache-Control", "no-store")?;
    h.set("X-Content-Type-Options", "nosniff")?;
    h.set("Referrer-Policy", "no-referrer")?;
    h.set("Vary", "Origin")?;
    h.set("Content-Security-Policy","default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")?;
    if let Some(o) = origin {
        h.set("Access-Control-Allow-Origin", &o)?;
    }
    h.set("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS")?;
    h.set(
        "Access-Control-Allow-Headers",
        "Authorization, Content-Type, Accept, MCP-Protocol-Version, Mcp-Session-Id, Last-Event-ID",
    )?;
    h.set(
        "Access-Control-Expose-Headers",
        "WWW-Authenticate, Retry-After",
    )?;
    Ok(res)
}

async fn route_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match http::route(req, ctx.env).await {
        Ok(r) => Ok(r),
        Err(e) => e.response(),
    }
}
