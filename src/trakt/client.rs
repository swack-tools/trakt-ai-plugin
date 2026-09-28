use crate::error::{ApiError, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit, Url};

#[derive(Default, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub media_type: Option<String>,
    pub mode: Option<String>,
    pub detail: Option<String>,
    pub query: Option<String>,
    pub genres: Option<String>,
    pub years: Option<String>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
}
impl Query {
    pub fn parse(v: Value) -> Result<Self> {
        let q: Self =
            serde_json::from_value(v).map_err(|_| ApiError::new(400, "invalid_parameters"))?;
        if q.limit.is_some_and(|n| n == 0 || n > 100) || q.page == Some(0) {
            return Err(ApiError::new(400, "invalid_pagination"));
        }
        if q.mode
            .as_deref()
            .is_some_and(|m| !matches!(m, "all" | "recent"))
        {
            return Err(ApiError::new(400, "invalid_mode"));
        }
        if q.detail
            .as_deref()
            .is_some_and(|d| !matches!(d, "compact" | "full"))
        {
            return Err(ApiError::new(400, "invalid_detail"));
        }
        if q.media_type
            .as_deref()
            .is_some_and(|t| !matches!(t, "movie" | "show" | "movies" | "shows" | "all"))
        {
            return Err(ApiError::new(400, "invalid_media_type"));
        }
        if q.query
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 500)
        {
            return Err(ApiError::new(400, "invalid_query"));
        }
        if q.genres.as_ref().is_some_and(|s| {
            s.len() > 200
                || s.is_empty()
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c == b'-' || c == b',')
        }) {
            return Err(ApiError::new(400, "invalid_genres"));
        }
        if let Some(s) = &q.years {
            let years: Vec<_> = s.split('-').collect();
            if years.is_empty()
                || years.len() > 2
                || years.iter().any(|y| {
                    y.len() != 4
                        || y.parse::<u32>()
                            .map_or(true, |n| !(1800..=2200).contains(&n))
                })
                || years.len() == 2 && years[0] > years[1]
            {
                return Err(ApiError::new(400, "invalid_years"));
            }
        }
        Ok(q)
    }
    pub fn plural(&self) -> Result<&str> {
        match self.media_type.as_deref().unwrap_or("movies") {
            "movie" | "movies" => Ok("movies"),
            "show" | "shows" => Ok("shows"),
            _ => Err(ApiError::new(400, "invalid_media_type")),
        }
    }
    pub fn pairs(&self) -> Vec<(&str, String)> {
        let mut q = vec![
            ("extended", "full".into()),
            ("limit", self.limit.unwrap_or(20).to_string()),
            ("page", self.page.unwrap_or(1).to_string()),
        ];
        for (k, v) in [
            ("query", &self.query),
            ("genres", &self.genres),
            ("years", &self.years),
        ] {
            if let Some(s) = v {
                q.push((k, s.clone()));
            }
        }
        q
    }
    pub fn matches(&self, item: &Value) -> bool {
        let genres = self.genres.as_ref().is_none_or(|g| {
            g.split(',').any(|g| {
                item["genres"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(g)))
            })
        });
        let years = self.years.as_ref().is_none_or(|y| {
            let mut p = y.split('-');
            let lo = p.next().unwrap().parse::<u64>().unwrap();
            let hi = p.next().map(|y| y.parse::<u64>().unwrap()).unwrap_or(lo);
            item["year"].as_u64().is_some_and(|n| n >= lo && n <= hi)
        });
        genres && years
    }
}
pub struct Upstream {
    pub status: u16,
    pub data: Value,
    pub headers: Headers,
}
pub struct Client<'a> {
    pub env: &'a Env,
}
impl<'a> Client<'a> {
    pub async fn send(
        &self,
        path: &str,
        method: Method,
        query: &[(&str, String)],
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Upstream> {
        let base = self
            .env
            .var(if path.starts_with("/oauth/") {
                "TRAKT_AUTH_URL"
            } else {
                "TRAKT_API_URL"
            })?
            .to_string();
        let mut url = Url::parse(&format!("{}{path}", base.trim_end_matches('/')))
            .map_err(|_| ApiError::new(500, "invalid_upstream_configuration"))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;
        headers.set(
            "User-Agent",
            &format!("trakt-mcp/1.0 (+{})", crate::config::base(self.env)?),
        )?;
        headers.set("trakt-api-version", "2")?;
        headers.set(
            "trakt-api-key",
            &self.env.secret("TRAKT_CLIENT_ID")?.to_string(),
        )?;
        if let Some(t) = token {
            headers.set("Authorization", &format!("Bearer {t}"))?;
        }
        let mut init = RequestInit::new();
        init.with_method(method)
            .with_headers(headers)
            .with_redirect(worker::RequestRedirect::Manual);
        if let Some(b) = body {
            init.with_body(Some(serde_json::to_string(b)?.into()));
        }
        let req = Request::new_with_init(url.as_str(), &init)?;
        let mut res = Fetch::Request(req)
            .send_with_signal(&worker::AbortSignal::from(
                worker::worker_sys::web_sys::AbortSignal::timeout_with_u32(20000),
            ))
            .await
            .map_err(|_| ApiError::new(502, "trakt_unavailable"))?;
        let status = res.status_code();
        let headers = res.headers().clone();
        // Successful deletes commonly have no response stream. Do not report
        // a runtime failure after Trakt has already applied the mutation.
        if matches!(status, 204 | 205) {
            return Ok(Upstream {
                status,
                data: Value::Null,
                headers,
            });
        }
        let mut bytes = Vec::new();
        let mut stream = res.stream()?;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                return Err(ApiError::new(502, "trakt_response_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let data = if bytes.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                Err(_) if (200..300).contains(&status) => {
                    return Err(ApiError::new(502, "invalid_trakt_response"));
                }
                Err(_) => Value::Null,
            }
        };
        Ok(Upstream {
            status,
            data,
            headers,
        })
    }
    pub async fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
        token: Option<&str>,
    ) -> Result<Value> {
        let r = self.send(path, Method::Get, query, token, None).await?;
        if r.status != 200 {
            return Err(upstream_error(&r));
        }
        if !r.data.is_array() {
            return Err(ApiError::new(502, "invalid_trakt_response"));
        }
        let requested_page = query
            .iter()
            .find(|(k, _)| *k == "page")
            .and_then(|(_, v)| v.parse::<u64>().ok())
            .unwrap_or(1);
        let pagination =
            response_pagination(&r.headers, requested_page, r.data.as_array().unwrap().len())?;
        Ok(serde_json::json!({"data":r.data,"pagination":pagination}))
    }
}
pub fn upstream_error(r: &Upstream) -> ApiError {
    let mut e = match r.status {
        401 => ApiError::new(401, "trakt_login_required"),
        403 => ApiError::new(403, "trakt_forbidden"),
        404 => ApiError::new(404, "trakt_not_found"),
        400 | 422 => ApiError::new(400, "trakt_validation_failed"),
        409 => ApiError::new(409, "trakt_conflict"),
        402 | 420 => ApiError::new(403, "trakt_subscription_required"),
        429 => ApiError::new(429, "trakt_rate_limited"),
        _ => ApiError::new(502, "trakt_upstream_error"),
    };
    e.retry_after = r
        .headers
        .get("Retry-After")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok());
    e
}

