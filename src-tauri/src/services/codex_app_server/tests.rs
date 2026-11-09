use super::*;
use serde_json::json;

fn server(pid: i32) -> Instance {
    Instance {
        pid,
        started_us: pid as u64 * 1_000_000,
    }
}

#[test]
fn route_change_waits_for_settling_and_running_turns() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    assert_eq!(m.observe([1; 32], old.clone(), false, 0), Action::None);
    assert_eq!(m.observe([2; 32], old.clone(), false, 1), Action::None);
    assert_eq!(m.observe([2; 32], old.clone(), false, 3), Action::None);
    assert_eq!(m.observe([2; 32], old.clone(), true, 8), Action::None);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 9),
        Action::Restart(old.clone())
    );
    assert_eq!(m.observe([2; 32], old, false, 10), Action::None);
    assert_eq!(
        m.observe([2; 32], vec![server(11)], false, 11),
        Action::Recovered
    );
    assert_eq!(
        m.observe([2; 32], vec![server(11)], false, 20),
        Action::None
    );
}

#[test]
fn another_tools_restart_is_adopted_without_terminating_its_replacement() {
    let mut m = Monitor::default();
    m.observe([1; 32], vec![server(10)], false, 0);
    m.observe([2; 32], vec![server(10)], false, 1);
    assert_eq!(
        m.observe([2; 32], vec![server(11)], false, 2),
        Action::Recovered
    );
    assert_eq!(
        m.observe([2; 32], vec![server(11)], false, 10),
        Action::None
    );
}

#[test]
fn rapid_switches_restart_only_the_final_route_and_timeout_is_bounded() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), false, 1);
    m.observe([3; 32], old.clone(), false, 3);
    assert_eq!(m.observe([3; 32], old.clone(), false, 5), Action::None);
    assert_eq!(
        m.observe([3; 32], old.clone(), false, 6),
        Action::Restart(old.clone())
    );
    assert_eq!(m.observe([3; 32], vec![], false, 7), Action::None);
    assert_eq!(m.observe([3; 32], vec![], false, 27), Action::TimedOut);
    assert_eq!(m.observe([3; 32], old, false, 40), Action::None);
}

#[test]
fn startup_and_pid_reuse_do_not_restart_a_new_server() {
    let mut m = Monitor::default();
    m.observe([1; 32], vec![server(10)], false, 0);
    let replacement = Instance {
        pid: 10,
        started_us: 99_000_000,
    };
    assert_eq!(
        m.observe([2; 32], vec![replacement.clone()], false, 2),
        Action::None
    );
    assert_eq!(
        m.observe([2; 32], vec![replacement], false, 10),
        Action::None
    );
}

#[test]
fn a_turn_starting_during_the_final_recheck_defers_instead_of_losing_the_restart() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), false, 1);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 5),
        Action::Restart(old.clone())
    );
    m.defer();
    assert_eq!(m.observe([2; 32], old.clone(), true, 7), Action::None);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 50),
        Action::Restart(old)
    );
}

#[test]
fn selected_profile_overrides_the_top_level_route() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let text = "model_provider='unused'\nprofile='active'\n[profiles.active]\nmodel_provider='varyai'\n[model_providers.varyai]\nbase_url='http://127.0.0.1:1234/intent/v1'\n[model_providers.unused]\nbase_url='https://synthetic.example/v1'\n";
    fs::write(&config, text)?;
    let before = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(
        &config,
        text.replace(
            "https://synthetic.example",
            "https://other.synthetic.example",
        ),
    )?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, before);
    fs::write(&config, text.replace("profile='active'", "profile='other'"))?;
    assert!(fingerprint(&config).is_err());
    Ok(())
}

#[test]
fn legacy_turn_activity_and_unreadable_history_fail_safely(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let sessions = dir.path().join("sessions/2026/11/06");
    fs::create_dir_all(&sessions)?;
    let rollout = sessions.join("synthetic.jsonl");
    let start = json!({"timestamp":"2026-11-06T00:00:00Z", "type":"event_msg", "payload":{"type":"task_started"}});
    let since =
        chrono::DateTime::parse_from_rfc3339("2026-11-05T23:59:00Z")?.timestamp_micros() as u64;
    fs::write(&rollout, format!("{start}\n"))?;
    assert!(activity::busy(dir.path(), since).map_err(|_| "activity")?);
    fs::write(
        &rollout,
        format!(
            "{start}\n{}\n",
            json!({"type":"event_msg", "payload":{"type":"task_complete"}})
        ),
    )?;
    assert!(!activity::busy(dir.path(), since).map_err(|_| "activity")?);
    fs::write(&rollout, "partial JSON")?;
    assert!(activity::busy(dir.path(), since).is_err());
    fs::remove_file(&rollout)?;
    fs::write(dir.path().join("thread_history_1.sqlite"), "invalid SQLite")?;
    assert!(activity::busy(dir.path(), since).is_err());
    Ok(())
}

