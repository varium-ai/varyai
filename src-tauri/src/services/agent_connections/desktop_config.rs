//! Desktop owns secret encryption and user settings; Vary owns its gateway projection.
use super::{at, changes, parse, put, read, replace_checked, FileEdit, Receipt, CONFLICT, FAILED};
use crate::{claude_desktop_config::PROFILE_ID, config::atomic_write_private};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SEALED: &str = "__sealed__:v1:";
const KEY: &str = "inferenceGatewayApiKey";
const CONNECTION: &[&str] = &[
    "inferenceProvider",
    "inferenceGatewayBaseUrl",
    KEY,
    "inferenceGatewayAuthScheme",
];
const SETTINGS: &str = "claude-desktop-profile-settings.json";

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Mode,
    Profile,
    Meta,
    Carry,
}

pub(super) fn seed_tool_search(files: &mut [FileEdit], previous: Option<&Receipt>) {
    for file in files
        .iter_mut()
        .filter(|file| matches!(file.desktop, Some(Kind::Profile)))
    {
        let default = vec!["alwaysStartWithDefaultModel".to_owned()];
        if !file.fields.contains(&default) {
            file.fields.push(default);
        }
        let marker = vec!["toolSearchEnabled".to_owned()];
        let seen = previous.is_some_and(|receipt| {
            receipt.files.iter().any(|prior| {
                prior.path == file.path
                    && (prior.fields.contains(&marker)
                        || prior.installed.get("toolSearchEnabled").is_some())
            })
        });
        if !seen && file.installed.get("toolSearchEnabled").is_none() {
            file.installed["toolSearchEnabled"] = json!(true);
        }
        if !file.fields.contains(&marker) {
            file.fields.push(marker);
        }
    }
}

pub(super) fn preserve_carried(
    files: &mut Vec<FileEdit>,
    previous: Option<&Receipt>,
) -> Result<(), String> {
    if let Some(previous) = previous {
        for prior in &previous.files {
            if matches!(prior.desktop, Some(Kind::Carry))
                && !files.iter().any(|file| file.path == prior.path)
            {
                files.push(
                    FileEdit::prepare(prior.path.clone(), |_| Ok(()))?.for_desktop(Kind::Carry),
                );
            }
        }
    }
    for file in files.iter_mut().filter(|file| file.desktop_carried) {
        if previous.is_some_and(|receipt| {
            receipt
                .files
                .iter()
                .any(|prior| prior.path == file.path && prior.desktop_carried)
        }) {
            let current = parse(file.original.as_deref())?;
            for key in if matches!(file.desktop, Some(Kind::Mode)) {
                vec!["mcpServers"]
            } else {
                vec!["locale"]
            } {
                put(
                    &mut file.installed,
                    &[key.into()],
                    current.get(key).cloned(),
                )?;
            }
            file.fields.clear();
            changes(
                Some(&current),
                Some(&file.installed),
                vec![],
                &mut file.fields,
            );
        }
    }
    Ok(())
}

