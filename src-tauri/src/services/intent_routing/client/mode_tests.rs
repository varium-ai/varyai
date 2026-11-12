use super::*;

const BASE: &str = "http://127.0.0.1:1234/intent/v1";
const NATIVE: &str = "http://127.0.0.1:1234/backend-api/codex";

fn thread_fixture(
    dir: &Path,
    id: &str,
    provider: &str,
    quiet: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let sessions = dir.join("sessions");
    fs::create_dir_all(&sessions)?;
    let path = sessions.join(format!("rollout-{id}.jsonl"));
    fs::write(
        &path,
        format!(
            "{}\n{}\n{{\"type\":\"event_msg\",\"payload\":{{\"message\":\"synthetic\"}}}}\n",
            json!({"type":"session_meta","payload":{"id":id,"model_provider":provider}}),
            json!({"type":"turn_context","payload":{"model":tests::group().alias()}})
        ),
    )?;
    if quiet {
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(120),
            ))?;
    }
    let db = rusqlite::Connection::open(dir.join("state_5.sqlite"))?;
    db.execute("CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL, model TEXT)", [])?;
    db.execute(
        "INSERT INTO threads (id, rollout_path, model_provider, model) VALUES (?1,?2,?3,?4)",
        rusqlite::params![id, path.to_string_lossy(), provider, tests::group().alias()],
    )?;
    Ok(path)
}

fn thread_provider(dir: &Path, id: &str) -> Result<String, rusqlite::Error> {
    rusqlite::Connection::open(dir.join("state_5.sqlite"))?.query_row(
        "SELECT model_provider FROM threads WHERE id=?1",
        [id],
        |row| row.get(0),
    )
}

#[test]
fn standalone_connection_moves_old_and_new_chats_back_on_disconnect(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    thread_fixture(dir.path(), "old-chat", "openai", true)?;
    thread_fixture(dir.path(), "unrelated", "other-provider", true)?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "old-chat")?, PROVIDER);
    assert_eq!(thread_provider(dir.path(), "unrelated")?, "other-provider");
    thread_fixture(dir.path(), "new-chat", PROVIDER, true)?;
    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "old-chat")?, "openai");
    assert_eq!(thread_provider(dir.path(), "new-chat")?, "openai");
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "old-chat")?, PROVIDER);
    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "old-chat")?, "openai");
    Ok(())
}

#[test]
fn magpie_paginated_chats_follow_takeover_without_rewriting_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        "model_provider='magpie'\nmodel='codex/synthetic'\n",
    )?;
    let path = thread_fixture(dir.path(), "paged-magpie", "magpie", true)?;
    let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?;
    db.execute(
        "ALTER TABLE threads ADD COLUMN history_mode TEXT NOT NULL DEFAULT 'legacy'",
        [],
    )?;
    db.execute("UPDATE threads SET history_mode='paginated'", [])?;
    // Paginated segments can contain just a suffix, with no session_meta.
    fs::write(
        &path,
        "{\"type\":\"event_msg\",\"payload\":{\"message\":\"synthetic suffix\"}}\n",
    )?;
    let quiet = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_times(fs::FileTimes::new().set_modified(quiet))?;
    let before = fs::read(&path)?;
    thread_fixture(dir.path(), "unrelated-paged", "other-provider", true)?;

    for _ in 0..2 {
        manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
        let installed: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(installed["model_provider"].as_str(), Some("varyai"));
        assert_eq!(thread_provider(dir.path(), "paged-magpie")?, PROVIDER);
        assert!(!manager.thread_sync_pending());
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(fs::metadata(&path)?.modified()?, quiet);
        manager.restore()?;
        assert_eq!(thread_provider(dir.path(), "paged-magpie")?, "magpie");
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(
            thread_provider(dir.path(), "unrelated-paged")?,
            "other-provider"
        );
    }
    Ok(())
}

