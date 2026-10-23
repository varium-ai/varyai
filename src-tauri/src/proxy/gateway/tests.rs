use super::*;

fn policy() -> KeyPolicy {
    KeyPolicy {
        name: "Synthetic project".into(),
        models: vec!["allowed".into()],
        token_limit: Some(100),
        period: Period::Daily,
    }
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
