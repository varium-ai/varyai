use super::*;
use serde_json::json;

#[test]
fn magpie_key_model_scopes_keep_each_keys_own_allowlist() -> Result<(), Box<dyn std::error::Error>>
{
    let (_dir, paths) = fixture()?;
    let primary = "synthetic-primary-scoped";
    let backup = "synthetic-backup-scoped";
    let first = format!("{:x}", Sha256::digest(primary.as_bytes()))[..10].to_owned();
    let second = format!("{:x}", Sha256::digest(backup.as_bytes()))[..10].to_owned();
    let mut scopes = serde_json::Map::new();
    scopes.insert(first, json!(["chat-a"]));
    scopes.insert(second, json!(["chat-b"]));
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"scoped","name":"Scoped","chat":"https://synthetic.invalid/v1","key":primary,"keys":[{"key":backup}],"models":["chat-a","chat-b"],"accountModels":scopes}]}),
    )?;
    let imported = readers::provider_backup(&paths.provider_backup)?.remove(0);
    let transport = imported.record.transport.ok_or("transport missing")?;
    assert_eq!(transport.primary_models, Some(vec!["chat-a".into()]));
    assert_eq!(transport.keys[0].models, Some(vec!["chat-b".into()]));
    Ok(())
}

#[test]
fn magpie_advanced_settings_do_not_block_usable_api_imports(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    for advanced in [
        json!({"proxy":"http://proxy.invalid:8080"}),
        json!({"extraBody":{"synthetic_private_parameter":true}}),
        json!({"keyPlan":"synthetic-plan"}),
        json!({"insecureTLS":true}),
        json!({"keySpendCaps":{"0123456789":{"usd":1,"period":"month"}}}),
        json!({"maxRPM":1}),
        json!({"maxConcurrency":1}),
        json!({"accountConcurrency":1}),
        json!({"queueLimit":2}),
        json!({"queueWait":1}),
        json!({"queueTimeout":1}),
        json!({"maxQueue":1}),
        json!({"queueTimeoutMs":1}),
        json!({"maxAccounts":1}),
        json!({"accountWeights":{"synthetic":2}}),
        json!({"accountCaps":{"synthetic":1}}),
        json!({"accountWindowCaps":{"synthetic":{"week":1}}}),
        json!({"keyWeight":2}),
        json!({"keys":[{"key":"synthetic-backup","weight":2}]}),
        json!({"keys":[{"key":"synthetic-backup","plan":"synthetic-plan"}]}),
        json!({"priceRate":1.5}),
        json!({"routing":"rotate"}),
        json!({"sink":true}),
        json!({"fallback":["synthetic-fallback"]}),
        json!({"decide":"synthetic-decision"}),
    ] {
        let mut provider = json!({"id":"synthetic","chat":"https://synthetic.invalid/v1","key":"synthetic-only","models":["synthetic-model"],"headers":{"x-workspace":"synthetic-workspace"}});
        provider
            .as_object_mut()
            .ok_or("fixture")?
            .extend(advanced.as_object().ok_or("fixture")?.clone());
        write(&paths.provider_backup, json!({"providers":[provider]}))?;
        let original = std::fs::read(&paths.provider_backup)?;
        let db = Database::memory()?;
        let item = preview(&db, &paths)?.remove(4).items.remove(0);
        assert!(item.skip.is_none());
        assert_eq!(item.model_count, 1);
        let public = serde_json::to_string(&item)?;
        assert!(!public.contains("synthetic-only"));
        assert!(!public.contains("synthetic-workspace"));
        assert!(!public.contains("synthetic_private_parameter"));
        let result = import(
            &db,
            &paths,
            vec![Pick {
                source: "magpie".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None,
            }],
        )?;
        assert_eq!(result.added, 1);
        assert_eq!(std::fs::read(&paths.provider_backup)?, original);
        let saved = db.list_routing_upstreams()?.remove(0);
        let resolved = db.routing_discovery_provider(&saved.id, saved.revision)?;
        let variants = crate::services::routing_upstreams::variants(&resolved);
        assert_eq!(
            variants.len(),
            if advanced.get("keys").is_some() { 2 } else { 1 }
        );
        let (base, key) = variants[0].resolve_usage_credentials(&crate::app_config::AppType::Codex);
        assert_eq!(base, "https://synthetic.invalid/v1");
        assert!(key == "synthetic-only");
        assert_eq!(
            variants[0].settings_config["modelCatalog"]["models"][0]["model"],
            "synthetic-model"
        );
        assert_eq!(
            variants[0]
                .meta
                .as_ref()
                .and_then(|m| m.local_proxy_request_overrides.as_ref())
                .and_then(|o| o.headers.get("x-workspace"))
                .map(String::as_str),
            Some("synthetic-workspace")
        );
        let executable = serde_json::to_value(&variants[0])?;
        assert!(!executable
            .to_string()
            .contains("synthetic_private_parameter"));
    }
    Ok(())
}

#[test]
fn magpie_basic_import_still_rejects_invalid_transport_and_missing_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    for invalid in [
        json!({"chat":"file:///synthetic"}),
        json!({"chat":"https://synthetic.invalid/v1?key=synthetic-private"}),
        json!({"key":""}),
        json!({"headers":{"x-workspace":1}}),
        json!({"headers":{"cookie":"synthetic-private"}}),
        json!({"headers":{"x-workspace":"bad\nvalue"}}),
    ] {
        let mut provider = json!({"id":"synthetic","chat":"https://synthetic.invalid/v1","key":"synthetic-only","models":["synthetic-model"],"maxRPM":1});
        provider
            .as_object_mut()
            .ok_or("fixture")?
            .extend(invalid.as_object().ok_or("fixture")?.clone());
        write(&paths.provider_backup, json!({"providers":[provider]}))?;
        let db = Database::memory()?;
        let item = preview(&db, &paths)?.remove(4).items.remove(0);
        assert!(item.skip.is_some());
        assert!(import(
            &db,
            &paths,
            vec![Pick {
                source: "magpie".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None,
            }]
        )
        .is_err());
        assert!(db.list_routing_upstreams()?.is_empty());
    }
    Ok(())
}

fn fixture() -> Result<(tempfile::TempDir, ImportPaths), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let paths = ImportPaths::at(dir.path());
    Ok((dir, paths))
}

fn write(path: &Path, value: Value) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
    std::fs::write(path, serde_json::to_vec(&value)?)?;
    Ok(())
}

#[test]
fn provider_import_preserves_saved_icons_without_network_requests(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","icon":"deepseek-color","chat":"https://relay.example/v1","key":"fixture-key"}]}),
    )?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    import(
        &db,
        &paths,
        vec![Pick {
            source: "magpie".into(),
            reference: item.reference,
            fingerprint: item.fingerprint,
            mode: ImportMode::Add,
            name: None,
        }],
    )?;
    assert_eq!(
        serde_json::to_value(db.list_routing_upstreams()?.remove(0))?["icon"],
        "deepseek-color"
    );
    Ok(())
}

