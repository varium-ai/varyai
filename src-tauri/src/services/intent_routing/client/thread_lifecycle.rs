//! Join provider transitions to the configuration receipt without blocking requests.
use super::*;

impl Manager {
    pub(crate) fn thread_sync_pending(&self) -> bool {
        self.load()
            .ok()
            .flatten()
            .is_some_and(|r| r.thread_sync_pending)
    }

    pub(crate) fn sync_threads(&self) -> Result<(), String> {
        let Some(mut receipt) = self.load()? else {
            return Ok(());
        };
        if receipt.thread_restore_pending {
            return self.restore();
        }
        if read(&self.config)?
            .as_deref()
            .is_some_and(|current| projection_owned(current, &receipt.installed))
        {
            self.reconcile_thread_receipt(&mut receipt, false)?;
        }
        Ok(())
    }

    pub(super) fn reconcile_thread_receipt(
        &self,
        receipt: &mut Receipt,
        restoring: bool,
    ) -> Result<(), String> {
        let original: toml::Value = receipt
            .original
            .as_deref()
            .unwrap_or("")
            .parse()
            .map_err(|_| FAILED)?;
        let installed: toml::Value = receipt.installed.parse().map_err(|_| FAILED)?;
        // Codex applies the selected profile's provider over the top-level
        // provider. Inactive profiles do not affect the provider whose
        // threads are visible, and a selected profile without an override
        // inherits the top-level value.
        // A receipt adopted from an orphaned VaryAI configuration has no
        // previous journal to name the user's provider. The old lifecycle
        // treated top-level VaryAI as Codex's default OpenAI bucket. A named
        // profile remains authoritative, including when it names VaryAI.
        let source = source_provider(&original);
        let target = effective_provider(&installed);
        if !restoring && source == target && receipt.thread_recovery_id.is_none() {
            return Ok(());
        }
        // Keep a user's provider bucket intact when any named profile points
        // at it. Codex can select that profile later even when it is not the
        // active one during this projection.
        if !restoring
            && target == PROVIDER
            && source != "openai"
            && receipt.thread_recovery_id.is_none()
            && profile_names_provider(&original, &source)
        {
            return Ok(());
        }
        // A profile-owned provider was never taken over by this receipt. Do
        // not move its history merely because the top-level projection is
        // ours; only a receipt with a recovery journal may restore a prior
        // takeover.
        if target != PROVIDER && receipt.thread_recovery_id.is_none() {
            return Ok(());
        }
        let recovery_id = receipt
            .thread_recovery_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
            .clone();
        if recovery_id.len() != 32 || !recovery_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CONFLICT.into());
        }
        let home = self.config.parent().ok_or(FAILED)?;
        let recovery = self
            .receipt
            .parent()
            .ok_or(FAILED)?
            .join("codex-thread-recovery")
            .join(&recovery_id);
        receipt.thread_restore_pending = restoring;
        receipt.thread_sync_pending = true;
        self.save_thread_receipt(receipt)?;
        let result = if !restoring && target == PROVIDER {
            let models = self.published_thread_models(receipt, &installed)?;
            threads::transition_with_models(
                home,
                &recovery.join("into-routing"),
                source.as_str(),
                PROVIDER,
                &models,
            )
        } else {
            // Restore the recorded chats, then include chats created during takeover.
            let restored = threads::restore(
                home,
                &recovery.join("into-routing"),
                source.as_str(),
                PROVIDER,
            );
            match restored {
                Ok(mut report) => match threads::transition(
                    home,
                    &recovery.join("out-of-routing"),
                    PROVIDER,
                    if target == PROVIDER {
                        source.as_str()
                    } else {
                        target.as_str()
                    },
                ) {
                    Ok(new_chats) => {
                        report.incomplete |= new_chats.incomplete;
                        Ok(report)
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };
        receipt.thread_sync_pending = result.map_or(true, |report| report.incomplete);
        self.save_thread_receipt(receipt)
    }

    fn published_thread_models(
        &self,
        receipt: &Receipt,
        installed: &toml::Value,
    ) -> Result<std::collections::HashSet<String>, String> {
        let empty = std::collections::HashSet::new;
        if !read(&self.config)?
            .as_deref()
            .is_some_and(|current| projection_owned(current, &receipt.installed))
        {
            return Ok(empty());
        }
        if let Some(menu) = self.runtime.menu_snapshot()? {
            let token = installed
                .get("model_providers")
                .and_then(|providers| providers.get(PROVIDER))
                .and_then(|provider| provider.get("experimental_bearer_token"))
                .and_then(toml::Value::as_str);
            return Ok(if token == Some(menu.token.as_str()) {
                menu.groups.keys().cloned().collect()
            } else {
                empty()
            });
        }
        // Recovery without an active runtime may use only the exact owned
        // catalog whose alias set was recorded by this publication.
        let Some(path) = installed
            .get("model_catalog_json")
            .and_then(toml::Value::as_str)
            .map(Path::new)
            .filter(|path| {
                path.parent() == self.receipt.parent()
                    && receipt.catalogs.iter().any(|owned| owned == path)
            })
        else {
            return Ok(empty());
        };
        let catalog = read(path)?.and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let aliases = catalog
            .as_ref()
            .and_then(|catalog| catalog.get("models"))
            .and_then(Value::as_array)
            .and_then(|models| {
                models
                    .iter()
                    .map(|model| model.get("slug")?.as_str().map(str::to_owned))
                    .collect::<Option<std::collections::HashSet<_>>>()
            });
        let recorded = receipt.model_aliases.iter().cloned().collect();
        Ok(aliases
            .filter(|aliases| aliases == &recorded)
            .unwrap_or_default())
    }

    fn save_thread_receipt(&self, receipt: &Receipt) -> Result<(), String> {
        crate::config::atomic_write_private(
            &self.receipt,
            &serde_json::to_vec(receipt).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }
}

fn effective_provider(config: &toml::Value) -> String {
    selected_profile_provider(config)
        .or_else(|| {
            config
                .get("model_provider")
                .and_then(toml::Value::as_str)
                .filter(|provider| !provider.is_empty())
        })
        .unwrap_or("openai")
        .to_owned()
}

fn source_provider(config: &toml::Value) -> String {
    let source = effective_provider(config);
    if selected_profile_provider(config).is_none() && source == PROVIDER {
        "openai".to_owned()
    } else {
        source
    }
}

fn selected_profile_provider(config: &toml::Value) -> Option<&str> {
    let profile = config.get("profile").and_then(toml::Value::as_str)?;
    if profile.is_empty() {
        return None;
    }
    config
        .get("profiles")
        .and_then(|profiles| profiles.get(profile))
        .and_then(|profile| profile.get("model_provider"))
        .and_then(toml::Value::as_str)
        .filter(|provider| !provider.is_empty())
}

fn profile_names_provider(config: &toml::Value, provider: &str) -> bool {
    config
        .get("profiles")
        .and_then(toml::Value::as_table)
        .is_some_and(|profiles| {
            profiles.values().any(|profile| {
                profile.get("model_provider").and_then(toml::Value::as_str) == Some(provider)
            })
        })
}
