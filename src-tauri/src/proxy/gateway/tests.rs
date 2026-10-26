use super::*;

fn policy() -> KeyPolicy {
    KeyPolicy {
        name: "Synthetic project".into(),
        models: vec!["allowed".into()],
        all_models: false,
        token_limit: Some(100),
        period: Period::Daily,
    }
}

#[test]
fn saved_gateway_secrets_survive_restart_without_entering_the_database_or_list(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    let store = dir.path().join("gateway-secrets");
    let db = Database::init_at(&path)?;
    let created = db.create_saved_gateway_key(policy(), &store)?;
    let id = created.key.id.clone();
    assert_eq!(
        db.reveal_gateway_key(&id, &store)?.as_deref(),
        Some(created.secret.as_str())
    );
    assert_eq!(
        db.reveal_gateway_key(&id, &store)?.as_deref(),
        Some(created.secret.as_str())
    );
    let listing = serde_json::to_string(&db.list_gateway_keys()?)?;
    assert!(!listing.contains(&created.secret));
    let bytes = std::fs::read(dir.path().join("vary-ai.db"))?;
    assert!(!bytes
        .windows(created.secret.len())
        .any(|window| window == created.secret.as_bytes()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store.join(&id))?.permissions().mode() & 0o777,
            0o600
        );
    }
    drop(db);
    let db = Database::init_at(&path)?;
    assert_eq!(
        db.reveal_gateway_key(&id, &store)?.as_deref(),
        Some(created.secret.as_str())
    );
    db.revoke_gateway_key(&id)?;
    assert!(db.reveal_gateway_key(&id, &store)?.is_none());
    db.delete_saved_gateway_key(&id, &store)?;
    assert!(!store.join(&id).exists());
    assert!(db.reveal_gateway_key(&id, &store)?.is_none());
    Ok(())
}

#[test]
fn legacy_missing_tampered_and_unwritable_secret_files_are_handled_safely(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let store = dir.path().join("gateway-secrets");
    let db = Database::memory()?;
    let legacy = db.create_gateway_key(policy())?;
    assert!(db.reveal_gateway_key(&legacy.key.id, &store)?.is_none());
    let saved = db.create_saved_gateway_key(policy(), &store)?;
    assert!(db.delete_saved_gateway_key(&saved.key.id, &store).is_err());
    assert!(store.join(&saved.key.id).exists());
    std::fs::write(store.join(&saved.key.id), "synthetic-tampered-secret")?;
    assert!(db.reveal_gateway_key(&saved.key.id, &store).is_err());
    std::fs::remove_file(store.join(&saved.key.id))?;
    assert!(db.reveal_gateway_key(&saved.key.id, &store)?.is_none());
    let blocked = dir.path().join("not-a-directory");
    std::fs::write(&blocked, "fixture")?;
    assert!(db.create_saved_gateway_key(policy(), &blocked).is_err());
    assert_eq!(db.list_gateway_keys()?.len(), 2);
    assert!(db.reveal_gateway_key("../outside", &store)?.is_none());
    Ok(())
}

#[test]
fn deletes_only_revoked_unbound_keys_and_removes_their_usage(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let a = db.create_gateway_key(policy())?;
    let b = db.create_gateway_key(policy())?;
    let grant_a = db.authorize_gateway_key(&a.secret, "allowed", 0)?;
    let grant_b = db.authorize_gateway_key(&b.secret, "allowed", 0)?;
    db.record_gateway_usage(&grant_a, 20)?;
    db.record_gateway_usage(&grant_b, 30)?;
    assert!(db.delete_gateway_key(&a.key.id).is_err());
    db.bind_gateway_agent("claude-desktop", Some(&a.key.id))?;
    db.revoke_gateway_key(&a.key.id)?;
    assert!(db.delete_gateway_key(&a.key.id).is_err());
    db.bind_gateway_agent("claude-desktop", None)?;
    db.delete_gateway_key(&a.key.id)?;
    assert!(matches!(
        db.authorize_gateway_key(&a.secret, "allowed", 0),
        Err(GatewayError::Unauthorized)
    ));
    assert!(db.list_gateway_usage(Some(&a.key.id))?.is_empty());
    assert_eq!(db.list_gateway_usage(Some(&b.key.id))?[0].tokens, 30);
    let remaining = db.list_gateway_keys()?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, b.key.id);
    // Usage from an already admitted request must not recreate deleted counters.
    db.record_gateway_usage(&grant_a, 10)?;
    assert!(db.list_gateway_usage(Some(&a.key.id))?.is_empty());
    let conn = db
        .extension_connection()
        .lock()
        .map_err(|_| "database lock")?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM gateway_usage WHERE key_id=?1",
        [&a.key.id],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn usage_preserves_each_forwarded_model_and_keeps_legacy_rows_unknown(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    let key = db.create_gateway_key(KeyPolicy {
        period: Period::Never,
        ..policy()
    })?;
    db.charge_gateway_key(&key.key.id, 0, 7)?;
    db.extension_connection().lock().map_err(|_| "database lock")?.execute("INSERT INTO gateway_usage_details(key_id,window,agent,model,tokens,requests,unmetered,last_used) VALUES(?1,0,'external','allowed',7,1,0,1)", [&key.key.id])?;
    drop(db);
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db"))?;
    ext.execute_batch("DROP TABLE IF EXISTS gateway_usage_models; PRAGMA user_version=3;")?;
    drop(ext);
    let db = Database::init_at(&path)?;
    for (model, tokens) in [("upstream-a", 20), ("upstream-b", 30)] {
        let grant = db.authorize_gateway_key(&key.secret, "allowed", 0)?;
        grant.upstream_model.record(Some(model));
        db.record_gateway_usage(&grant, tokens)?;
    }
    let rows = db.list_gateway_usage(Some(&key.key.id))?;
    assert_eq!(db.list_gateway_keys()?[0].used_tokens, 57);
    assert_eq!(rows.len(), 3);
    assert!(rows
        .iter()
        .any(|row| row.upstream_model.as_deref() == Some("upstream-a") && row.tokens == 20));
    assert!(rows
        .iter()
        .any(|row| row.upstream_model.as_deref() == Some("upstream-b") && row.tokens == 30));
    assert!(rows
        .iter()
        .any(|row| row.upstream_model.is_none() && row.tokens == 7));
    db.revoke_gateway_key(&key.key.id)?;
    db.delete_gateway_key(&key.key.id)?;
    assert!(db.list_gateway_usage(Some(&key.key.id))?.is_empty());
    Ok(())
}