#[test]
fn preview_has_five_sources_and_never_serializes_credentials_or_changes_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"https://relay.example","ANTHROPIC_AUTH_TOKEN":"synthetic-private-key","ANTHROPIC_MODEL":"exact-model"}}),
    )?;
    let before = std::fs::read(&paths.claude)?;
    let db = Database::memory()?;
    let preview = preview(&db, &paths)?;
    assert_eq!(preview.len(), 5);
    assert_eq!(preview[4].id, "magpie");
    assert_eq!(preview[4].name, "Magpie");
    assert_eq!(preview[1].items[0].model_count, 1);
    assert!(serde_json::to_value(&preview)?[1]["items"][0]
        .get("apiKey")
        .is_none());
    assert!(std::fs::read(&paths.claude)? == before);
    assert!(db.list_routing_upstreams()?.is_empty());
    Ok(())
}

#[test]
fn confirmation_rereads_and_rejects_a_changed_source_atomically(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","name":"Relay","chat":"https://relay.example/v1","key":"synthetic-first","models":["exact"]}]}),
    )?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","name":"Relay","chat":"https://relay.example/v1","key":"synthetic-changed"}]}),
    )?;
    assert_eq!(
        import(
            &db,
            &paths,
            vec![Pick {
                source: "magpie".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None
            }]
        ),
        Err(CONFLICT.into())
    );
    assert!(db.list_routing_upstreams()?.is_empty());
    Ok(())
}

#[test]
fn provider_backup_import_preserves_models_protocols_and_key_pool_and_deduplicates(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","name":"Relay","chat":"https://relay.example/v1","responses":"https://relay.example/v1","anthropic":"https://relay.example/anthropic","baseAPI":"responses","key":"synthetic-first","keyName":"Personal","keys":[{"name":"Backup","key":"synthetic-second","off":true}],"models":["vendor/exact"],"unlisted":true}]}),
    )?;
    let before = std::fs::read(&paths.provider_backup)?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    let result = import(
        &db,
        &paths,
        vec![Pick {
            source: "magpie".into(),
            reference: item.reference,
            fingerprint: item.fingerprint,
            mode: ImportMode::Add,
            name: None,
        }],
    )?;
    assert_eq!(result.added, 1);
    let saved = db.list_routing_upstreams()?.remove(0);
    assert!(saved.has_api_key);
    let executable = db.routing_discovery_provider(&saved.id, saved.revision)?;
    assert_eq!(executable.settings_config["apiFormat"], "openai_responses");
    assert_eq!(
        executable.settings_config["modelCatalog"]["models"][0]["model"],
        "vendor/exact"
    );
    assert_eq!(
        executable.settings_config["routingModelPolicy"]["groupsOnly"],
        true
    );
    assert_eq!(saved.transport.as_ref().ok_or("transport")?.keys.len(), 1);
    assert_eq!(
        saved
            .transport
            .as_ref()
            .and_then(|transport| transport.primary_name.as_deref()),
        Some("Personal")
    );
    assert!(saved.transport.as_ref().ok_or("transport")?.keys[0]
        .key
        .is_none());
    assert!(!serde_json::to_string(&saved)?.contains("synthetic"));
    assert_eq!(preview(&db, &paths)?[4].items[0].status, "new");
    assert!(std::fs::read(&paths.provider_backup)? == before);
    Ok(())
}

#[test]
fn different_keys_can_join_a_vendor_but_oauth_and_self_routes_are_not_imported(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","name":"Relay","chat":"https://relay.example/v1","key":"synthetic-first"}]}),
    )?;
    let db = Database::memory()?;
    let first = preview(&db, &paths)?.remove(4).items.remove(0);
    import(
        &db,
        &paths,
        vec![Pick {
            source: "magpie".into(),
            reference: first.reference,
            fingerprint: first.fingerprint,
            mode: ImportMode::Add,
            name: None,
        }],
    )?;
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"https://relay.example/v1","ANTHROPIC_AUTH_TOKEN":"synthetic-other"}}),
    )?;
    write(
        &paths.opencode_auth,
        json!({"openai":{"type":"oauth","access":"synthetic-oauth"}}),
    )?;
    let sources = preview(&db, &paths)?;
    assert_eq!(sources[1].items[0].status, "vendor");
    assert_eq!(sources[3].items[0].skip.as_deref(), Some("subscription"));
    let item = &sources[1].items[0];
    import(
        &db,
        &paths,
        vec![Pick {
            source: "claude-code".into(),
            reference: item.reference.clone(),
            fingerprint: item.fingerprint.clone(),
            mode: ImportMode::Key,
            name: None,
        }],
    )?;
    assert_eq!(db.list_routing_upstreams()?.len(), 1);
    let saved = db.list_routing_upstreams()?.remove(0);
    assert_eq!(saved.id, "relay");
    assert_eq!(saved.transport.ok_or("transport")?.keys.len(), 1);
    Ok(())
}

#[test]
fn malformed_sources_are_sanitized_and_missing_sources_are_distinct(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    std::fs::create_dir_all(paths.provider_backup.parent().ok_or("parent")?)?;
    std::fs::write(&paths.provider_backup, b"{synthetic-secret-bad-json")?;
    let sources = preview(&Database::memory()?, &paths)?;
    assert!(!sources[0].found);
    assert!(sources[4].found);
    assert!(sources[4].error.is_some());
    assert!(!serde_json::to_string(&sources)?.contains("synthetic-secret"));
    Ok(())
}

#[test]
fn native_import_is_atomic_deduplicates_a_batch_and_retains_redacted_secrets(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let mut first = candidate(
        "a".into(),
        "fixture",
        "Relay".into(),
        "https://relay.example/v1".into(),
        "synthetic-main".into(),
        "openai_chat",
        vec![],
    )
    .record;
    first.transport = Some(ImportedTransport {
        endpoints: BTreeMap::from([("openai_chat".into(), "https://relay.example/v1".into())]),
        headers: Some(BTreeMap::from([(
            "x-workspace".into(),
            "synthetic-workspace".into(),
        )])),
        keys: vec![ImportedKey {
            id: "backup".into(),
            name: "Backup".into(),
            key: Some("synthetic-backup".into()),
            enabled: true,
            api_format: None,
            models: None,
        }],
        ..Default::default()
    });
    let mut invalid = first.clone();
    invalid.name.clear();
    invalid.source = UpstreamSource::Codex;
    assert!(db
        .import_native_providers(vec![
            Selected {
                record: first.clone(),
                join: false,
                overwrite: false
            },
            Selected {
                record: invalid,
                join: false,
                overwrite: false
            }
        ])
        .is_err());
    assert!(db.list_routing_upstreams()?.is_empty());
    let result = db.import_native_providers(vec![
        Selected {
            record: first.clone(),
            join: false,
            overwrite: false,
        },
        Selected {
            record: first,
            join: false,
            overwrite: false,
        },
    ])?;
    assert_eq!(
        result,
        ImportResult {
            added: 1,
            skipped: 1
        }
    );
    let mut redacted = db.list_routing_upstreams()?.remove(0);
    assert!(!serde_json::to_string(&redacted)?.contains("synthetic"));
    redacted.name = "Updated".into();
    let saved = db.save_routing_upstream(redacted.clone())?;
    let private = db.private_routing_upstreams()?.remove(0);
    assert!(private
        .transport
        .as_ref()
        .is_some_and(|t| t.headers.is_some() && t.keys.iter().all(|k| k.key.is_some())));
    let mut changed = saved;
    if let UpstreamSource::Api {
        base_url, api_key, ..
    } = &mut changed.source
    {
        *base_url = "https://other.example/v1".into();
        *api_key = Some("synthetic-replacement".into());
    }
    assert!(db.save_routing_upstream(changed).is_err());
    redacted.id.clear();
    redacted.revision = 0;
    assert!(db
        .save_routing_upstream_copy(redacted, &private.id, private.revision)
        .is_ok());
    Ok(())
}

