use super::*;
fn manager(path: PathBuf) -> Arc<Manager> {
    Manager::new(
        path.clone(),
        Arc::new(CodexOAuthManager::new(path.with_file_name("accounts"))),
    )
}
#[tokio::test]
async fn history_keeps_observation_time_and_does_not_write_account_labels() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path().join("quota-controls.json"));
    let now = chrono::Utc::now().timestamp();
    let point = Point {
        at: now - 120,
        remaining: 73.0,
        resets_at: now + 3600,
        span: 18000,
    };
    let points = manager
        .note("synthetic-private-label@example.test", point)
        .await
        .unwrap();
    assert_eq!(points[0].at, now - 120);
    assert_eq!(
        manager
            .history("synthetic-private-label@example.test")
            .await
            .unwrap()
            .len(),
        1
    );
    let bytes = std::fs::read_to_string(root.path().join("quota-history.json")).unwrap();
    assert!(!bytes.contains("synthetic-private-label"));
    assert!(!root.path().join("quota-controls.json").exists());
}
#[tokio::test]
async fn damaged_history_neither_changes_policy_nor_overwrites_the_file() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path().join("quota-controls.json"));
    std::fs::write(root.path().join("quota-history.json"), b"damaged-fixture").unwrap();
    assert!(manager.history("synthetic").await.is_err());
    assert!(manager.read().await.unwrap().entries.is_empty());
    assert_eq!(
        std::fs::read(root.path().join("quota-history.json")).unwrap(),
        b"damaged-fixture"
    );
}
#[tokio::test]
async fn credit_opt_out_holds_unknown_or_exhausted_allowance_but_not_a_known_healthy_window() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path().join("quota-controls.json"));
    let id = hash(&uuid::Uuid::new_v4().to_string());
    let entry = Entry {
        account: "synthetic".into(),
        policy: Policy {
            credits: CreditsMode::Off,
            ..Default::default()
        },
        ..Default::default()
    };
    manager.read().await.unwrap();
    manager.publish(&id, &entry, None);
    assert_eq!(credits(&id, false), CreditsMode::Off);
    let mut quota = SubscriptionQuota::not_found("synthetic");
    quota.success = true;
    quota.tiers.push(crate::services::subscription::QuotaTier {
        name: "seven_day".into(),
        utilization: 12.0,
        resets_at: Some((chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339()),
        used_value_usd: None,
        max_value_usd: None,
    });
    manager.publish(&id, &entry, Some(quota.clone()));
    assert_eq!(credits(&id, false), CreditsMode::On);
    quota.tiers[0].utilization = 100.0;
    manager.publish(&id, &entry, Some(quota));
    assert_eq!(credits(&id, false), CreditsMode::Off);
    assert!(observed_week_full(&id));
}
#[tokio::test]
async fn failed_policy_persistence_does_not_publish_a_new_policy() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path().join("quota-controls.json"));
    manager.read().await.unwrap();
    std::fs::create_dir(root.path().join("quota-controls.json")).unwrap();
    let id = "a".repeat(64);
    let result = manager
        .update(|store| {
            store.entries.insert(
                id.clone(),
                Entry {
                    account: "synthetic".into(),
                    policy: Policy {
                        week: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            Ok(())
        })
        .await;
    assert!(result.is_err());
    assert!(!manager.read().await.unwrap().entries.contains_key(&id));
}
