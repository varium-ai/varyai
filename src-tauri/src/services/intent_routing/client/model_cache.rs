//! Expire Codex's shared model cache after a successful owned route change.
use super::*;

pub(super) fn expire(config: &Path) {
    if expire_inner(config).is_err() {
        // Cache maintenance must never undo a successful connection change.
        log::warn!("Codex model cache expiration unavailable; reload the client to refresh models");
    }
}

fn expire_inner(config: &Path) -> Result<(), String> {
    let path = config.parent().ok_or(FAILED)?.join("models_cache.json");
    let Some(current) = read(&path)? else {
        return Ok(());
    };
    // Keep unknown cache fields and model metadata exactly as Codex encoded them.
    let Ok(mut cache) =
        serde_json::from_str::<BTreeMap<String, Box<serde_json::value::RawValue>>>(&current)
    else {
        return Ok(());
    };
    let Some(fetched_at) = cache.get_mut("fetched_at") else {
        return Ok(());
    };
    let Ok(timestamp) = serde_json::from_str::<String>(fetched_at.get()) else {
        return Ok(());
    };
    const STALE: &str = "1970-01-01T00:00:00Z";
    if timestamp == STALE {
        return Ok(());
    }
    *fetched_at = serde_json::value::to_raw_value(STALE).map_err(|_| FAILED)?;
    let replacement = serde_json::to_string_pretty(&cache).map_err(|_| FAILED)?;
    // Reuse guarded replacement so a concurrent Codex cache write wins.
    replace_checked(&path, Some(&current), Some(&replacement))
}