#[test]
fn backup_keys_use_their_own_protocol_endpoint_and_do_not_copy_pool_secrets(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","anthropic":"https://relay.example/anthropic","key":"synthetic-main","keys":[{"name":"Backup","key":"synthetic-backup","protocol":"anthropic"}]}]}),
    )?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    import(
        &db,
        &paths,
        vec![Pick {
            source: "magpie".into(),
            reference: item.reference,
            fingerprint: item.fingerprint,
            mode: ImportMode::Add,
            name: None,
        }],
    )?;
    let saved = db.list_routing_upstreams()?.remove(0);
    let provider = db.routing_discovery_provider(&saved.id, saved.revision)?;
    let variants = crate::services::routing_upstreams::variants(&provider);
    assert_eq!(variants.len(), 2);
    assert!(variants
        .iter()
        .all(|p| p.settings_config.get("routingImportedTransport").is_none()));
    assert_eq!(variants[1].settings_config["apiFormat"], "anthropic");
    let (base, token) = variants[1].resolve_usage_credentials(&crate::app_config::AppType::Codex);
    assert_eq!(base, "https://relay.example/anthropic");
    assert!(token == "synthetic-backup");
    Ok(())
}

#[test]
fn cc_switch_codex_and_opencode_read_native_models_without_source_writes(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    std::fs::create_dir_all(paths.cc_switch.parent().ok_or("parent")?)?;
    let conn = rusqlite::Connection::open(&paths.cc_switch)?;
    conn.execute_batch(
        "CREATE TABLE providers(id TEXT,app_type TEXT,name TEXT,settings_config TEXT);",
    )?;
    conn.execute("INSERT INTO providers VALUES ('a','claude','Claude relay',?1)", [json!({"env":{"ANTHROPIC_BASE_URL":"https://relay.example","ANTHROPIC_API_KEY":"synthetic-one","ANTHROPIC_MODEL":"exact-a"}}).to_string()])?;
    drop(conn);
    std::fs::create_dir_all(paths.codex.parent().ok_or("parent")?)?;
    std::fs::write(&paths.codex, "model_provider='relay'\nmodel='exact-b'\n[model_providers.relay]\nbase_url='https://relay.example/v1'\nwire_api='responses'\nexperimental_bearer_token='synthetic-two'\n")?;
    write(
        &paths.opencode,
        json!({"provider":{"relay":{"npm":"@ai-sdk/anthropic","options":{"baseURL":"https://relay.example/anthropic","apiKey":"synthetic-three"},"models":{"exact-c":{"name":"C"}}}}}),
    )?;
    let db = Database::memory()?;
    let sources = preview(&db, &paths)?;
    for index in [0, 2, 3] {
        assert_eq!(sources[index].items.len(), 1);
        assert_eq!(sources[index].items[0].model_count, 1);
        assert!(sources[index].items[0].skip.is_none());
    }
    let picks = [0, 2, 3]
        .into_iter()
        .map(|index| {
            let item = &sources[index].items[0];
            Pick {
                source: sources[index].id.clone(),
                reference: item.reference.clone(),
                fingerprint: item.fingerprint.clone(),
                mode: ImportMode::Add,
                name: None,
            }
        })
        .collect();
    assert_eq!(import(&db, &paths, picks)?.added, 3);
    assert!(!serde_json::to_string(&sources)?.contains("synthetic"));
    Ok(())
}

fn subscription_auth(subject: &str, workspace: &str) -> Value {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = URL_SAFE_NO_PAD.encode(json!({"sub":subject,"email":"person@example.test","https://api.openai.com/auth":{"chatgpt_account_id":workspace,"chatgpt_plan_type":"plus"}}).to_string());
    json!({"auth_mode":"chatgpt","tokens":{"id_token":format!("{header}.{payload}."),"access_token":"synthetic-access","refresh_token":"synthetic-refresh","account_id":workspace}})
}

#[test]
fn magpie_visible_subscriptions_replace_policy_rows_and_hide_unlisted_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[
            {"id":"relay","name":"Relay","chat":"https://relay.example/v1","key":"synthetic-key"},
            {"id":"codex","name":"Codex","models":["codex-fixture"]},
            {"id":"antigravity","name":"Antigravity","models":["google-fixture"]},
            {"id":"claude","hidden":true,"models":["hidden-fixture"]},
            {"id":"removed","models":["removed-fixture"]}
        ]}),
    )?;
    write(
        &paths.provider_backup.with_file_name("logins.json"),
        json!([
            {"id":"codex-one","agent":"codex","user":"Codex fixture","auth":subscription_auth("fixture-user","fixture-workspace")},
            {"id":"google-one","agent":"antigravity","user":"person@example.test","on":true,"project":"fixture-project","auth":{"access_token":"synthetic-google-access","refresh_token":"synthetic-google-refresh","expiry_date":4102444800000_i64}},
            {"id":"old-plugin","agent":"kiro","user":"Unlisted fixture","auth":{}},
            {"id":"hidden","agent":"claude","user":"Hidden fixture","auth":{}},
            {"id":"hidden-codex","agent":"codex","hidden":"removed","user":"Hidden Codex fixture","auth":subscription_auth("hidden-user","fixture-workspace")}
        ]),
    )?;
    let source = preview(&Database::memory()?, &paths)?.remove(4);
    assert!(source.error.is_none());
    assert_eq!(source.items.len(), 3);
    assert!(source.items.iter().all(|item| item.skip.is_none()));
    assert!(source
        .items
        .iter()
        .any(|item| item.name == "Antigravity" && item.model_count == 1 && item.subscription));
    assert!(source
        .items
        .iter()
        .any(|item| item.name == "Codex" && item.model_count == 1 && item.subscription));
    let public = serde_json::to_string(&source)?;
    for private in [
        "synthetic-google-access",
        "synthetic-google-refresh",
        "Unlisted fixture",
        "Hidden fixture",
        "Hidden Codex fixture",
    ] {
        assert!(!public.contains(private));
    }
    Ok(())
}