// Upgrade pre-compatibility receipts in memory without changing their recovery baseline.
pub(super) fn adopt(receipt: &mut Receipt) {
    if receipt.app != "claude-desktop" {
        return;
    }
    // A carried connector or locale becomes native ownership when the user
    // edits or removes it. Refresh must not adopt it as a new gateway value.
    for file in &mut receipt.files {
        let profile_setting =
            matches!(file.desktop, Some(Kind::Profile)) && file.original.is_some();
        if !file.desktop_carried && !profile_setting {
            continue;
        }
        let Ok(live) = read(&file.path).and_then(|text| parse(text.as_deref())) else {
            continue;
        };
        let Ok(mut baseline) = parse(file.original.as_deref()) else {
            continue;
        };
        if live == baseline {
            continue;
        }
        let mut released = vec![];
        let mut changed = false;
        for field in &file.fields {
            if field.first().is_some_and(|key| {
                (file.desktop_carried && (key == "mcpServers" || key == "locale"))
                    || (profile_setting && key == "toolSearchEnabled")
            }) && at(&live, field) != at(&file.installed, field)
                && put(&mut baseline, field, at(&live, field).cloned()).is_ok()
                && put(&mut file.installed, field, at(&live, field).cloned()).is_ok()
            {
                changed = true;
                if !profile_setting {
                    released.push(field.clone());
                }
            }
        }
        if changed {
            if let Ok(text) = file.format.encode(file.original.as_deref(), &baseline) {
                file.original = Some(text);
                file.fields.retain(|field| !released.contains(field));
            }
        }
    }
    let profile = receipt
        .files
        .iter()
        .find(|file| {
            file.path.file_name().and_then(|n| n.to_str()) == Some(&format!("{PROFILE_ID}.json"))
                && file.installed["inferenceProvider"] == "gateway"
        })
        .map(|file| file.path.clone());
    let Some(profile) = profile else {
        return;
    };
    for file in &mut receipt.files {
        if file.desktop.is_some() {
            continue;
        }
        file.desktop = if file.path == profile {
            Some(Kind::Profile)
        } else if file.path == profile.with_file_name("_meta.json") {
            Some(Kind::Meta)
        } else if file.path.file_name().and_then(|n| n.to_str())
            == Some("claude_desktop_config.json")
            && file.installed["deploymentMode"] == "3p"
        {
            Some(Kind::Mode)
        } else {
            None
        };
    }
}

fn open_key(value: &str) -> Option<String> {
    let trimmed = value.trim_start_matches([' ', '\t', '\r', '\n']);
    let Some(sealed) = trimmed.strip_prefix(SEALED) else {
        return Some(value.into());
    };
    let bytes = STANDARD.decode(sealed.trim()).ok()?;
    let plain = unprotect(&bytes)?;
    let envelope = plain.strip_prefix("anthropic.claude.crossProfileSecret.v1:")?;
    let data: Value = serde_json::from_str(envelope).ok()?;
    (data["configLibrary"]["key"] == KEY)
        .then(|| data["configLibrary"]["value"].as_str().map(str::to_owned))
        .flatten()
}

#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Option<String> {
    None
}

#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    if bytes.is_empty() {
        return None;
    }
    let purpose = b"anthropic.claude.crossProfileSecret.v1";
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().ok()?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: purpose.len().try_into().ok()?,
        pbData: purpose.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI owns the output allocation. Never prompt or expose its plaintext in diagnostics.
    unsafe {
        if CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        ) == 0
        {
            return None;
        }
        let result = if output.pbData.is_null() {
            None
        } else {
            String::from_utf8(
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
            )
            .ok()
        };
        LocalFree(output.pbData.cast());
        result
    }
}

pub(crate) fn key_matches(value: &Value, expected: &Value) -> bool {
    match (value.as_str(), expected.as_str()) {
        (Some(value), Some(expected)) => open_key(value).is_none_or(|open| open == expected),
        _ => value == expected,
    }
}

pub(crate) fn keep_key(value: &Value, token: &str, previous_token: Option<&str>) -> bool {
    value.as_str().is_some_and(|value| match open_key(value) {
        Some(open) => open == token,
        // Only an existing connection can vouch for an unreadable seal on refresh.
        None => previous_token == Some(token),
    })
}

pub(super) fn refreshable(file: &FileEdit) -> Result<bool, String> {
    if !matches!(file.desktop, Some(Kind::Profile)) {
        return file.intact();
    }
    let live = parse(read(&file.path)?.as_deref())?;
    if !live.is_object() {
        return Err(FAILED.into());
    }
    // Repair credentials only in the same journaled gateway profile. Endpoint,
    // provider, protocol, model and active-profile changes still require recovery.
    Ok(CONNECTION
        .iter()
        .filter(|key| **key != KEY)
        .chain([&"inferenceModels"])
        .all(|key| matches(&live, &file.installed, key)))
}

