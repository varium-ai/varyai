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

#[test]
fn retired_experimental_metadata_does_not_prevent_reading_original_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
    let original = serde_json::to_value(fixture())?;
    for kind in ["adaptive", "capability", "speed", "expiring"] {
        let mut stored = original.clone();
        stored["policy"] = json!({"kind":kind,"economicalMemberId":"balanced"});
        if kind == "expiring" {
            stored["routing"] = json!("expiring");
        }
        let group: RoutingGroup = serde_json::from_value(stored)?;
        group.validate()?;
        assert_eq!(group.routing, RoutingStrategy::Ordered);
        assert_eq!(serde_json::to_value(group)?, original);
    }
    let mut unknown = original;
    unknown["unsupportedExperimentalField"] = json!(true);
    assert!(serde_json::from_value::<RoutingGroup>(unknown).is_err());
    Ok(())
}

#[test]
fn reference_execution_controls_round_trip_and_disabled_members_are_not_planned() {
    let mut value = serde_json::to_value(fixture()).expect("fixture");
    value["affinity"] = json!("off");
    value["firstToken"] = json!(2);
    value["patience"] = json!(3);
    value["off"] = json!(["balanced"]);
    value["sink"] = json!(true);
    value["levels"] = json!(["low", "high", "max"]);
    let group: RoutingGroup = serde_json::from_value(value).expect("reference controls");
    assert!(group.validate().is_ok());
    let plan = group.preview(None).expect("plan");
    assert_eq!(plan.candidates.len(), 1);
    assert_eq!(plan.candidates[0].id, "strong");
    assert!(!group.includes_in_menu(&group.members[0]));
    let saved = serde_json::to_value(group).expect("safe settings");
    assert_eq!(saved["affinity"], "off");
    assert_eq!(saved["firstToken"], 2);
    assert_eq!(saved["patience"], 3);
    assert_eq!(saved["sink"], true);
    assert_eq!(saved["levels"], json!(["low", "high", "max"]));
    for (field, invalid) in [
        ("firstToken", json!(-1)),
        ("patience", json!(241)),
        ("affinity", json!("unknown")),
        ("off", json!(["missing"])),
        ("off", json!(["balanced", "balanced"])),
        ("levels", json!(["high", "low"])),
        ("levels", json!(["high", "high"])),
        ("levels", json!(["unknown"])),
    ] {
        let mut value = saved.clone();
        value[field] = invalid;
        assert!(serde_json::from_value::<RoutingGroup>(value)
            .map_or(true, |group| group.validate().is_err()));
    }
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
fn effort_seats_and_ordered_rule_targets_keep_distinct_member_identities() {
    let mut value = serde_json::to_value(fixture()).expect("fixture");
    value["members"][0]["effort"] = json!("low");
    value["members"][1]["modelId"] = value["members"][0]["modelId"].clone();
    value["members"][1]["effort"] = json!("high");
    value["rules"][0]["fallbackMemberIds"] = json!(["balanced"]);
    let group: RoutingGroup = serde_json::from_value(value.clone()).expect("effort seats");
    assert!(group.validate().is_ok());
    let plan = group.preview(Some("debug")).expect("rule plan");
    assert_eq!(
        plan.candidates
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["strong", "balanced"]
    );
    let stored = serde_json::to_value(group).expect("settings");
    assert_eq!(stored["members"][0]["effort"], "low");
    assert_eq!(stored["rules"][0]["fallbackMemberIds"], json!(["balanced"]));
    for invalid in [json!("high"), json!("auto"), json!("ultra")] {
        let mut duplicate = value.clone();
        duplicate["members"][0]["effort"] = invalid;
        assert!(serde_json::from_value::<RoutingGroup>(duplicate)
            .map_or(true, |g| g.validate().is_err()));
    }
    for invalid in [
        json!(["missing"]),
        json!(["strong"]),
        json!(["balanced", "balanced"]),
    ] {
        let mut bad_rule = value.clone();
        bad_rule["rules"][0]["fallbackMemberIds"] = invalid;
        assert!(serde_json::from_value::<RoutingGroup>(bad_rule)
            .map_or(true, |g| g.validate().is_err()));
    }
}

#[test]
fn deterministic_rules_need_conditions_not_a_classifier_and_keep_output_effort_separate() {
    let mut value = serde_json::to_value(fixture()).expect("fixture");
    value["rules"][0]["description"] = json!("");
    value["rules"][0]["conditions"] =
        json!({"tokens":4000, "images":true, "effort":"high", "agents":["codex"], "compact":false});
    value["rules"][0]["useEffort"] = json!("max");
    let group: RoutingGroup = serde_json::from_value(value.clone()).expect("conditions");
    assert!(group.validate().is_ok());
    let saved = serde_json::to_value(group).expect("json");
    assert_eq!(saved["rules"][0]["conditions"]["effort"], "high");
    assert_eq!(saved["rules"][0]["useEffort"], "max");
    for conditions in [
        json!({}),
        json!({"tokens":0}),
        json!({"agents":["unknown"]}),
        json!({"effort":"auto"}),
    ] {
        let mut invalid = value.clone();
        invalid["rules"][0]["conditions"] = conditions;
        assert!(
            serde_json::from_value::<RoutingGroup>(invalid).map_or(true, |g| g.validate().is_err())
        );
    }
}

#[test]
fn context_preferences_round_trip_independently_of_group_capacity() {
    let db = Database::memory().expect("database");
    assert_eq!(
        db.routing_context_settings().expect("defaults"),
        context::Settings::default()
    );
    let settings = context::Settings {
        full_window: true,
        compact_at: Some(400000),
    };
    let generation = db.routing_generation();
    db.set_routing_context_settings(&settings)
        .expect("save settings");
    assert!(db.routing_generation() > generation);
    assert_eq!(
        db.routing_context_settings().expect("read settings"),
        settings
    );
    assert!(db
        .set_routing_context_settings(&context::Settings {
            full_window: false,
            compact_at: Some(0)
        })
        .is_err());
    for window in [None, Some(-1), Some(0), Some(2048), Some(1000000)] {
        let mut group = fixture();
        group.context_window = window;
        assert!(group.validate().is_ok());
        let restored: RoutingGroup =
            serde_json::from_value(serde_json::to_value(&group).expect("json")).expect("group");
        assert_eq!(restored.context_window, window);
    }
    for invalid in [-2, 1, 2047, i64::MAX] {
        let mut group = fixture();
        group.context_window = Some(invalid);
        assert!(group.validate().is_err());
    }
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
    db.register_routing_fixture("codex", &a.id)
        .expect("explicit import");
    db.save_provider("codex", &b).expect("b");
    db.register_routing_fixture("codex", &b.id)
        .expect("explicit import");
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
    assert_eq!(group.preview(None).expect("default").candidates.len(), 2);
    assert!(group.preview(Some("outside-group")).is_err());
    assert!(!group.enabled);
}

#[test]
fn group_order_persists_without_rewriting_configs_and_rejects_stale_membership() {
    let db = Database::memory().expect("memory");
    db.save_provider("codex", &provider()).expect("provider");
    db.register_routing_fixture("codex", &provider().id)
        .expect("explicit import");
    let mut first = fixture();
    first.id = "alpha".into();
    let first = db.save_intent_routing_group(first).expect("first");
    let mut last = fixture();
    last.id = "zeta".into();
    let last = db.save_intent_routing_group(last).expect("last");
    db.set_routing_group_order(vec!["zeta".into(), "alpha".into()], None)
        .expect("arrange");
    assert_eq!(
        db.list_intent_routing_groups().expect("list"),
        vec![last.clone(), first.clone()]
    );
    assert!(db
        .set_routing_group_order(vec!["zeta".into(), "zeta".into()], None)
        .is_err());
    let mut added = fixture();
    added.id = "middle".into();
    let added = db.save_intent_routing_group(added).expect("added");
    assert!(db
        .set_routing_group_order(vec!["zeta".into(), "alpha".into()], None)
        .is_err());
    assert_eq!(
        db.list_intent_routing_groups().expect("list"),
        vec![last, first, added]
    );
}

#[test]
fn draft_storage_checks_revisions_provider_references_and_activation_boundary() {
    let db = Database::memory().expect("memory database");
    db.save_provider("codex", &provider())
        .expect("fixture provider");
    db.register_routing_fixture("codex", &provider().id)
        .expect("explicit import");
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
    let saved = db
        .save_intent_routing_group(active)
        .expect("publish global group");
    assert!(saved.enabled);
    assert_eq!(saved.client, "global");
    assert_eq!(db.list_intent_routing_groups().expect("list"), vec![saved]);
}

#[test]
fn global_drafts_retain_unavailable_upstreams_without_claiming_transport_readiness() {
    let db = Database::memory().expect("db");
    let mut unsupported = provider();
    unsupported.settings_config["apiFormat"] = json!("unsupported");
    db.save_provider("codex", &unsupported).expect("source");
    db.register_routing_fixture("codex", &unsupported.id)
        .expect("explicit import");
    let group = db
        .save_intent_routing_group(fixture())
        .expect("retain draft reference");
    assert_eq!(group.client, "global");
    let record = db
        .list_routing_upstreams()
        .expect("native record")
        .remove(0);
    let mut invalid = record.clone();
    if let crate::services::routing_upstreams::UpstreamSource::Api { api_format, .. } =
        &mut invalid.source
    {
        *api_format = "unsupported".into();
    }
    assert!(db.save_routing_upstream(invalid).is_err());
    assert!(db.validate_intent_routing_providers(&group).is_ok());
    assert!(db
        .list_routing_upstreams()
        .expect("registry")
        .iter()
        .any(|r| r.id == "fixture"));
}

#[test]
fn global_group_aliases_cannot_collide_or_reference_another_group() {
    let db = Database::memory().expect("db");
    db.save_provider("codex", &provider()).expect("source");
    db.register_routing_fixture("codex", &provider().id)
        .expect("explicit import");
    let mut first = fixture();
    first.model_alias = Some("auto/shared".into());
    db.save_intent_routing_group(first).expect("first");
    let mut duplicate = fixture();
    duplicate.id = "duplicate".into();
    duplicate.model_alias = Some("auto/shared".into());
    assert!(db.save_intent_routing_group(duplicate).is_err());
    let mut nested = fixture();
    nested.members[0].model_id = "auto/shared".into();
    assert!(nested.validate().is_err());
}

#[test]
fn static_targets_accept_explicit_protocols_but_classifiers_require_chat_credentials() {
    let mut p = provider();
    assert!(eligible_provider(&p, false));
    p.settings_config["apiFormat"] = json!("openai_chat");
    assert!(eligible_provider(&p, false));
    assert!(eligible_provider(&p, true));
    p.settings_config["apiFormat"] = json!("gemini_native");
    assert!(eligible_provider(&p, false));
    assert!(!eligible_provider(&p, true));
    p.category = Some("official".into());
    assert!(!eligible_provider(&p, true));
}

#[test]
fn version_two_extension_upgrades_without_moving_groups_into_shared_storage() {
    let dir = tempfile::tempdir().expect("temporary storage");
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path).expect("initialize");
    db.save_provider("codex", &provider()).expect("provider");
    db.register_routing_fixture("codex", &provider().id)
        .expect("explicit import");
    drop(db);
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db")).expect("extension");
    ext.execute_batch(
        "DROP TABLE intent_routing_groups;
         DROP TABLE intent_routing_preferences;
         DROP TABLE gateway_agent_bindings;
         DROP TABLE gateway_usage_details;
         DROP TABLE gateway_usage_models;
         DROP TABLE IF EXISTS routing_upstream_origins;
         DROP TABLE routing_upstreams;
         PRAGMA user_version=2;",
    )
    .expect("v2 fixture");
    drop(ext);
    let db = Database::init_at(&path).expect("upgrade v2");
    assert!(db
        .list_routing_upstreams()
        .expect("empty registry")
        .is_empty());
    db.register_routing_fixture("codex", "fixture")
        .expect("explicit import after upgrade");
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
        crate::database::SHARED_SCHEMA_VERSION
    );
}

