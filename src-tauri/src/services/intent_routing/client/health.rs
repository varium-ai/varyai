use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfigurationIssue {
    ProviderMissing,
    ProfileOverride,
    CatalogMissing,
}

fn names_provider(doc: &toml::Value) -> bool {
    let names = |value: &toml::Value| {
        value.get("model_provider").and_then(toml::Value::as_str) == Some(PROVIDER)
    };
    names(doc)
        || doc
            .get("profiles")
            .and_then(toml::Value::as_table)
            .is_some_and(|profiles| profiles.values().any(names))
}

pub(super) fn configuration_issue(
    config: &Path,
    current: Option<&str>,
) -> Result<Option<ConfigurationIssue>, String> {
    let Some(current) = current else {
        return Ok(None);
    };
    let doc: toml::Value = current.parse().map_err(|_| FAILED)?;
    if names_provider(&doc)
        && doc
            .get("model_providers")
            .and_then(|v| v.get(PROVIDER))
            .is_none()
    {
        return Ok(Some(ConfigurationIssue::ProviderMissing));
    }
    let native_projection = doc
        .get("openai_base_url")
        .and_then(toml::Value::as_str)
        .filter(|url| valid_native_url(url))
        .and_then(|url| url::Url::parse(url).ok())
        .zip(
            doc.get("model_providers")
                .and_then(|providers| providers.get(PROVIDER))
                .and_then(|provider| provider.get("base_url"))
                .and_then(toml::Value::as_str)
                .and_then(|url| url::Url::parse(url).ok()),
        )
        .is_some_and(|(native, menu)| {
            native.origin() == menu.origin() && menu.path() == "/intent/v1"
        });
    if !references_projection(&doc) && !native_projection {
        return Ok(None);
    }
    let profile = doc
        .get("profile")
        .and_then(toml::Value::as_str)
        .and_then(|name| doc.get("profiles")?.get(name));
    if profile.is_some_and(|profile| {
        [
            "model",
            "model_provider",
            "model_catalog_json",
            "openai_base_url",
        ]
        .iter()
        .any(|key| {
            profile
                .get(*key)
                .is_some_and(|value| Some(value) != doc.get(*key))
        })
    }) {
        return Ok(Some(ConfigurationIssue::ProfileOverride));
    }
    if let Some(catalog) = doc.get("model_catalog_json").and_then(toml::Value::as_str) {
        let path = Path::new(catalog);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config.parent().ok_or(FAILED)?.join(path)
        };
        if !path.is_file() {
            return Ok(Some(ConfigurationIssue::CatalogMissing));
        }
    }
    Ok(None)
}

impl Manager {
    /// Restore a missing definition that Codex already names, including profiles.
    /// Existing definitions and unrelated routing choices remain authoritative.
    pub(crate) fn sync_configuration(&self, base: &str) -> Result<(), String> {
        let Some(current) = read(&self.config)? else {
            return Ok(());
        };
        let value: toml::Value = current.parse().map_err(|_| FAILED)?;
        let receipt = self.load()?;
        let menu = self.runtime.menu_snapshot()?;
        let receipt_routes = receipt
            .as_ref()
            .and_then(|receipt| receipt.installed.parse::<toml::Value>().ok())
            .is_some_and(|installed| {
                ["model_provider", "model_catalog_json", "openai_base_url"]
                    .iter()
                    .all(|key| value.get(*key) == installed.get(*key))
                    && installed
                        .get("model_providers")
                        .and_then(|v| v.get(PROVIDER))
                        .is_some()
            });
        if (names_provider(&value) || receipt_routes)
            && value
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
                .is_none()
        {
            let mut doc: DocumentMut = current.parse().map_err(|_| FAILED)?;
            let provider = if let Some(receipt) = &receipt {
                let installed: DocumentMut = receipt.installed.parse().map_err(|_| FAILED)?;
                installed
                    .get("model_providers")
                    .and_then(|v| v.get(PROVIDER))
                    .filter(|v| v.is_table_like())
                    .cloned()
                    .ok_or(CONFLICT)?
            } else {
                let token = menu
                    .as_ref()
                    .map(|m| m.token.clone())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
                let provider =
                    provider_table(base, &token, requires_openai_auth(&self.config, &doc), None)?;
                let parsed: toml::Value = provider.to_string().parse().map_err(|_| FAILED)?;
                if !parsed.as_table().is_some_and(recognized_orphan_provider) {
                    return Err(CONFLICT.into());
                }
                toml_edit::Item::Table(provider)
            };
            if doc.get("model_providers").is_none() {
                doc["model_providers"] = toml_edit::table();
            }
            doc["model_providers"]
                .as_table_like_mut()
                .ok_or(CONFLICT)?
                .insert(PROVIDER, provider);
            self.backup_orphan(&current)?;
            replace_checked(&self.config, Some(&current), Some(&doc.to_string()))?;
        }
        // Rebuild only a missing catalog owned by the current receipt and menu.
        if let (Some(receipt), Some(menu)) = (receipt, menu) {
            if let Some(path) = value
                .get("model_catalog_json")
                .and_then(toml::Value::as_str)
                .map(Path::new)
            {
                if receipt.catalogs.iter().any(|owned| owned == path)
                    && path.parent() == self.receipt.parent()
                    && read(path)?.is_none()
                {
                    let chosen = value
                        .get("model_reasoning_effort")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("");
                    let catalog = serde_json::to_string_pretty(&menu_catalog(&menu, chosen)?)
                        .map_err(|_| FAILED)?;
                    replace_checked(path, None, Some(&catalog))?;
                }
            }
        }
        self.sync_threads()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_profile_provider_override_is_reported_without_an_alias_model(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, manager) = super::super::tests::setup();
        let text = "model='native-model'\nprofile='custom'\nopenai_base_url='http://127.0.0.1:1234/backend-api/codex'\n[profiles.custom]\nmodel_provider='another-provider'\n[model_providers.varyai]\nbase_url='http://127.0.0.1:1234/intent/v1'\n";
        assert_eq!(
            configuration_issue(&manager.config, Some(text))?,
            Some(ConfigurationIssue::ProfileOverride)
        );
        Ok(())
    }

