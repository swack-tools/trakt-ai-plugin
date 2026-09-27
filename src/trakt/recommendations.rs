use super::client::{Client, Query};
use crate::error::Result;
use serde_json::Value;
pub async fn get_recommendations(c: &Client<'_>, token: &str, q: &Query) -> Result<Value> {
    let mut pairs = q.pairs();
    pairs.retain(|(k, _)| !matches!(*k, "page" | "query"));
    c.get(
        &format!("/recommendations/{}", q.plural()?),
        &pairs,
        Some(token),
    )
    .await
}