#[test]
fn fingerprint_ignores_editor_preferences_and_inactive_providers_but_tracks_routing(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let text = "model_provider='magpie'\nmodel='one'\n[model_providers.magpie]\nbase_url='http://127.0.0.1:1234/v1'\nexperimental_bearer_token='synthetic-one'\n";
    fs::write(&config, text)?;
    let original = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(
        &config,
        format!(
            "{}\n[model_providers.unused]\nbase_url='https://synthetic.example/v1'\n",
            text.replace("model='one'", "model='two'")
        ),
    )?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, original);
    fs::write(&config, text.replace("1234/v1", "1235/v1"))?;
    assert_ne!(fingerprint(&config).map_err(|_| "fingerprint")?, original);
    Ok(())
}

#[test]
fn fingerprint_tracks_account_identity_but_ignores_catalog_contents_and_token_refresh(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let auth = dir.path().join("auth.json");
    fs::write(&config, "model_catalog_json='catalog.json'\n")?;
    fs::write(dir.path().join("catalog.json"), r#"{"models":[]}"#)?;
    let login = json!({"auth_mode":"chatgpt", "tokens":{"account_id":"synthetic-account", "access_token":"synthetic-old"}});
    fs::write(&auth, serde_json::to_vec(&login)?)?;
    let original = fingerprint(&config).map_err(|_| "fingerprint")?;
    let mut refreshed = login;
    refreshed["tokens"]["access_token"] = json!("synthetic-new");
    fs::write(&auth, serde_json::to_vec(&refreshed)?)?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, original);
    refreshed["tokens"]["account_id"] = json!("synthetic-other");
    fs::write(&auth, serde_json::to_vec(&refreshed)?)?;
    let account_route = fingerprint(&config).map_err(|_| "fingerprint")?;
    assert_ne!(account_route, original);
    fs::write(
        dir.path().join("catalog.json"),
        r#"{"models":[{"slug":"synthetic"}]}"#,
    )?;
    let changed = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(dir.path().join("catalog.json"), "invalid")?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, changed);
    assert_eq!(changed, account_route);
    Ok(())
}

#[test]
fn catalog_flaps_and_partial_writes_only_affect_manual_freshness(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let catalog = dir.path().join("models.json");
    fs::write(&config, "model_catalog_json='models.json'\n")?;
    fs::write(&catalog, r#"{"models":[{"slug":"one"},{"slug":"two"}]}"#)?;
    let route = fingerprint(&config).map_err(|_| "fingerprint")?;
    let (original, modified) = catalog_sample(&config).map_err(|_| "catalog")?;
    assert!(modified > 0);
    for contents in [
        r#"{"models":[{"slug":"one"}]}"#,
        r#"{"models":[{"slug":"two"},{"slug":"one","description":"updated","priority":2}]}"#,
        r#"{"models":[]}"#,
    ] {
        fs::write(&catalog, contents)?;
        assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, route);
        assert_ne!(
            catalog_fingerprint(&config).map_err(|_| "catalog")?,
            original
        );
    }
    fs::write(&catalog, r#"{"models":[{"slug":"partial"#)?;
    assert!(catalog_sample(&config).is_err());
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, route);
    fs::remove_file(&catalog)?;
    assert!(catalog_sample(&config).is_err());
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, route);
    Ok(())
}

#[test]
fn invalid_selected_route_fields_are_unknown_instead_of_a_default_route(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    for invalid in [
        "model_provider=42\n",
        "openai_base_url=42\n",
        "model_provider='relay'\n[model_providers.relay]\nrequires_openai_auth='false'\n",
        "model_provider='relay'\n[model_providers.relay]\nbase_url=42\n",
    ] {
        fs::write(&config, invalid)?;
        assert!(fingerprint(&config).is_err());
    }
    Ok(())
}

