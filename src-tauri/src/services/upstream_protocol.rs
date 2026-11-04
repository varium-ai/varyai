//! Local projections for native protocol conversion and request compatibility.
use crate::{app_config::AppType, database::Database, error::AppError, provider::Provider};
use serde_json::json;

pub(crate) fn native_format(app: &AppType, provider: &Provider) -> &'static str {
    let config = &provider.settings_config;
    match if *app == AppType::Hermes {
        config["api_mode"].as_str()
    } else {
        config["protocol"].as_str()
    } {
        Some("anthropic" | "anthropic_messages") => "anthropic",
        Some("openai-responses" | "codex_responses") => "openai_responses",
        _ => "openai_chat",
    }
}

pub(crate) fn requires_bridge(app: &AppType, provider: &Provider) -> bool {
    provider.category.as_deref() != Some("official")
        && provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .is_some_and(|format| format != native_format(app, provider))
}

pub(crate) fn trae_native_responses(provider: &Provider) -> bool {
    provider.category.as_deref() != Some("official")
        && native_format(&AppType::Trae, provider) == "openai_responses"
        && !requires_bridge(&AppType::Trae, provider)
}

pub(crate) fn project(
    db: &Database,
    app: &AppType,
    provider: &Provider,
) -> Result<Provider, AppError> {
    let responses_route = *app == AppType::Trae
        && trae_native_responses(provider)
        && db.get_bool_flag(super::trae_bridge::ENABLED_KEY)?;
    if !matches!(app, AppType::Hermes | AppType::Trae)
        || !(requires_bridge(app, provider) || responses_route)
    {
        return Ok(provider.clone());
    }
    let config = futures::executor::block_on(db.get_proxy_config())?;
    let ip = crate::proxy::types::loopback_listener_ip(&config.listen_address)
        .map_err(AppError::Config)?;
    let mut url = url::Url::parse(&format!(
        "http://{}/",
        std::net::SocketAddr::new(ip, config.listen_port)
    ))
    .map_err(|_| AppError::Config("Invalid local routing address".into()))?;
    url.path_segments_mut()
        .map_err(|_| AppError::Config("Invalid local routing address".into()))?
        .extend(["upstream", app.as_str(), "providers", &provider.id, "v1"]);
    let mut projected = provider.clone();
    let (base, key) = if *app == AppType::Hermes {
        ("base_url", "api_key")
    } else {
        ("baseUrl", "apiKey")
    };
    projected.settings_config[base] = json!(url.as_str());
    projected.settings_config[key] = json!("varyai-local");
    Ok(projected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trae_responses_projection_follows_routing_without_enabling_auditing(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Database::memory()?;
        db.update_security_settings(&crate::database::SecurityAuditSettings { enabled: false })?;
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "protocol":"openai-responses", "baseUrl":"https://upstream.example/api/v1",
                "apiKey":"synthetic-key", "models":[{"model":"fixture"}]
            }),
            None,
        );
        let original = provider.settings_config.clone();
        assert_eq!(
            project(&db, &AppType::Trae, &provider)?.settings_config,
            original
        );
        db.set_setting(super::super::trae_bridge::ENABLED_KEY, "true")?;
        let projected = project(&db, &AppType::Trae, &provider)?;
        assert!(projected.settings_config["baseUrl"]
            .as_str()
            .is_some_and(|url| url.contains("/upstream/trae/providers/fixture/v1")));
        assert_eq!(projected.settings_config["apiKey"], "varyai-local");
        assert_eq!(projected.settings_config["protocol"], "openai-responses");
        assert_eq!(provider.settings_config, original);
        assert!(!db.get_security_settings()?.enabled);
        provider.category = Some("official".into());
        assert_eq!(
            project(&db, &AppType::Trae, &provider)?.settings_config,
            original
        );
        provider.category = None;
        db.set_setting(super::super::trae_bridge::ENABLED_KEY, "false")?;
        assert_eq!(
            project(&db, &AppType::Trae, &provider)?.settings_config,
            original
        );
        Ok(())
    }

    #[test]
    fn projection_keeps_saved_upstream_and_native_protocol(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Database::memory()?;
        for app in [AppType::Hermes, AppType::Trae] {
            let mut provider = Provider::with_id(
                "fixture".into(),
                "Fixture".into(),
                json!({"base_url":"https://upstream.example/v1beta","api_key":"synthetic-key","api_mode":"chat_completions","baseUrl":"https://upstream.example/v1beta","apiKey":"synthetic-key","protocol":"openai"}),
                None,
            );
            let original = provider.settings_config.clone();
            provider
                .meta
                .get_or_insert_with(Default::default)
                .api_format = Some("gemini_native".into());
            let projected = project(&db, &app, &provider)?;
            let base = if app == AppType::Hermes {
                "base_url"
            } else {
                "baseUrl"
            };
            assert!(projected.settings_config[base]
                .as_str()
                .is_some_and(|base| base
                    .contains(&format!("/upstream/{}/providers/fixture/v1", app.as_str()))));
            assert_eq!(provider.settings_config, original);
            assert_eq!(projected.settings_config["api_mode"], "chat_completions");
            assert_eq!(projected.settings_config["protocol"], "openai");
            provider.meta = None;
            assert_eq!(project(&db, &app, &provider)?.settings_config, original);
        }
        Ok(())
    }
}