pub(super) fn thread_model_fixture(
    dir: &Path,
    id: &str,
    provider: &str,
    paginated: bool,
    database_model: Option<&str>,
    latest_turn_model: Option<&str>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let db = rusqlite::Connection::open(dir.join("state_5.sqlite"))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            model TEXT,
            history_mode TEXT NOT NULL DEFAULT 'legacy'
        )",
    )?;
    let path = thread_fixture(dir, id, provider, true)?;
    db.execute(
        "UPDATE threads SET model=?1, history_mode=?2 WHERE id=?3",
        rusqlite::params![
            database_model,
            if paginated { "paginated" } else { "legacy" },
            id
        ],
    )?;
    let contents = if paginated {
        // The paginated row is authoritative; a local suffix has no model metadata.
        format!(
            "{}\n",
            json!({"type":"event_msg","payload":{"message":"fixture suffix"}})
        )
    } else {
        let mut contents = format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":id,"model_provider":provider}})
        );
        if let Some(model) = latest_turn_model {
            contents.push_str(&format!(
                "{}\n",
                json!({"type":"turn_context","payload":{"model":model}})
            ));
        }
        contents
    };
    fs::write(&path, contents)?;
    fs::File::options().write(true).open(&path)?.set_times(
        fs::FileTimes::new()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)),
    )?;
    Ok(path)
}

#[test]
fn standalone_connection_migrates_legacy_providers_without_changing_saved_models(
) -> Result<(), Box<dyn std::error::Error>> {
    let alias = tests::group().alias();
    let unpublished = "codex/fixture-native-model";
    for (database_model, latest_turn_model) in [
        (unpublished, unpublished),
        (alias.as_str(), unpublished),
        (unpublished, alias.as_str()),
    ] {
        let (dir, manager) = tests::setup();
        fs::write(
            &manager.config,
            "model_provider='magpie'\nmodel='codex/fixture-native-model'\n",
        )?;
        let path = thread_model_fixture(
            dir.path(),
            "unpublished-thread",
            "magpie",
            false,
            Some(database_model),
            Some(latest_turn_model),
        )?;
        thread_model_fixture(
            dir.path(),
            "compatible-thread",
            "magpie",
            false,
            Some(&alias),
            Some(&alias),
        )?;
        let before = fs::read_to_string(&path)?;
        let expected = before.replace(
            "\"model_provider\":\"magpie\"",
            "\"model_provider\":\"varyai\"",
        );
        for _ in 0..2 {
            manager.write_projection(&[tests::group()], "fixture-local-token", BASE)?;
            manager.sync_threads()?;
            let installed: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
            assert_eq!(installed["model_provider"].as_str(), Some(PROVIDER));
            assert_eq!(
                installed["model"].as_str(),
                Some("Development-routing-group")
            );
            assert_eq!(manager.selected_model()?.as_deref(), Some(alias.as_str()));
            assert_eq!(
                thread_provider(dir.path(), "unpublished-thread")?,
                PROVIDER,
                "provider migration is independent of saved-model availability"
            );
            assert_eq!(thread_provider(dir.path(), "compatible-thread")?, PROVIDER);
            assert_eq!(fs::read_to_string(&path)?, expected);
            let model: String = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?
                .query_row(
                    "SELECT model FROM threads WHERE id='unpublished-thread'",
                    [],
                    |row| row.get(0),
                )?;
            assert_eq!(model, database_model);
            assert!(!manager.thread_sync_pending());
        }
    }
    Ok(())
}

#[test]
fn standalone_connection_migrates_paginated_providers_without_changing_saved_models(
) -> Result<(), Box<dyn std::error::Error>> {
    let alias = tests::group().alias();
    for database_model in [Some("codex/fixture-native-model"), Some(""), None] {
        let (dir, manager) = tests::setup();
        fs::write(
            &manager.config,
            "model_provider='magpie'\nmodel='codex/fixture-native-model'\n",
        )?;
        let path = thread_model_fixture(
            dir.path(),
            "unpublished-paged",
            "magpie",
            true,
            database_model,
            None,
        )?;
        thread_model_fixture(
            dir.path(),
            "compatible-paged",
            "magpie",
            true,
            Some(&alias),
            None,
        )?;
        let before = fs::read(&path)?;
        let modified = fs::metadata(&path)?.modified()?;
        for _ in 0..2 {
            manager.write_projection(&[tests::group()], "fixture-local-token", BASE)?;
            manager.sync_threads()?;
            assert_eq!(
                thread_provider(dir.path(), "unpublished-paged")?,
                PROVIDER,
                "paginated provider migration preserves even an unavailable saved model"
            );
            assert_eq!(thread_provider(dir.path(), "compatible-paged")?, PROVIDER);
            assert_eq!(fs::read(&path)?, before);
            assert_eq!(fs::metadata(&path)?.modified()?, modified);
            let model: Option<String> =
                rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?.query_row(
                    "SELECT model FROM threads WHERE id='unpublished-paged'",
                    [],
                    |row| row.get(0),
                )?;
            assert_eq!(model.as_deref(), database_model);
            assert!(!manager.thread_sync_pending());
        }
    }
    Ok(())
}

