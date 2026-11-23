use super::*;
use crate::services::routing_upstreams::RoutingUpstream;
use serde_json::json;

fn provider(id: &str, name: &str) -> Provider {
    Provider::with_id(
        id.into(),
        name.into(),
        json!({
            "auth":{"OPENAI_API_KEY":"synthetic-secret"},
            "config":"model='configured-only'",
            "routingExecutionIdentity":"stable-synthetic-identity",
            "modelCatalog":{"models":[{"model":"synthetic-model","displayName":"Synthetic Model","privateConfig":"synthetic-private"}]}
        }),
        None,
    )
}

fn saved(db: &Database) -> Result<RoutingUpstream, String> {
    db.save_routing_upstream(
        serde_json::from_value(json!({
            "schemaVersion":1,"id":"","name":"Synthetic supplier","revision":0,
            "enabled":false,"source":{"kind":"api","baseUrl":"https://synthetic.invalid/v1",
            "apiKey":"synthetic-secret","apiFormat":"openai_responses","mode":"picked",
            "models":[{"model":"synthetic-model"}],"catalog":[{"model":"synthetic-model"}]}
        }))
        .map_err(|_| "synthetic fixture invalid")?,
    )
}

#[test]
fn canonical_ids_only_accept_generated_direct_model_namespaces() {
    let id = format!("model-{}", "a".repeat(64));
    assert_eq!(canonical_id(&id), Some(id.as_str()));
    let qualified = format!("vary-provider/{id}");
    assert_eq!(canonical_id(&qualified), Some(id.as_str()));
    for custom in [
        "model-custom".into(),
        format!("custom/{id}"),
        format!("vary-provider/{id}/extra"),
    ] {
        assert_eq!(canonical_id(&custom), None);
    }
}

#[test]
fn labels_project_only_safe_fields_and_keep_immutable_execution_identity() {
    let labels = labels_for_providers(&[provider("renamed", "Synthetic supplier")]);
    let expected = super::super::intent_routing::menu::direct_id(
        "stable-synthetic-identity",
        "synthetic-model",
    );
    let found = labels
        .iter()
        .find(|row| row.id == expected)
        .expect("model label");
    assert_eq!(found.model, "synthetic-model");
    assert_eq!(found.display_name, "Synthetic Model");
    assert_eq!(found.provider_name, "Synthetic supplier");
    let encoded = serde_json::to_string(&labels).expect("serialize safe labels");
    assert!(!encoded.contains("synthetic-secret"));
    assert!(!encoded.contains("synthetic-private"));
    assert!(!encoded.contains("OPENAI_API_KEY"));
}

#[test]
fn lookup_resolves_disabled_local_models_without_discovery(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let record = saved(&db)?;
    let id = super::super::intent_routing::menu::direct_id(&record.id, "synthetic-model");
    let labels = db.get_usage_model_labels(&[format!("vary-provider/{id}"), id.clone()])?;
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].id, id);
    assert_eq!(labels[0].provider_name, "Synthetic supplier");
    Ok(())
}

#[test]
fn historical_labels_survive_native_database_reopen_and_public_id_reuse(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("varyai.db");
    let first = labels_for_providers(&[provider("old", "Old supplier")]);
    let id = first[0].id.clone();
    {
        let db = Database::init_native_at(&path)?;
        db.remember_usage_model_labels(&first)?;
        let mut other = provider("old", "New supplier");
        other.settings_config["routingExecutionIdentity"] = json!("different-owner");
        db.remember_usage_model_labels(&labels_for_providers(&[other]))?;
    }
    let db = Database::init_native_at(&path)?;
    let labels = db.get_usage_model_labels(std::slice::from_ref(&id))?;
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].provider_name, "Old supplier");
    assert_eq!(labels[0].id, id);
    Ok(())
}

