//! Public deployment identity comes from operator configuration, never request Host.
use crate::error::{ApiError, Result};
use worker::Env;

fn parse_origin(value: &str) -> Result<String> {
    let url =
        worker::Url::parse(value).map_err(|_| ApiError::new(500, "invalid_public_base_url"))?;
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

pub fn base(env: &Env) -> Result<String> {
    let value = env
        .var("PUBLIC_BASE_URL")
        .map_err(|_| ApiError::new(500, "missing_public_base_url"))?
        .to_string();
    parse_origin(&value)
}

pub fn docs_base(env: &Env) -> Result<String> {
    match env.var("DOCS_BASE_URL") {
        Ok(value) => {
            let text = value.to_string();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                base(env)
            } else {
                parse_origin(trimmed)
            }
        }
        Err(_) => base(env),
    }
}

pub fn resource(env: &Env) -> Result<String> {
    Ok(format!("{}/", base(env)?))
}

#[cfg(test)]
mod tests {
    use super::parse_origin;

    #[test]
    fn parse_origin_accepts_https_origins_and_rejects_unsafe_urls() {
        assert_eq!(
            parse_origin("https://trakt.ai.swacktech.com").unwrap(),
            "https://trakt.ai.swacktech.com"
        );
        assert_eq!(
            parse_origin("https://trakt.ai.swacktech.com/").unwrap(),
            "https://trakt.ai.swacktech.com"
        );
        for bad in [
            "http://trakt.ai.swacktech.com",
            "https://user@trakt.ai.swacktech.com",
            "https://trakt.ai.swacktech.com/docs",
            "https://trakt.ai.swacktech.com/?q=1",
            "https://trakt.ai.swacktech.com/#frag",
            "not-a-url",
        ] {
            assert!(parse_origin(bad).is_err(), "{bad}");
        }
    }
}
