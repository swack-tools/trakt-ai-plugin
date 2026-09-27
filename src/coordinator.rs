use crate::{
    error::{ApiError, Result as ApiResult},
    mcp, oauth, security,
    trakt::{
        auth::{self, get_optional, now},
        client::Client,
    },
};
use futures::{channel::mpsc, lock::Mutex};
use serde_json::{Value, json};
use std::{collections::HashMap, rc::Rc};
use worker::*;
#[durable_object]
pub struct TraktCoordinator {
    state: State,
    env: Env,
    lock: Rc<Mutex<()>>,
    streams: HashMap<String, mpsc::Sender<std::result::Result<Vec<u8>, worker::Error>>>,
}
#[durable_object]
impl DurableObject for TraktCoordinator {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            lock: Rc::new(Mutex::new(())),
            streams: HashMap::new(),
        }
    }
    async fn fetch(&mut self, req: Request) -> worker::Result<Response> {
        let lock = self.lock.clone();
        let _guard = lock.lock().await;
        match self.handle(req).await {
            Ok(r) => Ok(r),
            Err(e) => e.response(),
        }
    }
}
impl TraktCoordinator {
    async fn handle(&mut self, mut req: Request) -> ApiResult<Response> {
        let path = req.path();
        let mut storage = self.state.storage();
        match path.as_str() {
            "/_rate" => {
                let mut rate = get_optional::<(u64, u32)>(&mut storage, "rate")
                    .await?
                    .unwrap_or((now(), 0));
                if now() >= rate.0 + 60 {
                    rate = (now(), 0);
                }
                rate.1 += 1;
                storage.put("rate", rate).await?;
                if rate.1 > 20 {
                    return Err(ApiError::new(429, "rate_limited"));
                }
                return Ok(Response::empty()?);
            }
            "/_register" => {
                let v = req.json().await?;
                return Ok(Response::from_json(
                    &oauth::register(&mut storage, v).await?,
                )?);
            }
            "/_client" => {
                let r = get_optional::<oauth::Registration>(&mut storage, "registration")
                    .await?
                    .ok_or(ApiError::new(400, "invalid_client"))?;
                return Ok(Response::from_json(&r)?);
            }
            "/_begin" => {
                let v = req.json().await?;
                return Ok(Response::from_json(
                    &oauth::begin(&self.env, &mut storage, v).await?,
                )?);
            }
            "/_browser" => {
                let v: Value = req.json().await?;
                let id = v["session_id"]
                    .as_str()
                    .ok_or(ApiError::new(400, "invalid_flow"))?
                    .to_string();
                return Ok(Response::from_json(
                    &oauth::browser_step(&self.env, &mut storage, &id, v).await?,
                )?);
            }
            "/_exchange" => {
                let v = req.json().await?;
                return Ok(Response::from_json(
                    &oauth::exchange(&self.env, &mut storage, v).await?,
                )?);
            }
            "/_device" => {
                let v: Value = req.json().await?;
                let id = v["_id"].as_str().ok_or(ApiError::new(500, "missing_id"))?;
                let token =
                    oauth::create(&mut storage, id, None, crate::config::resource(&self.env)?)
                        .await?;
                let mut d =
                    auth::request_device_code(&Client { env: &self.env }, &mut storage).await?;
                d["session_token"] = json!(token);
                return Ok(Response::from_json(&d)?);
            }
            _ => {}
        }
        let token = req
            .headers()
            .get("Authorization")?
            .and_then(|v| v.strip_prefix("Bearer ").map(String::from))
            .ok_or(ApiError::new(401, "unauthorized"))?;
        let session = oauth::authenticate(&mut storage, &token).await?;
        let mut rate = get_optional::<(u64, u32)>(&mut storage, "authenticated_rate")
            .await?
            .unwrap_or((now(), 0));
        if now() >= rate.0 + 60 {
            rate = (now(), 0);
        }
        rate.1 += 1;
        storage.put("authenticated_rate", rate).await?;
        if rate.1 > 120 {
            let mut e = ApiError::new(429, "rate_limited");
            e.retry_after = Some(60);
            return Err(e);
        }
        if path == "/auth/session" {
            self.streams.clear();
            self.env
                .kv("TRAKT_SESSIONS")?
                .delete(&format!("user:{}:tokens", session.id))
                .await?;
            storage.delete_all().await?;
            return Ok(Response::empty()?.with_status(204));
        }
        if path == "/sse" {
            self.streams.retain(|_, s| !s.is_closed());
            if self.streams.len() >= 8 {
                return Err(ApiError::new(429, "too_many_streams"));
            }
            let channel = security::random();
            let (mut tx, rx) = mpsc::channel(2);
            tx.try_send(Ok(format!(
                "event: endpoint\ndata: /messages?session_id={channel}\n\n"
            )
            .into_bytes()))
                .map_err(|_| ApiError::new(500, "stream_error"))?;
            self.streams.insert(channel, tx);
            let mut r = Response::from_stream(rx)?;
            r.headers_mut().set("Content-Type", "text/event-stream")?;
            r.headers_mut().set("Cache-Control", "no-store")?;
            return Ok(r);
        }
        if matches!(path.as_str(), "/mcp" | "/messages") {
            let channel = if path == "/messages" {
                let id = req
                    .url()?
                    .query_pairs()
                    .find(|(k, _)| k == "session_id")
                    .map(|(_, v)| v.into_owned())
                    .ok_or(ApiError::new(400, "missing_sse_session"))?;
                if self.streams.get(&id).is_none_or(|tx| tx.is_closed()) {
                    return Err(ApiError::new(404, "sse_session_expired"));
                }
                Some(id)
            } else {
                None
            };
            let parsed = req.json::<Value>().await;
            let reply = match parsed {
                Ok(v) => mcp::handlers::handle(&self.env, &mut storage, &session.id, v).await,
                Err(_) => Some(mcp::protocol::error(Value::Null, -32700, "Parse error")),
            };
            if let Some(channel) = channel {
                if let Some(v) = reply {
                    let event = format!("event: message\ndata: {v}\n\n").into_bytes();
                    if event.len() > 1024 * 1024 {
                        self.streams.remove(&channel);
                        return Err(ApiError::new(413, "sse_response_too_large"));
                    }
                    let failed = self
                        .streams
                        .get_mut(&channel)
                        .unwrap()
                        .try_send(Ok(event))
                        .is_err();
                    if failed {
                        self.streams.remove(&channel);
                        return Err(ApiError::new(429, "slow_sse_consumer"));
                    }
                }
                return Ok(Response::empty()?.with_status(202));
            }
            return Ok(match reply {
                Some(v) => Response::from_json(&v)?,
                None => Response::empty()?.with_status(202),
            });
        }
        let (name, args) = match path.as_str() {
            "/auth/device/code" => ("trakt_request_login", json!({})),
            "/auth/device/token" => ("trakt_confirm_login", req.json::<Value>().await?),
            "/sync/watched" => ("trakt_get_watched_history", crate::http::query(&req)?),
            "/recommendations" => ("trakt_get_recommendations", crate::http::query(&req)?),
            "/search" => ("trakt_search", crate::http::query(&req)?),
            _ => return Err(ApiError::new(404, "not_found")),
        };
        let mut data =
            mcp::handlers::operation(&self.env, &mut storage, &session.id, name, args).await?;
        if path == "/auth/device/token" {
            data = oauth::issue(&mut storage).await?;
        }
        Ok(Response::from_json(&data)?)
    }
}
