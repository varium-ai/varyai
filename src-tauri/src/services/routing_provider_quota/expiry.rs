//! Nonblocking, credential-scoped observations of relay subscription expiry.

use super::{origin, read, Endpoint};
use http::HeaderMap;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};
use tokio::sync::Semaphore;

struct Reading {
    expiry: Option<i64>,
    refresh_at: i64,
    valid_until: i64,
}

static READINGS: LazyLock<Mutex<HashMap<[u8; 32], Reading>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static IN_FLIGHT: Semaphore = Semaphore::const_new(8);

/// Return a fresh observation if available and refresh stale data off the request path.
pub(crate) fn observe(url: &Url, headers: &HeaderMap, now: i64) -> Option<i64> {
    // Only probe the standard gateway endpoints served by the same Sub2API key.
    if !matches!(
        url.path(),
        "/v1/chat/completions" | "/v1/responses" | "/v1/messages"
    ) && !(url.path().starts_with("/v1beta/models/")
        && (url.path().ends_with(":generateContent")
            || url.path().ends_with(":streamGenerateContent")))
    {
        return None;
    }
    let key = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok()?.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-api-key")?.to_str().ok())
        .or_else(|| headers.get("x-goog-api-key")?.to_str().ok())
        .filter(|key| !key.trim().is_empty())?;
    let origin = origin(url.as_str()).ok()?;
    let mut digest = Sha256::new();
    digest.update(origin.as_str());
    digest.update([0]);
    digest.update(key);
    let identity: [u8; 32] = digest.finalize().into();

    let mut readings = READINGS.try_lock().ok()?;
    let previous = readings.get(&identity);
    let expiry = previous
        .filter(|reading| reading.valid_until > now)
        .and_then(|reading| reading.expiry)
        .filter(|expiry| *expiry > now);
    if previous.is_some_and(|reading| reading.refresh_at > now) {
        return expiry;
    }
    let valid_until = previous.map_or(0, |reading| reading.valid_until);
    let Ok(permit) = IN_FLIGHT.try_acquire() else {
        return expiry;
    };
    if readings.len() >= 1024 {
        readings.retain(|_, reading| reading.refresh_at > now);
        if readings.len() >= 1024 {
            return expiry;
        }
    }
    readings.insert(
        identity,
        Reading {
            expiry,
            refresh_at: now.saturating_add(15),
            valid_until,
        },
    );
    drop(readings);

    let key = key.to_owned();
    tokio::spawn(async move {
        let _permit = permit;
        let result = read(&origin, &key, Endpoint::Sub2api).await;
        let sampled = chrono::Utc::now().timestamp();
        let expiry = result
            .ok()
            .and_then(|allowance| allowance.expiry)
            .filter(|expiry| *expiry > sampled);
        if let Ok(mut readings) = READINGS.lock() {
            readings.insert(
                identity,
                Reading {
                    expiry,
                    refresh_at: sampled.saturating_add(if expiry.is_some() { 30 } else { 300 }),
                    valid_until: sampled.saturating_add(60),
                },
            );
        }
    });
    expiry
}
