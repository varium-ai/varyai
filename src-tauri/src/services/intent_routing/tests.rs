use super::*;
use crate::{database::Database, provider::Provider};
use serde_json::json;

pub(super) fn fixture() -> RoutingGroup {
    serde_json::from_value(json!({
        "id": "development", "name": "Development", "revision": 0,
        "client": "codex", "enabled": false, "intentRecognitionEnabled": false,
        "members": [
            {"id":"balanced", "providerId":"fixture", "modelId":"balanced-model"},
            {"id":"strong", "providerId":"fixture", "modelId":"strong-model"}
        ],
        "defaultMemberId":"balanced", "fallbackMemberIds":["balanced"],
        "classifierTarget": null, "classificationBudgetMs": 2000,
        "rules": [{"id":"debug", "name":"Debugging", "description":"Diagnose defects", "enabled":true, "targetMemberId":"strong"}]
    })).expect("synthetic group")
}

fn provider() -> Provider {
    Provider::with_id(
        "fixture".into(),
        "Fixture".into(),
        json!({
            "auth":{"OPENAI_API_KEY":"synthetic-key"},
            "config":"model_provider = \"fixture\"\n[model_providers.fixture]\nbase_url = \"https://fixture.example/v1\"\nwire_api = \"responses\"",
            "apiFormat":"openai_responses"
        }),
        None,
    )
}

#[test]
fn all_reference_strategies_round_trip_and_manual_plan_ignores_rules_and_fallbacks() {
    for strategy in [
        "smart",
        "ordered",
        "round_robin",
        "least_used",
        "pace",
        "free_first",
        "fastest",
        "weighted",
        "manual",
    ] {
        let mut value = serde_json::to_value(fixture()).expect("fixture");
        value["routing"] = json!(strategy);
        value["manualMemberId"] = json!("balanced");
        value["members"][0]["weight"] = json!(3);
        let group: RoutingGroup = serde_json::from_value(value).expect("strategy");
        assert!(group.validate().is_ok());
        let saved = serde_json::to_value(&group).expect("saved");
        assert_eq!(saved["routing"], strategy);
        assert_eq!(saved["members"][0]["weight"], 3);
        if strategy == "manual" {
            let plan = group.preview(Some("debug")).expect("manual");
            assert_eq!(plan.reason, "manual");
            assert_eq!(plan.candidates.len(), 1);
            assert_eq!(plan.candidates[0].id, "balanced");
        }
    }
}

#[test]
fn old_groups_default_weight_and_invalid_manual_picks_or_weights_are_rejected() {
    let mut value = serde_json::to_value(fixture()).expect("fixture");
    value["manualMemberId"] = json!("balanced");
    assert_eq!(value["members"][0]["weight"], 1);
    for (path, invalid) in [
        ("/members/0/weight", json!(0)),
        ("/members/0/weight", json!(1001)),
        ("/manualMemberId", json!("missing")),
    ] {
        let mut value = value.clone();
        value["routing"] = json!("manual");
        *value
            .pointer_mut(path)
            .unwrap_or_else(|| panic!("missing {path}")) = invalid;
        let parsed = serde_json::from_value::<RoutingGroup>(value);
        assert!(parsed.is_err() || parsed.is_ok_and(|g| g.validate().is_err()));
    }
}

#[test]
fn discovery_normalizes_vendor_spelling_without_changing_upstream_ids() {
    let mut a = provider();
    a.sort_index = Some(0);
    a.settings_config["modelCatalog"] = json!({"models":[
        {"model":"openai/GPT_5.6_Sol"},
        {"model":"gpt-4o-2024-08-06"},
        {"model":"gpt-4o"},
        {"model":"qwen-2507"},
        {"model":"qwen-2508"}
    ]});
    let mut b = a.clone();
    b.id = "second".into();
    b.sort_index = Some(1);
    b.settings_config["modelCatalog"] = json!({"models":[
        {"model":"gpt-5.6-sol", "displayName":"GPT-5.6 Sol"},
        {"model":"vendor/claude-opus-5.5-20260801"}
    ]});
    let mut c = b.clone();
    c.id = "third".into();
    c.settings_config["modelCatalog"] = json!({"models":[{"model":"claude_opus_5p5"}]});
    let groups = discover_groups(vec![c.clone(), b.clone(), a.clone()]);
    assert_eq!(groups.len(), 2);
    let group = groups
        .iter()
        .find(|g| g.name == "GPT-5.6 Sol")
        .expect("friendly name");
    assert_eq!(group.alias(), "auto/gpt-5.6-sol");
    assert_eq!(group.routing, RoutingStrategy::Smart);
    assert_eq!(group.members[0].model_id, "openai/GPT_5.6_Sol");
    assert_eq!(group.members[1].model_id, "gpt-5.6-sol");
    assert_eq!(discover_groups(vec![a, b, c]), groups);
}