#[test]
fn imported_groups_are_always_disabled() {
    let source = Database::memory().expect("source");
    source
        .save_provider("codex", &provider())
        .expect("provider");
    source
        .register_routing_fixture("codex", &provider().id)
        .expect("explicit import");
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
fn automatic_groups_default_to_off_until_explicitly_enabled() {
    let db = Database::memory().expect("db");
    for id in ["first", "second"] {
        let mut source = provider();
        source.id = id.into();
        source.settings_config["modelCatalog"] = json!({"models":[{"model":"shared-model"}]});
        db.save_provider("codex", &source).expect("provider");
        db.register_routing_fixture("codex", id)
            .expect("explicit import");
    }
    let catalog = db.intent_routing_catalog().expect("default catalog");
    assert!(!catalog.auto_create);
    assert!(catalog.groups.is_empty());

    db.set_intent_routing_auto_create(true).expect("on");
    let enabled = db.intent_routing_catalog().expect("enabled catalog");
    assert!(enabled.auto_create);
    assert_eq!(enabled.groups.len(), 1);

    db.set_intent_routing_auto_create(false).expect("off");
    let disabled = db.intent_routing_catalog().expect("disabled catalog");
    assert!(!disabled.auto_create);
    assert!(disabled.groups.is_empty());
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
    db.register_routing_fixture("codex", &a.id)
        .expect("explicit import");
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
    db.register_routing_fixture("codex", &b.id)
        .expect("explicit import");
    let catalog = db.intent_routing_catalog().expect("catalog");
    assert_eq!(catalog.groups.len(), 1);
    let auto = catalog.groups[0].clone();
    assert!(auto.auto_generated);
    assert_eq!(auto.members.len(), 2);
    assert!(auto.enabled);
    assert_eq!(auto.revision, 0);
    assert!(db
        .list_intent_routing_groups()
        .expect("saved only")
        .is_empty());
    db.delete_intent_routing_group(&auto.id, 0).expect("hide");
    let hidden = db.intent_routing_catalog().expect("removed");
    assert_eq!(hidden.removed_groups.len(), 1);
    assert_eq!(hidden.removed_groups[0].id, auto.id);
    let second = db
        .list_routing_upstreams()
        .expect("registered")
        .into_iter()
        .find(|upstream| upstream.id == "second")
        .expect("second reference");
    db.delete_routing_upstream(&second.id, second.revision)
        .expect("temporarily remove provider");
    assert_eq!(
        db.intent_routing_catalog()
            .expect("removed with one provider")
            .removed_groups[0]
            .id,
        auto.id
    );
    db.register_routing_fixture("codex", "second")
        .expect("explicit reimport");
    db.restore_discovered_routing_group(&auto.id)
        .expect("restore");
    assert_eq!(
        db.intent_routing_catalog().expect("back").groups,
        vec![auto.clone()]
    );
    db.delete_intent_routing_group(&auto.id, 0)
        .expect("hide again");
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
    assert!(db
        .intent_routing_catalog()
        .expect("owned is not removed")
        .removed_groups
        .is_empty());
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
    ext.execute_batch(
        "DROP TABLE intent_routing_preferences;
         DROP TABLE gateway_agent_bindings;
         DROP TABLE gateway_usage_details;
         DROP TABLE gateway_usage_models;
         DROP TABLE IF EXISTS routing_upstream_origins;
         DROP TABLE routing_upstreams;
         PRAGMA user_version=3;",
    )
    .expect("v3");
    drop(ext);
    let db = Database::init_at(&path).expect("upgrade");
    assert!(
        !db.intent_routing_catalog()
            .expect("upgrade state")
            .auto_create
    );
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
    db.set_intent_routing_auto_create(false).expect("off");
    drop(db);
    let db = Database::init_at(&path).expect("reopen with discovery off");
    assert!(
        !db.intent_routing_catalog()
            .expect("saved off state")
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
    db.register_routing_fixture("codex", &a.id)
        .expect("explicit import");
    db.save_provider("codex", &b).expect("b");
    db.register_routing_fixture("codex", &b.id)
        .expect("explicit import");
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
    // Native providers do not change kind when a historical fixture changes.
    b.category = Some("official".into());
    db.save_provider("codex", &b).expect("native");
    let first = db
        .intent_routing_catalog()
        .expect("discovered")
        .groups
        .remove(0);
    b.settings_config["modelCatalog"] = json!({"models":[]});
    db.save_provider("codex", &b).expect("removed model");
    db.register_routing_fixture("codex", &b.id)
        .expect("explicit native update");
    assert!(db
        .intent_routing_catalog()
        .expect("updated")
        .groups
        .is_empty());
    b.settings_config["modelCatalog"] = json!({"models":[{"model":"shared"}]});
    db.save_provider("codex", &b).expect("restored model");
    db.register_routing_fixture("codex", &b.id)
        .expect("explicit native update");
    assert_eq!(
        db.intent_routing_catalog().expect("stable").groups[0],
        first
    );
    for i in 0..15 {
        let mut extra = b.clone();
        extra.id = format!("extra-{i}");
        db.save_provider("codex", &extra).expect("extra");
        db.register_routing_fixture("codex", &extra.id)
            .expect("explicit import");
    }
    let large = db.intent_routing_catalog().expect("expanded members");
    assert_eq!(large.groups.len(), 1);
    assert_eq!(large.groups[0].members.len(), 17);
    assert_eq!(large.omitted_groups, 0);
}

#[test]
fn official_and_oauth_catalogs_create_editable_groups_without_transport_approval() {
    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};

    for kind in ["official", "codex_oauth", "managed_binding"] {
        let db = Database::memory().expect("db");
        db.set_intent_routing_auto_create(true).expect("on");
        let mut api = provider();
        api.settings_config["modelCatalog"] = json!({"models":[{"model":"shared"}]});
        db.save_provider("codex", &api).expect("api provider");
        db.register_routing_fixture("codex", &api.id)
            .expect("explicit import");
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
                        auth_provider: Some("codex_oauth".into()),
                        account_id: None,
                    }),
                    ..Default::default()
                });
            }
        }
        db.save_provider("codex", &account)
            .expect("account provider");
        db.register_routing_fixture("codex", &account.id)
            .expect("explicit import");
        assert!(!eligible_provider(&account, false));
        let groups = db.intent_routing_catalog().expect("catalog").groups;
        assert_eq!(groups.len(), 1, "{kind}");
        assert_eq!(groups[0].members.len(), 2);
        let mut draft = groups[0].clone();
        draft.name = "Edited account group".into();
        let saved = db
            .save_intent_routing_group(draft)
            .expect("save account draft");
        assert!(saved.enabled);
        assert!(!saved.auto_generated);
        assert_eq!(
            db.validate_intent_routing_providers(&saved),
            Err("intentRouting.providerUnavailable".into())
        );
        let mut active = saved.clone();
        active.enabled = true;
        let published = db
            .save_intent_routing_group(active)
            .expect("save publication preference");
        assert!(published.enabled);
        assert!(db.validate_intent_routing_providers(&published).is_err());
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
