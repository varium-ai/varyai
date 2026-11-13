//! Launch freshness tracks only the bridge's startup argument, never argv file age.
use crate::config::atomic_write_private;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};

static OBSERVATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    version: u8,
    digest: String,
    changed_ms: i64,
}

fn read(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta)
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 4 * 1024 * 1024 =>
        {
            Err(())
        }
        Ok(_) => std::fs::read_to_string(path).map(Some).map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn sample(path: &Path, value: Value, initial_ms: i64, now_ms: i64) -> Result<Snapshot, ()> {
    let object = value.as_object().ok_or(())?;
    let port = match object.get("remote-debugging-port") {
        Some(Value::String(port)) if port == "0" => Value::from(0),
        Some(port) => port.clone(),
        None => Value::Null,
    };
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&port).map_err(|_| ())?)
    );
    let previous = read(path)?
        .map(|text| serde_json::from_str::<Snapshot>(&text).map_err(|_| ()))
        .transpose()?;
    if previous
        .as_ref()
        .is_some_and(|old| old.version != 1 || old.changed_ms <= 0)
    {
        return Err(());
    }
    let changed_ms = match previous.as_ref() {
        Some(old) if old.digest == digest => old.changed_ms,
        Some(old) if initial_ms > old.changed_ms => initial_ms,
        Some(old) => now_ms.max(old.changed_ms.saturating_add(1)),
        None => initial_ms,
    };
    if changed_ms <= 0 {
        return Err(());
    }
    let next = Snapshot {
        version: 1,
        digest,
        changed_ms,
    };
    if previous.as_ref() != Some(&next) {
        atomic_write_private(path, &serde_json::to_vec(&next).map_err(|_| ())?).map_err(|_| ())?;
    }
    Ok(next)
}

pub(super) fn record(directory: &Path, argv: &Path) -> Option<i64> {
    // Status reads and bridge preparation can overlap; serialize read/compare/write.
    let _guard = OBSERVATION.lock().ok()?;
    let text = read(argv).ok()?;
    let value: Value = json5::from_str(text.as_deref().unwrap_or("{}")).ok()?;
    let now = chrono::Utc::now().timestamp_millis();
    let modified = if text.is_some() {
        i64::try_from(
            std::fs::metadata(argv)
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis(),
        )
        .ok()?
    } else {
        now
    };
    sample(
        &directory.join("launch-freshness.json"),
        value,
        modified,
        now,
    )
    .ok()
    .map(|snapshot| snapshot.changed_ms)
}

pub(super) fn applied_at_ms(directory: &Path, argv: &Path) -> Option<i64> {
    read(argv).ok()??;
    record(directory, argv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preferences_json_order_and_zero_representation_preserve_launch_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        let first = sample(
            &path,
            json!({"remote-debugging-port":"0","theme":"light"}),
            1000,
            2000,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let next = sample(
            &path,
            json!({"theme":"dark","remote-debugging-port":0}),
            5000,
            6000,
        )
        .unwrap();
        assert_eq!(first.changed_ms, 1000);
        assert_eq!(next, first);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn actual_launch_setting_changes_advance_time_and_store_no_private_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        sample(&path, json!({"remote-debugging-port":0}), 1000, 2000).unwrap();
        let changed = sample(
            &path,
            json!({"remote-debugging-port":1234,"apiKey":"synthetic-private"}),
            1000,
            6000,
        )
        .unwrap();
        assert_eq!(changed.changed_ms, 6000);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("synthetic-private"));
        assert!(!text.contains("1234"));
    }
    #[test]
    fn delayed_observation_uses_the_actual_file_change_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        sample(&path, json!({"remote-debugging-port":0}), 1000, 2000).unwrap();
        let changed = sample(&path, json!({"remote-debugging-port":1234}), 5000, 9000).unwrap();
        // A client launched at 7000 has already read this change, even if Vary observes it later.
        assert_eq!(changed.changed_ms, 5000);
    }

    #[test]
    fn malformed_input_or_receipt_does_not_overwrite_the_last_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        sample(&path, json!({"remote-debugging-port":0}), 1000, 2000).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(sample(&path, json!([]), 3000, 4000).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, "{broken").unwrap();
        assert!(sample(&path, json!({"remote-debugging-port":0}), 3000, 4000).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    }
    #[test]
    fn reconnect_after_restoration_tracks_a_real_launch_change() {
        let dir = tempfile::tempdir().unwrap();
        let argv = dir.path().join("argv.json");
        super::super::prepare_connection(dir.path(), &argv).unwrap();
        let first = applied_at_ms(dir.path(), &argv).unwrap();
        let receipt = dir.path().join("launch-freshness.json");
        let before: Snapshot = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        super::super::restore_connection(dir.path(), &argv).unwrap();
        assert_eq!(applied_at_ms(dir.path(), &argv), None);
        let restored: Snapshot = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        assert_ne!(restored.digest, before.digest);
        assert!(restored.changed_ms > first);
        super::super::prepare_connection(dir.path(), &argv).unwrap();
        let reconnected = applied_at_ms(dir.path(), &argv).unwrap();
        assert!(reconnected > restored.changed_ms);
    }

    #[test]
    fn invalid_argv_cannot_turn_unknown_startup_into_current() {
        let dir = tempfile::tempdir().unwrap();
        let argv = dir.path().join("argv.json");
        std::fs::write(&argv, "{broken").unwrap();
        assert_eq!(applied_at_ms(dir.path(), &argv), None);
        assert!(!dir.path().join("launch-freshness.json").exists());
    }
}
