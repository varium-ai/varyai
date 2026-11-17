//! Native request-header extension; no generated or cache-derived session identities.
use super::FAILED;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const ID: &str = "varyai-harness-session-identity";
pub(crate) const FILE: &str = "varyai-harness-session-id.mjs";
pub(crate) const SOURCE: &str = include_str!("../../../resources/harness-session-id.mjs");

pub(crate) fn path(profile: &Path) -> PathBuf {
    profile.with_file_name(FILE)
}

pub(crate) fn entry(profile: &Path) -> Result<Value, String> {
    let path = path(profile);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().map_err(|_| FAILED)?.join(path)
    };
    let name = url::Url::from_file_path(path).map_err(|_| FAILED)?;
    Ok(json!({"id":ID,"name":name.as_str()}))
}

pub(super) fn recognized(entry: &Value) -> bool {
    entry["id"] == ID
        && entry["name"]
            .as_str()
            .and_then(|name| url::Url::parse(name).ok())
            .is_some_and(|url| url.scheme() == "file" && url.path().ends_with(&format!("/{FILE}")))
}

fn conflicts(value: &Value) -> bool {
    match value {
        Value::Object(fields) => value["id"] == ID || fields.values().any(conflicts),
        Value::Array(values) => values.iter().any(conflicts),
        _ => false,
    }
}

pub(crate) fn configure(value: &mut Value, profile: &Path) -> Result<(), String> {
    if conflicts(&value["patches"]) {
        return Err(FAILED.into());
    }
    value["sessionHeader"] = entry(profile)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_patch_round_trip_preserves_foreign_entries_and_normalizes_old_receipts() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile with spaces/cordis.patch.yml");
        let source =
            "# Synthetic native plugin\n- id: native-plugin\n  config: {preserved: true}\n";
        let mut value = super::super::parse(source).unwrap();
        let before = value.clone();
        configure(&mut value, &profile).unwrap();
        let encoded = super::super::encode(source, &value).unwrap();
        assert!(encoded.contains("# Synthetic native plugin"));
        assert_eq!(super::super::parse(&encoded).unwrap(), value);
        assert_eq!(super::super::normalize(&value).unwrap(), value);
        assert_eq!(super::super::normalize(&before).unwrap(), before);
        value.as_object_mut().unwrap().remove("sessionHeader");
        assert_eq!(
            super::super::parse(&super::super::encode(&encoded, &value).unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn occupied_plugin_identity_is_not_overwritten() {
        let mut value =
            super::super::parse(&format!("- id: {ID}\n  name: synthetic-foreign-module\n"))
                .unwrap();
        let before = value.clone();
        assert!(configure(&mut value, Path::new("/synthetic/profile.yml")).is_err());
        assert_eq!(value, before);
    }
}
