use super::client::{Client, Query};
use crate::error::{ApiError, Result};
use serde_json::{Value, json};

pub async fn watched(c: &Client<'_>, token: &str, q: &Query) -> Result<Value> {
    if q.media_type.as_deref().unwrap_or("all") == "all" {
        Ok(json!({"movies":page(c, token, q, "movies").await?,
            "shows":page(c, token, q, "shows").await?}))
    } else {
        page(c, token, q, q.plural()?).await
    }
}

async fn page(c: &Client<'_>, token: &str, q: &Query, media: &str) -> Result<Value> {
    let recent = q.mode.as_deref() == Some("recent");
    let requested = q.page.unwrap_or(1);
    let path = format!(
        "/sync/{}/{media}",
        if recent { "history" } else { "watched" }
    );
    let mut result = c
        .get(
            &path,
            &[
                ("extended", "full".into()),
                ("page", requested.to_string()),
                ("limit", q.limit.unwrap_or(100).to_string()),
            ],
            Some(token),
        )
        .await?;
    if result["pagination"]["has_more"].is_null() {
        // /sync/watched documents a complete array. Some deployments paginate
        // it (observed in production), so prefer headers whenever present.
        // History is explicitly paginated: absent headers cannot prove coverage.
        if recent || requested != 1 {
            return Err(ApiError::new(502, "invalid_trakt_pagination"));
        }
        let count = result["data"].as_array().unwrap().len();
        result["pagination"] = json!({"page":1,"page_count":1,"limit":count,
            "item_count":count,"has_more":false,"next_page":null});
    }
    if q.detail.as_deref() != Some("full") {
        for row in result["data"].as_array_mut().unwrap() {
            compact(row);
        }
    }
    Ok(result)
}

/// Keep the identity, viewing evidence and genre/date fields needed for a profile
/// without overwhelming MCP clients with artwork, translations and plot text.
pub fn compact(row: &mut Value) {
    if let Some(object) = row.as_object_mut() {
        object.retain(|key, _| {
            matches!(
                key.as_str(),
                "movie"
                    | "show"
                    | "episode"
                    | "plays"
                    | "last_watched_at"
                    | "last_updated_at"
                    | "total_count"
                    | "id"
                    | "watched_at"
                    | "action"
                    | "type"
            )
        });
        for key in ["movie", "show", "episode"] {
            if let Some(media) = object.get_mut(key).and_then(Value::as_object_mut) {
                media.retain(|field, _| {
                    matches!(
                        field.as_str(),
                        "ids"
                            | "title"
                            | "year"
                            | "genres"
                            | "released"
                            | "first_aired"
                            | "runtime"
                            | "season"
                            | "number"
                    )
                });
            }
        }
    }
}
