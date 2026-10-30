use crate::error::{ApiError, Result};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
pub fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
pub fn hash(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn equal(a: &str, b: &str) -> bool {
    bool::from(hash(a).as_bytes().ct_eq(hash(b).as_bytes()))
}
pub fn token_id(token: &str) -> Result<&str> {
    let (id, secret) = token
        .split_once('.')
        .ok_or(ApiError::new(401, "invalid_token"))?;
    if id.len() != 32
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
        || secret.len() != 64
        || !secret.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(ApiError::new(401, "invalid_token"));
    }
    Ok(id)
}
pub fn pkce(verifier: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = Sha256::digest(verifier.as_bytes());
    let mut out = String::new();
    let mut bits = 0u32;
    let mut count = 0;
    for b in bytes {
        bits = (bits << 8) | b as u32;
        count += 8;
        while count >= 6 {
            count -= 6;
            out.push(ALPHABET[((bits >> count) & 63) as usize] as char);
        }
    }
    if count > 0 {
        out.push(ALPHABET[((bits << (6 - count)) & 63) as usize] as char);
    }
    out
}
pub fn valid_verifier(s: &str) -> bool {
    (43..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn valid_redirect(s: &str) -> bool {
    worker::Url::parse(s).is_ok_and(|u| {
        u.fragment().is_none()
            && u.username().is_empty()
            && u.password().is_none()
            && (u.scheme() == "https"
                || u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
    })
}
/// Loopback redirects match on any port (RFC 8252 section 7.3); all others match exactly.
pub fn redirect_matches(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(a), Ok(b)) = (
        worker::Url::parse(registered),
        worker::Url::parse(requested),
    ) else {
        return false;
    };
    let loopback = |u: &worker::Url| {
        u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    };
    loopback(&a)
        && loopback(&b)
        && a.host_str() == b.host_str()
        && a.path() == b.path()
        && a.query() == b.query()
        && b.fragment().is_none()
}
/// Hosted Claude registers every user's client from Anthropic's published egress
/// range, so that range shares one larger bucket instead of the per-IP limit.
pub fn rate_bucket(ip: &str) -> (String, u32) {
    let anthropic = ip.parse::<std::net::Ipv4Addr>().is_ok_and(|a| {
        u32::from(a) >> 11 == u32::from(std::net::Ipv4Addr::new(160, 79, 104, 0)) >> 11
    });
    if anthropic {
        ("anthropic".into(), 600)
    } else {
        (format!("ip:{}", hash(ip)), 20)
    }
}