#[test]
fn legacy_groups_keep_ordered_routing_and_invalid_aliases_are_rejected() {
    let mut group = fixture();
    assert_eq!(group.routing, RoutingStrategy::Ordered);
    assert_eq!(group.alias(), "vary-auto/development");
    group.model_alias = Some("auto/model".into());
    assert!(group.validate().is_ok());
    for alias in [
        "vary-provider/model",
        "auto/../../secret",
        "auto/a b",
        "auto/",
    ] {
        group.model_alias = Some(alias.into());
        assert!(group.validate().is_err());
    }
}

#[test]
fn normalized_discovery_preserves_legacy_manual_overrides_and_hidden_groups() {
    use sha2::{Digest, Sha256};
    let db = Database::memory().expect("db");
    let mut a = provider();
    a.settings_config["modelCatalog"] = json!({"models":[{"model":"gpt-5.6-sol"}]});
    let mut b = a.clone();
    b.id = "second".into();
    db.save_provider("codex", &a).expect("a");
    db.save_provider("codex", &b).expect("b");
    db.set_intent_routing_auto_create(true).expect("on");
    let mut legacy = discover_groups(vec![a, b])[0].clone();
    legacy.id = format!("auto-{:x}", Sha256::digest(b"gpt-5.6-sol"));
    legacy.name = "Existing custom group".into();
    db.save_intent_routing_group(legacy.clone()).expect("save");
    assert_eq!(
        db.intent_routing_catalog().expect("catalog").groups.len(),
        1
    );
    let db = Database::memory().expect("hidden db");
    db.extension_connection()
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES (?1,'true')",
            [format!("hidden:{}", legacy.id)],
        )
        .expect("hidden");
    db.set_intent_routing_auto_create(true).expect("on");
    assert!(db
        .intent_routing_catalog_with_providers(Some(vec![
            {
                let mut p = provider();
                p.settings_config["modelCatalog"] = json!({"models":[{"model":"gpt-5.6-sol"}]});
                p
            },
            {
                let mut p = provider();
                p.id = "second".into();
                p.settings_config["modelCatalog"] = json!({"models":[{"model":"gpt-5.6-sol"}]});
                p
            }
        ]))
        .expect("catalog")
        .groups
        .is_empty());
}

#[test]
fn validates_bounds_and_group_local_references() {
    assert!(fixture().validate().is_ok());
    let mut invalid = fixture();
    invalid.default_member_id = "outside-group".into();
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.members.push(invalid.members[0].clone());
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.rules[0].target_member_id = "outside-group".into();
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.members[0].model_id = "vary-auto/other".into();
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.classification_budget_ms = 5001;
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.rules[0].description = "x".repeat(501);
    assert!(invalid.validate().is_err());
    invalid = fixture();
    invalid.intent_recognition_enabled = true;
    assert!(invalid.validate().is_err());
}