#[test]
fn final_rechecks_require_a_valid_catalog_and_preserve_the_route(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let catalog = dir.path().join("models.json");
    fs::write(&config, "model_catalog_json='models.json'\n")?;
    fs::write(&catalog, "invalid")?;
    let route = fingerprint(&config).map_err(|_| "fingerprint")?;
    assert!(!files_unchanged(&config, route, None));
    fs::write(&catalog, r#"{"models":[]}"#)?;
    let digest = catalog_fingerprint(&config).map_err(|_| "catalog")?;
    assert!(files_unchanged(&config, route, Some(digest)));
    fs::write(&catalog, "partial")?;
    assert!(!files_unchanged(&config, route, Some(digest)));
    fs::write(&catalog, r#"{"models":[]}"#)?;
    fs::write(&config, "model_provider='")?;
    assert!(!files_unchanged(&config, route, Some(digest)));
    Ok(())
}

#[test]
fn catalog_pointer_and_provider_label_are_not_routing_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let text = "model_provider='relay'\nmodel_catalog_json='one.json'\n[model_providers.relay]\nname='Before'\nbase_url='https://synthetic.example/v1'\n";
    fs::write(&config, text)?;
    let before = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(
        &config,
        text.replace("one.json", "two.json")
            .replace("Before", "After"),
    )?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, before);
    Ok(())
}

#[test]
fn providers_without_openai_auth_ignore_account_switches() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let text = "model_provider='relay'\n[model_providers.relay]\nbase_url='https://synthetic.example/v1'\nrequires_openai_auth=false\n";
    fs::write(&config, text)?;
    let before = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(
        dir.path().join("auth.json"),
        r#"{"tokens":{"account_id":"synthetic"}}"#,
    )?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, before);
    fs::write(&config, text.replace("false", "true"))?;
    let with_auth = fingerprint(&config).map_err(|_| "fingerprint")?;
    fs::write(
        dir.path().join("auth.json"),
        r#"{"tokens":{"account_id":"other"}}"#,
    )?;
    assert_ne!(fingerprint(&config).map_err(|_| "fingerprint")?, with_auth);
    Ok(())
}

#[test]
fn missing_config_restores_default_route_but_invalid_profile_is_unknown(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let default = fingerprint(&config).map_err(|_| "default route")?;
    fs::write(&config, "model='synthetic'\n")?;
    assert_eq!(fingerprint(&config).map_err(|_| "fingerprint")?, default);
    fs::write(&config, "profile='missing'\n")?;
    assert!(fingerprint(&config).is_err());
    Ok(())
}

#[test]
fn returning_to_loaded_route_cancels_an_unsignalled_restart() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), true, 1);
    m.observe([1; 32], old.clone(), false, 2);
    assert_eq!(m.observe([1; 32], old, false, 10), Action::None);
}

#[test]
fn later_route_changes_never_signal_the_same_instance_twice() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), false, 1);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 5),
        Action::Restart(old.clone())
    );
    m.observe([3; 32], old.clone(), false, 6);
    assert_eq!(m.observe([3; 32], old.clone(), false, 10), Action::None);
    assert_eq!(m.observe([3; 32], old.clone(), false, 25), Action::TimedOut);
    assert_eq!(m.observe([3; 32], old, false, 40), Action::None);
}

#[test]
fn missing_replacement_failure_remains_visible_until_a_new_server_appears() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), false, 1);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 5),
        Action::Restart(old)
    );
    m.observe([2; 32], vec![], false, 6);
    assert_eq!(m.observe([2; 32], vec![], false, 25), Action::TimedOut);
    assert_eq!(m.snapshot(), ReloadState::Failed);
    m.observe([2; 32], vec![], false, 40);
    assert_eq!(m.snapshot(), ReloadState::Failed);
    m.observe([2; 32], vec![server(11)], false, 41);
    assert_eq!(m.snapshot(), ReloadState::Current);
}

#[test]
fn partial_replacements_receive_a_later_route_without_repeating_old_signals() {
    let mut m = Monitor::default();
    let old = vec![server(10), server(11)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe([2; 32], old.clone(), false, 1);
    assert_eq!(
        m.observe([2; 32], old.clone(), false, 5),
        Action::Restart(old)
    );
    let running = vec![server(11), server(12)];
    m.observe([2; 32], running.clone(), false, 6);
    m.observe([3; 32], running.clone(), false, 7);
    assert_eq!(
        m.observe([3; 32], running, false, 10),
        Action::Restart(vec![server(12)])
    );
}

#[test]
fn manual_catalog_refresh_waits_for_idle_and_observes_replacement() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe_catalog(Some([10; 32]), None);
    assert_eq!(m.snapshot(), ReloadState::Current);
    m.observe_catalog(Some([11; 32]), None);
    assert_eq!(m.snapshot(), ReloadState::StaleCatalog);
    assert_eq!(m.observe([1; 32], old.clone(), false, 10), Action::None);
    m.request_restart(11);
    assert_eq!(m.snapshot(), ReloadState::WaitingForIdle);
    assert_eq!(m.observe([1; 32], old.clone(), true, 15), Action::None);
    assert_eq!(
        m.observe([1; 32], old.clone(), false, 16),
        Action::Restart(old)
    );
    assert_eq!(m.snapshot(), ReloadState::Restarting);
    assert_eq!(
        m.observe([1; 32], vec![server(11)], false, 17),
        Action::Recovered
    );
    m.observe_catalog(Some([11; 32]), None);
    assert_eq!(m.snapshot(), ReloadState::Current);
}