#[test]
fn corrupt_optional_history_does_not_hide_current_names_or_echo_content(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let record = saved(&db)?;
    {
        let conn = db
            .extension_connection()
            .lock()
            .map_err(|_| "synthetic lock")?;
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES (?1,?2)",
            [CACHE_KEY, "synthetic corrupt cache"],
        )?;
    }
    let id = super::super::intent_routing::menu::direct_id(&record.id, "synthetic-model");
    assert_eq!(db.get_usage_model_labels(&[id])?.len(), 1);
    assert_eq!(
        db.remember_usage_model_labels(&[]).err().as_deref(),
        Some(FAILED)
    );
    assert_eq!(
        db.get_usage_model_labels(&vec![String::new(); MAX_REQUEST_IDS + 1])
            .err()
            .as_deref(),
        Some(FAILED)
    );
    Ok(())
}

#[test]
fn retained_name_history_is_bounded_and_evicts_oldest_entries(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let labels = (0..=MAX_HISTORY)
        .map(|index| {
            let model = format!("synthetic-model-{index}");
            UsageModelLabel {
                id: super::super::intent_routing::menu::direct_id("synthetic-cache", &model),
                display_name: model.clone(),
                model,
                provider_name: "Synthetic supplier".into(),
            }
        })
        .collect::<Vec<_>>();
    db.remember_usage_model_labels(&labels)?;
    {
        let conn = db
            .extension_connection()
            .lock()
            .map_err(|_| "synthetic lock")?;
        let encoded: String = conn.query_row(
            "SELECT value FROM intent_routing_preferences WHERE key=?1",
            [CACHE_KEY],
            |row| row.get(0),
        )?;
        assert!(encoded.len() <= MAX_HISTORY_BYTES);
        assert_eq!(read_history(&conn)?.len(), MAX_HISTORY);
    }
    let result =
        db.get_usage_model_labels(&[labels[0].id.clone(), labels[MAX_HISTORY].id.clone()])?;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, labels[MAX_HISTORY].id);
    Ok(())
}

#[tokio::test]
async fn corrupt_optional_history_cannot_interrupt_catalog_preparation(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = std::sync::Arc::new(Database::memory()?);
    let mut record = saved(&db)?;
    record.enabled = true;
    db.save_routing_upstream(record)?;
    {
        let conn = db
            .extension_connection()
            .lock()
            .map_err(|_| "synthetic lock")?;
        conn.execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES (?1,?2)",
            [CACHE_KEY, "synthetic corrupt cache"],
        )?;
    }
    let dir = tempfile::tempdir()?;
    let manager = std::sync::Arc::new(
        crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(dir.path().to_owned()),
    );
    let (providers, _) =
        crate::services::intent_routing::catalog::prepare_providers(db, manager, false).await?;
    assert_eq!(providers.len(), 1);
    Ok(())
}

#[test]
fn lookup_resolves_routing_group_uuids() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let group_id = "e44ea5a8-b978-42de-b350-0427d58dfe82";
    let group: crate::services::intent_routing::RoutingGroup = serde_json::from_value(json!({
        "id": group_id,
        "name": "Synthetic Routing Group",
        "enabled": true,
        "revision": 0,
        "members": [{"id": "member", "providerId": "synthetic-provider", "modelId": "synthetic-model"}],
        "defaultMemberId": "member",
        "fallbackMemberIds": [],
        "intentRecognitionEnabled": false,
        "classifierTarget": null,
        "classificationBudgetMs": 1000,
        "rules": []
    }))?;
    {
        let conn = db
            .extension_connection()
            .lock()
            .map_err(|_| "synthetic lock")?;
        conn.execute(
            "INSERT INTO intent_routing_groups(id, revision, config) VALUES (?1, ?2, ?3)",
            rusqlite::params![group_id, group.revision, serde_json::to_string(&group)?],
        )?;
    }

    let labels =
        db.get_usage_model_labels(&[group_id.to_string(), format!("vary-auto/{group_id}")])?;
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].id, group_id);
    assert_eq!(labels[0].display_name, "Synthetic Routing Group");
    assert_eq!(labels[0].provider_name, "");
    Ok(())
}
