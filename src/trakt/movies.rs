use super::client::Client;
use crate::error::Result;
use serde_json::Value;
pub async fn get_watched_movies(c: &Client<'_>, token: &str) -> Result<Value> {
    c.get(
        "/sync/watched/movies",
        &[("extended", "full".into())],
        Some(token),
    )
    .await
}
