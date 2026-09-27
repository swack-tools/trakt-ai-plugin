use super::{
    client::{Client, Query},
    movies, shows,
};
use crate::error::Result;
use serde_json::{Value, json};
pub async fn watched(c: &Client<'_>, token: &str, q: &Query) -> Result<Value> {
    match q.media_type.as_deref().unwrap_or("all") {
        "all" => Ok(
            json!({"movies":movies::get_watched_movies(c,token).await?,"shows":shows::get_watched_shows(c,token).await?}),
        ),
        _ => {
            if q.plural()? == "movies" {
                movies::get_watched_movies(c, token).await
            } else {
                shows::get_watched_shows(c, token).await
            }
        }
    }
}
