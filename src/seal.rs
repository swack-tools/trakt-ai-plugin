//! Encryption for Trakt grants and the connection key K that protects them.
//! K is never stored: it travels sealed inside plugin credentials, so a storage
//! dump alone cannot decrypt a grant.
use crate::error::{ApiError, Result};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub type Key = [u8; 32];
/// A retired sealing key can still open tokens for as long as a refresh token lives.
pub const RETIRED_KEY_SECONDS: u64 = 30 * 86400;
const KEY_PREFIX: &str = "tck";

#[derive(Deserialize)]
struct RawKey {
    kid: String,
    key: String,
    #[serde(default)]
    retired_at: Option<u64>,
}

#[derive(Deserialize)]
struct RawKeys {
    index: String,
    keys: Vec<RawKey>,
}

struct SealKey {
    kid: String,
    key: Key,
    retired_at: Option<u64>,
}

pub struct Keys {
    index: Key,
    keys: Vec<SealKey>,
}

fn config_error() -> ApiError {
    ApiError::new(500, "invalid_seal_configuration")
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn key_from_hex(text: &str) -> Option<Key> {
    from_hex(text)?.try_into().ok()
}

/// 256 bits hashed from three v4 UUIDs, 366 random bits from the platform's CSPRNG.
pub fn random_key() -> Key {
    let mut hasher = Sha256::new();
    for _ in 0..3 {
        hasher.update(uuid::Uuid::new_v4().as_bytes());
    }
    hasher.finalize().into()
}

fn encrypt(key: &Key, aad: &str, plain: &[u8]) -> Result<String> {
    let nonce_source = random_key();
    let nonce = &nonce_source[..12];
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| config_error())?;
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plain,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| config_error())?;
    Ok(format!("{}{}", to_hex(nonce), to_hex(&sealed)))
}

fn decrypt(key: &Key, aad: &str, text: &str) -> Option<Vec<u8>> {
    let bytes = from_hex(text)?;
    if bytes.len() < 12 {
        return None;
    }
    let (nonce, sealed) = bytes.split_at(12);
    Aes256Gcm::new_from_slice(key)
        .ok()?
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: aad.as_bytes(),
            },
        )
        .ok()
}

fn hmac_hex(key: &Key, message: &str) -> Result<String> {
    let mut mac =
        <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(key).map_err(|_| config_error())?;
    mac.update(message.as_bytes());
    Ok(to_hex(&mac.finalize().into_bytes()))
}

impl Keys {
    /// The secret is JSON: an index key, plus sealing keys whose first unretired entry
    /// seals new tokens. Retired keys only open tokens, and only for 30 days.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawKeys = serde_json::from_str(text).map_err(|_| config_error())?;
        let index = key_from_hex(&raw.index).ok_or_else(config_error)?;
        let keys = raw
            .keys
            .into_iter()
            .map(|k| {
                let valid = !k.kid.is_empty() && k.kid.bytes().all(|b| b.is_ascii_alphanumeric());
                Some(SealKey {
                    key: key_from_hex(&k.key).filter(|_| valid)?,
                    kid: k.kid,
                    retired_at: k.retired_at,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(config_error)?;
        if !keys.iter().any(|k| k.retired_at.is_none()) {
            return Err(config_error());
        }
        Ok(Self { index, keys })
    }

    pub fn from_env(env: &worker::Env) -> Result<Self> {
        Self::parse(
            &env.secret("TOKEN_SEAL_KEYS")
                .map_err(|_| config_error())?
                .to_string(),
        )
    }

    fn active(&self) -> Result<&SealKey> {
        self.keys
            .iter()
            .find(|k| k.retired_at.is_none())
            .ok_or_else(config_error)
    }

    fn usable(&self, kid: &str, now: u64) -> Option<&SealKey> {
        self.keys.iter().find(|k| {
            k.kid == kid
                && k.retired_at
                    .is_none_or(|at| now <= at.saturating_add(RETIRED_KEY_SECONDS))
        })
    }

    /// Unsalted hashes of small user IDs or IP addresses could be reversed by guessing.
    pub fn grant_id(&self, user: &str) -> Result<String> {
        hmac_hex(&self.index, &format!("grant:{user}"))
    }

    pub fn ip_id(&self, ip: &str) -> Result<String> {
        hmac_hex(&self.index, &format!("ip:{ip}"))
    }

    /// `{connection}.{kid}.{sealed}`. The connection ID routes the request; the rest
    /// is opaque to the client. `kind` keeps a code from working as a token.
    pub fn seal_token(&self, kind: &str, id: &str, key: &Key, expires: u64) -> Result<String> {
        let active = self.active()?;
        let plain = json!({"k":to_hex(key),"exp":expires,"n":to_hex(&random_key()[..8])});
        let sealed = encrypt(
            &active.key,
            &token_aad(kind, id, &active.kid),
            plain.to_string().as_bytes(),
        )?;
        Ok(format!("{id}.{}.{sealed}", active.kid))
    }

    pub fn open_token(&self, kind: &str, token: &str, now: u64) -> Option<Key> {
        let mut parts = token.split('.');
        let (id, kid, sealed) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let key = self.usable(kid, now)?;
        let plain = decrypt(&key.key, &token_aad(kind, id, kid), sealed)?;
        let value: Value = serde_json::from_slice(&plain).ok()?;
        if value["exp"].as_u64()? <= now {
            return None;
        }
        key_from_hex(value["k"].as_str()?)
    }

    /// Fresh Trakt tokens wait here only while a login finishes; see the privacy page.
    pub fn seal_pending(&self, id: &str, plain: &str) -> Result<String> {
        let active = self.active()?;
        let sealed = encrypt(&active.key, &pending_aad(id, &active.kid), plain.as_bytes())?;
        Ok(format!("{}.{sealed}", active.kid))
    }

    pub fn open_pending(&self, id: &str, text: &str, now: u64) -> Option<String> {
        let (kid, sealed) = text.split_once('.')?;
        let key = self.usable(kid, now)?;
        String::from_utf8(decrypt(&key.key, &pending_aad(id, kid), sealed)?).ok()
    }
}

fn token_aad(kind: &str, id: &str, kid: &str) -> String {
    format!("trakt-mcp:token:v1:{kind}:{id}:{kid}")
}

fn pending_aad(id: &str, kid: &str) -> String {
    format!("trakt-mcp:pending:v1:{id}:{kid}")
}

/// The associated data binds a grant to its user and format version.
fn grant_aad(grant: &str) -> String {
    format!("trakt-mcp:grant:v1:{grant}")
}

pub fn seal_grant(key: &Key, grant: &str, plain: &str) -> Result<String> {
    encrypt(key, &grant_aad(grant), plain.as_bytes())
}

pub fn open_grant(key: &Key, grant: &str, sealed: &str) -> Option<String> {
    String::from_utf8(decrypt(key, &grant_aad(grant), sealed)?).ok()
}

/// What a person saves: K plus the grant it opens, because K alone names no user.
pub fn connection_key(grant: &str, key: &Key) -> String {
    format!("{KEY_PREFIX}_{grant}_{}", to_hex(key))
}

pub fn parse_connection_key(text: &str) -> Option<(String, Key)> {
    let mut parts = text.trim().split('_');
    let (prefix, grant, key) = (parts.next()?, parts.next()?, parts.next()?);
    if prefix != KEY_PREFIX || parts.next().is_some() || grant.len() != 64 {
        return None;
    }
    from_hex(grant)?;
    Some((grant.to_string(), key_from_hex(key)?))
}