#[test]
fn selected_profile_without_provider_override_inherits_top_level_for_threads(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        "model='original'\nmodel_provider='openai'\nprofile='work'\n[profiles.work]\nmodel='profile-model'\n[profiles.other]\nmodel_provider='unrelated-provider'\n",
    )?;
    thread_fixture(dir.path(), "profile-inherited", "openai", true)?;

    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "profile-inherited")?, PROVIDER);
    assert!(!manager.thread_sync_pending());

    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "profile-inherited")?, "openai");
    Ok(())
}

#[test]
fn selected_profile_provider_override_keeps_its_thread_bucket(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        "model='original'\nmodel_provider='openai'\nprofile='work'\n[profiles.work]\nmodel_provider='custom'\n[model_providers.custom]\nname='Synthetic relay'\nbase_url='https://synthetic.example/v1'\n",
    )?;
    thread_fixture(dir.path(), "profile-owned", "custom", true)?;

    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "profile-owned")?, "custom");
    assert!(!manager.thread_sync_pending());
    assert_eq!(
        manager.status()?.configuration_issue,
        Some(ConfigurationIssue::ProfileOverride)
    );

    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "profile-owned")?, "custom");
    Ok(())
}

#[test]
fn provider_named_by_an_inactive_profile_keeps_its_thread_bucket(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        "model='original'\nmodel_provider='custom'\n[model_providers.custom]\nname='Synthetic relay'\nbase_url='https://synthetic.example/v1'\n[profiles.relay]\nmodel_provider='custom'\n",
    )?;
    thread_fixture(dir.path(), "profile-named", "custom", true)?;

    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "profile-named")?, "custom");
    assert!(!manager.thread_sync_pending());

    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "profile-named")?, "custom");
    Ok(())
}

#[test]
fn adopted_top_level_routing_uses_openai_as_thread_source() -> Result<(), Box<dyn std::error::Error>>
{
    let (dir, manager) = tests::setup();
    let bearer = "a".repeat(16);
    let config = format!(
        "model='original'\nmodel_provider='varyai'\n[model_providers.varyai]\nname='VaryAI models and routing groups'\nbase_url='http://127.0.0.1:1234/intent/v1'\nwire_api='responses'\nrequires_openai_auth=false\nsupports_websockets=false\nsupports_standalone_web_search=false\nexperimental_bearer_token='{bearer}'\n"
    );
    fs::write(&manager.config, config)?;
    thread_fixture(dir.path(), "adopted-routing", "openai", true)?;

    manager.write_projection_inner(
        &[tests::group()],
        "synthetic-local-token",
        BASE,
        None,
        true,
        None,
    )?;
    assert_eq!(thread_provider(dir.path(), "adopted-routing")?, PROVIDER);
    assert!(!manager.thread_sync_pending());
    Ok(())
}

#[test]
fn active_chat_sync_retries_in_background_without_a_configuration_warning(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    let path = thread_fixture(dir.path(), "active-chat", "openai", false)?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "active-chat")?, "openai");
    assert!(manager.thread_sync_pending());
    assert_eq!(manager.status()?.configuration_issue, None);
    fs::File::options().write(true).open(&path)?.set_times(
        fs::FileTimes::new()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)),
    )?;
    manager.sync_configuration(BASE)?;
    assert_eq!(thread_provider(dir.path(), "active-chat")?, PROVIDER);
    assert!(!manager.thread_sync_pending());
    assert_eq!(manager.status()?.configuration_issue, None);
    Ok(())
}

