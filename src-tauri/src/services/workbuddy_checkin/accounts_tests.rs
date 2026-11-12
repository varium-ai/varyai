use super::*;
use axum::{extract::Request, response::IntoResponse, Json, Router};
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};

fn grant(uid: &str) -> ManagedAccount {
    serde_json::from_value(json!({
        "uid":uid,"accessToken":format!("synthetic-{uid}"),
        "refreshToken":format!("synthetic-refresh-{uid}"),
        "domain":"copilot.tencent.com","expiresAt":4102444800000_i64,
        "label":format!("Account {uid}"),"enterpriseId":""
    }))
    .expect("synthetic grant")
}

#[tokio::test]
async fn saved_auto_mode_survives_reopening_without_a_native_account() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("checkin.json");
    let auth = dir.path().join("missing-native.info");
    let manager = Manager::with_paths(path.clone(), auth.clone());
    let mut store = Store::default();
    store.account_auto.insert("synthetic-account".into(), true);
    manager.write_store(&store).await.expect("save preference");
    let reopened = Manager::with_paths(path, auth);
    let list = reopened
        .accounts()
        .await
        .expect("unavailable native account");
    assert!(list.accounts.is_empty());
    let value = serde_json::to_value(list).expect("status");
    assert_eq!(value["nativeStatus"]["autoConfigured"], true);
    assert_eq!(value["nativeStatus"]["autoEnabled"], false);
}

