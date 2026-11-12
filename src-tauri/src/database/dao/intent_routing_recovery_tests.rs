use super::*;
use crate::provider::Provider;
use serde_json::json;

#[test]
fn saved_group_deletion_is_recoverable_without_reviving_stale_revisions(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let provider = Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"auth":{"OPENAI_API_KEY":"synthetic-only"},"apiFormat":"openai_responses","config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://fixture.invalid/v1'\nwire_api='responses'"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.register_routing_fixture("codex", &provider.id)?;
    let group: RoutingGroup = serde_json::from_value(
        json!({"id":"manual-fixture","name":"Synthetic saved group","revision":0,"client":"global","enabled":false,"intentRecognitionEnabled":false,"members":[{"id":"one","providerId":"fixture","modelId":"synthetic-model"}],"defaultMemberId":"one","fallbackMemberIds":[],"classifierTarget":null,"classificationBudgetMs":2000,"rules":[]}),
    )?;
    let saved = db.save_intent_routing_group(group)?;
    db.delete_intent_routing_group(&saved.id, saved.revision)?;
    let catalog = db.intent_routing_catalog()?;
    assert!(catalog.groups.is_empty());
    assert!(!catalog.auto_create);
    assert_eq!(catalog.removed_groups.len(), 1);
    assert_eq!(catalog.removed_groups[0].name, saved.name);
    db.restore_discovered_routing_group(&saved.id)?;
    let restored = db.list_intent_routing_groups()?.remove(0);
    let mut expected = saved.clone();
    expected.revision += 1;
    assert_eq!(restored, expected);
    assert!(db.intent_routing_catalog()?.removed_groups.is_empty());
    assert!(db.save_intent_routing_group(saved.clone()).is_err());
    assert!(db
        .delete_intent_routing_group(&saved.id, saved.revision)
        .is_err());
    assert_eq!(db.list_intent_routing_groups()?, vec![restored.clone()]);
    assert!(db.restore_discovered_routing_group(&saved.id).is_err());
    db.delete_intent_routing_group(&restored.id, restored.revision)?;
    let mut replacement = restored.clone();
    replacement.revision = 0;
    replacement.name = "Synthetic replacement".into();
    let replacement = db.save_intent_routing_group(replacement)?;
    assert!(replacement.revision > restored.revision);
    assert_eq!(replacement.name, "Synthetic replacement");
    assert!(db.intent_routing_catalog()?.removed_groups.is_empty());
    assert!(db.save_intent_routing_group(restored).is_err());
    db.delete_intent_routing_group(&replacement.id, replacement.revision)?;
    let upstream = db.list_routing_upstreams()?.remove(0);
    db.delete_routing_upstream(&upstream.id, upstream.revision)?;
    assert_eq!(
        db.restore_discovered_routing_group(&saved.id),
        Err("intentRouting.providerUnavailable".into())
    );
    assert!(db.list_intent_routing_groups()?.is_empty());
    assert_eq!(db.intent_routing_catalog()?.removed_groups.len(), 1);
    Ok(())
}

#[test]
fn malformed_saved_group_archives_are_not_discarded_by_restore(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let value = r#"{"id":"auto-fixture","name":"Synthetic","group":{"invalid":true}}"#;
    db.extension_connection()
        .lock()
        .expect("synthetic database")
        .execute(
            "INSERT INTO intent_routing_preferences(key,value) VALUES ('hidden:auto-fixture',?1)",
            [value],
        )?;
    assert!(db.restore_discovered_routing_group("auto-fixture").is_err());
    let retained: String = db
        .extension_connection()
        .lock()
        .expect("synthetic database")
        .query_row(
            "SELECT value FROM intent_routing_preferences WHERE key='hidden:auto-fixture'",
            [],
            |row| row.get(0),
        )?;
    assert_eq!(retained, value);
    assert!(archived_group("true")?.is_none());
    Ok(())
}