#[test]
fn chats_follow_repeated_native_and_standalone_transitions(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    thread_fixture(dir.path(), "original-chat", "openai", true)?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    thread_fixture(dir.path(), "created-chat", PROVIDER, true)?;
    for _ in 0..2 {
        sign_in(&manager)?;
        manager.write_projection_with_native(
            &[tests::group()],
            "synthetic-local",
            BASE,
            Some(NATIVE),
        )?;
        assert_eq!(thread_provider(dir.path(), "original-chat")?, "openai");
        assert_eq!(thread_provider(dir.path(), "created-chat")?, "openai");
        fs::remove_file(dir.path().join("auth.json"))?;
        manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
        assert_eq!(thread_provider(dir.path(), "original-chat")?, PROVIDER);
        assert_eq!(thread_provider(dir.path(), "created-chat")?, PROVIDER);
    }
    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "created-chat")?, "openai");
    Ok(())
}

#[test]
fn explicit_reapply_replaces_an_existing_native_url_and_restores_it_on_disconnect(
) -> Result<(), Box<dyn std::error::Error>> {
    for previous_url in [
        "http://127.0.0.1:23456/backend-api/codex",
        "https://synthetic.example/backend-api/codex",
    ] {
        let (dir, manager) = tests::setup();
        let original = format!("model='original'\nopenai_base_url='{previous_url}'\n");
        fs::write(&manager.config, &original)?;
        sign_in(&manager)?;
        let auth_before = fs::read(dir.path().join("auth.json"))?;
        assert_eq!(
            manager.write_projection_with_native(
                &[tests::group()],
                "synthetic-local",
                BASE,
                Some(NATIVE)
            ),
            Err("intentRouting.nativeConflict".into())
        );
        manager.write_projection_for_intent(
            &[tests::group()],
            "synthetic-local",
            BASE,
            Some(NATIVE),
            ApplyIntent::Reapply,
            None,
        )?;
        let applied: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(applied["openai_base_url"].as_str(), Some(NATIVE));
        assert_eq!(manager.native_owned_url()?.as_deref(), Some(NATIVE));
        manager.write_projection_with_native(
            &[tests::group()],
            "synthetic-local",
            BASE,
            Some(NATIVE),
        )?;
        assert_eq!(manager.native_owned_url()?.as_deref(), Some(NATIVE));
        assert_eq!(fs::read(dir.path().join("auth.json"))?, auth_before);
        manager.restore()?;
        tests::assert_restored_config(&manager, &original);
    }
    Ok(())
}

#[test]
fn explicit_reapply_recovers_a_pending_restore_and_preserves_external_choices(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(&manager.config, "model='original'\n")?;
    thread_fixture(dir.path(), "old-quiet-chat", "openai", true)?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    let old_receipt = manager.load()?.ok_or("fixture receipt")?;
    let old_recovery = old_receipt
        .thread_recovery_id
        .clone()
        .ok_or("fixture recovery")?;
    let active = thread_fixture(dir.path(), "active-reapply-chat", PROVIDER, false)?;
    let history = fs::read(&active)?;
    manager.restore()?;
    assert!(
        manager
            .load()?
            .ok_or("pending receipt")?
            .thread_restore_pending
    );
    let mut edited: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    edited["model_provider"] = toml_edit::value("external");
    edited["model"] = toml_edit::value("external-choice");
    edited["theme"] = toml_edit::value("keep-external");
    fs::write(&manager.config, edited.to_string())?;
    assert_eq!(
        manager.write_projection(&[tests::group()], "synthetic-local", BASE),
        Err(CONFLICT.into())
    );

    manager.write_projection_for_intent(
        &[tests::group()],
        "synthetic-renewed",
        BASE,
        None,
        ApplyIntent::Reapply,
        None,
    )?;
    assert!(manager.status()?.configuration_owned);
    assert!(!manager.load()?.ok_or("new receipt")?.thread_restore_pending);
    assert_eq!(fs::read(&active)?, history);
    assert_eq!(thread_provider(dir.path(), "old-quiet-chat")?, PROVIDER);
    let archive = fs::read_dir(dir.path())?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("codex-reapply-recovery-")
        })
        .ok_or("missing archived recovery")?;
    let archived: Receipt = serde_json::from_slice(&fs::read(archive.path())?)?;
    assert_eq!(
        archived.thread_recovery_id.as_deref(),
        Some(old_recovery.as_str())
    );
    assert!(dir
        .path()
        .join("codex-thread-recovery")
        .join(old_recovery)
        .exists());

    fs::OpenOptions::new()
        .write(true)
        .open(&active)?
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)),
        )?;
    manager.restore()?;
    assert_eq!(
        thread_provider(dir.path(), "active-reapply-chat")?,
        "external"
    );
    assert_eq!(thread_provider(dir.path(), "old-quiet-chat")?, "external");
    let restored: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(restored["model"].as_str(), Some("external-choice"));
    assert_eq!(restored["model_provider"].as_str(), Some("external"));
    assert_eq!(restored["theme"].as_str(), Some("keep-external"));
    assert!(manager.thread_sync_pending());
    let mut openai: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    openai["model_provider"] = toml_edit::value("openai");
    fs::write(&manager.config, openai.to_string())?;
    manager.sync_threads()?;
    assert_eq!(thread_provider(dir.path(), "old-quiet-chat")?, "openai");
    assert_eq!(
        thread_provider(dir.path(), "active-reapply-chat")?,
        "external"
    );
    Ok(())
}

