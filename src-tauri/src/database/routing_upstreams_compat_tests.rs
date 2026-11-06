//! Breaking provider-contract reset and native storage invariants.
use super::{Database, EXTENSION_SCHEMA_VERSION, SHARED_SCHEMA_VERSION};
use crate::provider::Provider;
use crate::services::routing_upstreams::{RoutingUpstream, UpstreamSource};
use serde_json::json;

fn custom() -> RoutingUpstream {
    RoutingUpstream {
        notes: None,
        icon: None,
        transport: None,
        schema_version: 1,
        preset_id: None,
        region: None,
        codex_settings: None,
        id: String::new(),
        name: "Portable fixture".into(),
        revision: 0,
        has_api_key: false,
        enabled: true,
        source: UpstreamSource::Api {
            catalog: vec![],
            mode: crate::services::codex_provider::ModelMode::Picked,
            groups_only: false,
            base_url: "https://synthetic.example/v1".into(),
            api_key: Some("synthetic-portable-key".into()),
            api_format: "openai_chat".into(),
            models: vec![json!({"model":"exact-model", "custom":{"keep":true}})],
            auth_type: None,
        },
    }
}

#[test]
fn provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let mut raw = serde_json::to_value(custom())?;
    raw["notes"] = json!("Synthetic original provider note");
    let saved = db.save_routing_upstream(serde_json::from_value(raw)?)?;
    let editor = db.routing_upstream_editor(&saved.id, saved.revision)?;
    let mut updated = serde_json::to_value(&editor)?;
    assert_eq!(updated["notes"], "Synthetic original provider note");
    updated["notes"] = json!("Synthetic revised provider note");
    let saved = db.save_routing_upstream(serde_json::from_value(updated)?)?;
    let reloaded = db.routing_upstream_editor(&saved.id, saved.revision)?;
    assert_eq!(
        serde_json::to_value(&reloaded)?["notes"],
        "Synthetic revised provider note"
    );
    assert!(!format!("{reloaded:?}").contains("provider note"));
    let executable = db.routing_discovery_provider(&reloaded.id, reloaded.revision)?;
    assert!(!executable
        .settings_config
        .to_string()
        .contains("provider note"));
    Ok(())
}

#[test]
fn key_pool_editor_preserves_private_keys_and_applies_enabled_preference_order(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::routing_upstreams::{variants, ImportedKey, ImportedTransport};
    let db = Database::memory()?;
    let mut record = custom();
    record.transport = Some(ImportedTransport {
        primary_name: Some("Personal".into()),
        keys: vec![ImportedKey {
            id: "backup".into(),
            name: "Backup".into(),
            key: Some("synthetic-backup-key".into()),
            enabled: true,
            api_format: None,
        }],
        ..Default::default()
    });
    let saved = db.save_routing_upstream(record)?;
    let summaries = db.routing_upstream_keys(&saved.id, saved.revision)?;
    assert_eq!(summaries.len(), 2);
    assert!(summaries.iter().all(|key| !key.label.contains("synthetic")));
    assert!(
        db.reveal_routing_upstream_key(&saved.id, saved.revision, "backup")?
            == "synthetic-backup-key"
    );
    assert!(db
        .reveal_routing_upstream_key(&saved.id, saved.revision + 1, "backup")
        .is_err());
    let mut value = serde_json::to_value(saved)?;
    value["transport"]["preferredKey"] = json!("backup");
    let preferred = db.save_routing_upstream(serde_json::from_value(value)?)?;
    let provider = db.routing_providers()?.remove(0);
    let candidates = variants(&provider);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].settings_config["routingKeyLabel"], "Backup");
    assert_eq!(candidates[1].settings_config["routingKeyLabel"], "Personal");
    assert!(candidates
        .iter()
        .all(|provider| provider.settings_config.get("routingKeyId").is_none()));
    let encoded = serde_json::to_string(&summaries)?;
    assert!(!encoded.contains("observation") && !encoded.contains("synthetic"));
    assert!(
        candidates[0]
            .resolve_usage_credentials(&crate::app_config::AppType::Codex)
            .1
            == "synthetic-backup-key"
    );
    let mut value = serde_json::to_value(preferred)?;
    value["transport"]["primaryOff"] = json!(true);
    let disabled = db.save_routing_upstream(serde_json::from_value(value)?)?;
    assert_eq!(variants(&db.routing_providers()?.remove(0)).len(), 1);
    assert!(variants(&db.routing_providers()?.remove(0))[0]
        .settings_config
        .get("routingKeyLabel")
        .is_none());
    let mut value = serde_json::to_value(disabled)?;
    value["transport"]["keys"][0]["enabled"] = json!(false);
    db.save_routing_upstream(serde_json::from_value(value)?)?;
    assert!(db.routing_providers()?.is_empty());
    Ok(())
}

