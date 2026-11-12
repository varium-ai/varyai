//! T3 Code uses an independent Codex instance, leaving existing logins intact.
use super::{read, FileEdit, Format, Receipt, CONFLICT, FAILED};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) fn settings_path() -> PathBuf {
    let isolated = std::env::var("CC_SWITCH_TEST_HOME").is_ok_and(|home| !home.trim().is_empty());
    std::env::var_os("T3CODE_HOME")
        .filter(|path| !isolated && !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::get_home_dir().join(".t3"))
        .join("userdata/settings.json")
}

pub(super) fn project(
    settings: &Path,
    catalog: &Value,
    base: &str,
    receipt: &mut Receipt,
) -> Result<(), String> {
    let home = settings.parent().ok_or(FAILED)?.join("varyai-codex");
    let config_path = home.join("config.toml");
    let catalog_path = home.join("models.json");
    let current = Format::Toml.parse(read(&config_path)?.as_deref())?;
    let owned = current
        .pointer("/model_providers/varyai/experimental_bearer_token")
        .and_then(Value::as_str)
        == Some(receipt.token.as_str());
    if !owned && (read(&config_path)?.is_some() || read(&catalog_path)?.is_some()) {
        return Err(CONFLICT.into());
    }
    let mut catalog = catalog.clone();
    let mut selected = String::new();
    let settings_edit = FileEdit::prepare(settings.to_path_buf(), |value| {
        if value.get("providers").is_some() {
            // Older T3 schemas need an explicit migration in T3 before integration.
            return Err(FAILED.into());
        }
        if let Some(existing) = value.pointer("/providerInstances/varyai") {
            if !owned || existing.pointer("/config/homePath") != Some(&json!(home)) {
                return Err(CONFLICT.into());
            }
        }
        let visible = crate::services::intent_routing::client::aliases::project(
            &mut catalog,
            &mut receipt.model_map,
        )?;
        selected = crate::services::intent_routing::client::aliases::selected(
            &visible,
            &receipt.model_map,
            &receipt.model,
        )
        .ok_or(FAILED)?
        .to_owned();
        receipt.visible_models = Some(visible);
        let models = catalog["models"].as_array().ok_or(FAILED)?;
        for (key, next) in [
            ("driver", json!("codex")),
            ("displayName", json!("VaryAI")),
            ("enabled", json!(true)),
            ("environment", json!([])),
        ] {
            super::put(
                value,
                &["providerInstances".into(), "varyai".into(), key.into()],
                Some(next),
            )?;
        }
        for (key, next) in [
            ("setupMode", json!("existing")),
            ("homePath", json!(home)),
            ("shadowHomePath", json!("")),
            ("launchArgs", json!("")),
            (
                "customModels",
                json!(models
                    .iter()
                    .filter(|model| model["visibility"] != "hide")
                    .map(|model| json!({"slug":model["slug"],"name":model["display_name"]}))
                    .collect::<Vec<_>>()),
            ),
        ] {
            super::put(
                value,
                &[
                    "providerInstances".into(),
                    "varyai".into(),
                    "config".into(),
                    key.into(),
                ],
                Some(next),
            )?;
        }
        for (key, next) in [("instanceId", json!("varyai")), ("model", json!(selected))] {
            super::put(
                value,
                &["defaultModelSelection".into(), key.into()],
                Some(next),
            )?;
        }
        Ok(())
    })?;
    receipt
        .files
        .push(FileEdit::prepare(catalog_path.clone(), |value| {
            *value = catalog.clone();
            Ok(())
        })?);
    receipt.files.push(FileEdit::prepare_format(
        config_path,
        Format::Toml,
        |value| {
            value["model"] = json!(selected);
            value["model_provider"] = json!("varyai");
            value["model_catalog_json"] = json!(catalog_path);
            value["web_search"] = json!("disabled");
            super::put(
                value,
                &["model_providers".into(), "varyai".into()],
                Some(json!({
                    "name":"VaryAI", "base_url":format!("{base}/v1"), "wire_api":"responses",
                    "requires_openai_auth":false, "experimental_bearer_token":receipt.token,
                    "supports_websockets":false
                })),
            )
        },
    )?);
    receipt.files.push(settings_edit);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_connections::{read, Manager, Receipt, CONFLICT, FAILED};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn t3code_projects_an_isolated_provider_and_restores_only_owned_fields() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let settings = dir.path().join("userdata/settings.json");
        std::fs::create_dir_all(settings.parent().ok_or(FAILED)?).map_err(|_| FAILED)?;
        let original = json!({"providerInstances":{"personal":{"driver":"codex","config":{"homePath":"/synthetic/personal"}}},"defaultModelSelection":{"instanceId":"personal","model":"native-model"},"theme":"dark"});
        std::fs::write(&settings, original.to_string()).map_err(|_| FAILED)?;
        let mut receipt = Receipt {
            app: "t3code".into(),
            token: "synthetic-local-token".into(),
            model: "vary-provider/example".into(),
            model_map: BTreeMap::new(),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        };
        project(
            &settings,
            &json!({"models":[{"slug":"vary-provider/example","display_name":"Example"}]}),
            "http://127.0.0.1:1/connections/t3code",
            &mut receipt,
        )?;
        assert!(!settings
            .parent()
            .ok_or(FAILED)?
            .join("varyai-codex")
            .exists());
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        manager.install(receipt)?;
        let installed: serde_json::Value =
            serde_json::from_str(&read(&settings)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        assert_eq!(
            installed["providerInstances"]["personal"],
            original["providerInstances"]["personal"]
        );
        assert_eq!(installed["providerInstances"]["varyai"]["driver"], "codex");
        assert_eq!(
            installed["providerInstances"]["varyai"]["config"]["setupMode"],
            "existing"
        );
        assert_eq!(
            installed["providerInstances"]["varyai"]["config"]["customModels"][0]["slug"],
            "vary-provider/example"
        );
        assert_eq!(
            installed["defaultModelSelection"],
            json!({"instanceId":"varyai","model":"vary-provider/example"})
        );
        assert_eq!(
            installed["providerInstances"]["varyai"]["config"]["customModels"][0]["name"],
            "Example"
        );
        let home = std::path::PathBuf::from(
            installed["providerInstances"]["varyai"]["config"]["homePath"]
                .as_str()
                .ok_or(FAILED)?,
        );
        let config: toml::Value = read(&home.join("config.toml"))?
            .ok_or(FAILED)?
            .parse()
            .map_err(|_| FAILED)?;
        assert_eq!(
            config["model_providers"]["varyai"]["base_url"].as_str(),
            Some("http://127.0.0.1:1/connections/t3code/v1")
        );
        assert_eq!(
            config["model_providers"]["varyai"]["experimental_bearer_token"].as_str(),
            Some("synthetic-local-token")
        );
        assert_eq!(
            config["model_providers"]["varyai"]["requires_openai_auth"].as_bool(),
            Some(false)
        );
        assert!(!home.join("auth.json").exists());
        let mut edited = installed;
        edited["theme"] = json!("light");
        std::fs::write(&settings, edited.to_string()).map_err(|_| FAILED)?;
        manager.disconnect("t3code")?;
        let restored: serde_json::Value =
            serde_json::from_str(&read(&settings)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        assert_eq!(restored["providerInstances"], original["providerInstances"]);
        assert_eq!(
            restored["defaultModelSelection"],
            original["defaultModelSelection"]
        );
        assert_eq!(restored["theme"], "light");
        assert!(!home.join("config.toml").exists());
        assert!(!home.join("models.json").exists());
        Ok(())
    }

    #[test]
    fn t3code_refuses_an_existing_provider_named_varyai_without_writing() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let settings = dir.path().join("settings.json");
        let source = r#"{"providerInstances":{"varyai":{"driver":"claude"}}}"#;
        std::fs::write(&settings, source).map_err(|_| FAILED)?;
        let mut receipt = Receipt {
            app: "t3code".into(),
            token: "synthetic-local-token".into(),
            model: "example".into(),
            model_map: BTreeMap::new(),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        };
        assert_eq!(
            project(
                &settings,
                &json!({"models":[]}),
                "http://127.0.0.1:1/connections/t3code",
                &mut receipt
            ),
            Err(CONFLICT.into())
        );
        assert_eq!(read(&settings)?.as_deref(), Some(source));
        assert!(!dir.path().join("varyai-codex").exists());
        Ok(())
    }
}