#[test]
fn disconnect_keeps_recovery_and_provider_until_active_chat_can_move(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(&manager.config, "model='original'\n")?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    let path = thread_fixture(dir.path(), "active-created-chat", PROVIDER, false)?;
    manager.restore()?;
    assert!(manager.receipt.exists());
    assert!(manager.thread_sync_pending());
    let restored: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert!(restored["model_providers"].get(PROVIDER).is_some());
    fs::File::options().write(true).open(&path)?.set_times(
        fs::FileTimes::new()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)),
    )?;
    manager.sync_threads()?;
    assert_eq!(
        thread_provider(dir.path(), "active-created-chat")?,
        "openai"
    );
    assert!(!manager.receipt.exists());
    let restored: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(restored["model"].as_str(), Some("original"));
    assert!(restored["model_providers"].get(PROVIDER).is_some());
    Ok(())
}

fn sign_in(manager: &Manager) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        manager
            .config
            .parent()
            .ok_or("fixture home")?
            .join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"synthetic-access"}}"#,
    )?;
    Ok(())
}

#[test]
fn chatgpt_connection_preserves_openai_provider_and_remote_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    for original in [
        "model='native-model'\n",
        "model='native-model'\nmodel_provider='openai'\n",
    ] {
        let (_dir, manager) = tests::setup();
        fs::write(&manager.config, original)?;
        sign_in(&manager)?;
        for _ in 0..2 {
            manager.write_projection_with_native(
                &[tests::group()],
                "synthetic-local",
                BASE,
                Some(NATIVE),
            )?;
            let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
            assert_eq!(
                doc.get("model_provider")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("openai"),
                "openai"
            );
            assert!(doc.get("model_catalog_json").is_none());
            assert_eq!(doc["model"].as_str(), Some("native-model"));
            assert!(doc.get("web_search").is_none());
            assert_eq!(doc["openai_base_url"].as_str(), Some(NATIVE));
            assert!(doc["model_providers"].get(PROVIDER).is_some());
            assert!(manager.status()?.configuration_owned);
        }
        manager.restore()?;
        tests::assert_restored_config(&manager, original);
    }
    Ok(())
}

#[test]
fn user_provider_and_api_only_sign_in_keep_standalone_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    for original in [
        "model_provider='custom'\n[model_providers.custom]\nname='Synthetic relay'\nbase_url='https://synthetic.example/v1'\n",
        "forced_login_method='api'\n",
    ] {
        let (_dir, manager) = tests::setup();
        fs::write(&manager.config, original)?;
        sign_in(&manager)?;
        manager.write_projection_with_native(&[tests::group()], "synthetic-local", BASE, Some(NATIVE))?;
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(doc["model_provider"].as_str(), Some(PROVIDER));
        assert!(doc.get("model_catalog_json").is_some());
    }
    let (dir, manager) = tests::setup();
    fs::write(
        dir.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"synthetic-api-key"}"#,
    )?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(
        doc["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
        Some(false)
    );
    Ok(())
}

