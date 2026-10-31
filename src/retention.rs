//! Delete a Durable Object's records a fixed time after last use.
use crate::{error::Result, trakt::auth::now};
use std::time::Duration;
use worker::{Env, State};

const PURGE_AT: &str = "purge_at";
pub const PENDING_EXPIRES: &str = "pending_login_expires";
const DEFAULT_SECONDS: u64 = 30 * 86400;
/// Unfinished logins and abandoned authorization pages leave nothing behind for long.
pub const UNFINISHED_SECONDS: u64 = 3600;

pub fn seconds(env: &Env) -> u64 {
    env.var("RETENTION_SECONDS")
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(DEFAULT_SECONDS)
}

async fn schedule(state: &State, at: u64) -> Result<()> {
    state
        .storage()
        .set_alarm(Duration::from_secs(at.saturating_sub(now())))
        .await?;
    Ok(())
}

/// Move the purge date. Skips most writes because last use only needs rough accuracy.
pub async fn touch(env: &Env, state: &State, force: bool) -> Result<()> {
    let retention = seconds(env);
    extend(state, retention, force).await
}

pub async fn extend(state: &State, lifetime: u64, force: bool) -> Result<()> {
    let storage = state.storage();
    let at = now() + lifetime;
    let current = storage.get::<u64>(PURGE_AT).await?;
    let slack = (lifetime / 2).min(3600);
    if force || current.is_none_or(|c| c + slack < at) {
        storage.put(PURGE_AT, at).await?;
        schedule(state, next_alarm(state, at).await?).await?;
    }
    Ok(())
}

async fn next_alarm(state: &State, purge: u64) -> Result<u64> {
    let pending = state.storage().get::<u64>(PENDING_EXPIRES).await?;
    Ok(pending.map_or(purge, |p| p.min(purge)))
}

/// Keep an alarm for whichever comes first: the purge or a parked login's expiry.
pub async fn rearm(state: &State) -> Result<()> {
    let storage = state.storage();
    if let Some(purge) = storage.get::<u64>(PURGE_AT).await? {
        schedule(state, next_alarm(state, purge).await?).await?;
    }
    Ok(())
}

pub async fn alarm(state: &State) -> Result<bool> {
    let storage = state.storage();
    if let Some(expires) = storage.get::<u64>(PENDING_EXPIRES).await?
        && expires <= now()
    {
        crate::trakt::auth::drop_pending(&storage).await?;
    }
    match storage.get::<u64>(PURGE_AT).await? {
        Some(purge) if purge > now() => {
            schedule(state, next_alarm(state, purge).await?).await?;
            Ok(false)
        }
        _ => {
            storage.delete_all().await?;
            Ok(true)
        }
    }
}
