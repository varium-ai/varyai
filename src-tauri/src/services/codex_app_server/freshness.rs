//! Semantic startup snapshots survive harmless rewrites and VaryAI restarts.
use super::{catalog_sample, fingerprint, modified_us, read};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: u8,
    route: [u8; 32],
    catalog: [u8; 32],
    pub(crate) route_changed_us: u64,
    pub(crate) catalog_changed_us: u64,
}

impl Snapshot {
    pub(crate) fn applied_ms(&self) -> Option<i64> {
        i64::try_from(self.route_changed_us.max(self.catalog_changed_us) / 1000).ok()
    }
}

pub(crate) fn load(receipt: &Path) -> Result<Option<Snapshot>, ()> {
    let snapshot: Option<Snapshot> = read(receipt)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| ()))
        .transpose()?;
    if snapshot.as_ref().is_some_and(|old| old.version != 1) {
        return Err(());
    }
    Ok(snapshot)
}

/// Call under the Codex switch lock. Persist digests only, never source values.
pub(crate) fn sample(config: &Path, receipt: &Path) -> Result<Snapshot, ()> {
    let route = fingerprint(config)?;
    let (catalog, catalog_modified) = catalog_sample(config)?;
    let route_modified = modified_us(config)?;
    let previous = load(receipt)?;
    let now = u64::try_from(chrono::Utc::now().timestamp_micros()).map_err(|_| ())?;
    let changed = |same: bool, before: u64, modified: u64| {
        if same {
            before
        } else if modified > before {
            modified
        } else {
            now.max(before.saturating_add(1))
        }
    };
    let next = Snapshot {
        version: 1,
        route,
        catalog,
        route_changed_us: previous.as_ref().map_or(route_modified, |old| {
            changed(old.route == route, old.route_changed_us, route_modified)
        }),
        catalog_changed_us: previous.as_ref().map_or(catalog_modified, |old| {
            changed(
                old.catalog == catalog,
                old.catalog_changed_us,
                catalog_modified,
            )
        }),
    };
    if previous.as_ref() != Some(&next) {
        crate::config::atomic_write_private(receipt, &serde_json::to_vec(&next).map_err(|_| ())?)
            .map_err(|_| ())?;
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn repeated_application_and_metadata_rewrites_keep_persisted_change_times(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let config = dir.path().join("config.toml");
        let receipt = dir.path().join("freshness.json");
        let catalog = dir.path().join("models.json");
        let text = "model='one'\nmodel_catalog_json='models.json'\n";
        fs::write(&config, text)?;
        fs::write(&catalog, r#"{"models":[{"slug":"one"},{"slug":"two"}]}"#)?;
        let first = sample(&config, &receipt).map_err(|_| "sample")?;
        let receipt_bytes = fs::read(&receipt)?;
        let receipt_time = fs::metadata(&receipt)?.modified()?;
        fs::write(&config, text.replace("'one'", "'two'"))?;
        fs::write(
            &catalog,
            r#"{"models":[{"slug":"two","priority":9},{"slug":"one","description":"changed"}]}"#,
        )?;
        assert_eq!(sample(&config, &receipt).map_err(|_| "sample")?, first);
        assert_eq!(fs::read(&receipt)?, receipt_bytes);
        assert_eq!(fs::metadata(&receipt)?.modified()?, receipt_time);
        fs::write(
            &catalog,
            r#"{"models":[{"slug":"one","visibility":"hide"},{"slug":"two"}]}"#,
        )?;
        let changed = sample(&config, &receipt).map_err(|_| "sample")?;
        assert_eq!(changed.route_changed_us, first.route_changed_us);
        assert!(changed.catalog_changed_us > first.catalog_changed_us);
        assert_eq!(sample(&config, &receipt).map_err(|_| "sample")?, changed);
        Ok(())
    }

    #[test]
    fn route_changes_advance_only_route_time_and_never_persist_credentials(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let config = dir.path().join("config.toml");
        let receipt = dir.path().join("freshness.json");
        let text = "model_provider='varyai'\n[model_providers.varyai]\nbase_url='http://127.0.0.1:1/v1'\nexperimental_bearer_token='synthetic-private-key'\n";
        fs::write(&config, text)?;
        let first = sample(&config, &receipt).map_err(|_| "sample")?;
        fs::write(&config, text.replace(":1/v1", ":2/v1"))?;
        let changed = sample(&config, &receipt).map_err(|_| "sample")?;
        assert!(changed.route_changed_us > first.route_changed_us);
        assert_eq!(changed.catalog_changed_us, first.catalog_changed_us);
        let saved = fs::read_to_string(&receipt)?;
        assert!(!saved.contains("synthetic-private-key"));
        assert!(!saved.contains("127.0.0.1"));
        fs::write(&config, "invalid TOML =")?;
        assert!(sample(&config, &receipt).is_err());
        assert_eq!(fs::read_to_string(&receipt)?, saved);
        Ok(())
    }
}