#[test]
fn incomplete_auth_read_does_not_switch_an_existing_chatgpt_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    sign_in(&manager)?;
    manager.write_projection_with_native(
        &[tests::group()],
        "synthetic-local",
        BASE,
        Some(NATIVE),
    )?;
    fs::write(dir.path().join("auth.json"), "{\"tokens\":")?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(
        doc.get("model_provider")
            .and_then(toml::Value::as_str)
            .unwrap_or("openai"),
        "openai"
    );
    assert!(doc.get("model_catalog_json").is_none());
    assert_eq!(
        doc["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
        Some(true)
    );
    fs::remove_file(dir.path().join("auth.json"))?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(doc["model_provider"].as_str(), Some(PROVIDER));
    assert_eq!(
        doc["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
        Some(false)
    );
    Ok(())
}

#[test]
fn native_mode_detects_url_edits_and_legacy_takeover_can_join_openai(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, manager) = tests::setup();
    fs::write(&manager.config, "model='native-model'\n")?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    sign_in(&manager)?;
    manager.write_projection_with_native(
        &[tests::group()],
        "synthetic-local",
        BASE,
        Some(NATIVE),
    )?;
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    assert!(doc.get("model_provider").is_none());
    doc["openai_base_url"] = toml_edit::value("https://synthetic.example/changed");
    fs::write(&manager.config, doc.to_string())?;
    assert!(!manager.status()?.configuration_owned);
    manager.restore()?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(
        doc["openai_base_url"].as_str(),
        Some("https://synthetic.example/changed")
    );
    Ok(())
}

fn historical_varyai_provider(id: &str, base: &str) -> String {
    format!(
        "[model_providers.{id}]\nname='VaryAI models and routing groups'\nbase_url='{base}'\nwire_api='responses'\nrequires_openai_auth=false\nexperimental_bearer_token='synthetic-historical-token'\nsupports_websockets=false\nsupports_standalone_web_search=false\n"
    )
}

#[test]
fn historical_varyai_ids_join_the_current_provider_and_restore_each_origin(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    let original = format!(
        "model='original'\n{}{}",
        historical_varyai_provider("historical_a", "http://127.0.0.1:4321/v1"),
        historical_varyai_provider("historical_b", "http://127.0.0.1:4322/intent/v1"),
    );
    fs::write(&manager.config, &original)?;
    let first = thread_fixture(dir.path(), "historical-first", "historical_a", true)?;
    let second = thread_fixture(dir.path(), "historical-second", "historical_b", true)?;
    let before = [fs::read(&first)?, fs::read(&second)?];
    thread_fixture(dir.path(), "ordinary-openai", "openai", true)?;
    for _ in 0..2 {
        manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
        assert_eq!(thread_provider(dir.path(), "historical-first")?, PROVIDER);
        assert_eq!(thread_provider(dir.path(), "historical-second")?, PROVIDER);
        manager.sync_threads()?;
    }
    manager.restore()?;
    assert_eq!(
        thread_provider(dir.path(), "historical-first")?,
        "historical_a"
    );
    assert_eq!(
        thread_provider(dir.path(), "historical-second")?,
        "historical_b"
    );
    assert_eq!(thread_provider(dir.path(), "ordinary-openai")?, "openai");
    assert_eq!(fs::read(first)?, before[0]);
    assert_eq!(fs::read(second)?, before[1]);
    tests::assert_restored_config(&manager, &original);
    Ok(())
}

