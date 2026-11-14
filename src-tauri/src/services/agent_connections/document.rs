use super::{parse, FAILED};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(super) enum Format {
    #[default]
    Json,
    // Persist the old receipt format; OpenCode upgrades it when loading.
    #[serde(rename(serialize = "Json5"))]
    Jsonc,
    Json5,
    Toml,
    Yaml,
    HarnessCredentials,
    Cordis,
    Env,
}
impl Format {
    pub(super) fn parse(self, text: Option<&str>) -> Result<Value, String> {
        let value = match self {
            Self::HarnessCredentials => return super::harness_credentials::parse(text),
            Self::Cordis => return crate::services::deepseek_harness::parse(text.unwrap_or("")),
            Self::Json => return parse(text),
            // Accept legacy receipts for recovery; Jsonc encoding validates the
            // emitted document against OpenCode's stricter syntax.
            Self::Jsonc | Self::Json5 => {
                json5::from_str(text.unwrap_or("{}")).map_err(|_| FAILED)?
            }
            Self::Toml => serde_json::to_value(
                text.unwrap_or("")
                    .parse::<toml::Value>()
                    .map_err(|_| FAILED)?,
            )
            .map_err(|_| FAILED)?,
            Self::Yaml => {
                serde_yaml::from_str(text.filter(|text| !text.trim().is_empty()).unwrap_or("{}"))
                    .map_err(|_| FAILED)?
            }
            Self::Env => {
                let mut value = json!({});
                for line in text.unwrap_or("").lines() {
                    if let Some((key, raw)) = env_line(line)? {
                        if value.get(key).is_some() {
                            return Err(FAILED.into());
                        }
                        let raw = raw.trim();
                        let decoded = if raw.starts_with('"') {
                            serde_json::from_str::<String>(raw).map_err(|_| FAILED)?
                        } else if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
                            raw[1..raw.len() - 1].to_owned()
                        } else {
                            raw.to_owned()
                        };
                        value[key] = json!(decoded);
                    }
                }
                value
            }
        };
        if !value.is_object() {
            return Err(FAILED.into());
        }
        Ok(value)
    }
    pub(super) fn encode(self, source: Option<&str>, next: &Value) -> Result<String, String> {
        let before = self.parse(source)?;
        match self {
            Self::HarnessCredentials => super::harness_credentials::encode(source, next),
            Self::Cordis => crate::services::deepseek_harness::encode(source.unwrap_or(""), next),
            Self::Json => serde_json::to_string_pretty(next).map_err(|_| FAILED.into()),
            Self::Jsonc => super::jsonc::encode(source.unwrap_or("{}"), &before, next),
            Self::Json5 => {
                crate::openclaw_config::patch_document(source.unwrap_or("{}"), &before, next)
                    .map_err(|_| FAILED.into())
            }
            Self::Toml => {
                let mut doc = source
                    .unwrap_or("")
                    .parse::<toml_edit::DocumentMut>()
                    .map_err(|_| FAILED)?;
                crate::services::agent_clients::patch_toml(doc.as_item_mut(), &before, next)
                    .map_err(|_| FAILED)?;
                Ok(doc.to_string())
            }
            Self::Yaml => {
                let mut text = source.unwrap_or("").to_owned();
                let a = before.as_object().ok_or(FAILED)?;
                let b = next.as_object().ok_or(FAILED)?;
                for (key, value) in b {
                    if a.get(key) != Some(value) {
                        let yaml = serde_yaml::to_value(value).map_err(|_| FAILED)?;
                        text = crate::hermes_config::replace_yaml_section(&text, key, &yaml)
                            .map_err(|_| FAILED)?;
                    }
                }
                for key in a.keys().filter(|key| !b.contains_key(*key)) {
                    text = crate::hermes_config::remove_all_sections(&text, key);
                }
                if self.parse(Some(&text))? != *next {
                    return Err(FAILED.into());
                }
                Ok(text)
            }
            Self::Env => {
                let mut lines = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for line in source.unwrap_or("").lines() {
                    if let Some((key, _)) = env_line(line)? {
                        seen.insert(key.to_owned());
                        if before.get(key) != next.get(key) {
                            if let Some(value) = next.get(key) {
                                lines.push(env_entry(key, value)?);
                            }
                            continue;
                        }
                    }
                    lines.push(line.to_owned());
                }
                for (key, value) in next.as_object().ok_or(FAILED)? {
                    if !seen.contains(key) {
                        lines.push(env_entry(key, value)?);
                    }
                }
                Ok(format!("{}\n", lines.join("\n")))
            }
        }
    }
}
fn env_line(line: &str) -> Result<Option<(&str, &str)>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=').ok_or(FAILED)?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    {
        return Err(FAILED.into());
    }
    Ok(Some((key, value)))
}
fn env_entry(key: &str, value: &Value) -> Result<String, String> {
    let value = value.as_str().ok_or(FAILED)?;
    if value.bytes().any(|c| c.is_ascii_control()) {
        return Err(FAILED.into());
    }
    let encoded = serde_json::to_string(value).map_err(|_| FAILED)?;
    Ok(format!("{key}={encoded}"))
}