#[tokio::test]
async fn magpie_native_subscriptions_import_with_bound_accounts_models_and_no_source_writes(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::{
        codex_oauth_auth::CodexOAuthManager, copilot_auth::CopilotAuthManager,
    };
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[
            {"id":"antigravity","models":["google-fixture"]},
            {"id":"gemini","models":["gemini-fixture"]},
            {"id":"claude","models":["claude-fixture"]},
            {"id":"chatgpt-api","models":["direct-fixture"]},
            {"id":"copilot","models":["copilot-fixture"]}
        ]}),
    )?;
    let logins = paths.provider_backup.with_file_name("logins.json");
    write(
        &logins,
        json!([
            {"agent":"antigravity","user":"google@example.test","project":"fixture-project","auth":{"access_token":"synthetic-google-access","refresh_token":"synthetic-google-refresh","expiry_date":4102444800000_i64}},
            {"agent":"gemini","user":"gemini@example.test","auth":{"access_token":"synthetic-gemini-access","refresh_token":"synthetic-gemini-refresh","expiry_date":4102444800000_i64}},
            {"agent":"claude","user":"claude@example.test","auth":{"claudeAiOauth":{"accessToken":"synthetic-claude-access","refreshToken":"synthetic-claude-refresh"}},"profile":{"emailAddress":"claude@example.test"}},
            {"agent":"chatgpt-api","user":"direct@example.test","auth":{"clientId":"fixture-client","sub":"fixture-subject","access":"synthetic-direct-access","refresh":"synthetic-direct-refresh","expires":"2100-01-01T00:00:00Z"}},
            {"agent":"copilot","user":"fixture-login","auth":{"oauth_token":"synthetic-github-token"}}
        ]),
    )?;
    let original = std::fs::read(&logins)?;
    let db = Arc::new(Database::memory()?);
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("codex-store")));
    let copilot = CopilotAuthManager::new(dir.path().join("copilot-store"));
    let sources =
        preview_with_managers(db.clone(), manager.clone(), &copilot, paths.clone()).await?;
    let items = &sources[4].items;
    assert_eq!(items.len(), 5);
    assert!(items
        .iter()
        .all(|i| i.subscription && i.skip.is_none() && i.model_count == 1));
    assert!(!serde_json::to_string(&sources)?.contains("synthetic-"));
    let picks = items
        .iter()
        .map(|i| Pick {
            source: "magpie".into(),
            reference: i.reference.clone(),
            fingerprint: i.fingerprint.clone(),
            mode: ImportMode::Add,
            name: None,
        })
        .collect();
    assert_eq!(
        import_with_managers(db.clone(), manager.clone(), &copilot, paths.clone(), picks)
            .await?
            .added,
        5
    );
    assert_eq!(copilot.list_accounts().await.len(), 1);
    let reloaded = CopilotAuthManager::new(dir.path().join("copilot-store"));
    assert_eq!(
        reloaded.list_accounts().await[0].id,
        copilot.list_accounts().await[0].id
    );
    assert!(copilot.list_accounts().await[0].id.starts_with("imported-"));
    let records = db.list_routing_upstreams()?;
    assert_eq!(records.len(), 5);
    for record in &records {
        let provider = db.routing_discovery_provider(&record.id, record.revision)?;
        assert_eq!(
            provider.settings_config["modelCatalog"]["models"]
                .as_array()
                .ok_or("models")?
                .len(),
            1
        );
        assert!(crate::services::subscription_provider::binding(&provider).is_some());
    }
    assert!(!serde_json::to_string(&records)?.contains("synthetic-"));
    assert_eq!(std::fs::read(&logins)?, original);
    let repeated = preview_with_managers(db, manager, &copilot, paths).await?;
    assert!(repeated[4].items.iter().all(|i| i.status == "new"));
    Ok(())
}

#[test]
fn provider_backup_backup_endpoints_cannot_point_back_into_the_local_gateway(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","responses":"http://127.0.0.1:15721/connections/codex","baseAPI":"chat","key":"synthetic-key","keys":[{"name":"Backup","key":"synthetic-backup","protocol":"responses"}]}]}),
    )?;
    let sources = preview(&Database::memory()?, &paths)?;
    assert!(sources[4].items.is_empty());
    Ok(())
}

#[tokio::test]
async fn mixed_subscription_batch_rejects_changed_visibility_and_rolls_back_all_account_stores(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::{
        codex_oauth_auth::CodexOAuthManager, copilot_auth::CopilotAuthManager,
    };
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    let providers = json!({"providers":[{"id":"antigravity","models":["google-fixture"]},{"id":"codex"},{"id":"copilot"}]});
    write(&paths.provider_backup, providers.clone())?;
    let logins = paths.provider_backup.with_file_name("logins.json");
    let rows = json!([
        {"agent":"antigravity","user":"google@example.test","auth":{"refresh_token":"synthetic-google-refresh"}},
        {"agent":"copilot","user":"fixture-login","auth":{"oauth_token":"synthetic-github-token"}},
        {"agent":"codex","user":"codex@example.test","auth":subscription_auth("fixture-user","fixture-workspace")}
    ]);
    write(&logins, rows.clone())?;
    let db = Arc::new(Database::memory()?);
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("codex-store")));
    let copilot = CopilotAuthManager::new(dir.path().join("copilot-store"));
    let source = preview_with_managers(db.clone(), manager.clone(), &copilot, paths.clone())
        .await?
        .remove(4);
    assert_eq!(source.items.len(), 3);
    let picks = || {
        source
            .items
            .iter()
            .map(|i| Pick {
                source: "magpie".into(),
                reference: i.reference.clone(),
                fingerprint: i.fingerprint.clone(),
                mode: ImportMode::Add,
                name: None,
            })
            .collect()
    };
    let mut hidden = providers.clone();
    hidden["providers"][0]["hidden"] = json!(true);
    write(&paths.provider_backup, hidden)?;
    assert_eq!(
        import_with_managers(
            db.clone(),
            manager.clone(),
            &copilot,
            paths.clone(),
            picks()
        )
        .await,
        Err(CONFLICT.into())
    );
    write(&paths.provider_backup, providers)?;
    let mut changed = rows.clone();
    changed[0]["auth"]["refresh_token"] = json!("synthetic-rotated-refresh");
    write(&logins, changed)?;
    assert_eq!(
        import_with_managers(
            db.clone(),
            manager.clone(),
            &copilot,
            paths.clone(),
            picks()
        )
        .await,
        Err(CONFLICT.into())
    );
    assert!(!paths.subscription_store.exists());
    assert!(copilot.list_accounts().await.is_empty());
    assert!(manager.list_accounts().await.is_empty());
    write(&logins, rows)?;
    db.import_native_providers(
        (0..crate::services::routing_upstreams::MAX_UPSTREAMS)
            .map(|i| Selected {
                record: candidate(
                    i.to_string(),
                    "fixture",
                    format!("Existing {i}"),
                    "https://fixture.example/v1".into(),
                    format!("synthetic-key-{i}"),
                    "openai_chat",
                    vec![],
                )
                .record,
                join: false,
                overwrite: false,
            })
            .collect(),
    )?;
    assert_eq!(
        import_with_managers(
            db.clone(),
            manager.clone(),
            &copilot,
            paths.clone(),
            picks()
        )
        .await,
        Err("routingUpstreams.limit".into())
    );
    assert_eq!(
        db.list_routing_upstreams()?.len(),
        crate::services::routing_upstreams::MAX_UPSTREAMS
    );
    assert!(!paths.subscription_store.exists());
    assert!(!dir.path().join("copilot-store/copilot_auth.json").exists());
    assert!(copilot.list_accounts().await.is_empty());
    assert!(manager.list_accounts().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn provider_backup_codex_subscriptions_import_into_managed_accounts_without_touching_sources(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"codex","models":["exact-subscription-model"]}]}),
    )?;
    write(
        &paths.provider_backup.with_file_name("settings.json"),
        json!({"modelEfforts":{"codex/exact-subscription-model":["low","high"]}}),
    )?;
    let logins = paths.provider_backup.with_file_name("logins.json");
    write(
        &logins,
        json!([{"id":"saved-one","agent":"codex","user":"Saved account","plan":"plus","on":true,"auth":subscription_auth("synthetic-user","synthetic-workspace")},{"id":"other","agent":"claude","user":"Unsupported account","auth":{}}]),
    )?;
    let before = std::fs::read(&logins)?;
    let db = Arc::new(Database::memory()?);
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("managed")));
    let sources = preview_with_accounts(db.clone(), manager.clone(), paths.clone()).await?;
    let items = &sources[4].items;
    let account = items
        .iter()
        .find(|it| it.subscription && it.skip.is_none())
        .ok_or("subscription missing")?;
    assert_eq!(account.model_count, 1);
    assert!(!items.iter().any(|it| it.name == "Unsupported account"));
    let public = serde_json::to_string(&sources)?;
    let auth = subscription_auth("synthetic-user", "synthetic-workspace");
    for field in ["refresh_token", "access_token", "id_token"] {
        let token = auth["tokens"][field].as_str().ok_or("fixture token")?;
        assert!(!public.contains(token));
    }
    assert_eq!(manager.list_accounts().await.len(), 0);
    let pick = Pick {
        source: "magpie".into(),
        reference: account.reference.clone(),
        fingerprint: account.fingerprint.clone(),
        mode: ImportMode::Add,
        name: None,
    };
    assert_eq!(
        import_with_accounts(db.clone(), manager.clone(), paths.clone(), vec![pick])
            .await?
            .added,
        1
    );
    let accounts = manager.list_accounts().await;
    assert_eq!(accounts.len(), 1);
    let provider = db.list_routing_upstreams()?.remove(0);
    let public = serde_json::to_string(&provider)?;
    for field in ["refresh_token", "access_token", "id_token"] {
        let token = auth["tokens"][field].as_str().ok_or("fixture token")?;
        assert!(!public.contains(token));
    }
    assert!(matches!(provider.source, UpstreamSource::Codex));
    let imported_settings = provider.codex_settings.ok_or("settings")?;
    assert_eq!(imported_settings.accounts, vec![accounts[0].id.clone()]);
    assert_eq!(
        imported_settings.models[0]["modelPreferences"]["reasoningLevels"],
        json!(["low", "high"])
    );
    assert!(std::fs::read(&logins)? == before);
    assert!(!paths.codex.with_file_name("auth.json").exists());
    let repeated = preview_with_accounts(db.clone(), manager, paths).await?;
    assert!(repeated[4]
        .items
        .iter()
        .any(|it| it.subscription && it.status == "new"));
    Ok(())
}