#[test]
fn strict_classifier_output_accepts_only_one_known_id_or_explicit_null() {
    let group = fixture();
    assert_eq!(
        group.parse_classification(r#"{"intentId":"debug"}"#),
        Ok(Some("debug".into()))
    );
    assert_eq!(group.parse_classification(r#"{"intentId":null}"#), Ok(None));
    for output in [
        "{}",
        "1",
        r#"{"intentId":"unknown"}"#,
        r#"{"intentId":null,"extra":1}"#,
        r#"{"intentId":"debug","intentId":null}"#,
        "```json\n{}\n```",
        r#"{"intentId":null} {}"#,
    ] {
        assert_eq!(group.parse_classification(output), Err("invalid_output"));
    }
    let mut disabled = group;
    disabled.rules[0].enabled = false;
    assert!(disabled
        .parse_classification(r#"{"intentId":"debug"}"#)
        .is_err());
}

#[test]
fn plans_exact_models_in_group_order_without_duplicate_retries() {
    let group = fixture();
    let plan = group.preview(Some("debug")).expect("plan");
    assert_eq!(
        plan.candidates
            .iter()
            .map(|m| m.model_id.as_str())
            .collect::<Vec<_>>(),
        vec!["strong-model", "balanced-model"]
    );
    assert_eq!(group.preview(None).expect("default").candidates.len(), 1);
    assert!(group.preview(Some("outside-group")).is_err());
    assert!(!group.enabled);
}

#[test]
fn draft_storage_checks_revisions_provider_references_and_activation_boundary() {
    let db = Database::memory().expect("memory database");
    db.save_provider("codex", &provider())
        .expect("fixture provider");
    let first = db.save_intent_routing_group(fixture()).expect("save draft");
    assert_eq!(first.revision, 1);
    assert_eq!(
        db.list_intent_routing_groups().expect("list"),
        vec![first.clone()]
    );
    assert_eq!(
        db.save_intent_routing_group(fixture()),
        Err("intentRouting.conflict".into())
    );
    let mut updated = first.clone();
    updated.name = "Updated".into();
    assert_eq!(
        db.save_intent_routing_group(updated)
            .expect("update")
            .revision,
        2
    );
    assert!(db.delete_intent_routing_group(&first.id, 1).is_err());
    db.delete_intent_routing_group(&first.id, 2)
        .expect("delete current revision");
    let mut invalid = fixture();
    invalid.members[0].provider_id = "missing".into();
    assert!(db.save_intent_routing_group(invalid).is_err());
    let mut active = fixture();
    active.enabled = true;
    assert_eq!(
        db.save_intent_routing_group(active),
        Err("intentRouting.notReady".into())
    );
    assert!(db.list_intent_routing_groups().expect("list").is_empty());
}

#[test]
fn targets_reject_oauth_and_cross_protocol_providers_without_disclosing_credentials() {
    let mut p = provider();
    assert!(eligible_provider(&p, false));
    p.settings_config["apiFormat"] = json!("openai_chat");
    assert!(!eligible_provider(&p, false));
    assert!(eligible_provider(&p, true));
    p.category = Some("official".into());
    assert!(!eligible_provider(&p, true));
}

#[test]
fn version_two_extension_upgrades_without_moving_groups_into_shared_storage() {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path).expect("initialize");
    db.save_provider("codex", &provider()).expect("provider");
    drop(db);
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db")).expect("extension");
    ext.execute_batch("DROP TABLE intent_routing_groups; PRAGMA user_version=2;")
        .expect("v2 fixture");
    drop(ext);
    let db = Database::init_at(&path).expect("upgrade v2");
    db.save_intent_routing_group(fixture()).expect("save");
    drop(db);
    let db = Database::init_at(&path).expect("reopen");
    assert_eq!(db.list_intent_routing_groups().expect("list").len(), 1);
    let shared = rusqlite::Connection::open(path).expect("shared");
    assert!(shared
        .prepare("SELECT * FROM intent_routing_groups")
        .is_err());
    assert_eq!(
        shared
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .expect("version"),
        20
    );
}

#[test]
fn imported_groups_are_always_disabled() {
    let source = Database::memory().expect("source");
    source
        .save_provider("codex", &provider())
        .expect("provider");
    source.save_intent_routing_group(fixture()).expect("draft");
    {
        let conn = source.extension_connection().lock().expect("fixture lock");
        let mut active = fixture();
        active.enabled = true;
        active.revision = 1;
        conn.execute(
            "UPDATE intent_routing_groups SET config=?1",
            [serde_json::to_string(&active).expect("fixture JSON")],
        )
        .expect("future export");
    }
    let sql = source.export_sql_string().expect("export");
    let target = Database::memory().expect("target");
    target.import_sql_string(&sql).expect("import");
    let groups = target.list_intent_routing_groups().expect("groups");
    assert_eq!(groups.len(), 1);
    assert!(!groups[0].enabled);
}

#[test]
fn automatic_groups_follow_catalogs_and_preserve_user_overrides_and_hides() {
    let db = Database::memory().expect("db");
    let mut a = provider();
    a.settings_config["modelCatalog"] = json!({"models":[
        {"model":"shared-model"}, {"model":"shared-model"}, {"model":"Only-A"},
        {"model":"Model-case"}, {"model":"dated-2026-01-01"}
    ]});
    db.save_provider("codex", &a).expect("a");
    assert!(!db.intent_routing_catalog().expect("catalog").auto_create);
    db.set_intent_routing_auto_create(true).expect("on");
    assert!(db
        .intent_routing_catalog()
        .expect("one provider")
        .groups
        .is_empty());
    let mut b = a.clone();
    b.id = "second".into();
    b.settings_config["modelCatalog"] = json!({"models":[
        {"model":"shared-model"}, {"model":"model-case:batch"}, {"model":"dated-2026-02-01"}
    ]});
    db.save_provider("codex", &b).expect("b");
    let catalog = db.intent_routing_catalog().expect("catalog");
    assert_eq!(catalog.groups.len(), 1);
    let auto = catalog.groups[0].clone();
    assert!(auto.auto_generated);
    assert_eq!(auto.members.len(), 2);
    assert!(!auto.enabled);
    assert_eq!(auto.revision, 0);
    assert!(db
        .list_intent_routing_groups()
        .expect("saved only")
        .is_empty());
    db.delete_intent_routing_group(&auto.id, 0).expect("hide");
    db.set_intent_routing_auto_create(false).expect("off");
    db.set_intent_routing_auto_create(true).expect("on");
    assert!(db
        .intent_routing_catalog()
        .expect("hidden stays")
        .groups
        .is_empty());
    let mut edited = auto;
    edited.name = "My group".into();
    let saved = db
        .save_intent_routing_group(edited)
        .expect("take ownership");
    assert!(!saved.auto_generated);
    db.set_intent_routing_auto_create(false).expect("off");
    assert_eq!(
        db.intent_routing_catalog().expect("kept").groups,
        vec![saved.clone()]
    );
    db.set_intent_routing_auto_create(true).expect("on");
    assert_eq!(
        db.intent_routing_catalog().expect("override").groups,
        vec![saved.clone()]
    );
    db.delete_intent_routing_group(&saved.id, saved.revision)
        .expect("delete override");
    assert!(db
        .intent_routing_catalog()
        .expect("no resurrection")
        .groups
        .is_empty());
}

#[test]
fn automatic_group_setting_survives_reopen_and_v3_upgrade_but_import_disables_it() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("cc-switch.db");
    drop(Database::init_at(&path).expect("initialize"));
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db")).expect("extension");
    ext.execute_batch("DROP TABLE intent_routing_preferences; PRAGMA user_version=3;")
        .expect("v3");
    drop(ext);
    let db = Database::init_at(&path).expect("upgrade");
    db.set_intent_routing_auto_create(true).expect("on");
    drop(db);
    let db = Database::init_at(&path).expect("reopen");
    assert!(db.intent_routing_catalog().expect("state").auto_create);
    let dump = db.export_sql_string().expect("export");
    let imported = Database::memory().expect("import db");
    imported.import_sql_string(&dump).expect("import");
    assert!(
        !imported
            .intent_routing_catalog()
            .expect("import state")
            .auto_create
    );
}

#[test]
fn discovery_tracks_catalogs_independently_of_transport_eligibility() {
    let db = Database::memory().expect("db");
    db.set_intent_routing_auto_create(true).expect("on");
    let mut a = provider();
    a.settings_config["modelCatalog"] = json!({"models":[{"model":"shared"}]});
    let mut b = a.clone();
    b.id = "second".into();
    b.category = Some("official".into());
    db.save_provider("codex", &a).expect("a");
    db.save_provider("codex", &b).expect("b");
    assert_eq!(
        db.intent_routing_catalog()
            .expect("official included")
            .groups
            .len(),
        1
    );
    b.category = None;
    b.settings_config["apiFormat"] = json!("openai_chat");
    db.save_provider("codex", &b).expect("chat");
    assert_eq!(
        db.intent_routing_catalog()
            .expect("chat included")
            .groups
            .len(),
        1
    );
    b.settings_config["apiFormat"] = json!("openai_responses");
    db.save_provider("codex", &b).expect("native");
    let first = db
        .intent_routing_catalog()
        .expect("discovered")
        .groups
        .remove(0);
    b.settings_config["modelCatalog"] = json!({"models":[]});
    db.save_provider("codex", &b).expect("removed model");
    assert!(db
        .intent_routing_catalog()
        .expect("updated")
        .groups
        .is_empty());
    b.settings_config["modelCatalog"] = json!({"models":[{"model":"shared"}]});
    db.save_provider("codex", &b).expect("restored model");
    assert_eq!(
        db.intent_routing_catalog().expect("stable").groups[0],
        first
    );
    for i in 0..15 {
        let mut extra = b.clone();
        extra.id = format!("extra-{i}");
        db.save_provider("codex", &extra).expect("extra");
    }
    let large = db.intent_routing_catalog().expect("too many members");
    assert!(large.groups.is_empty());
    assert_eq!(large.omitted_groups, 1);
}

#[test]
fn official_and_oauth_catalogs_create_editable_drafts_without_transport_approval() {
    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};

    for kind in ["official", "codex_oauth", "managed_binding"] {
        let db = Database::memory().expect("db");
        db.set_intent_routing_auto_create(true).expect("on");
        let mut api = provider();
        api.settings_config["modelCatalog"] = json!({"models":[{"model":"shared"}]});
        db.save_provider("codex", &api).expect("api provider");
        let mut account = Provider::with_id(
            "account".into(),
            "Account fixture".into(),
            json!({"modelCatalog":{"models":[{"model":"shared"}]}}),
            None,
        );
        match kind {
            "official" => account.category = Some("official".into()),
            "codex_oauth" => {
                account.meta = Some(ProviderMeta {
                    provider_type: Some("codex_oauth".into()),
                    ..Default::default()
                });
            }
            _ => {
                account.meta = Some(ProviderMeta {
                    auth_binding: Some(AuthBinding {
                        source: AuthBindingSource::ManagedAccount,
                        auth_provider: Some("codex".into()),
                        account_id: None,
                    }),
                    ..Default::default()
                });
            }
        }
        db.save_provider("codex", &account)
            .expect("account provider");
        assert!(!eligible_provider(&account, false));
        let groups = db.intent_routing_catalog().expect("catalog").groups;
        assert_eq!(groups.len(), 1, "{kind}");
        assert_eq!(groups[0].members.len(), 2);
        let mut draft = groups[0].clone();
        draft.name = "Edited account group".into();
        let saved = db
            .save_intent_routing_group(draft)
            .expect("save account draft");
        assert!(!saved.enabled);
        assert!(!saved.auto_generated);
        assert_eq!(
            db.validate_intent_routing_providers(&saved),
            Err("intentRouting.providerUnavailable".into())
        );
        let mut active = saved.clone();
        active.enabled = true;
        assert_eq!(
            db.save_intent_routing_group(active),
            Err("intentRouting.notReady".into())
        );
        let mut classifier = saved;
        classifier.classifier_target = Some(Target {
            provider_id: account.id.clone(),
            model_id: "shared".into(),
        });
        assert_eq!(
            db.save_intent_routing_group(classifier),
            Err("intentRouting.providerUnavailable".into())
        );
        account.settings_config = json!({});
        db.save_provider("codex", &account).expect("empty catalog");
        assert!(discover_groups([api, account]).is_empty());
    }
}
