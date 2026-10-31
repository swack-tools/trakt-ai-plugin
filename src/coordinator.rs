use crate::{
    error::{ApiError, Result as ApiResult},
    grant, mcp, oauth, retention, seal, security,
    trakt::{
        auth::{self, Conn, get_optional, now},
        client::Client,
    },
};
use futures::{channel::mpsc, lock::Mutex};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use worker::*;
type SseStreams = HashMap<String, mpsc::Sender<std::result::Result<Vec<u8>, worker::Error>>>;

#[durable_object]
pub struct TraktStore {
    state: State,
    env: Env,
    lock: Rc<Mutex<()>>,
    streams: RefCell<SseStreams>,
}
impl DurableObject for TraktStore {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            lock: Rc::new(Mutex::new(())),
            streams: RefCell::new(HashMap::new()),
        }
    }
    async fn fetch(&self, req: Request) -> worker::Result<Response> {
        let lock = self.lock.clone();
        let _guard = lock.lock().await;
        match self.handle(req).await {
            Ok(r) => Ok(r),
            Err(e) => e.response(),
        }
    }
    async fn alarm(&self) -> worker::Result<Response> {
        let lock = self.lock.clone();
        let _guard = lock.lock().await;
        // A failed alarm must return an error so the runtime retries the deletion.
        let purged = retention::alarm(&self.state)
            .await
            .map_err(|e| worker::Error::RustError(e.code.to_string()))?;
        if purged {
            self.streams.borrow_mut().clear();
        }
        Response::ok("")
    }
}
impl TraktStore {
    async fn handle(&self, mut req: Request) -> ApiResult<Response> {
        let path = req.path();
        let mut storage = self.state.storage();
        if path.starts_with("/_grant/") {
            let v: Value = req.json().await?;
            return Ok(Response::from_json(
                &grant::handle(&self.env, &self.state, &path, v).await,
            )?);
        }
        match path.as_str() {
            "/_purge" => {
                self.streams.borrow_mut().clear();
                storage.delete_all().await?;
                return Ok(Response::empty()?);
            }
            "/_rate" => {
                let v: Value = req.json().await?;
                let limit = v["limit"].as_u64().unwrap_or(20).min(1000) as u32;
                let mut rate = get_optional::<(u64, u32)>(&mut storage, "rate")
                    .await?
                    .unwrap_or((now(), 0));
                if now() >= rate.0 + 60 {
                    rate = (now(), 0);
                }
                rate.1 += 1;
                storage.put("rate", rate).await?;
                retention::extend(&self.state, retention::UNFINISHED_SECONDS, false).await?;
                if rate.1 > limit {
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
                let begun = oauth::begin(&self.env, &mut storage, v).await?;
                retention::extend(&self.state, retention::UNFINISHED_SECONDS, true).await?;
                return Ok(Response::from_json(&begun)?);
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
            "/_trakt_callback" => {
                let v: Value = req.json().await?;
                let id = v["session_id"]
                    .as_str()
                    .ok_or(ApiError::new(400, "invalid_flow"))?
                    .to_string();
                return Ok(Response::from_json(
                    &oauth::trakt_callback(&self.env, &self.state, &mut storage, &id, v).await?,
                )?);
            }
            "/_exchange" => {
                let v = req.json().await?;
                return Ok(Response::from_json(
                    &oauth::exchange(&self.env, &self.state, &mut storage, v).await?,
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
                retention::extend(&self.state, retention::UNFINISHED_SECONDS, true).await?;
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
        // A standalone device session has no key until its login finishes.
        let conn = Conn {
            id: session.id.clone(),
            key: seal::Keys::from_env(&self.env)?.open_token("access", &token, now()),
            grant: session.grant.clone(),
        };
        if session.refresh_hash.is_some() {
            retention::touch(&self.env, &self.state, false).await?;
        }
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
            return self.sign_out(&conn, &mut storage).await;
        }
        if path == "/sse" {
            self.streams.borrow_mut().retain(|_, s| !s.is_closed());
            if self.streams.borrow().len() >= 8 {
                return Err(ApiError::new(429, "too_many_streams"));
            }
            let channel = security::random();
            let (mut tx, rx) = mpsc::channel(2);
            tx.try_send(Ok(format!(
                "event: endpoint\ndata: /messages?session_id={channel}\n\n"
            )
            .into_bytes()))
                .map_err(|_| ApiError::new(500, "stream_error"))?;
            self.streams.borrow_mut().insert(channel, tx);
            let mut r = Response::from_stream(rx)?;
            r.headers_mut().set("Content-Type", "text/event-stream")?;
            r.headers_mut().set("Cache-Control", "no-store")?;
            return Ok(r);
        }
        if matches!(path.as_str(), "/mcp" | "/messages") {
            if path == "/mcp"
                && !mcp::protocol::supported_version_header(
                    req.headers().get("MCP-Protocol-Version")?.as_deref(),
                )
            {
                return Err(ApiError::new(400, "unsupported_protocol_version"));
            }
            let channel = if path == "/messages" {
                let id = req
                    .url()?
                    .query_pairs()
                    .find(|(k, _)| k == "session_id")
                    .map(|(_, v)| v.into_owned())
                    .ok_or(ApiError::new(400, "missing_sse_session"))?;
                if self
                    .streams
                    .borrow()
                    .get(&id)
                    .is_none_or(|tx| tx.is_closed())
                {
                    return Err(ApiError::new(404, "sse_session_expired"));
                }
                Some(id)
            } else {
                None
            };
            let parsed = req.json::<Value>().await;
            let reply = match parsed {
                Ok(v) => {
                    mcp::handlers::handle(&self.env, &self.state, &mut storage, &conn, v).await
                }
                Err(_) => Some(mcp::protocol::error(Value::Null, -32700, "Parse error")),
            };
            if let Some(channel) = channel {
                if let Some(v) = reply {
                    let event = format!("event: message\ndata: {v}\n\n").into_bytes();
                    // Tool results include both the legacy JSON text and a structured
                    // copy. Allow the former 1 MiB text payload plus its duplicate,
                    // while retaining a finite per-event bound for slow consumers.
                    if event.len() > 3 * 1024 * 1024 {
                        self.streams.borrow_mut().remove(&channel);
                        return Err(ApiError::new(413, "sse_response_too_large"));
                    }
                    let failed = self
                        .streams
                        .borrow_mut()
                        .get_mut(&channel)
                        .unwrap()
                        .try_send(Ok(event))
                        .is_err();
                    if failed {
                        self.streams.borrow_mut().remove(&channel);
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
        if path == "/auth/device/token" {
            return self
                .finish_device_session(&conn, &mut storage, &mut req)
                .await;
        }
        let (name, args) = match path.as_str() {
            "/auth/device/code" => ("trakt_request_login", json!({})),
            "/sync/watched" => ("trakt_get_watched_history", crate::http::query(&req)?),
            "/recommendations" => ("trakt_get_recommendations", crate::http::query(&req)?),
            "/search" => ("trakt_search", crate::http::query(&req)?),
            _ => return Err(ApiError::new(404, "not_found")),
        };
        let data =
            mcp::handlers::operation(&self.env, &self.state, &mut storage, &conn, name, args)
                .await?;
        Ok(Response::from_json(&data)?)
    }

    /// Standalone device login: a fresh key unless this connection already has one.
    async fn finish_device_session(
        &self,
        conn: &Conn,
        storage: &mut Storage,
        req: &mut Request,
    ) -> ApiResult<Response> {
        let args: Value = req.json().await?;
        let code = args["device_code"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or(ApiError::new(400, "device_code_required"))?;
        let key = conn.key.unwrap_or_else(seal::random_key);
        let c = Client { env: &self.env };
        oauth::finish_device_login(&c, &self.state, storage, &conn.id, code, &key).await?;
        let issued = oauth::issue(&self.env, &self.state, storage, &key).await?;
        Ok(Response::from_json(&issued)?)
    }

    /// Sign-out ends every client of this Trakt account, because they share one grant.
    /// A rate-limited Trakt revocation is reported, and local deletion still happens.
    async fn sign_out(&self, conn: &Conn, storage: &mut Storage) -> ApiResult<Response> {
        let mut reply = None;
        if let (Some(grant_id), Some(key)) = (&conn.grant, &conn.key) {
            let out = grant::request(
                &self.env,
                grant_id,
                "/_grant/revoke",
                json!({"k":grant::wire_key(key)}),
            )
            .await?;
            for other in out["connections"].as_array().into_iter().flatten() {
                if let Some(other) = other.as_str().filter(|other| *other != conn.id) {
                    let _ = crate::http::call(&self.env, other, "/_purge", json!({})).await;
                }
            }
            if out["revoked"] == false {
                reply = Some(
                    json!({"trakt_revoked":false,"retry_after":out["retry_after"],
                    "error_description":"Your records were deleted, but Trakt did not confirm revoking access. Revoke this app in your Trakt settings."}),
                );
            }
        }
        self.streams.borrow_mut().clear();
        storage.delete_all().await?;
        Ok(match reply {
            Some(v) => Response::from_json(&v)?,
            None => Response::empty()?.with_status(204),
        })
    }
}