#[tokio::test]
async fn provider_backup_subscription_rejects_stale_auth_and_failed_batches_leave_no_managed_accounts(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    let logins = paths.provider_backup.with_file_name("logins.json");
    write(
        &logins,
        json!([{"id":"one","agent":"codex","user":"Saved account","on":true,"auth":subscription_auth("synthetic-user","synthetic-workspace")}]),
    )?;
    let db = Arc::new(Database::memory()?);
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("managed")));
    let sources = preview_with_accounts(db.clone(), manager.clone(), paths.clone()).await?;
    let item = sources[4].items.first().ok_or("subscription missing")?;
    let pick = Pick {
        source: "magpie".into(),
        reference: item.reference.clone(),
        fingerprint: item.fingerprint.clone(),
        mode: ImportMode::Add,
        name: Some("bad\nname".into()),
    };
    assert!(
        import_with_accounts(db.clone(), manager.clone(), paths.clone(), vec![pick])
            .await
            .is_err()
    );
    assert!(manager.list_accounts().await.is_empty());
    assert!(db.list_routing_upstreams()?.is_empty());
    write(
        &logins,
        json!([{"id":"one","agent":"codex","user":"Saved account","on":true,"auth":subscription_auth("other-user","synthetic-workspace")}]),
    )?;
    let pick = Pick {
        source: "magpie".into(),
        reference: item.reference.clone(),
        fingerprint: item.fingerprint.clone(),
        mode: ImportMode::Add,
        name: None,
    };
    assert_eq!(
        import_with_accounts(db.clone(), manager.clone(), paths, vec![pick]).await,
        Err(CONFLICT.into())
    );
    assert!(manager.list_accounts().await.is_empty());
    Ok(())
}

#[test]
fn provider_backup_saved_subscription_rotation_flags_do_not_disable_explicit_imports(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"codex","models":["exact-model"]}]}),
    )?;
    write(
        &paths.provider_backup.with_file_name("logins.json"),
        json!([
            {"id":"active","agent":"codex","user":"Saved active account","auth":subscription_auth("active-user","workspace")},
            {"id":"standby","agent":"codex","user":"Saved standby account","on":false,"auth":subscription_auth("standby-user","workspace")},
            {"id":"paused","agent":"codex","user":"Paused account","on":true,"paused":true,"auth":subscription_auth("paused-user","workspace")}
        ]),
    )?;
    let candidates = subscriptions::read(&paths.provider_backup)?;
    assert_eq!(candidates.len(), 3);
    assert!(candidates.iter().all(|c| c.skip.is_none()));
    assert!(candidates[0].record.enabled);
    assert!(candidates[1].record.enabled);
    assert!(!candidates[2].record.enabled);
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"codex","models":["exact-model"],"off":true}]}),
    )?;
    assert!(subscriptions::read(&paths.provider_backup)?
        .iter()
        .all(|c| !c.record.enabled));
    Ok(())
}

#[tokio::test]
async fn account_store_rolls_back_when_provider_transaction_rejects_the_batch(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    let dir = tempfile::tempdir()?;
    let manager = CodexOAuthManager::new(dir.path().to_owned());
    let value = subscription_auth("synthetic-user", "synthetic-workspace");
    let failed = manager
        .import_provider_accounts(vec![value.clone()], |_| Err::<(), _>(CONFLICT.into()))
        .await;
    assert_eq!(failed, Err(CONFLICT.into()));
    assert!(manager.list_accounts().await.is_empty());
    assert!(CodexOAuthManager::new(dir.path().to_owned())
        .list_accounts()
        .await
        .is_empty());
    manager
        .import_provider_accounts(vec![value.clone()], |_| Ok(()))
        .await?;
    let first = manager.list_accounts().await.remove(0);
    manager
        .import_provider_accounts(vec![value], move |ids| {
            assert_eq!(ids, vec![first.id]);
            Ok(())
        })
        .await?;
    assert_eq!(manager.list_accounts().await.len(), 1);
    Ok(())
}

#[test]
fn provider_backup_model_context_overrides_and_vendor_catalog_remain_independent(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","key":"synthetic-model-key","models":["picked"],"pickedFrom":["picked","other"],"contexts":{"*":128000,"picked":256000}}]}),
    )?;
    let candidate = readers::provider_backup(&paths.provider_backup)?.remove(0);
    let UpstreamSource::Api {
        models, catalog, ..
    } = candidate.record.source
    else {
        return Err("API fixture".into());
    };
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["contextWindow"], 256000);
    assert_eq!(catalog.len(), 2);
    assert_eq!(
        catalog
            .iter()
            .find(|row| row["model"] == "other")
            .ok_or("vendor model")?["contextWindow"],
        128000
    );
    Ok(())
}

#[test]
fn provider_import_rejects_both_application_default_gateway_ports() {
    let gateways = gateways::Gateways::default();
    for port in [15721, crate::proxy::types::DEFAULT_PROXY_PORT] {
        assert!(gateways.matches_url(&format!("http://127.0.0.1:{port}/v1")));
    }
    assert!(!gateways.matches_url("https://synthetic-upstream.example/v1"));
}

