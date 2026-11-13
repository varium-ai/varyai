use super::{models, Model};
use crate::proxy::intent::Menu;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct PickerModel {
    #[serde(flatten)]
    model: Model,
    reference: String,
}

pub(crate) fn picker_models(menu: &Menu) -> Vec<PickerModel> {
    models(menu)
        .into_iter()
        .map(|model| {
            let reference = menu
                .groups
                .get(&model.id)
                .and_then(|session| {
                    if model.id.starts_with("vary-provider/") {
                        session
                            .targets
                            .first()
                            .map(|target| format!("{}/{}", target.provider_id, target.model))
                    } else {
                        Some(format!("group/{}", session.group.id))
                    }
                })
                .unwrap_or_else(|| model.id.clone());
            PickerModel { model, reference }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::Provider, proxy::providers::codex_oauth_auth::CodexOAuthManager,
        services::intent_routing::menu,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn picker_references_distinguish_copies_without_changing_names_or_routing_ids(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let providers = ["gs", "gs-2"].map(|id| {
            Provider::with_id(
                id.into(),
                "gs".into(),
                json!({
                    "auth":{"OPENAI_API_KEY":"synthetic-picker-key"},
                    "apiFormat":"openai_responses",
                    "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.example/v1'\nwire_api='responses'",
                    "modelCatalog":{"models":[{"model":"vendor/glm-5.3-flash","modelPreferences":{"name":"GLM-5.3-Flash"}}]}
                }),
                None,
            )
        });
        let menu = menu::prepare(manager, providers.to_vec(), vec![], None).await?;
        let before = models(&menu);
        let choices = picker_models(&menu);
        assert_eq!(choices.len(), 2);
        for (index, (choice, original)) in choices.iter().zip(&before).enumerate() {
            assert_eq!(choice.model.id, original.id);
            assert_eq!(choice.model.name, original.name);
            assert_eq!(choice.model.group, "gs");
            let serialized = serde_json::to_value(choice)?;
            assert_eq!(serialized["id"], original.id);
            assert_eq!(
                serialized["name"],
                format!("GLM-5.3-Flash · gs ({})", index + 1)
            );
            assert_eq!(serialized["group"], "gs");
            assert_eq!(
                serialized["reference"],
                format!("{}/vendor/glm-5.3-flash", ["gs", "gs-2"][index])
            );
            assert!(!serialized.to_string().contains("synthetic-picker-key"));
            assert_eq!(
                menu.groups[&choice.model.id].targets[0].model,
                "vendor/glm-5.3-flash"
            );
        }
        Ok(())
    }
}