#[test]
fn historical_varyai_migration_checks_signatures_and_profiles_without_model_gates(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        format!(
            "model='original'\n[profiles.retained]\nmodel_provider='profile_provider'\n{}{}{}{}",
            historical_varyai_provider("eligible", "http://127.0.0.1:4321/v1"),
            historical_varyai_provider("unsupported", "http://127.0.0.1:4321/v1"),
            historical_varyai_provider("profile_provider", "http://127.0.0.1:4321/v1"),
            historical_varyai_provider("same_name", "https://synthetic.example/v1"),
        ),
    )?;
    for (id, provider) in [
        ("eligible-chat", "eligible"),
        ("unsupported-chat", "unsupported"),
        ("profile-chat", "profile_provider"),
        ("same-name-chat", "same_name"),
        ("unconfigured-chat", "unconfigured"),
    ] {
        let path = thread_fixture(dir.path(), id, provider, true)?;
        if provider == "unsupported" {
            let text =
                fs::read_to_string(&path)?.replace(&tests::group().alias(), "unpublished-model");
            fs::write(&path, text)?;
            fs::File::options().write(true).open(&path)?.set_times(
                fs::FileTimes::new().set_modified(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(120),
                ),
            )?;
            rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?.execute(
                "UPDATE threads SET model='unpublished-model' WHERE id=?1",
                [id],
            )?;
        }
    }
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "eligible-chat")?, PROVIDER);
    assert_eq!(thread_provider(dir.path(), "unsupported-chat")?, PROVIDER);
    let saved_model: String = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?
        .query_row(
            "SELECT model FROM threads WHERE id='unsupported-chat'",
            [],
            |row| row.get(0),
        )?;
    assert_eq!(saved_model, "unpublished-model");
    for (id, provider) in [
        ("profile-chat", "profile_provider"),
        ("same-name-chat", "same_name"),
        ("unconfigured-chat", "unconfigured"),
    ] {
        assert_eq!(thread_provider(dir.path(), id)?, provider);
    }
    assert!(!manager.thread_sync_pending());
    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "eligible-chat")?, "eligible");
    assert_eq!(
        thread_provider(dir.path(), "unsupported-chat")?,
        "unsupported"
    );
    Ok(())
}

#[test]
fn historical_varyai_busy_restore_retains_each_source_until_recovery(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        historical_varyai_provider("historical", "http://127.0.0.1:4321/v1"),
    )?;
    let path = thread_fixture(dir.path(), "busy-history", "historical", false)?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "busy-history")?, "historical");
    assert!(manager.thread_sync_pending());
    let quiet = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_times(fs::FileTimes::new().set_modified(quiet))?;
    manager.sync_threads()?;
    assert_eq!(thread_provider(dir.path(), "busy-history")?, PROVIDER);
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
    thread_fixture(dir.path(), "new-during-takeover", PROVIDER, true)?;
    manager.restore()?;
    assert!(manager.thread_sync_pending());
    assert!(manager.receipt.exists());
    assert_eq!(thread_provider(dir.path(), "busy-history")?, PROVIDER);
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_times(fs::FileTimes::new().set_modified(quiet))?;
    manager.sync_threads()?;
    assert_eq!(thread_provider(dir.path(), "busy-history")?, "historical");
    assert_eq!(
        thread_provider(dir.path(), "new-during-takeover")?,
        "openai"
    );
    assert!(!manager.receipt.exists());
    Ok(())
}

#[test]
fn historical_varyai_paginated_identity_changes_preserve_suffix_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        historical_varyai_provider("historical", "http://127.0.0.1:4321/v1"),
    )?;
    let path = thread_fixture(dir.path(), "paged-history", "historical", true)?;
    let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?;
    db.execute(
        "ALTER TABLE threads ADD COLUMN history_mode TEXT NOT NULL DEFAULT 'legacy'",
        [],
    )?;
    db.execute("UPDATE threads SET history_mode='paginated'", [])?;
    let suffix = b"{\"type\":\"event_msg\",\"payload\":{\"message\":\"synthetic suffix\"}}\n";
    fs::write(&path, suffix)?;
    fs::File::options().write(true).open(&path)?.set_times(
        fs::FileTimes::new()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)),
    )?;
    manager.write_projection(&[tests::group()], "synthetic-local", BASE)?;
    assert_eq!(thread_provider(dir.path(), "paged-history")?, PROVIDER);
    assert_eq!(fs::read(&path)?, suffix);
    manager.restore()?;
    assert_eq!(thread_provider(dir.path(), "paged-history")?, "historical");
    assert_eq!(fs::read(path)?, suffix);
    Ok(())
}