#[test]
fn agent_binding_is_independent_of_client_credentials_and_survives_revocation(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let a = db.create_gateway_key(policy())?;
    let b = db.create_gateway_key(policy())?;
    db.bind_gateway_agent("claude-desktop", Some(&a.key.id))?;
    assert!(db.authorize_gateway_agent("claude", "other", 1)?.is_none());
    assert!(matches!(
        db.authorize_gateway_agent("claude-desktop", "other", 1),
        Err(GatewayError::Forbidden)
    ));
    let grant = db
        .authorize_gateway_agent("claude-desktop", "allowed", 1)?
        .ok_or("missing grant")?;
    db.record_gateway_usage(&grant, 100)?;
    assert!(matches!(
        db.authorize_gateway_agent("claude-desktop", "allowed", 1),
        Err(GatewayError::Exhausted(_))
    ));
    assert!(db.authorize_gateway_key(&b.secret, "allowed", 1).is_ok());
    assert_eq!(
        db.list_gateway_usage(Some(&a.key.id))?[0].agent,
        "claude-desktop"
    );
    db.revoke_gateway_key(&a.key.id)?;
    assert!(matches!(
        db.authorize_gateway_agent("claude-desktop", "allowed", 1),
        Err(GatewayError::Unauthorized)
    ));
    db.bind_gateway_agent("claude-desktop", None)?;
    assert!(db
        .authorize_gateway_agent("claude-desktop", "allowed", 1)?
        .is_none());
    assert!(db
        .bind_gateway_agent("not-an-agent", Some(&b.key.id))
        .is_err());
    Ok(())
}

#[test]
fn independent_usage_revocation_and_budget_updates() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let a = db.create_gateway_key(policy())?;
    let b = db.create_gateway_key(policy())?;
    let now = 1_700_000_000;
    let grant = db.authorize_gateway_key(&a.secret, "allowed", now)?;
    assert!(matches!(
        db.authorize_gateway_key(&a.secret, "other", now),
        Err(GatewayError::Forbidden)
    ));
    db.charge_gateway_key(&grant.id, grant.window, 50)?;
    let mut lower = policy();
    lower.token_limit = Some(10);
    db.update_gateway_key(&a.key.id, lower)?;
    assert!(matches!(
        db.authorize_gateway_key(&a.secret, "allowed", now),
        Err(GatewayError::Exhausted(_))
    ));
    assert!(db.authorize_gateway_key(&b.secret, "allowed", now).is_ok());
    assert!(db
        .authorize_gateway_key(&a.secret, "allowed", now + 86400)
        .is_ok());
    db.revoke_gateway_key(&b.key.id)?;
    assert!(matches!(
        db.authorize_gateway_key(&b.secret, "allowed", now),
        Err(GatewayError::Unauthorized)
    ));
    assert!(!serde_json::to_string(&db.list_gateway_keys()?)?.contains(&a.secret));
    Ok(())
}

#[test]
fn calendar_windows_and_usage_semantics() {
    assert_eq!(Period::Daily.window(86401), (86400, Some(172800)));
    let a = Period::Monthly.window(1_700_000_000);
    assert!(a.0 < 1_700_000_000 && a.1.is_some_and(|end| end > 1_700_000_000));
    let usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 10,
        cache_read_tokens: 40,
        cache_creation_tokens: 5,
        ..Default::default()
    };
    assert_eq!(total_tokens(&usage, false), 155);
    assert_eq!(total_tokens(&usage, true), 115);
}