#[test]
fn key_pool_edits_cannot_move_hidden_credentials_to_another_endpoint(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::routing_upstreams::{ImportedKey, ImportedTransport};
    let db = Database::memory()?;
    let mut record = custom();
    record.transport = Some(ImportedTransport {
        keys: vec![ImportedKey {
            id: "backup".into(),
            name: "Backup".into(),
            key: Some("synthetic-backup-key".into()),
            enabled: true,
            api_format: None,
        }],
        ..Default::default()
    });
    let saved = db.save_routing_upstream(record)?;
    let mut value = serde_json::to_value(&saved)?;
    value["source"]["baseUrl"] = json!("https://other.example/v1");
    value["source"]["apiKey"] = json!("synthetic-replacement");
    assert!(db
        .save_routing_upstream(serde_json::from_value(value)?)
        .is_err());
    assert!(db
        .reveal_routing_upstream_key(&saved.id, saved.revision, "unknown")
        .is_err());
    Ok(())
}

#[test]
fn provider_icons_round_trip_and_reach_the_shared_catalog() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::memory()?;
    let mut value = serde_json::to_value(custom())?;
    value["icon"] = json!("deepseek");
    let record: RoutingUpstream = serde_json::from_value(value)?;
    let saved = db.save_routing_upstream(record)?;
    assert_eq!(serde_json::to_value(&saved)?["icon"], "deepseek");
    assert_eq!(
        serde_json::to_value(db.list_routing_upstreams()?.remove(0))?["icon"],
        "deepseek"
    );
    let resolved = db.routing_discovery_provider(&saved.id, saved.revision)?;
    assert_eq!(resolved.icon.as_deref(), Some("deepseek"));
    Ok(())
}

#[test]
fn automatic_group_exclusions_preserve_owned_groups_and_unlisted_stand_ins() {
    let db = Database::memory().expect("database");
    db.set_intent_routing_auto_create(true).expect("on");
    let mut saved = Vec::new();
    for _ in 0..2 {
        saved.push(db.save_routing_upstream(custom()).expect("provider"));
    }
    let catalog = db.intent_routing_catalog().expect("discovered");
    assert_eq!(catalog.groups.len(), 1);
    let alias = catalog.groups[0].alias();
    db.set_routing_auto_group_skip(vec![saved[0].id.clone()])
        .expect("exclude");
    let skipped = db.intent_routing_catalog().expect("skipped");
    assert!(skipped.groups.is_empty());
    let stand_in = skipped
        .stand_ins
        .iter()
        .find(|g| g.alias() == alias)
        .expect("stand-in");
    assert_eq!(stand_in.members[0].provider_id, saved[1].id);
    db.set_routing_auto_group_skip(vec![]).expect("restore");
    let mut group = db
        .intent_routing_catalog()
        .expect("restored")
        .groups
        .remove(0);
    group.name = "Owned fixture".into();
    db.save_intent_routing_group(group).expect("take ownership");
    db.set_routing_auto_group_skip(vec![saved[0].id.clone()])
        .expect("exclude again");
    let owned = db.intent_routing_catalog().expect("owned");
    assert_eq!(owned.groups.len(), 1);
    assert_eq!(owned.groups[0].members.len(), 2);
    assert!(!owned.groups[0].auto_generated);
    assert!(db
        .set_routing_auto_group_skip(vec!["bad\nidentity".into()])
        .is_err());
}