    #[test]
    fn sync_repairs_retained_provider_beside_chatgpt() -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        fs::write(
            dir.path().join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"synthetic"}}"#,
        )?;
        manager.write_projection_with_native(
            &[super::super::tests::group()],
            "synthetic-local",
            "http://127.0.0.1:1234/intent/v1",
            Some("http://127.0.0.1:1234/backend-api/codex"),
        )?;
        let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
        assert!(doc.get("model_provider").is_none());
        doc["model_providers"]
            .as_table_like_mut()
            .ok_or("providers")?
            .remove(PROVIDER);
        fs::write(&manager.config, doc.to_string())?;
        manager.sync_configuration("http://127.0.0.1:1234/intent/v1")?;
        assert!(manager.status()?.configuration_owned);
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert!(doc.get("model_provider").is_none());
        Ok(())
    }

    #[test]
    fn sync_restores_missing_provider_at_top_level_and_in_profiles(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for selection in [
            "model_provider = 'varyai'\n",
            "profile = 'work'\n[profiles.work]\nmodel_provider = 'varyai'\n",
        ] {
            let dir = tempfile::tempdir()?;
            let config = dir.path().join("config.toml");
            let before = format!("# synthetic fixture\nopenai_base_url = 'http://127.0.0.1:3425/backend-api/codex'\n{selection}\n[model_providers.custom]\nbase_url = 'https://synthetic.example/v1'\n[plugins.example]\nenabled = true\n");
            fs::write(&config, &before)?;
            let manager = Manager::new(
                config.clone(),
                dir.path().join("receipt.json"),
                Arc::default(),
            );
            manager.sync_configuration("http://127.0.0.1:1234/intent/v1")?;
            let after: toml::Value = fs::read_to_string(&config)?.parse()?;
            let provider = after
                .get("model_providers")
                .and_then(|v| v.get(PROVIDER))
                .ok_or("missing repaired provider")?;
            assert_eq!(provider["wire_api"].as_str(), Some("responses"));
            assert_eq!(
                provider["base_url"].as_str(),
                Some("http://127.0.0.1:1234/intent/v1")
            );
            assert!(provider["experimental_bearer_token"]
                .as_str()
                .is_some_and(|token| !token.is_empty()));
            let mut expected: toml::Value = before.parse()?;
            expected["model_providers"]
                .as_table_mut()
                .ok_or("providers")?
                .insert(PROVIDER.into(), provider.clone());
            assert_eq!(after, expected);
            let once = fs::read(&config)?;
            manager.sync_configuration("http://127.0.0.1:1234/intent/v1")?;
            assert_eq!(fs::read(config)?, once);
        }
        Ok(())
    }

    #[test]
    fn sync_restores_the_owned_table_without_resetting_other_edits(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (dir, manager) = super::super::tests::setup();
        manager.write_projection(
            &[super::super::tests::group()],
            "synthetic-local-token",
            "http://127.0.0.1:1234/intent/v1",
        )?;
        let installed: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        let mut edited: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
        edited["model_providers"]
            .as_table_like_mut()
            .ok_or("providers")?
            .remove(PROVIDER);
        edited["theme"] = toml_edit::value("synthetic-theme");
        fs::write(&manager.config, edited.to_string())?;
        assert!(!manager.status()?.configuration_owned);
        manager.sync_configuration("http://127.0.0.1:4321/intent/v1")?;
        assert!(manager.status()?.configuration_owned);
        let after: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(
            after["model_providers"][PROVIDER],
            installed["model_providers"][PROVIDER]
        );
        assert_eq!(after["theme"].as_str(), Some("synthetic-theme"));
        assert!(!dir.path().join("auth.json").exists());
        Ok(())
    }

    #[test]
    fn sync_preserves_existing_provider_and_unrelated_configuration(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for before in ["model_provider = 'custom'\n[model_providers.custom]\nbase_url = 'https://synthetic.example/v1'\n", "model_provider = 'varyai'\n[model_providers.varyai]\nname = 'User customization'\nbase_url = 'https://synthetic.example/v1'\n", "model = 'vary-provider/synthetic'\n"] {
            let (_dir, manager) = super::super::tests::setup();
            fs::write(&manager.config, before)?;
            manager.sync_configuration("http://127.0.0.1:1234/intent/v1")?;
            assert_eq!(fs::read_to_string(&manager.config)?, before);
            assert!(!manager.receipt.exists());
        }
        Ok(())
    }
}