#[tokio::test]
async fn account_selection_and_auto_flags_never_claim_for_another_account() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let auth = dir.path().join("native.info");
    let original = json!({"account":{"uid":"native-user"},"auth":{"accessToken":"synthetic-native","domain":"copilot.tencent.com"}}).to_string();
    std::fs::write(&auth, &original).expect("native fixture");
    let calls = Arc::new(StdMutex::new(Vec::<String>::new()));
    let captured = calls.clone();
    let app = Router::new().fallback(move |request: Request| {
        let captured = captured.clone();
        async move {
            let uid = request.headers()["x-user-id"]
                .to_str()
                .expect("uid")
                .to_owned();
            let body = if request.uri().path() == STATUS_PATH {
                json!({"code":0,"data":{"active":true,"today_checked_in":false}})
            } else {
                assert_eq!(request.uri().path(), CLAIM_PATH);
                captured.lock().expect("calls").push(uid);
                json!({"code":0,"data":{"credit":10,"streak_days":1}})
            };
            Json(body).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    let mut manager = Manager::with_paths(dir.path().join("checkin.json"), auth.clone());
    manager.endpoint = format!("http://{address}");
    let first = grant("first");
    let second = grant("second");
    let first_id = first.account().key();
    let second_id = second.account().key();
    manager
        .write_managed(&BTreeMap::from([
            (first_id.clone(), first),
            (second_id.clone(), second),
        ]))
        .await
        .expect("managed grants");

    assert_eq!(
        manager.accounts().await.expect("accounts").accounts.len(),
        3
    );
    manager
        .set_auto_for(&second_id, true)
        .await
        .expect("enable second");
    manager.run_auto().await.expect("automatic check-in");
    assert_eq!(*calls.lock().expect("calls"), vec!["second"]);
    manager.check_in_for(&first_id).await.expect("select first");
    manager
        .check_in_for(&second_id)
        .await
        .expect("deduplicated second");
    assert_eq!(*calls.lock().expect("calls"), vec!["second", "first"]);
    assert!(manager.check_in_for("unknown-account").await.is_err());
    assert_eq!(
        std::fs::read_to_string(&auth).expect("native unchanged"),
        original
    );
    let public =
        serde_json::to_string(&manager.accounts().await.expect("status")).expect("serialize");
    assert!(!public.contains("synthetic-"));
    assert!(!public.contains("refreshToken"));
    manager
        .remove_account(&second_id)
        .await
        .expect("remove only selected account");
    assert!(!manager
        .read_store()
        .await
        .expect("state")
        .account_auto
        .contains_key(&second_id));
    assert_eq!(
        manager.accounts().await.expect("remaining").accounts.len(),
        2
    );
    task.abort();
}

#[tokio::test]
async fn browser_authorization_and_refresh_keep_native_sessions_untouched() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let token_polls = Arc::new(AtomicUsize::new(0));
    let polls = token_polls.clone();
    let app = Router::new().fallback(move |request: Request| {
        let polls = polls.clone();
        async move {
            let body = match request.uri().path() {
                "/v2/plugin/auth/state" => json!({"code":0,"data":{"state":"synthetic-state","authUrl":"https://www.workbuddy.cn/login"}}),
                "/v2/plugin/auth/token" if polls.fetch_add(1,Ordering::SeqCst) == 0 => json!({"code":11217}),
                "/v2/plugin/auth/token" => json!({"code":0,"data":{"accessToken":"synthetic-independent","refreshToken":"synthetic-independent-refresh","expiresIn":3600}}),
                "/v2/plugin/login/account" => {
                    assert_eq!(request.headers()["authorization"],"Bearer synthetic-independent");
                    json!({"code":0,"data":{"uid":"browser-user","nickname":"Browser account","type":"personal"}})
                }
                "/v2/plugin/auth/token/refresh" => {
                    assert!(!request.headers().contains_key("authorization"), "expired access tokens must not block refresh");
                    assert_eq!(request.headers()["x-refresh-token"],"synthetic-independent-refresh");
                    json!({"code":0,"data":{"accessToken":"synthetic-rotated","refreshToken":"synthetic-rotated-refresh","expiresIn":3600}})
                }
                STATUS_PATH => {
                    assert_eq!(request.headers()["authorization"],"Bearer synthetic-rotated");
                    json!({"code":0,"data":{"active":true,"today_checked_in":true}})
                }
                _ => panic!("unexpected request"),
            };
            Json(body)
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    let dir = tempfile::tempdir().expect("temp directory");
    let auth = dir.path().join("native.info");
    let original = json!({"account":{"uid":"native-user"},"auth":{"accessToken":"synthetic-native","refreshToken":"synthetic-native-refresh"}}).to_string();
    std::fs::write(&auth, &original).expect("native fixture");
    let mut manager = Manager::with_paths(dir.path().join("checkin.json"), auth.clone());
    manager.endpoint = format!("http://{address}");
    let start =
        serde_json::to_value(manager.start_login().await.expect("start")).expect("public flow");
    let login_id = start["loginId"].as_str().expect("login id");
    assert!(start["url"]
        .as_str()
        .expect("official URL")
        .starts_with("https://www.workbuddy.cn/"));
    assert!(
        !serde_json::to_value(manager.poll_login(login_id).await.expect("pending")).expect("state")
            ["complete"]
            .as_bool()
            .expect("bool")
    );
    let completed =
        serde_json::to_value(manager.poll_login(login_id).await.expect("complete")).expect("state");
    let account_id = completed["accountId"].as_str().expect("account");
    assert!(
        !manager.path.exists(),
        "login does not enable or claim check-in"
    );
    let accounts_path = dir.path().join("workbuddy-checkin-accounts.json");
    let mut saved: Value =
        serde_json::from_slice(&std::fs::read(&accounts_path).expect("private grants"))
            .expect("grants");
    saved[account_id]["expiresAt"] = json!(1);
    std::fs::write(&accounts_path, saved.to_string()).expect("expired grant fixture");
    assert_eq!(
        manager
            .check_in_for(account_id)
            .await
            .expect("refresh and check")
            .outcome,
        Outcome::AlreadyChecked
    );
    let saved = std::fs::read_to_string(&accounts_path).expect("rotated grants");
    assert!(saved.contains("synthetic-rotated-refresh"));
    assert!(!saved.contains("synthetic-native"));
    assert_eq!(
        std::fs::read_to_string(auth).expect("native unchanged"),
        original
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(accounts_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let next = serde_json::to_value(manager.start_login().await.expect("next")).expect("flow");
    let next_id = next["loginId"].as_str().expect("id");
    manager.cancel_login(next_id).await;
    assert!(manager.poll_login(next_id).await.is_err());
    task.abort();
}

#[tokio::test]
async fn personal_and_enterprise_contexts_with_the_same_uid_remain_separate() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let profiles = Arc::new(AtomicUsize::new(0));
    let app = Router::new().fallback(move |request: Request| {
        let profiles = profiles.clone();
        async move {
            Json(match request.uri().path() {
                "/v2/plugin/auth/state" => json!({"code":0,"data":{"state":"synthetic-state","authUrl":"https://www.workbuddy.cn/login"}}),
                "/v2/plugin/auth/token" => json!({"code":0,"data":{"accessToken":"synthetic-browser","refreshToken":"synthetic-refresh","expiresIn":3600}}),
                "/v2/plugin/login/account" => json!({"code":0,"data":{"uid":"same-user","nickname":"Same person","type":if profiles.fetch_add(1,Ordering::SeqCst) == 0 { "personal" } else { "enterprise" }}}),
                "/v2/plugin/accounts" => json!({"code":0,"data":{"accounts":[{"uid":"same-user","enterpriseId":"synthetic-team","enterpriseName":"Team","type":"enterprise"}]}}),
                _ => panic!("authorization does not request billing"),
            })
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut manager = Manager::with_paths(
        directory.path().join("checkin.json"),
        directory.path().join("missing-native.info"),
    );
    manager.endpoint = format!("http://{address}");
    let mut ids = Vec::new();
    for _ in 0..2 {
        let login =
            serde_json::to_value(manager.start_login().await.expect("start")).expect("flow");
        let status = serde_json::to_value(
            manager
                .poll_login(login["loginId"].as_str().expect("id"))
                .await
                .expect("authorized context"),
        )
        .expect("status");
        ids.push(status["accountId"].clone());
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        manager.accounts().await.expect("accounts").accounts.len(),
        2
    );
    assert!(
        !manager.path.exists(),
        "no automatic switch or reward during login"
    );
    task.abort();
}

#[tokio::test]
async fn cancelling_while_profile_is_in_flight_prevents_saving_the_grant() {
    let waiting = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let observed = waiting.clone();
    let resumed = release.clone();
    let app = Router::new().fallback(move |request: Request| {
        let observed = observed.clone(); let resumed = resumed.clone();
        async move {
            Json(match request.uri().path() {
                "/v2/plugin/auth/state" => json!({"code":0,"data":{"state":"synthetic-state","authUrl":"https://www.workbuddy.cn/login"}}),
                "/v2/plugin/auth/token" => json!({"code":0,"data":{"accessToken":"synthetic-browser","refreshToken":"synthetic-refresh","expiresIn":3600}}),
                "/v2/plugin/login/account" => {
                    observed.notify_one(); resumed.notified().await;
                    json!({"code":0,"data":{"uid":"synthetic-user","nickname":"Cancelled account","type":"personal"}})
                },
                _ => panic!("unexpected authorization request"),
            })
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut manager = Manager::with_paths(
        directory.path().join("checkin.json"),
        directory.path().join("missing.info"),
    );
    manager.endpoint = format!("http://{address}");
    let manager = Arc::new(manager);
    let login = serde_json::to_value(manager.start_login().await.expect("start")).expect("flow");
    let id = login["loginId"].as_str().expect("id").to_owned();
    let poll_manager = manager.clone();
    let poll_id = id.clone();
    let polling = tokio::spawn(async move { poll_manager.poll_login(&poll_id).await });
    tokio::time::timeout(Duration::from_secs(2), waiting.notified())
        .await
        .expect("profile request started");
    let cancelled = tokio::time::timeout(Duration::from_secs(1), manager.cancel_login(&id)).await;
    release.notify_one();
    let result = polling.await.expect("poll task");
    server.abort();
    assert!(
        cancelled.is_ok(),
        "cancellation must not wait for a vendor response"
    );
    assert!(
        result.is_err(),
        "a cancelled response cannot commit a new account"
    );
    assert!(manager
        .accounts()
        .await
        .expect("accounts")
        .accounts
        .is_empty());
    assert!(!directory
        .path()
        .join("workbuddy-checkin-accounts.json")
        .exists());
}
