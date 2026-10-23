use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

const KEY_PREFIX: &str = "connection_preset:";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionPreset {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
}

fn preset_key(id: &str) -> Result<String, AppError> {
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| AppError::Message("Invalid connection preset ID".into()))?;
    Ok(format!("{KEY_PREFIX}{id}"))
}

impl Database {
    pub fn list_connection_presets(&self) -> Result<Vec<ConnectionPreset>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key GLOB ?1 ORDER BY key")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([format!("{KEY_PREFIX}*")], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.map(|row| {
            let json = row.map_err(|e| AppError::Database(e.to_string()))?;
            serde_json::from_str(&json)
                .map_err(|_| AppError::Database("Invalid connection preset data".into()))
        })
        .collect()
    }

    pub fn save_connection_preset(&self, preset: &ConnectionPreset) -> Result<(), AppError> {
        let mut preset = preset.clone();
        let key = preset_key(&preset.id)?;
        preset.id = key.trim_start_matches(KEY_PREFIX).to_owned();
        preset.name = preset.name.trim().to_owned();
        preset.base_url = preset.base_url.trim().to_owned();
        preset.api_key = preset.api_key.trim().to_owned();
        let invalid = || AppError::Message("Invalid connection preset fields".into());
        let endpoint = reqwest::Url::parse(&preset.base_url).map_err(|_| invalid())?;
        if preset.name.is_empty()
            || preset.api_format.as_deref().is_some_and(|format| {
                !matches!(
                    format,
                    "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"
                )
            })
            || preset.name.chars().count() > 80
            || preset.name.chars().any(char::is_control)
            || preset.api_key.is_empty()
            || preset.api_key.len() > 16384
            || preset.api_key.chars().any(char::is_control)
            || preset.base_url.len() > 4096
            || preset
                .base_url
                .chars()
                .any(|c| c.is_whitespace() || c == '\\')
            || !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(invalid());
        }
        let json = serde_json::to_string(&preset)
            .map_err(|_| AppError::Message("Could not encode connection preset".into()))?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_connection_preset(&self, id: &str) -> Result<(), AppError> {
        self.delete_setting(&preset_key(id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ConnectionPreset {
        ConnectionPreset {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Fixture relay".into(),
            base_url: "https://relay.example/custom/v1/".into(),
            api_key: "synthetic-credential".into(),
            api_format: None,
        }
    }

    #[test]
    fn saves_updates_and_deletes_independent_named_connections() {
        let db = Database::memory().expect("memory database");
        let mut first = fixture();
        let second = fixture();
        db.save_connection_preset(&first).expect("save first");
        db.save_connection_preset(&second).expect("save second");
        first.name = "Updated name".into();
        db.save_connection_preset(&first).expect("update first");
        let rows = db.list_connection_presets().expect("list presets");
        assert_eq!(rows.len(), 2);
        let row = rows
            .iter()
            .find(|row| row.id == first.id)
            .expect("first row");
        assert_eq!(row.name, first.name);
        assert_eq!(row.base_url, first.base_url);
        assert_eq!(row.api_key, first.api_key);
        db.delete_connection_preset(&first.id)
            .expect("delete first");
        let rows = db.list_connection_presets().expect("list remaining");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, second.id);
    }

    #[test]
    fn rejects_invalid_fields_without_persisting_or_exposing_input() {
        let db = Database::memory().expect("memory database");
        for endpoint in [
            "file:///private/path",
            "https://user:synthetic-secret@relay.example/v1",
            "https://relay.example/v1?key=synthetic-secret",
            "https://relay.example/v1#synthetic-secret",
        ] {
            let mut preset = fixture();
            preset.base_url = endpoint.into();
            let error = db.save_connection_preset(&preset).expect_err("invalid URL");
            assert!(!error.to_string().contains("synthetic-secret"));
        }
        let mut preset = fixture();
        preset.name.clear();
        assert!(db.save_connection_preset(&preset).is_err());
        preset = fixture();
        preset.api_key.clear();
        assert!(db.save_connection_preset(&preset).is_err());
        assert!(db.list_connection_presets().expect("empty list").is_empty());
    }

    #[test]
    fn preserves_protocols_and_reads_legacy_presets_without_a_format() {
        let db = Database::memory().expect("memory database");
        let legacy = fixture();
        db.set_setting(
            &preset_key(&legacy.id).expect("key"),
            &serde_json::to_string(&legacy).expect("fixture JSON"),
        )
        .expect("legacy preset");
        assert!(db.list_connection_presets().expect("legacy list")[0]
            .api_format
            .is_none());
        for format in [
            "anthropic",
            "openai_chat",
            "openai_responses",
            "gemini_native",
        ] {
            let mut preset = fixture();
            preset.api_format = Some(format.into());
            db.save_connection_preset(&preset).expect("save format");
            let rows = db.list_connection_presets().expect("list formats");
            assert_eq!(
                rows.iter()
                    .find(|p| p.id == preset.id)
                    .expect("saved preset")
                    .api_format
                    .as_deref(),
                Some(format)
            );
        }
        let mut invalid = fixture();
        invalid.api_format = Some("synthetic-private-format".into());
        let error = db
            .save_connection_preset(&invalid)
            .expect_err("invalid format");
        assert!(!error.to_string().contains("synthetic-private-format"));
        assert_eq!(
            db.list_connection_presets().expect("unchanged list").len(),
            5
        );
    }
}
