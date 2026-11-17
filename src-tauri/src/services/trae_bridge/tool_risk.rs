//! Edition-bound, warning-only native hooks. Existing user hooks remain intact.
use super::{
    shared::{Client, Edition},
    unavailable,
};
use crate::error::AppError;
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};

static TOKENS: Mutex<[Option<String>; 2]> = Mutex::new([None, None]);

fn index(edition: Edition) -> usize {
    match edition {
        Edition::China => 0,
        Edition::International => 1,
    }
}

pub(crate) fn authorized(edition: Edition, value: &str) -> bool {
    TOKENS.lock().ok().is_some_and(|tokens| {
        tokens[index(edition)].as_deref().is_some_and(|token| {
            value.len() == token.len()
                && value
                    .bytes()
                    .zip(token.bytes())
                    .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                    == 0
        })
    })
}

fn updated_hooks(
    mut value: Value,
    previous: &[Value],
    desired: Option<&Value>,
) -> Result<Value, AppError> {
    if !value.is_object() || value.get("version").is_some_and(|version| version != 1) {
        return Err(unavailable());
    }
    let root = value.as_object_mut().ok_or_else(unavailable)?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or_else(unavailable)?;
    let entries = hooks.entry("PreToolUse").or_insert_with(|| json!([]));
    let entries = entries.as_array_mut().ok_or_else(unavailable)?;
    entries.retain(|entry| !previous.contains(entry));
    if let Some(desired) = desired {
        if !entries.contains(desired) {
            entries.push(desired.clone());
        }
    }
    Ok(value)
}

fn read_json(path: &Path, limit: u64) -> Result<Option<Value>, AppError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unavailable()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(unavailable());
    }
    let bytes = std::fs::read(path).map_err(|_| unavailable())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| unavailable())
}

fn write_changed(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    crate::config::atomic_write_private(path, bytes)
}

pub(crate) fn sync(edition: Edition, origin: Option<&str>) -> Result<(), AppError> {
    use std::sync::atomic::{AtomicU8, Ordering};
    static FAILED: AtomicU8 = AtomicU8::new(0);
    let result = sync_inner(edition, origin);
    let bit = 1 << index(edition);
    if result.is_err() {
        if FAILED.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
            log::warn!(
                "[ToolRisk] client={} native_hook=unavailable; native settings preserved",
                edition.app()
            );
        }
    } else {
        FAILED.fetch_and(!bit, Ordering::Relaxed);
    }
    result
}

