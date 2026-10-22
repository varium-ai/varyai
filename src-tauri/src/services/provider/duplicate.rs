use super::*;

impl ProviderService {
    /// Duplicate the saved record only; applying it remains an explicit switch.
    pub fn duplicate(
        state: &AppState,
        app_type: AppType,
        id: &str,
        name: &str,
    ) -> Result<Provider, AppError> {
        let _guard =
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
        if name.trim().is_empty() {
            return Err(AppError::InvalidInput("Provider name is required".into()));
        }
        let mut provider = state
            .db
            .get_all_providers(app_type.as_str())?
            .shift_remove(id)
            .ok_or_else(|| AppError::InvalidInput("Provider not found".into()))?;
        provider.id = uuid::Uuid::new_v4().to_string();
        provider.name = name.trim().to_string();
        if app_type == AppType::Pi {
            super::pi::align_native_display_name(&mut provider);
        }
        provider.created_at = Some(chrono::Utc::now().timestamp_millis());
        provider.sort_index = None;
        provider.in_failover_queue = false;
        Self::set_provider_live_config_managed(&mut provider, false);
        state.db.save_provider(app_type.as_str(), &provider)?;
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn duplicates_every_app_as_a_detached_record_without_switching_or_changing_source() {
        for app in AppType::all() {
            let state = AppState::new(Arc::new(Database::memory().expect("database")));
            let mut source = Provider::with_id(
                "source".into(),
                "Original".into(),
                json!({"id":"same-model","apiKey":"synthetic-key","url":"https://api.example/v1/chat/completions","unknown":{"preserve":true}}),
                Some("https://example.com".into()),
            );
            source.notes = Some("keep notes".into());
            source.icon = Some("openai".into());
            source.sort_index = Some(1);
            source.created_at = Some(1);
            source.in_failover_queue = true;
            ProviderService::set_provider_live_config_managed(&mut source, true);
            source
                .meta
                .as_mut()
                .expect("metadata")
                .custom_endpoints
                .insert(
                    "https://secondary.example/v1".into(),
                    CustomEndpoint {
                        url: "https://secondary.example/v1".into(),
                        added_at: 7,
                        last_used: None,
                    },
                );
            state
                .db
                .save_provider(app.as_str(), &source)
                .expect("source");
            state
                .db
                .set_current_provider(app.as_str(), &source.id)
                .expect("current");
            let before = state
                .db
                .get_provider_by_id(&source.id, app.as_str())
                .expect("read")
                .expect("source");
            let copy = ProviderService::duplicate(&state, app.clone(), &source.id, "Original copy")
                .expect("copy");
            assert_ne!(copy.id, source.id);
            assert_eq!(copy.name, "Original copy");
            assert_eq!(copy.settings_config, source.settings_config);
            assert_eq!(copy.notes, source.notes);
            assert_eq!(copy.icon, source.icon);
            assert_eq!(copy.website_url, source.website_url);
            assert_eq!(
                serde_json::to_value(&copy.meta.as_ref().expect("copy metadata").custom_endpoints)
                    .expect("JSON"),
                serde_json::to_value(
                    &source
                        .meta
                        .as_ref()
                        .expect("source metadata")
                        .custom_endpoints
                )
                .expect("JSON"),
            );
            assert_eq!(copy.sort_index, None);
            assert!(!copy.in_failover_queue);
            assert_eq!(
                ProviderService::provider_live_config_managed(&copy),
                Some(false)
            );
            assert_eq!(
                state
                    .db
                    .get_current_provider(app.as_str())
                    .expect("current"),
                Some(source.id.clone())
            );
            let after = state
                .db
                .get_provider_by_id(&source.id, app.as_str())
                .expect("read")
                .expect("source");
            assert_eq!(
                serde_json::to_value(before).expect("JSON"),
                serde_json::to_value(after).expect("JSON")
            );
            assert_eq!(
                state
                    .db
                    .get_all_providers(app.as_str())
                    .expect("list")
                    .len(),
                2
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn pi_duplicate_keeps_its_requested_name_in_the_native_configuration() {
        let _agent = crate::pi_config::test_support::TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().expect("database")));
        let source = Provider::with_id(
            "company".into(),
            "Company".into(),
            json!({"name":"Company","api":"openai-completions","baseUrl":"https://example.com/v1","apiKey":"synthetic-key","models":[{"id":"model"}]}),
            None,
        );
        state.db.save_provider("pi", &source).expect("source");
        let copy = ProviderService::duplicate(&state, AppType::Pi, "company", "Company backup")
            .expect("copy");
        assert_eq!(copy.settings_config["name"], "Company backup");
        assert!(crate::pi_config::read_pi_native_provider(&copy.id)
            .expect("native")
            .is_none());
        ProviderService::switch(&state, AppType::Pi, &copy.id).expect("enable copy");
        let listed = ProviderService::list(&state, AppType::Pi).expect("list");
        assert_eq!(listed[&copy.id].name, "Company backup");
        assert_eq!(listed[&copy.id].settings_config["name"], "Company backup");
    }

    #[test]
    fn duplicate_does_not_activate_a_record_when_there_is_no_current_provider() {
        let state = AppState::new(Arc::new(Database::memory().expect("database")));
        let source = Provider::with_id(
            "source".into(),
            "Original".into(),
            serde_json::json!({}),
            None,
        );
        state.db.save_provider("codex", &source).expect("source");
        ProviderService::duplicate(&state, AppType::Codex, &source.id, "Copy").expect("copy");
        assert!(state
            .db
            .get_current_provider("codex")
            .expect("current")
            .is_none());
        assert!(ProviderService::duplicate(&state, AppType::Codex, "missing", "Copy").is_err());
        assert!(ProviderService::duplicate(&state, AppType::Codex, &source.id, " ").is_err());
        assert_eq!(state.db.get_all_providers("codex").expect("list").len(), 2);
    }
}
