use super::*;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex as StdMutex,
};
use tokio_tungstenite::accept_async;

#[tokio::test]
async fn saved_auto_mode_survives_reopening_without_a_native_account() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("checkin.json");
    let db = Arc::new(crate::database::Database::memory().expect("database"));
    let manager = Manager::new(path.clone(), db.clone());
    let mut store = Store::default();
    store.enabled.insert(key("synthetic-account"), true);
    manager.write_store(&store).await.expect("save preference");
    let reopened = Manager::new(path, db);
    let store = reopened.read_store().await.expect("read saved preference");
    let value = serde_json::to_value(Manager::snapshot(&store, None)).expect("status");
    assert_eq!(value["autoConfigured"], true);
    assert_eq!(value["connected"], false);
    assert_eq!(value["accounts"], json!([]));
    assert_eq!(
        serde_json::to_value(Manager::snapshot(&Store::default(), None)).expect("status")
            ["autoConfigured"],
        false
    );
}

struct Fixture {
    _directory: tempfile::TempDir,
    manager: Manager,
    current: Arc<StdMutex<String>>,
    claims: Arc<AtomicUsize>,
    switches: Arc<StdMutex<Vec<String>>>,
    switch_fails: Arc<AtomicBool>,
    completed: Arc<StdMutex<HashSet<String>>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("temporary directory");
    let current = Arc::new(StdMutex::new(String::from("synthetic-a")));
    let claims = Arc::new(AtomicUsize::new(0));
    let switches = Arc::new(StdMutex::new(Vec::new()));
    let switch_fails = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(StdMutex::new(HashSet::<String>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let active = current.clone();
    let captured = claims.clone();
    let switched = switches.clone();
    let failing = switch_fails.clone();
    let checked = completed.clone();
    let server = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let active = active.clone();
            let captured = captured.clone();
            let switched = switched.clone();
            let failing = failing.clone();
            let checked = checked.clone();
            tokio::spawn(async move {
                let mut socket = accept_async(stream).await.expect("websocket");
                while let Some(Ok(Message::Text(raw))) = socket.next().await {
                    let command: Value = serde_json::from_str(&raw).expect("protocol message");
                    let result = match command["method"].as_str().expect("method") {
                        "Target.getTargets" => {
                            json!({"targetInfos":[{"targetId":"synthetic-page","type":"page","url":"doubaowork://doubaowork-chat/chat"}]})
                        }
                        "Target.attachToTarget" => json!({"sessionId":"synthetic-session"}),
                        "Target.detachFromTarget" => json!({}),
                        "Runtime.evaluate" => {
                            let expression =
                                command["params"]["expression"].as_str().expect("script");
                            let raw = expression
                                .rsplit("/* VARYAI_CHECKIN_INPUT */")
                                .next()
                                .expect("input")
                                .trim_end_matches(')');
                            let input: Value = serde_json::from_str(raw).expect("structured input");
                            let current = active.lock().expect("current").clone();
                            let value = if input["mode"] == "accounts" {
                                json!({"currentAccountId":current,"accounts":[{"id":"synthetic-a","label":"Account A"},{"id":"synthetic-b","label":"Account B"}]})
                            } else if input["mode"] == "switch" {
                                if input["expectedAccountId"] != current {
                                    json!({"outcome":"switch_required"})
                                } else if failing.load(Ordering::SeqCst) {
                                    json!({"outcome":"sign_in_required"})
                                } else {
                                    let target = input["targetAccountId"]
                                        .as_str()
                                        .expect("selected account")
                                        .to_owned();
                                    *active.lock().expect("current") = target.clone();
                                    switched.lock().expect("switches").push(target);
                                    json!({"outcome":"switched"})
                                }
                            } else if input["expectedAccountId"] != current {
                                json!({"outcome":"switch_required"})
                            } else if input["mode"] == "status" {
                                if checked.lock().expect("completed").contains(&current) {
                                    reward_result(&current, "already_checked")
                                } else {
                                    json!({"outcome":"not_checked"})
                                }
                            } else {
                                captured.fetch_add(1, Ordering::SeqCst);
                                checked.lock().expect("completed").insert(current.clone());
                                reward_result(&current, "claimed")
                            };
                            json!({"result":{"type":"object","value":value}})
                        }
                        _ => {
                            panic!("check-in must not intercept traffic")
                        }
                    };
                    if socket
                        .send(Message::Text(
                            json!({"id":command["id"],"result":result})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    let db = Arc::new(crate::database::Database::memory().expect("in-memory database"));
    db.set_setting(crate::services::doubao_work::CONSENT_KEY, "true")
        .expect("local connection consent");
    let mut manager = Manager::new(directory.path().join("checkin.json"), db);
    manager.test_endpoint = Some(format!("ws://{address}/devtools/browser/synthetic"));
    Fixture {
        _directory: directory,
        manager,
        current,
        claims,
        switches,
        switch_fails,
        completed,
        server,
    }
}

fn reward_result(current: &str, outcome: &str) -> Value {
    let (credit, streak, total) = if current == "synthetic-a" {
        (100, 2, 5)
    } else {
        (200, 4, 9)
    };
    json!({"outcome":outcome,"credit":credit,"streak":streak,"totalDays":total})
}

#[tokio::test]
async fn selection_auto_and_daily_cache_are_independent_for_native_accounts() {
    let f = fixture().await;
    let initial = f.manager.status().await.expect("status");
    assert!(initial.connected);
    assert_eq!(initial.accounts.len(), 2);
    assert_eq!(f.claims.load(Ordering::SeqCst), 0);
    let a = initial.accounts[0].id.clone();
    let b = initial.accounts[1].id.clone();
    let chosen = f.manager.check_in(&b).await.expect("switch and check in");
    assert_eq!(chosen.accounts[1].outcome, Outcome::Claimed);
    let reward = serde_json::to_value(&chosen.accounts[1]).expect("public reward");
    assert_eq!(reward["credit"].as_f64(), Some(200.0));
    assert_eq!(reward["streak"], 4);
    assert_eq!(reward["totalDays"], 9);
    assert_eq!(*f.current.lock().expect("current"), "synthetic-b");
    assert_eq!(f.claims.load(Ordering::SeqCst), 1);
    f.manager.set_auto(&a, true).await.expect("enable A");
    f.manager.run_auto().await.expect("automatic A");
    f.manager.check_in(&a).await.expect("cached A");
    assert_eq!(f.claims.load(Ordering::SeqCst), 2);
    assert_eq!(*f.current.lock().expect("current"), "synthetic-a");
    *f.current.lock().expect("current") = "synthetic-b".into();
    f.manager.run_auto().await.expect("B has not opted in");
    assert_eq!(f.claims.load(Ordering::SeqCst), 2);
    f.manager.set_auto(&b, true).await.expect("enable B");
    f.manager.run_auto().await.expect("automatic B");
    assert_eq!(f.claims.load(Ordering::SeqCst), 2);
    assert!(f.manager.check_in("unknown-id").await.is_err());
    let saved = std::fs::read_to_string(&f.manager.path).expect("daily records");
    assert!(!saved.contains("synthetic-"));
    assert!(!saved.contains("Account A"));
    let accounts = f
        .manager
        .status()
        .await
        .expect("refreshed accounts")
        .accounts;
    let accounts = serde_json::to_value(accounts).expect("public account data");
    assert_eq!(accounts[0]["credit"].as_f64(), Some(100.0));
    assert_eq!(accounts[0]["streak"], 2);
    assert_eq!(accounts[1]["credit"].as_f64(), Some(200.0));
    assert_eq!(accounts[1]["streak"], 4);
}

#[tokio::test]
async fn refresh_reads_official_rewards_and_accepts_legacy_records_without_claiming() {
    let f = fixture().await;
    f.completed
        .lock()
        .expect("completed")
        .insert("synthetic-a".into());
    let id = key("synthetic-a");
    let legacy = json!({"enabled":{},"records":{id:{"day":day(chrono::Utc::now().timestamp()),"checkedAt":chrono::Utc::now().timestamp(),"outcome":"already_checked"}}}).to_string();
    std::fs::write(&f.manager.path, &legacy).expect("old record without reward fields");
    let status = f.manager.status().await.expect("read-only refresh");
    let current = serde_json::to_value(&status.accounts[0]).expect("public reward");
    assert_eq!(current["outcome"], "already_checked");
    assert_eq!(current["credit"].as_f64(), Some(100.0));
    assert_eq!(current["streak"], 2);
    assert_eq!(current["totalDays"], 5);
    let other = serde_json::to_value(&status.accounts[1]).expect("other account");
    assert!(other["credit"].is_null());
    assert!(other["streak"].is_null());
    assert_eq!(f.claims.load(Ordering::SeqCst), 0);
    assert!(f.switches.lock().expect("switches").is_empty());
    assert_eq!(
        std::fs::read_to_string(&f.manager.path).expect("unchanged records"),
        legacy
    );
}

#[tokio::test]
async fn automatic_batch_switches_to_each_enabled_account_without_restoring() {
    let f = fixture().await;
    let accounts = f.manager.status().await.expect("accounts").accounts;
    for account in &accounts {
        f.manager
            .set_auto(&account.id, true)
            .await
            .expect("enable account");
    }
    f.manager.run_auto().await.expect("batch");
    assert_eq!(f.claims.load(Ordering::SeqCst), 2);
    assert_eq!(*f.current.lock().expect("current"), "synthetic-b");
    assert_eq!(*f.switches.lock().expect("switches"), vec!["synthetic-b"]);
    f.manager.run_auto().await.expect("cached batch");
    assert_eq!(f.claims.load(Ordering::SeqCst), 2);
    assert_eq!(*f.current.lock().expect("current"), "synthetic-b");
}

#[tokio::test]
async fn a_failed_native_switch_never_claims_for_the_previous_account() {
    let f = fixture().await;
    f.switch_fails.store(true, Ordering::SeqCst);
    let account = f.manager.status().await.expect("accounts").accounts[1]
        .id
        .clone();
    let result = f
        .manager
        .check_in(&account)
        .await
        .expect("failed switch status");
    assert_eq!(result.accounts[1].outcome, Outcome::SignInRequired);
    assert_eq!(f.claims.load(Ordering::SeqCst), 0);
    assert_eq!(*f.current.lock().expect("current"), "synthetic-a");
}

#[tokio::test]
async fn missing_consent_and_corrupt_records_never_claim_or_overwrite() {
    let f = fixture().await;
    f.manager
        .db
        .set_setting(crate::services::doubao_work::CONSENT_KEY, "false")
        .expect("revoke consent");
    assert!(!f.manager.status().await.expect("disconnected").connected);
    assert!(f.manager.check_in("unknown-id").await.is_err());
    assert_eq!(f.claims.load(Ordering::SeqCst), 0);
    std::fs::write(&f.manager.path, "invalid-private-fixture").expect("corrupt store");
    assert!(f.manager.run_auto().await.is_err());
    assert_eq!(
        std::fs::read_to_string(&f.manager.path).expect("unchanged"),
        "invalid-private-fixture"
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "Requires a connected, signed-in macOS Doubao Work client; reads status without claiming"]
async fn live_native_doubao_checkin_status() {
    let endpoint = crate::services::doubao_work::discover()
        .await
        .expect("connected native client");
    let directory = tempfile::tempdir().expect("temporary records");
    let db = Arc::new(crate::database::Database::memory().expect("temporary database"));
    db.set_setting(crate::services::doubao_work::CONSENT_KEY, "true")
        .expect("explicit read-only test consent");
    let mut manager = Manager::new(directory.path().join("checkin.json"), db);
    manager.test_endpoint = Some(endpoint);
    let native = manager.native().await.expect("official account stores");
    let current = native.current_account_id.expect("signed-in account");
    assert!(native.accounts.iter().any(|account| account.id == current));
    let status = manager
        .bridge(json!({"mode":"status","expectedAccountId":current}))
        .await
        .expect("official status");
    let result: NativeSnapshot = serde_json::from_value(status).expect("sanitized response");
    assert!(matches!(
        result.outcome,
        Some(Outcome::NotChecked | Outcome::AlreadyChecked | Outcome::Inactive)
    ));
    assert!(
        !manager.path.exists(),
        "read-only acceptance never claims or saves records"
    );
}
