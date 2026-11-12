use super::*;

fn picks(db: &Database, paths: &ImportPaths) -> Result<Vec<Pick>, String> {
    Ok(preview(db, paths)?
        .remove(4)
        .items
        .into_iter()
        .map(|item| Pick {
            source: "magpie".into(),
            reference: item.reference,
            fingerprint: item.fingerprint,
            mode: ImportMode::Add,
            name: None,
        })
        .collect())
}
fn relay(id: &str, name: &str, host: &str, key: &str) -> Value {
    json!({"id":id,"name":name,"chat":format!("https://{host}.example.test/v1"),"key":key})
}

#[tokio::test]
async fn production_import_uses_magpie_id_and_allows_reimport_to_overwrite(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[relay("source-relay","Display name","one","synthetic-one")]}),
    )?;
    let original = std::fs::read(&paths.provider_backup)?;
    let db = Arc::new(Database::memory()?);
    let mut choices = picks(&db, &paths)?;
    choices[0].name = Some("Custom display name".into());
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("accounts")));
    import_with_accounts(db.clone(), manager.clone(), paths.clone(), choices).await?;
    let saved = db.list_routing_upstreams()?.remove(0);
    assert_eq!(saved.id, "source-relay");
    assert_eq!(saved.name, "Custom display name");
    let shown = preview_with_accounts(db.clone(), manager.clone(), paths.clone()).await?;
    assert_eq!(shown[4].items[0].status, "new");
    let repeated =
        import_with_accounts(db.clone(), manager, paths.clone(), picks(&db, &paths)?).await?;
    assert_eq!(
        repeated,
        ImportResult {
            added: 1,
            skipped: 0
        }
    );
    let records = db.list_routing_upstreams()?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "source-relay");
    assert_eq!(records[0].name, "Display name");
    assert_eq!(std::fs::read(&paths.provider_backup)?, original);
    Ok(())
}

#[test]
fn magpie_id_collisions_overwrite_instead_of_generating_suffixes(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    let db = Database::memory()?;
    write(
        &paths.provider_backup,
        json!({"providers":[relay("relay","Existing","existing","synthetic-existing")]}),
    )?;
    import(&db, &paths, picks(&db, &paths)?)?;
    write(
        &paths.provider_backup,
        json!({"providers":[relay("relay","Incoming one","one","synthetic-one"),relay("relay","Incoming two","two","synthetic-two")]}),
    )?;
    import(&db, &paths, picks(&db, &paths)?)?;
    let saved = db.list_routing_upstreams()?;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, "relay");
    assert_eq!(saved[0].name, "Incoming two");
    assert_eq!(saved[0].revision, 3);
    Ok(())
}

#[test]
fn reimport_replaces_the_old_uuid_without_creating_a_compatibility_alias(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[relay("relay","Relay","one","synthetic-one")]}),
    )?;
    let db = Database::memory()?;
    let mut legacy = readers::provider_backup(&paths.provider_backup)?
        .remove(0)
        .record;
    let old = "11111111-1111-4111-8111-111111111111";
    legacy.id = old.into();
    db.save_routing_upstream(legacy)?;
    import(&db, &paths, picks(&db, &paths)?)?;
    let saved = db.list_routing_upstreams()?;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, "relay");
    let providers = db.routing_providers()?;
    assert_eq!(
        crate::services::routing_upstreams::execution_identity(&providers[0]),
        "relay"
    );
    Ok(())
}

#[test]
fn invalid_magpie_ids_are_rejected_instead_of_inventing_an_id(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    for id in [String::new(), "bad\nidentity".into(), "x".repeat(129)] {
        let db = Database::memory()?;
        write(
            &paths.provider_backup,
            json!({"providers":[relay(&id,"Team Relay","one","synthetic-one")]}),
        )?;
        assert!(preview(&db, &paths)?[4].items[0].skip.is_some());
        assert!(import(&db, &paths, picks(&db, &paths)?).is_err());
        assert!(db.list_routing_upstreams()?.is_empty());
    }
    Ok(())
}

#[test]
fn magpie_ids_are_kept_verbatim_at_the_utf8_byte_limit() -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    for id in ["x".repeat(128), "供应商".into()] {
        let db = Database::memory()?;
        write(
            &paths.provider_backup,
            json!({"providers":[relay(&id,"Relay","one","synthetic-one")]}),
        )?;
        import(&db, &paths, picks(&db, &paths)?)?;
        assert_eq!(db.list_routing_upstreams()?.remove(0).id, id);
    }
    Ok(())
}
