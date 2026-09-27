//! Public deployment identity comes from operator configuration, never request Host.
use crate::error::{ApiError, Result};
use worker::Env;
pub fn base(env: &Env) -> Result<String> {
    let value = env
        .var("PUBLIC_BASE_URL")
        .map_err(|_| ApiError::new(500, "missing_public_base_url"))?
        .to_string();
    let url =
        worker::Url::parse(&value).map_err(|_| ApiError::new(500, "invalid_public_base_url"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ApiError::new(500, "invalid_public_base_url"));
    }
    Ok(url.origin().ascii_serialization())
}
pub fn resource(env: &Env) -> Result<String> {
    Ok(format!("{}/", base(env)?))
}