fn matches(live: &Value, expected: &Value, key: &str) -> bool {
    if key == KEY {
        live.get(KEY).zip(expected.get(KEY)).is_some_and(|(a, b)| {
            // An opaque seal is not proof of the credential; scope it to the managed gateway.
            live["inferenceProvider"] == expected["inferenceProvider"]
                && live["inferenceGatewayBaseUrl"] == expected["inferenceGatewayBaseUrl"]
                && key_matches(a, b)
        }) || live.get(KEY) == expected.get(KEY)
    } else {
        live.get(key) == expected.get(key)
    }
}

fn entries(value: &Value) -> Result<Vec<Value>, String> {
    match value.get("entries") {
        None => Ok(vec![]),
        Some(Value::Array(rows)) if rows.iter().all(Value::is_object) => Ok(rows.clone()),
        _ => Err(FAILED.into()),
    }
}

pub(super) fn intact(file: &FileEdit, live: &Value, kind: Kind) -> Result<bool, String> {
    if !live.is_object() {
        return Err(FAILED.into());
    }
    match kind {
        Kind::Mode => Ok(live["deploymentMode"] == "3p"),
        Kind::Carry => Ok(true),
        Kind::Profile => Ok(CONNECTION
            .iter()
            .chain([&"inferenceModels"])
            .all(|key| matches(live, &file.installed, key))),
        Kind::Meta => {
            let rows = entries(live)?;
            Ok(live["appliedId"] == PROFILE_ID && rows.iter().any(|row| row["id"] == PROFILE_ID))
        }
    }
}

pub(super) struct Restoration {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
}
impl Restoration {
    pub(super) fn write(self) -> Result<(), String> {
        replace_checked(&self.path, self.before.as_deref(), self.after.as_deref())
    }
}

pub(super) fn restoration(
    file: &FileEdit,
    kind: Kind,
    keep_changed: bool,
) -> Result<Restoration, String> {
    let before = read(&file.path)?;
    let mut live = parse(before.as_deref())?;
    let original = parse(file.original.as_deref())?;
    if !live.is_object() || !original.is_object() {
        return Err(FAILED.into());
    }
    let mut remove = false;
    match kind {
        Kind::Mode => {
            if live["deploymentMode"] == "3p" {
                put(
                    &mut live,
                    &["deploymentMode".into()],
                    original.get("deploymentMode").cloned(),
                )?;
            }
            restore_carried(file, &original, &mut live)?;
        }
        Kind::Carry => restore_carried(file, &original, &mut live)?,
        Kind::Meta => {
            let current = entries(&live)?;
            let baseline = entries(&original)?;
            let installed = entries(&file.installed)?;
            let ours = |row: &Value| row["id"] == PROFILE_ID;
            let restored = if current == installed {
                baseline.clone()
            } else {
                let mut restored = current
                    .into_iter()
                    .filter(|row| !ours(row))
                    .collect::<Vec<_>>();
                for (index, row) in baseline.iter().enumerate().filter(|(_, row)| ours(row)) {
                    restored.insert(index.min(restored.len()), row.clone());
                }
                restored
            };
            if restored.is_empty() && original.get("entries").is_none() {
                live.as_object_mut().ok_or(FAILED)?.remove("entries");
            } else {
                live["entries"] = json!(restored);
            }
            if live["appliedId"] == PROFILE_ID {
                let prior = original
                    .get("appliedId")
                    .filter(|id| restored.iter().any(|row| row.get("id") == Some(id)));
                let next = prior
                    .or_else(|| restored.first().and_then(|row| row.get("id")))
                    .cloned();
                put(&mut live, &["appliedId".into()], next)?;
            }
        }
        Kind::Profile => {
            if before.is_some() {
                if !keep_changed {
                    for key in CONNECTION {
                        if !matches(&live, &file.installed, key)
                            && !matches(&live, &original, key)
                            && !file
                                .previous
                                .as_ref()
                                .is_some_and(|prev| matches(&live, prev, key))
                        {
                            return Err(CONFLICT.into());
                        }
                    }
                }
                let connection_owned = CONNECTION.iter().all(|key| {
                    matches(&live, &file.installed, key)
                        || matches(&live, &original, key)
                        || file
                            .previous
                            .as_ref()
                            .is_some_and(|prev| matches(&live, prev, key))
                });
                if file.original.is_none() && connection_owned {
                    remove = true;
                } else {
                    for field in &file.fields {
                        let key = field.first().map(String::as_str).unwrap_or("");
                        if (field.len() == 1
                            && CONNECTION.contains(&key)
                            && (matches(&live, &file.installed, key)
                                || matches(&live, &original, key)
                                || file
                                    .previous
                                    .as_ref()
                                    .is_some_and(|prev| matches(&live, prev, key))))
                            || at(&live, field) == at(&file.installed, field)
                            || file
                                .previous
                                .as_ref()
                                .is_some_and(|prev| at(&live, field) == at(prev, field))
                        {
                            put(&mut live, field, at(&original, field).cloned())?;
                        }
                    }
                }
            }
        }
    }
    let after = if remove {
        None
    } else if live == original {
        file.original.clone()
    } else if parse(before.as_deref())? == live {
        before.clone()
    } else {
        Some(serde_json::to_string_pretty(&live).map_err(|_| FAILED)?)
    };
    Ok(Restoration {
        path: file.path.clone(),
        before,
        after,
    })
}