#[test]
fn provider_import_rejects_magpie_gateway_and_loopback_aliases() {
    let gateways = gateways::Gateways::default();
    for host in [
        "127.0.0.1",
        "localhost",
        "LOCALHOST.",
        "127.0.0.2",
        "[::1]",
        "[::ffff:127.0.0.1]",
    ] {
        assert!(
            gateways.matches_url(&format!("http://{host}:3425/v1")),
            "gateway alias"
        );
    }
    for url in [
        "http://localhost:11434/v1",
        "http://127.0.0.1:1234/v1",
        "http://192.0.2.1:3425/v1",
        "https://relay.example:3425/v1",
    ] {
        assert!(!gateways.matches_url(url), "independent model service");
    }
}

#[test]
fn provider_import_custom_gateway_ports_are_blocked_across_sources_and_on_confirmation(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    let db = Database::memory()?;
    write(
        &paths.provider_backup.with_file_name("settings.json"),
        json!({"port":43121}),
    )?;
    write(
        &paths.provider_backup,
        json!({"providers":[
            {"id":"saved-gateway","chat":"http://localhost:43122/v1","key":"fixture-key"},
            {"id":"local-model","chat":"http://localhost:11434/v1","key":"fixture-key"}
        ]}),
    )?;
    std::fs::create_dir_all(paths.cc_switch.parent().ok_or("parent")?)?;
    let cc = rusqlite::Connection::open(&paths.cc_switch)?;
    cc.execute_batch("CREATE TABLE providers(id TEXT,app_type TEXT,name TEXT,settings_config TEXT); CREATE TABLE proxy_config(listen_port INTEGER); INSERT INTO proxy_config VALUES (43122);")?;
    cc.execute("INSERT INTO providers VALUES ('saved-relay','claude','Gateway fixture',?1)", [json!({"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:43121","ANTHROPIC_AUTH_TOKEN":"fixture-key"}}).to_string()])?;
    drop(cc);
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.2:43122","ANTHROPIC_AUTH_TOKEN":"fixture-key"}}),
    )?;
    std::fs::create_dir_all(paths.codex.parent().ok_or("parent")?)?;
    std::fs::write(&paths.codex, "[model_providers.saved]\nbase_url='http://[::1]:43121/v1'\nexperimental_bearer_token='fixture-key'\n")?;
    write(
        &paths.opencode,
        json!({"provider":{"saved":{"options":{"baseURL":"http://localhost:43121/v1","apiKey":"fixture-key"}}}}),
    )?;
    let blocked = readers::provider_backup(&paths.provider_backup)?.remove(0);
    let sources = preview(&db, &paths)?;
    for source in &sources[..4] {
        assert!(source.error.is_none());
        assert!(
            source.items.is_empty(),
            "gateway excluded from {}",
            source.id
        );
    }
    assert_eq!(sources[4].items.len(), 1);
    let local = &sources[4].items[0];
    assert_eq!(local.reference, "1/local-model");
    assert!(local.skip.is_none());
    let pick = |item: &Item| Pick {
        source: "magpie".into(),
        reference: item.reference.clone(),
        fingerprint: item.fingerprint.clone(),
        mode: ImportMode::Add,
        name: None,
    };
    let blocked_pick = Pick {
        source: "magpie".into(),
        reference: blocked.reference.clone(),
        fingerprint: fingerprint(&blocked)?,
        mode: ImportMode::Add,
        name: None,
    };
    assert_eq!(
        import(&db, &paths, vec![pick(local), blocked_pick]),
        Err(CONFLICT.into())
    );
    assert!(db.list_routing_upstreams()?.is_empty());
    assert_eq!(import(&db, &paths, vec![pick(local)])?.added, 1);
    Ok(())
}

#[test]
fn provider_import_recognizes_generated_routes_on_arbitrary_ports_and_secondary_endpoints(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.opencode,
        json!({"provider":{
            "magpie":{"options":{"baseURL":"http://localhost:43123/v1","apiKey":"fixture-key"}},
            "cc-switch":{"options":{"baseURL":"http://127.0.0.1:43124/v1","apiKey":"fixture-key"}},
            "custom":{"options":{"baseURL":"http://127.0.0.1:43125/v1","apiKey":"PROXY_MANAGED"}},
            "model":{"options":{"baseURL":"http://localhost:1234/v1","apiKey":"fixture-key"}}
        }}),
    )?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","responses":"http://127.0.0.1:3425/v1","key":"fixture-key"}]}),
    )?;
    let sources = preview(&Database::memory()?, &paths)?;
    assert_eq!(sources[3].items.len(), 1);
    assert_eq!(sources[3].items[0].reference, "model");
    assert!(sources[3].items[0].skip.is_none());
    assert!(sources[4].items.is_empty());
    Ok(())
}

#[test]
fn provider_import_rechecks_gateway_port_changes_before_writing(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    let db = Database::memory()?;
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"http://localhost:43126","ANTHROPIC_AUTH_TOKEN":"fixture-key"}}),
    )?;
    let item = preview(&db, &paths)?.remove(1).items.remove(0);
    assert!(item.skip.is_none());
    write(
        &paths.provider_backup.with_file_name("settings.json"),
        json!({"port":43126}),
    )?;
    assert_eq!(
        import(
            &db,
            &paths,
            vec![Pick {
                source: "claude-code".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None
            }]
        ),
        Err(CONFLICT.into())
    );
    assert!(db.list_routing_upstreams()?.is_empty());
    Ok(())
}

#[test]
fn provider_import_excludes_takeover_markers_across_native_and_saved_clients(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"http://localhost:43127","ANTHROPIC_AUTH_TOKEN":(["magpie", "claude"].join("-"))}}),
    )?;
    std::fs::create_dir_all(paths.codex.parent().ok_or("parent")?)?;
    std::fs::write(
        &paths.codex,
        "[model_providers.varyai]\nbase_url='http://127.0.0.1:43128/v1'\nexperimental_bearer_token='fixture-key'\n",
    )?;
    std::fs::create_dir_all(paths.cc_switch.parent().ok_or("parent")?)?;
    let conn = rusqlite::Connection::open(&paths.cc_switch)?;
    conn.execute_batch(
        "CREATE TABLE providers(id TEXT,app_type TEXT,name TEXT,settings_config TEXT);",
    )?;
    conn.execute("INSERT INTO providers VALUES ('saved','codex','Saved route',?1)", [json!({"config":"model_provider='magpie'\n[model_providers.magpie]\nbase_url='http://localhost:43129/v1'\nexperimental_bearer_token='fixture-key'"}).to_string()])?;
    drop(conn);
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"magpie","chat":"https://relay.example/v1","key":"fixture-key"}]}),
    )?;
    let sources = preview(&Database::memory()?, &paths)?;
    for source in &sources[..3] {
        assert!(source.error.is_none());
        assert!(
            source.items.is_empty(),
            "generated local route excluded from {}",
            source.id
        );
    }
    assert_eq!(sources[4].items.len(), 1);
    assert!(sources[4].items[0].skip.is_none());
    Ok(())
}