#[test]
fn v6_provider_contract_is_discarded_once_without_touching_other_resources(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    db.save_provider(
        "codex",
        &Provider::with_id(
            "native-config".into(),
            "Native fixture".into(),
            json!({"auth":{"untouched":true}}),
            None,
        ),
    )?;
    let ext = db.extension_connection().lock().map_err(|_| "lock")?;
    ext.execute_batch("CREATE TABLE IF NOT EXISTS routing_upstream_origins(app_type TEXT NOT NULL, provider_id TEXT NOT NULL, upstream_id TEXT NOT NULL UNIQUE, deleted INTEGER NOT NULL DEFAULT 0, last_revision INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(app_type,provider_id));
        INSERT INTO routing_upstreams VALUES ('retired',1,'{\"source\":{\"kind\":\"legacy\"}}');
        INSERT INTO intent_routing_groups VALUES ('retired-group',1,'{}');
        INSERT INTO intent_routing_preferences VALUES ('retired-selection','{}');
        INSERT INTO security_settings VALUES ('preserve-fixture','yes');
        PRAGMA user_version=6;")?;
    drop(ext);
    drop(db);
    let db = Database::init_at(&path)?;
    assert!(db.list_routing_upstreams()?.is_empty());
    assert!(db.list_intent_routing_groups()?.is_empty());
    assert!(db.get_provider_by_id("native-config", "codex")?.is_some());
    let ext = db.extension_connection().lock().map_err(|_| "lock")?;
    assert_eq!(
        ext.query_row(
            "SELECT value FROM security_settings WHERE key='preserve-fixture'",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "yes"
    );
    assert_eq!(Database::get_user_version(&ext)?, EXTENSION_SCHEMA_VERSION);
    assert!(!Database::table_exists(&ext, "routing_upstream_origins")?);
    drop(ext);
    let saved = db.save_routing_upstream(custom())?;
    drop(db);
    let db = Database::init_at(&path)?;
    assert_eq!(db.list_routing_upstreams()?[0].id, saved.id);
    let shared = db.conn.lock().map_err(|_| "lock")?;
    assert_eq!(Database::get_user_version(&shared)?, SHARED_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn native_provider_modes_keep_catalog_and_picks_independent(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let mut record = custom();
    if let UpstreamSource::Api {
        mode,
        models,
        catalog,
        ..
    } = &mut record.source
    {
        *mode = crate::services::codex_provider::ModelMode::Vendor;
        *catalog = vec![json!({"model":"catalog-a"}), json!({"model":"catalog-b"})];
        *models = vec![json!({"model":"picked-only"})];
    }
    let mut saved = db.save_routing_upstream(record)?;
    let vendor = db.routing_discovery_provider(&saved.id, saved.revision)?;
    assert_eq!(
        vendor.settings_config["modelCatalog"]["models"]
            .as_array()
            .ok_or("models")?
            .len(),
        2
    );
    if let UpstreamSource::Api { mode, .. } = &mut saved.source {
        *mode = crate::services::codex_provider::ModelMode::Picked;
    }
    saved = db.save_routing_upstream(saved)?;
    let picked = db.routing_discovery_provider(&saved.id, saved.revision)?;
    assert_eq!(
        picked.settings_config["modelCatalog"]["models"][0]["model"],
        "picked-only"
    );
    if let UpstreamSource::Api { mode, .. } = &mut saved.source {
        *mode = crate::services::codex_provider::ModelMode::Usage;
    }
    let saved = db.save_routing_upstream(saved)?;
    assert!(db
        .routing_discovery_provider(&saved.id, saved.revision)?
        .settings_config["modelCatalog"]["models"]
        .as_array()
        .ok_or("models")?
        .is_empty());
    Ok(())
}
