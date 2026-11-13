//! Native Harness gateway credentials share the connection journal and restoration rules.
use super::*;

pub(super) const KEY: &str = crate::services::deepseek_harness::GATEWAY_KEY_REF;

pub(super) fn path(profile: &Path) -> Result<PathBuf, String> {
    let parent = profile.parent().ok_or(FAILED)?;
    let home = if parent.ends_with(Path::new("profiles").join("desktop")) {
        parent.parent().and_then(Path::parent).ok_or(FAILED)?
    } else {
        // Synthetic/custom profile overrides keep credentials beside their profile.
        parent
    };
    Ok(home.join(".credentials.yaml"))
}

pub(super) fn parse(text: Option<&str>) -> Result<Value, String> {
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return Ok(json!({"version":1}));
    };
    let mut value: Value = serde_yaml::from_str(text).map_err(|_| FAILED)?;
    if value.is_null() || value.as_object().is_some_and(|value| value.is_empty()) {
        return Ok(json!({"version":1}));
    }
    if value["version"] != 1
        || value.get("refs").is_some_and(|refs| {
            !refs.is_null()
                && !refs.as_object().is_some_and(|refs| {
                    refs.iter().all(|(name, value)| {
                        !name.is_empty()
                            && name.bytes().enumerate().all(|(i, byte)| {
                                byte == b'_'
                                    || byte.is_ascii_alphabetic()
                                    || (i > 0 && byte.is_ascii_digit())
                            })
                            && value.as_str().is_some_and(|value| !value.is_empty())
                    })
                })
        })
    {
        return Err(FAILED.into());
    }
    if value.get("refs").is_some_and(Value::is_null)
        || value["refs"]
            .as_object()
            .is_some_and(|refs| refs.is_empty())
    {
        value.as_object_mut().ok_or(FAILED)?.remove("refs");
    }
    Ok(value)
}

pub(super) fn prepare(profile: &Path, token: &str) -> Result<FileEdit, String> {
    let mut file = FileEdit::prepare_format(path(profile)?, Format::HarnessCredentials, |value| {
        if value["refs"]
            .get(KEY)
            .is_some_and(|key| key.as_str() != Some(token))
        {
            return Err(CONFLICT.into());
        }
        put(value, &["refs".into(), KEY.into()], Some(json!(token)))
    })?;
    // Track an already matching key too: removing it must invalidate the connection.
    file.fields = vec![vec!["refs".into(), KEY.into()]];
    Ok(file)
}

pub(super) fn encode(source: Option<&str>, next: &Value) -> Result<String, String> {
    if let Ok(text) = Format::Yaml.encode(source, next) {
        return Ok(text);
    }
    // Empty/flow mappings and null documents may need a full YAML serialization.
    let mut text = source
        .unwrap_or_default()
        .lines()
        .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    text.push_str(&serde_yaml::to_string(next).map_err(|_| FAILED)?);
    if parse(Some(&text))? != *next {
        return Err(FAILED.into());
    }
    Ok(text)
}

pub(super) fn protect_profile(file: &mut FileEdit) -> Result<(), String> {
    for (id, entry) in file.installed["providers"].as_object().ok_or(FAILED)? {
        for (route, profile) in entry["config"]["providers"].as_object().ok_or(FAILED)? {
            if profile["apiKeyEnv"] == KEY {
                let prefix = vec![
                    "providers".into(),
                    id.clone(),
                    "config".into(),
                    "providers".into(),
                    route.clone(),
                ];
                for field in ["apiKeyEnv", "headers"] {
                    let mut field_path = prefix.clone();
                    field_path.push(field.into());
                    if !file.fields.contains(&field_path) {
                        file.fields.push(field_path);
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn secure(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::File::open(path)
            .and_then(|file| file.set_permissions(fs::Permissions::from_mode(0o600)))
            .map_err(|_| FAILED)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn upgrades(before: &Receipt, after: &Receipt) -> bool {
    before.app == "deepseek-harness"
        && before.files.len() == 1
        && matches!(before.files[0].format, Format::Cordis)
        && after.files.len() == 2
        && after
            .files
            .iter()
            .any(|file| file.path == before.files[0].path && matches!(file.format, Format::Cordis))
        && after.files.iter().any(|file| {
            matches!(file.format, Format::HarnessCredentials)
                && path(&before.files[0].path).is_ok_and(|expected| file.path == expected)
        })
}

pub(super) fn release(manager: &Manager, receipt: &Receipt) -> Result<(), String> {
    let profile = receipt
        .files
        .iter()
        .find(|file| matches!(file.format, Format::Cordis))
        .ok_or(CONFLICT)?;
    if receipt.files.len() > 2
        || receipt.files.iter().any(|file| {
            !std::ptr::eq(file, profile)
                && !(matches!(file.format, Format::HarnessCredentials)
                    && path(&profile.path).is_ok_and(|expected| file.path == expected)
                    && file.installed["refs"][KEY].as_str() == Some(receipt.token.as_str()))
        })
    {
        return Err(CONFLICT.into());
    }
    let mut edits = Vec::new();
    for file in &receipt.files {
        let source = read(&file.path)?;
        let baseline = file.format.parse(file.original.as_deref())?;
        let replacement = if matches!(file.format, Format::Cordis) {
            Some(crate::services::deepseek_harness::recover_for_reapply(
                source.as_deref().ok_or(CONFLICT)?,
                &file.installed,
                &baseline,
            )?)
        } else {
            let mut clean = file.format.parse(source.as_deref())?;
            put(
                &mut clean,
                &["refs".into(), KEY.into()],
                baseline["refs"].get(KEY).cloned(),
            )?;
            if clean == baseline {
                file.original.clone()
            } else {
                Some(file.format.encode(source.as_deref(), &clean)?)
            }
        };
        edits.push((&file.path, source, replacement));
    }
    // Save all validated inputs privately before releasing either native file.
    let id = new_token();
    for (index, (_, source, _)) in edits.iter().enumerate() {
        if let Some(source) = source {
            let backup = manager
                .root
                .join(format!("deepseek-harness-recovery-{id}-{index}.yml"));
            atomic_write_private(&backup, source.as_bytes()).map_err(|_| FAILED)?;
        }
    }
    for (index, (path, source, replacement)) in edits.iter().enumerate() {
        if let Err(error) = replace_checked(path, source.as_deref(), replacement.as_deref()) {
            for (path, source, replacement) in edits[..index].iter().rev() {
                let _ = replace_checked(path, replacement.as_deref(), source.as_deref());
            }
            return Err(error);
        }
    }
    manager.remember_model(&receipt.app, Some(&receipt.model));
    fs::remove_file(manager.path(&receipt.app)?).map_err(|_| FAILED.into())
}