fn sync_inner(edition: Edition, origin: Option<&str>) -> Result<(), AppError> {
    let client = Client(edition);
    let directory = client.directory()?;
    let config_path = directory.join("tool-risk.json");
    let previous = read_json(&config_path, 16384)?;
    if origin.is_none() {
        if let Ok(mut tokens) = TOKENS.lock() {
            tokens[index(edition)] = None;
        }
        if previous.is_none() {
            return Ok(());
        }
    }
    let hook_path = client.argv_path()?.with_file_name("hooks.json");
    let current =
        read_json(&hook_path, 1024 * 1024)?.unwrap_or_else(|| json!({"version":1,"hooks":{}}));
    let owned: Vec<Value> = previous
        .as_ref()
        .and_then(|value| value.get("ownedHooks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| {
            previous
                .as_ref()
                .and_then(|value| value.get("hook"))
                .cloned()
                .into_iter()
                .collect()
        });
    let desired: Option<Value> = if let Some(origin) = origin {
        let url = url::Url::parse(origin).map_err(|_| unavailable())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(unavailable());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let executable = client.installed_executable()?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        return Err(unavailable());
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let script = directory.join("tool-risk-hook.cjs");
            let source = include_bytes!("../../../resources/trae-bridge/tool-risk-hook.cjs");
            if previous.is_none()
                || std::fs::read(&script).ok().as_deref() != Some(source.as_slice())
            {
                std::fs::create_dir_all(&directory).map_err(|_| unavailable())?;
                #[cfg(target_os = "windows")]
                super::protect_directory(&directory)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                        .map_err(|_| unavailable())?;
                }
                write_changed(&script, source)?;
            }
            let mut tokens = TOKENS.lock().map_err(|_| unavailable())?;
            let token = tokens[index(edition)]
                .get_or_insert_with(|| {
                    format!(
                        "{}{}",
                        uuid::Uuid::new_v4().simple(),
                        uuid::Uuid::new_v4().simple()
                    )
                })
                .clone();
            drop(tokens);
            #[cfg(target_os = "windows")]
            let command = {
                let quote =
                    |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "''"));
                format!("$env:ELECTRON_RUN_AS_NODE='1'; Remove-Item Env:NODE_OPTIONS -ErrorAction SilentlyContinue; & {} {} {}", quote(&executable), quote(&script), quote(&config_path))
            };
            #[cfg(target_os = "macos")]
            let command = {
                let quote =
                    |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
                format!(
                    "env -u NODE_OPTIONS ELECTRON_RUN_AS_NODE=1 {} {} {}",
                    quote(&executable),
                    quote(&script),
                    quote(&config_path)
                )
            };
            let entry = json!({"matcher":"^RunCommand$","hooks":[{"type":"command","command":command,"timeout":2}]});
            Some(
                json!({"url":format!("{}/private/tool-risk/{}", origin.trim_end_matches('/'), edition.app()),"token":token,"hook":entry}),
            )
        }
    } else {
        None
    };
    let updated = updated_hooks(
        current.clone(),
        &owned,
        desired.as_ref().and_then(|value| value.get("hook")),
    )?;
    if desired == previous && updated == current {
        return Ok(());
    }
    // Persist ownership before touching the user's hook file so removal remains recoverable.
    if let Some(desired) = &desired {
        let mut pending = desired.clone();
        let mut pending_owned = owned;
        if let Some(entry) = desired.get("hook") {
            if !pending_owned.contains(entry) {
                pending_owned.push(entry.clone());
            }
        }
        pending["ownedHooks"] = json!(pending_owned);
        write_changed(
            &config_path,
            &serde_json::to_vec(&pending).map_err(|_| unavailable())?,
        )?;
    }
    if read_json(&hook_path, 1024 * 1024)?.unwrap_or_else(|| json!({"version":1,"hooks":{}}))
        != current
    {
        return Err(unavailable());
    }
    write_changed(
        &hook_path,
        &serde_json::to_vec_pretty(&updated).map_err(|_| unavailable())?,
    )?;
    if let Some(desired) = &desired {
        write_changed(
            &config_path,
            &serde_json::to_vec(desired).map_err(|_| unavailable())?,
        )?;
    }
    if desired.is_none() {
        match std::fs::remove_file(&config_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(unavailable()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installing_replacing_and_removing_only_changes_the_owned_entry() {
        let owned = json!({"matcher":"^RunCommand$","hooks":[{"type":"command","command":"synthetic-owned"}]});
        let user =
            json!({"matcher":"RunCommand","hooks":[{"type":"command","command":"synthetic-user"}]});
        let original = json!({"version":1,"extra":"preserved","hooks":{"PreToolUse":[user],"PostToolUse":[{"synthetic":"preserved"}]}});
        let installed = updated_hooks(original.clone(), &[], Some(&owned)).unwrap();
        assert_eq!(
            updated_hooks(
                installed.clone(),
                std::slice::from_ref(&owned),
                Some(&owned)
            )
            .unwrap(),
            installed
        );
        assert_eq!(
            updated_hooks(installed, std::slice::from_ref(&owned), None).unwrap(),
            original
        );
        let edited = json!({"hooks":{"PreToolUse":[{"command":"user-edited"}]}});
        assert_eq!(
            updated_hooks(edited.clone(), std::slice::from_ref(&owned), None).unwrap(),
            edited
        );
    }
    #[test]
    fn malformed_user_configuration_is_preserved_instead_of_overwritten() {
        for value in [
            json!([]),
            json!({"version":2}),
            json!({"hooks":[]}),
            json!({"hooks":{"PreToolUse":{}}}),
        ] {
            assert!(updated_hooks(value, &[], Some(&json!({"synthetic":true}))).is_err());
        }
    }
    #[test]
    fn interrupted_replacement_can_remove_both_pending_owned_entries() {
        let old = json!({"command":"synthetic-old"});
        let new = json!({"command":"synthetic-new"});
        let user = json!({"command":"synthetic-user"});
        let current = json!({"hooks":{"PreToolUse":[old,new,user]}});
        let removed = updated_hooks(current, &[old, new], None).unwrap();
        assert_eq!(removed["hooks"]["PreToolUse"], json!([user]));
    }
    #[test]
    fn native_edition_tokens_are_not_interchangeable() {
        let mut tokens = TOKENS.lock().unwrap();
        tokens[0] = Some("synthetic-china".into());
        tokens[1] = Some("synthetic-international".into());
        drop(tokens);
        assert!(authorized(Edition::China, "synthetic-china"));
        assert!(!authorized(Edition::International, "synthetic-china"));
        *TOKENS.lock().unwrap() = [None, None];
    }
}