#[tokio::test]
async fn provider_import_rechecks_current_application_listener_on_confirmation(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    write(
        &paths.claude,
        json!({"env":{"ANTHROPIC_BASE_URL":"http://localhost:43130","ANTHROPIC_AUTH_TOKEN":"fixture-key"}}),
    )?;
    let before = std::fs::read(&paths.claude)?;
    let db = Arc::new(Database::memory()?);
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("managed")));
    let item = preview_with_accounts(db.clone(), manager.clone(), paths.clone())
        .await?
        .remove(1)
        .items
        .remove(0);
    assert!(item.skip.is_none());
    let mut config = db.get_global_proxy_config().await?;
    config.listen_port = 43130;
    db.update_global_proxy_config(config).await?;
    let source = preview_with_accounts(db.clone(), manager.clone(), paths.clone())
        .await?
        .remove(1);
    assert!(source.error.is_none());
    assert!(source.items.is_empty());
    assert_eq!(
        import_with_accounts(
            db.clone(),
            manager.clone(),
            paths,
            vec![Pick {
                source: "claude-code".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None,
            }]
        )
        .await,
        Err(CONFLICT.into())
    );
    assert!(db.list_routing_upstreams()?.is_empty());
    assert!(manager.list_accounts().await.is_empty());
    assert!(std::fs::read(dir.path().join(".claude/settings.json"))? == before);
    Ok(())
}

#[tokio::test]
async fn provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        proxy::providers::codex_oauth_auth::CodexOAuthManager,
        services::intent_routing::{effort::Capability, menu},
    };
    use std::sync::Arc;
    let (dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{
            "id":"relay", "name":"Synthetic", "chat":"https://relay.example/v1", "key":"synthetic",
            "models":["vendor/exact","quiet","toggle"], "pickedFrom":["vendor/exact","quiet","toggle","unpicked"],
            "modelPrefs":{"toggle":{"efforts":[],"reasonless":false}}
        }]}),
    )?;
    let settings_path = paths.provider_backup.with_file_name("settings.json");
    write(
        &settings_path,
        json!({
            "modelEfforts":{"relay/vendor/exact":["max","low","high","high"], "relay/unpicked":["high"], "other/vendor/exact":["medium"]},
            "modelReasonless":{"relay/quiet":true},
            "modelNames":{"relay/vendor/exact":"Custom"}, "modelImages":{"relay/vendor/exact":false}
        }),
    )?;
    let before = [
        std::fs::read(&paths.provider_backup)?,
        std::fs::read(&settings_path)?,
    ];
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    assert_eq!(
        import(
            &db,
            &paths,
            vec![Pick {
                source: "magpie".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None
            }]
        )?
        .added,
        1
    );
    let saved = db.list_routing_upstreams()?.remove(0);
    let mut provider = db.routing_discovery_provider(&saved.id, saved.revision)?;
    crate::services::routing_upstreams::filter_models(&mut provider);
    assert_eq!(
        Capability::for_provider(&provider, "vendor/exact").levels,
        ["low", "high", "max"]
    );
    assert!(Capability::for_provider(&provider, "quiet").reasonless);
    assert!(!Capability::for_provider(&provider, "toggle").reasonless);
    let UpstreamSource::Api { catalog, .. } = &saved.source else {
        return Err("API fixture".into());
    };
    assert_eq!(
        catalog
            .iter()
            .find(|row| row["model"] == "unpicked")
            .ok_or("missing vendor model")?["modelPreferences"]["reasoningLevels"],
        json!(["high"])
    );
    let manager = Arc::new(CodexOAuthManager::new(dir.path().join("accounts")));
    let menu = menu::prepare(manager, vec![provider], vec![], None).await?;
    let models = crate::services::agent_connections::models(&menu);
    let exact = models
        .iter()
        .find(|model| model.name == "Custom" && model.group == "Synthetic")
        .ok_or("missing imported model")?;
    let session = menu.groups.get(&exact.id).ok_or("missing imported route")?;
    assert!(session
        .targets
        .iter()
        .any(|target| target.provider_id == saved.id && target.model == "vendor/exact"));
    assert_eq!(
        crate::services::agent_connections::pi_model(exact)["thinkingLevelMap"]["max"],
        "max"
    );
    assert_eq!(
        crate::services::agent_connections::opencode_model(exact)["variants"]["max"]
            ["reasoningEffort"],
        "max"
    );
    assert_eq!(
        before,
        [
            std::fs::read(&paths.provider_backup)?,
            std::fs::read(&settings_path)?
        ]
    );
    Ok(())
}

#[test]
fn codex_import_keeps_catalog_levels_when_the_selected_model_is_also_listed(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    std::fs::create_dir_all(paths.codex.parent().ok_or("parent")?)?;
    std::fs::write(&paths.codex,"model_provider='relay'\nmodel='exact'\nmodel_reasoning_effort='max'\nmodel_catalog_json='catalog.json'\n[model_providers.relay]\nbase_url='https://relay.example/v1'\nexperimental_bearer_token='synthetic'\n")?;
    write(
        &paths.codex.with_file_name("catalog.json"),
        json!({"models":[
            {"slug":"exact","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"max"}],"default_reasoning_level":"high"},
            {"slug":"hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"high"}]}
        ]}),
    )?;
    let candidate = readers::codex(&paths.codex, &paths.home)?.remove(0);
    let UpstreamSource::Api { models, .. } = candidate.record.source else {
        return Err("API fixture".into());
    };
    assert_eq!(models.len(), 1);
    let capability = crate::services::intent_routing::effort::Capability::parse(&models[0]);
    assert_eq!(capability.levels, ["low", "high", "max"]);
    assert_eq!(models[0]["defaultReasoningLevel"], "max");
    assert_eq!(capability.default_level.as_deref(), Some("max"));
    Ok(())
}

#[test]
fn provider_backup_keeps_vendor_levels_separate_from_manual_reasoning_preferences(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{
            "id":"relay","chat":"https://relay.example/v1","key":"synthetic",
            "models":[{"model":"exact","reasoningLevels":["low","high","max"]}],
        "pickedFrom":["exact"],
            "modelPrefs":{"exact":{"efforts":["high"],"reasonless":false}}
        }]}),
    )?;
    let source = readers::provider_backup(&paths.provider_backup)?
        .remove(0)
        .record
        .source;
    let UpstreamSource::Api {
        models, catalog, ..
    } = source
    else {
        return Err("API fixture".into());
    };
    assert_eq!(models[0]["reasoningLevels"], json!(["low", "high", "max"]));
    assert_eq!(
        models[0]["modelPreferences"]["reasoningLevels"],
        json!(["high"])
    );
    assert_eq!(
        crate::services::intent_routing::effort::Capability::parse(&models[0]).levels,
        ["high"]
    );
    assert_eq!(catalog[0]["reasoningLevels"], json!(["low", "high", "max"]));
    Ok(())
}

#[test]
fn invalid_reasoning_preferences_and_unreadable_sidecars_do_not_import_defaults(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","key":"synthetic","models":["exact"]}]}),
    )?;
    let sidecar = paths.provider_backup.with_file_name("settings.json");
    for setting in [
        json!({"modelEfforts":{"relay/exact":["high","unknown"]}}),
        json!({"modelEfforts":{"relay/exact":"high"}}),
        json!({"modelReasonless":{"relay/exact":"false"}}),
        json!([]),
    ] {
        write(&sidecar, setting)?;
        let db = Database::memory()?;
        let source = preview(&db, &paths)?.remove(4);
        assert!(source.error.is_some());
        assert!(source.items.is_empty());
        assert!(db.list_routing_upstreams()?.is_empty());
    }
    std::fs::write(&sidecar, b"{broken")?;
    assert!(preview(&Database::memory()?, &paths)?[4].error.is_some());
    Ok(())
}