#[test]
fn startup_freshness_offers_a_manual_refresh_without_an_automatic_restart() {
    let mut m = Monitor::default();
    let old = vec![server(10)];
    m.observe([1; 32], old.clone(), false, 0);
    m.observe_catalog(Some([10; 32]), Some(13_000_000));
    assert_eq!(m.snapshot(), ReloadState::StaleCatalog);
    assert_eq!(m.observe([1; 32], old, false, 10), Action::None);
    m.observe([1; 32], vec![server(20)], false, 11);
    m.observe_catalog(Some([10; 32]), Some(13_000_000));
    assert_eq!(m.snapshot(), ReloadState::Current);
}

#[test]
fn invalid_catalog_samples_do_not_forget_known_staleness() {
    let mut m = Monitor::default();
    m.observe([1; 32], vec![server(10)], false, 0);
    m.observe_catalog(Some([10; 32]), None);
    m.observe_catalog(Some([11; 32]), None);
    m.observe_catalog(None, None);
    assert_eq!(m.snapshot(), ReloadState::StaleCatalog);
}

#[test]
fn abandoned_rollouts_expire_only_after_their_last_file_activity(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("sessions"))?;
    let rollout = dir.path().join("sessions/synthetic.jsonl");
    let now = std::time::SystemTime::now();
    let started = chrono::Utc::now() - chrono::Duration::minutes(45);
    let since = (started - chrono::Duration::minutes(15)).timestamp_micros() as u64;
    fs::write(
        &rollout,
        format!(
            "{}\n",
            json!({"timestamp":started.to_rfc3339(), "type":"event_msg", "payload":{"type":"task_started"}})
        ),
    )?;
    assert!(activity::busy(dir.path(), since).map_err(|_| "activity")?);
    // Windows timestamp updates require a writable file handle.
    fs::File::options().write(true).open(&rollout)?.set_times(
        fs::FileTimes::new().set_modified(now - std::time::Duration::from_secs(40 * 60)),
    )?;
    assert!(!activity::busy(dir.path(), since).map_err(|_| "activity")?);
    Ok(())
}

#[test]
fn abandoned_database_turns_expire_by_database_and_wal_activity(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("thread_history_1.sqlite");
    let conn = rusqlite::Connection::open(&db)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; CREATE TABLE thread_turns(status TEXT, started_at INTEGER);",
    )?;
    let now = std::time::SystemTime::now();
    let started = chrono::Utc::now().timestamp() - 45 * 60;
    let since = (started - 15 * 60) as u64 * 1_000_000;
    conn.execute(
        "INSERT INTO thread_turns VALUES('inProgress', ?1)",
        [started],
    )?;
    fs::File::options().write(true).open(&db)?.set_times(
        fs::FileTimes::new().set_modified(now - std::time::Duration::from_secs(40 * 60)),
    )?;
    assert!(activity::busy(dir.path(), since).map_err(|_| "activity")?);
    drop(conn);
    fs::File::options().write(true).open(&db)?.set_times(
        fs::FileTimes::new().set_modified(now - std::time::Duration::from_secs(40 * 60)),
    )?;
    assert!(!activity::busy(dir.path(), since).map_err(|_| "activity")?);
    Ok(())
}

#[test]
fn paginated_in_progress_turns_block_restart_until_completed(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let conn = rusqlite::Connection::open(dir.path().join("thread_history_1.sqlite"))?;
    conn.execute_batch("CREATE TABLE thread_turns(status TEXT, started_at INTEGER); INSERT INTO thread_turns VALUES('inProgress', 20);")?;
    assert!(activity::busy(dir.path(), 10_000_000).map_err(|_| "activity")?);
    conn.execute_batch("UPDATE thread_turns SET status='completed'")?;
    assert!(!activity::busy(dir.path(), 10_000_000).map_err(|_| "activity")?);
    conn.execute_batch("UPDATE thread_turns SET status='inProgress', started_at=1")?;
    assert!(!activity::busy(dir.path(), 10_000_000).map_err(|_| "activity")?);
    Ok(())
}
