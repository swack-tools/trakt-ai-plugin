use crate::{
    error::{ApiError, Result},
    trakt::catalog::Call,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoverLists {
    view: Option<String>,
    genres: Option<String>,
    page: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListItems {
    owner: String,
    list_id: String,
    media_type: String,
    page: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Calendar {
    target: String,
    media_type: String,
    start_date: String,
    days: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateList {
    name: String,
    description: Option<String>,
    privacy: Option<String>,
    confirmed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeItems {
    list_id: String,
    items: Vec<ListItem>,
    confirmed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListItem {
    media_type: String,
    trakt_id: u64,
}

fn invalid() -> ApiError {
    ApiError::new(400, "invalid_parameters")
}

fn pagination(page: Option<u32>, limit: Option<u32>) -> Result<Value> {
    let (page, limit) = (page.unwrap_or(1), limit.unwrap_or(20));
    if page == 0 || limit == 0 || limit > 100 {
        return Err(invalid());
    }
    Ok(json!({"page":page,"limit":limit}))
}

fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn call(operation_id: &str, path_params: Value, query_params: Value) -> Result<Call> {
    serde_json::from_value(
        json!({"operation_id":operation_id,"path_params":path_params,
        "query_params":query_params}),
    )
    .map_err(|_| invalid())
}

fn write_call(operation_id: &str, path_params: Value, body: Value) -> Result<Call> {
    serde_json::from_value(
        json!({"operation_id":operation_id,"path_params":path_params,
        "body":body,"confirmed":true}),
    )
    .map_err(|_| invalid())
}

fn confirmed(value: Option<bool>) -> Result<()> {
    if value == Some(true) {
        Ok(())
    } else {
        Err(ApiError::new(400, "confirmation_required"))
    }
}

pub fn prepare(name: &str, args: Value) -> Result<Call> {
    match name {
        "trakt_discover_lists" => {
            let input: DiscoverLists = serde_json::from_value(args).map_err(|_| invalid())?;
            let operation_id = match input.view.as_deref().unwrap_or("trending") {
                "trending" => "getListsTrending",
                "popular" => "getListsPopular",
                _ => return Err(invalid()),
            };
            if input.genres.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 200
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b == b'-' || b == b',')
            }) {
                return Err(invalid());
            }
            let mut query = pagination(input.page, input.limit)?;
            if let Some(genres) = input.genres {
                query["genres"] = json!(genres);
            }
            call(operation_id, json!({}), query)
        }
        "trakt_get_list_items" => {
            let input: ListItems = serde_json::from_value(args).map_err(|_| invalid())?;
            if !segment(&input.owner) || !segment(&input.list_id) {
                return Err(invalid());
            }
            let operation_id = match input.media_type.as_str() {
                "movie" => "getUsersListsListItemsMovie",
                "show" => "getUsersListsListItemsShow",
                _ => return Err(invalid()),
            };
            call(
                operation_id,
                json!({"id":input.owner,"list_id":input.list_id}),
                pagination(input.page, input.limit)?,
            )
        }
        "trakt_get_calendar" => {
            let input: Calendar = serde_json::from_value(args).map_err(|_| invalid())?;
            if !matches!(input.target.as_str(), "my" | "all")
                || input.days == 0
                || input.days > 31
                || input.start_date.len() != 10
                || NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d").is_err()
            {
                return Err(invalid());
            }
            let operation_id = match input.media_type.as_str() {
                "movie" => "getCalendarsMovies",
                "show" => "getCalendarsShows",
                _ => return Err(invalid()),
            };
            call(
                operation_id,
                json!({"target":input.target,"start_date":input.start_date,"days":input.days}),
                json!({}),
            )
        }
        "trakt_create_list" => {
            let input: CreateList = serde_json::from_value(args).map_err(|_| invalid())?;
            confirmed(input.confirmed)?;
            let name = input.name.trim();
            if name.is_empty()
                || name.len() > 100
                || input
                    .description
                    .as_ref()
                    .is_some_and(|description| description.len() > 1000)
            {
                return Err(invalid());
            }
            let privacy = input.privacy.as_deref().unwrap_or("private");
            if !matches!(privacy, "private" | "public") {
                return Err(invalid());
            }
            let mut body = json!({"name":name,"privacy":privacy});
            if let Some(description) = input.description {
                body["description"] = json!(description);
            }
            write_call("postUsersListsCreate", json!({"id":"me"}), body)
        }
        "trakt_add_list_items" | "trakt_remove_list_items" => {
            let input: ChangeItems = serde_json::from_value(args).map_err(|_| invalid())?;
            confirmed(input.confirmed)?;
            if !segment(&input.list_id) || input.items.is_empty() || input.items.len() > 100 {
                return Err(invalid());
            }
            let mut seen = HashSet::new();
            let (mut movies, mut shows) = (Vec::new(), Vec::new());
            for item in input.items {
                if item.trakt_id == 0 || !seen.insert((item.media_type.clone(), item.trakt_id)) {
                    return Err(invalid());
                }
                let encoded = json!({"ids":{"trakt":item.trakt_id}});
                match item.media_type.as_str() {
                    "movie" => movies.push(encoded),
                    "show" => shows.push(encoded),
                    _ => return Err(invalid()),
                }
            }
            let operation_id = if name == "trakt_add_list_items" {
                "postUsersListsListAdd"
            } else {
                "postUsersListsListRemove"
            };
            let mut body = json!({});
            if !movies.is_empty() {
                body["movies"] = json!(movies);
            }
            if !shows.is_empty() {
                body["shows"] = json!(shows);
            }
            write_call(
                operation_id,
                json!({"id":"me","list_id":input.list_id}),
                body,
            )
        }
        _ => Err(ApiError::new(404, "unknown_tool")),
    }
}