/// Never infer completion from a short/filtered page. Trakt may clamp the limit.
/// Missing headers are unknown here; only an endpoint with a documented complete
/// unpaginated response can explicitly normalize that case.
pub fn pagination(numbers: [Option<u64>; 4], requested: u64, length: usize) -> Result<Value> {
    let [page, pages, limit, total] = numbers;
    if numbers.iter().all(Option::is_none) {
        return Ok(
            serde_json::json!({"page":null,"page_count":null,"limit":null,
            "item_count":null,"has_more":null,"next_page":null}),
        );
    }
    let invalid = || ApiError::new(502, "invalid_trakt_pagination");
    let (page, pages, limit, total) = (
        page.ok_or_else(invalid)?,
        pages.ok_or_else(invalid)?,
        limit.ok_or_else(invalid)?,
        total.ok_or_else(invalid)?,
    );
    if page == 0
        || page != requested
        || limit == 0
        || length as u64 > limit
        || (pages == 0 && (total != 0 || length != 0 || page != 1))
        || (pages > 0 && page > pages)
        || (page < pages && length as u64 != limit)
        || (total == 0 && length != 0)
    {
        return Err(invalid());
    }
    // A last-page flag is not proof of completeness when total/count headers
    // contradict it. Wide arithmetic also handles malicious u64 header values.
    if total > 0 && pages != total.div_ceil(limit)
        || pages > 0
            && page == pages
            && (page as u128 - 1) * (limit as u128) + (length as u128) != total as u128
    {
        return Err(invalid());
    }
    let more = page < pages;
    Ok(
        serde_json::json!({"page":page,"page_count":pages,"limit":limit,
        "item_count":total,"has_more":more,"next_page":if more {page.checked_add(1)} else {None}}),
    )
}

pub fn response_pagination(headers: &Headers, requested: u64, length: usize) -> Result<Value> {
    let mut numbers = [None; 4];
    for (slot, name) in numbers.iter_mut().zip([
        "X-Pagination-Page",
        "X-Pagination-Page-Count",
        "X-Pagination-Limit",
        "X-Pagination-Item-Count",
    ]) {
        if let Some(value) = headers.get(name)? {
            *slot = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| ApiError::new(502, "invalid_trakt_pagination"))?,
            );
        }
    }
    pagination(numbers, requested, length)
}
