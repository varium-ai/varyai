//! Preserve native catalog additions without taking ownership of them.
use super::{read, Receipt, CONFLICT, FAILED};
use serde_json::Value;
use std::collections::BTreeSet;

fn rows(value: &Value) -> Option<&Vec<Value>> {
    value.as_array().or_else(|| value.get("models")?.as_array())
}

fn append(value: &mut Value, additions: &[Value]) -> Result<(), String> {
    if additions.is_empty() {
        return Ok(());
    }
    let rows = if value.is_array() {
        value.as_array_mut().ok_or(CONFLICT)?
    } else {
        value
            .as_object_mut()
            .ok_or(CONFLICT)?
            .entry("models")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or(CONFLICT)?
    };
    for row in additions {
        if rows.iter().any(|old| old.get("id") == row.get("id")) {
            return Err(CONFLICT.into());
        }
        rows.push(row.clone());
    }
    Ok(())
}

pub(super) fn adopt(receipt: &mut Receipt) -> Result<(), String> {
    if receipt.suspended || !matches!(receipt.app.as_str(), "workbuddy" | "workbuddy-ai") {
        return Ok(());
    }
    for file in &mut receipt.files {
        let live = file.format.parse(read(&file.path)?.as_deref())?;
        if live == file.installed {
            continue;
        }
        if live.is_array() != file.installed.is_array() {
            return Err(CONFLICT.into());
        }
        let installed = rows(&file.installed).ok_or(CONFLICT)?;
        let current = rows(&live).ok_or(CONFLICT)?;
        if !installed
            .iter()
            .any(|row| row["apiKey"].as_str() == Some(&receipt.token))
        {
            return Err(CONFLICT.into());
        }
        let ids = current
            .iter()
            .map(|row| row.get("id").and_then(Value::as_str))
            .collect::<Option<BTreeSet<_>>>()
            .ok_or(CONFLICT)?;
        // Existing rows (including credentials and endpoints) must be unchanged.
        if ids.len() != current.len() || !installed.iter().all(|row| current.contains(row)) {
            return Err(CONFLICT.into());
        }
        let additions = current
            .iter()
            .filter(|row| !installed.contains(row))
            .cloned()
            .collect::<Vec<_>>();
        if additions
            .iter()
            .any(|row| row["apiKey"].as_str() == Some(&receipt.token))
        {
            return Err(CONFLICT.into());
        }
        let mut comparable = live.clone();
        if comparable.is_array() {
            comparable = file.installed.clone();
        } else {
            comparable["models"] = Value::Array(installed.clone());
        }
        if comparable != file.installed {
            return Err(CONFLICT.into());
        }
        if !additions.is_empty() {
            let mut original = file.format.parse(file.original.as_deref())?;
            append(&mut original, &additions)?;
            file.original = Some(serde_json::to_string_pretty(&original).map_err(|_| FAILED)?);
            if let Some(previous) = &mut file.previous {
                append(previous, &additions)?;
            }
        }
        // This is an in-memory baseline adjustment, not a client configuration write.
        file.installed = live;
    }
    Ok(())
}