#[tokio::test]
async fn imported_default_reasoning_reaches_codex_publication_without_restricting_choices(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        proxy::providers::codex_oauth_auth::CodexOAuthManager,
        services::intent_routing::{client, menu},
    };
    let (dir, paths) = fixture()?;
    write(
        &paths.opencode,
        json!({"provider":{"relay":{"options":{"baseURL":"https://relay.example/v1","apiKey":"synthetic"},"models":{
            "exact":{"options":{"reasoningEffort":"low"},"variants":{"low":{"reasoningEffort":"low"},"high":{"reasoningEffort":"high"},"max":{"reasoningEffort":"max"}}}
        }}}}),
    )?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(3).items.remove(0);
    import(
        &db,
        &paths,
        vec![Pick {
            source: "opencode".into(),
            reference: item.reference,
            fingerprint: item.fingerprint,
            mode: ImportMode::Add,
            name: None,
        }],
    )?;
    let record = db.list_routing_upstreams()?.remove(0);
    let mut provider = db.routing_discovery_provider(&record.id, record.revision)?;
    crate::services::routing_upstreams::filter_models(&mut provider);
    let manager = std::sync::Arc::new(CodexOAuthManager::new(dir.path().join("accounts")));
    let menu = menu::prepare(manager, vec![provider], vec![], None).await?;
    let alias = menu.default_model.as_deref().ok_or("missing model")?;
    assert_eq!(
        menu.groups[alias].model_entry(alias)["default_reasoning_level"],
        "low"
    );
    let catalog = client::menu_catalog(&menu, "")?;
    assert_eq!(catalog["models"][0]["default_reasoning_level"], "low");
    assert_eq!(
        catalog["models"][0]["supported_reasoning_levels"]
            .as_array()
            .ok_or("missing levels")?
            .len(),
        3
    );
    let models = crate::services::agent_connections::models(&menu);
    assert_eq!(models[0].default_reasoning_level.as_deref(), Some("low"));
    Ok(())
}

#[test]
fn opencode_import_reads_enabled_reasoning_variants_and_the_saved_default(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.opencode,
        json!({"provider":{"relay":{"options":{"baseURL":"https://relay.example/v1","apiKey":"synthetic"},"models":{
            "exact":{"reasoning":true,"options":{"reasoningEffort":"high"},"variants":{"fast":{"reasoningEffort":"low"},"deep":{"reasoningEffort":"max"},"off":{"reasoningEffort":"none","disabled":true}}},
            "toggle":{"reasoning":true}
        }}}}),
    )?;
    let candidate = readers::opencode(&paths)?.remove(0);
    let UpstreamSource::Api { models, .. } = candidate.record.source else {
        return Err("API fixture".into());
    };
    let exact = models
        .iter()
        .find(|row| row["model"] == "exact")
        .ok_or("missing model")?;
    assert_eq!(
        crate::services::intent_routing::effort::Capability::parse(exact).levels,
        ["low", "max"]
    );
    assert_eq!(exact["defaultReasoningLevel"], "high");
    let toggle = models
        .iter()
        .find(|row| row["model"] == "toggle")
        .ok_or("missing model")?;
    assert!(
        crate::services::intent_routing::effort::Capability::parse(toggle)
            .levels
            .is_empty()
    );
    Ok(())
}

#[test]
fn provider_backup_settings_change_invalidates_the_import_preview(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.provider_backup,
        json!({"providers":[{"id":"relay","chat":"https://relay.example/v1","key":"synthetic","models":["exact"]}]}),
    )?;
    let settings = paths.provider_backup.with_file_name("settings.json");
    write(&settings, json!({"modelEfforts":{"relay/exact":["low"]}}))?;
    let db = Database::memory()?;
    let item = preview(&db, &paths)?.remove(4).items.remove(0);
    write(&settings, json!({"modelEfforts":{"relay/exact":["high"]}}))?;
    assert_eq!(
        import(
            &db,
            &paths,
            vec![Pick {
                source: "magpie".into(),
                reference: item.reference,
                fingerprint: item.fingerprint,
                mode: ImportMode::Add,
                name: None
            }]
        ),
        Err(CONFLICT.into())
    );
    assert!(db.list_routing_upstreams()?.is_empty());
    Ok(())
}

#[test]
fn claude_import_reads_capability_flags_and_selected_effort_without_guessing_other_models(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, paths) = fixture()?;
    write(
        &paths.claude,
        json!({"effortLevel":"max","env":{
            "ANTHROPIC_BASE_URL":"https://relay.example", "ANTHROPIC_AUTH_TOKEN":"synthetic", "ANTHROPIC_MODEL":"exact", "ANTHROPIC_DEFAULT_HAIKU_MODEL":"unknown",
            "CLAUDE_CODE_MODEL_CAPABILITIES":"exact=effort,xhigh_effort,max_effort"
        }}),
    )?;
    let candidate = readers::claude(&paths.claude)?.remove(0);
    let UpstreamSource::Api { models, .. } = candidate.record.source else {
        return Err("API fixture".into());
    };
    assert_eq!(
        models[0]["reasoningLevels"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(models[0]["defaultReasoningLevel"], "max");
    assert!(models[1].get("reasoningLevels").is_none());
    Ok(())
}

#[test]
fn cc_switch_import_preserves_stored_model_capabilities_and_manual_overrides(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _paths) = fixture()?;
    let path = dir.path().join("config.json");
    write(
        &path,
        json!({"codex":{"providers":{"relay":{"name":"Synthetic","settingsConfig":{
        "config":"model_provider='relay'\nmodel='exact'\nmodel_reasoning_effort='max'\n[model_providers.relay]\nbase_url='https://relay.example/v1'",
            "auth":{"OPENAI_API_KEY":"synthetic"},
            "modelCatalog":{"models":[{"model":"exact","reasoningLevels":["low","high","max"],"defaultReasoningLevel":"low","modelPreferences":{"reasoningLevels":["low","high"]}}]}
        }}}}}),
    )?;
    let candidate = readers::cc_switch(&path)?.remove(0);
    let UpstreamSource::Api { models, .. } = candidate.record.source else {
        return Err("API fixture".into());
    };
    assert_eq!(models[0]["reasoningLevels"], json!(["low", "high", "max"]));
    assert_eq!(
        models[0]["modelPreferences"]["reasoningLevels"],
        json!(["low", "high"])
    );
    assert_eq!(models[0]["defaultReasoningLevel"], "max");
    Ok(())
}

#[test]
fn joining_imported_keys_fills_missing_capabilities_and_preserves_local_preferences(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut existing = candidate(
        "first".into(),
        "fixture",
        "Synthetic".into(),
        "https://relay.example/v1".into(),
        "synthetic".into(),
        "openai_chat",
        vec![json!({"model":"exact","modelPreferences":{"reasoningLevels":[]}})],
    )
    .record;
    let incoming=candidate("second".into(),"fixture","Synthetic".into(),"https://relay.example/v1".into(),"synthetic-other".into(),"openai_chat",vec![json!({"model":"exact","reasoningLevels":["low","high"],"defaultReasoningLevel":"low","modelPreferences":{"reasoningLevels":["high"]}})]).record;
    join_keys(&mut existing, &incoming)?;
    let UpstreamSource::Api { models, .. } = existing.source else {
        return Err("API fixture".into());
    };
    assert_eq!(models[0]["reasoningLevels"], json!(["low", "high"]));
    assert_eq!(models[0]["defaultReasoningLevel"], "low");
    assert_eq!(models[0]["modelPreferences"]["reasoningLevels"], json!([]));
    Ok(())
}

#[path = "identity_tests.rs"]
mod identity_tests;
