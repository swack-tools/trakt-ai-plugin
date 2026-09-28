use super::client::{Client, Query};
use crate::error::{ApiError, Result};
use serde_json::Value;
pub async fn search_media(c: &Client<'_>, q: &Query) -> Result<Value> {
    if q.query.is_none() {
        return Err(ApiError::new(400, "query_required"));
    }
    if q.mode.is_some() || q.detail.is_some() {
        return Err(ApiError::new(400, "invalid_parameters"));
    }
    let media = match q.media_type.as_deref().unwrap_or("all") {
        "all" => "movie,show",
        _ => {
            if q.plural()? == "movies" {
                "movie"
            } else {
                "show"
            }
        }
    };
    let mut pairs = q.pairs();
    pairs.retain(|(k, _)| !matches!(*k, "years" | "genres"));
    let mut result = c.get(&format!("/search/{media}"), &pairs, None).await?;
    // Current search schema does not advertise genre/year filters. Filter this page explicitly.
    if let Some(rows) = result["data"].as_array_mut() {
        rows.retain(|r| q.matches(&r[r["type"].as_str().unwrap_or("movie")]));
    }
    result["filters_applied_to_page"] = Value::Bool(q.genres.is_some() || q.years.is_some());
    Ok(result)
}