fn restore_carried(file: &FileEdit, original: &Value, live: &mut Value) -> Result<(), String> {
    for field in &file.fields {
        if field.first().is_some_and(|key| key == "deploymentMode") {
            continue;
        }
        if at(live, field) == at(&file.installed, field) {
            put(live, field, at(original, field).cloned())?;
        }
    }
    Ok(())
}

fn settings(profile: &Value) -> Value {
    let mut kept = profile.clone();
    if let Some(obj) = kept.as_object_mut() {
        for key in CONNECTION.iter().chain([&"inferenceModels"]) {
            obj.remove(*key);
        }
    }
    kept
}

pub(super) fn save_settings(root: &Path, receipt: &Receipt) -> Result<(), String> {
    if receipt.app != "claude-desktop" || receipt.suspended {
        return Ok(());
    }
    for file in &receipt.files {
        if matches!(file.desktop, Some(Kind::Profile)) {
            if let Some(text) = read(&file.path)? {
                let kept = settings(&parse(Some(&text))?);
                atomic_write_private(
                    &root.join(SETTINGS),
                    &serde_json::to_vec(&kept).map_err(|_| FAILED)?,
                )
                .map_err(|_| FAILED)?;
            }
        }
    }
    Ok(())
}

pub(super) fn restore_settings(root: &Path, receipt: &mut Receipt) -> Result<(), String> {
    if receipt.app != "claude-desktop" {
        return Ok(());
    }
    let Some(text) = read(&root.join(SETTINGS))? else {
        return Ok(());
    };
    let kept = settings(&parse(Some(&text))?);
    let kept = kept.as_object().ok_or(FAILED)?;
    for file in &mut receipt.files {
        if !matches!(file.desktop, Some(Kind::Profile)) {
            continue;
        }
        let original = parse(file.original.as_deref())?;
        if !kept.contains_key("toolSearchEnabled") && original.get("toolSearchEnabled").is_none() {
            file.installed
                .as_object_mut()
                .ok_or(FAILED)?
                .remove("toolSearchEnabled");
        }
        if file.original.is_none() {
            file.installed
                .as_object_mut()
                .ok_or(FAILED)?
                .extend(kept.clone());
            file.fields.clear();
            changes(
                Some(&json!({})),
                Some(&file.installed),
                vec![],
                &mut file.fields,
            );
        }
        let marker = vec!["toolSearchEnabled".to_owned()];
        if !file.fields.contains(&marker) {
            file.fields.push(marker);
        }
    }
    Ok(())
}