#[test]
fn upgrades_v1_extension_and_preserves_credentials_across_restart(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    {
        let _db = Database::init_at(&path)?;
    }
    {
        let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db"))?;
        ext.execute_batch(
            "DROP TABLE gateway_keys; DROP TABLE gateway_usage; PRAGMA user_version=1;",
        )?;
    }
    let db = Database::init_at(&path)?;
    let created = db.create_gateway_key(policy())?;
    let now = Utc::now().timestamp();
    let grant = db.authorize_gateway_key(&created.secret, "allowed", now)?;
    db.charge_gateway_key(&grant.id, grant.window, 100)?;
    drop(db);
    let db = Database::init_at(&path)?;
    assert!(matches!(
        db.authorize_gateway_key(&created.secret, "allowed", now),
        Err(GatewayError::Exhausted(_))
    ));
    db.revoke_gateway_key(&created.key.id)?;
    drop(db);
    let db = Database::init_at(&path)?;
    assert!(matches!(
        db.authorize_gateway_key(&created.secret, "allowed", now),
        Err(GatewayError::Unauthorized)
    ));
    Ok(())
}

#[test]
fn legacy_allowlists_stay_restricted_and_all_models_is_explicit(
) -> Result<(), Box<dyn std::error::Error>> {
    let legacy: KeyPolicy = serde_json::from_value(
        serde_json::json!({"name":"Legacy", "models":["allowed"], "tokenLimit":null, "period":"never"}),
    )?;
    assert!(!legacy.all_models);
    let db = Database::memory()?;
    let key = db.create_gateway_key(legacy)?;
    assert!(matches!(
        db.authorize_gateway_key(&key.secret, "other", 0),
        Err(GatewayError::Forbidden)
    ));
    let mut unrestricted = policy();
    unrestricted.all_models = true;
    unrestricted.models.clear();
    db.update_gateway_key(&key.key.id, unrestricted)?;
    assert!(db.authorize_gateway_key(&key.secret, "other", 0).is_ok());
    assert!(db.authorize_gateway_key(&key.secret, "", 0).is_err());
    Ok(())
}

#[test]
fn missing_usage_is_visible_and_bindings_persist_without_storing_secrets_in_details(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    let key = db.create_gateway_key(policy())?;
    db.bind_gateway_agent("gemini", Some(&key.key.id))?;
    let grant = db
        .authorize_gateway_agent("gemini", "allowed", 1)?
        .ok_or("missing grant")?;
    db.record_gateway_usage(&grant, 20)?;
    db.record_gateway_observation(&grant, None)?;
    drop(db);
    let db = Database::init_at(&path)?;
    assert_eq!(db.list_gateway_keys()?[0].agents, vec!["gemini"]);
    let rows = db.list_gateway_usage(Some(&key.key.id))?;
    assert_eq!(
        (rows[0].tokens, rows[0].requests, rows[0].unmetered),
        (20, 2, 1)
    );
    assert!(!serde_json::to_string(&rows)?.contains(&key.secret));
    Ok(())
}

#[test]
fn catalog_covers_every_agent_without_returning_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let p = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        serde_json::json!({"env":{"ANTHROPIC_MODEL":"fixture-model","ANTHROPIC_AUTH_TOKEN":"synthetic-private"},"modelPicker":{"options":[{"model":"fixture-extra"}]}}),
        None,
    );
    db.save_provider("claude", &p)?;
    let value = catalog(&db)?;
    let encoded = serde_json::to_string(&value)?;
    assert!(!encoded.contains("synthetic-private"));
    assert_eq!(
        value.agents.len(),
        crate::app_config::AppType::all().count()
    );
    assert!(value.models.iter().any(|m| m.model == "fixture-extra"));
    assert!(value
        .agents
        .iter()
        .any(|a| a.id == "claude-desktop" && a.local_routing));
    assert_eq!(value.agents.iter().filter(|a| !a.local_routing).count(), 3);
    Ok(())
}

#[test]
fn extension_v2_upgrades_without_losing_keys_or_usage() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("cc-switch.db");
    let db = Database::init_at(&path)?;
    let key = db.create_gateway_key(policy())?;
    let grant = db.authorize_gateway_key(&key.secret, "allowed", 1)?;
    db.charge_gateway_key(&grant.id, grant.window, 31)?;
    drop(db);
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db"))?;
    ext.execute_batch("DROP TABLE gateway_agent_bindings; DROP TABLE gateway_usage_details; PRAGMA user_version=2;")?;
    drop(ext);
    let db = Database::init_at(&path)?;
    assert!(db.authorize_gateway_key(&key.secret, "allowed", 1).is_ok());
    db.bind_gateway_agent("claude-desktop", Some(&key.key.id))?;
    assert!(db
        .authorize_gateway_agent("claude-desktop", "allowed", 1)?
        .is_some());
    assert!(db.list_gateway_usage(Some(&key.key.id))?.is_empty());
    let ext = rusqlite::Connection::open(dir.path().join("vary-ai.db"))?;
    let count: u64 = ext.query_row(
        "SELECT tokens FROM gateway_usage WHERE key_id=?1",
        [key.key.id],
        |r| r.get(0),
    )?;
    assert_eq!(count, 31);
    Ok(())
}
