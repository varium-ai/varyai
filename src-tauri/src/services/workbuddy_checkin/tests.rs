use super::*;
use axum::{extract::Request, response::IntoResponse, Json, Router};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Fixture {
    _dir: tempfile::TempDir,
    manager: Manager,
    calls: Arc<AtomicUsize>,
    device_calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn session(uid: &str) -> Value {
    json!({"account":{"uid":uid},"auth":{"accessToken":"synthetic-access-token","domain":"www.workbuddy.cn","expiresAt":4102444800000_i64}})
}

async fn fixture(status: Value, claim: Value, claim_status: u16, summary_status: u16) -> Fixture {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let device_calls = Arc::new(AtomicUsize::new(0));
    let device_count = device_calls.clone();
    let app = Router::new().fallback(move |req: Request| {
        let count = count.clone();
        let device_count = device_count.clone();
        let status = status.clone();
        let claim = claim.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            assert_eq!(req.method(), http::Method::POST);
            assert_eq!(
                req.headers()["authorization"],
                "Bearer synthetic-access-token"
            );
            assert_eq!(req.headers()["x-user-id"], "synthetic-user");
            assert_eq!(req.headers()["x-domain"], "www.workbuddy.cn");
            if let Some(token) = req.headers().get("x-device-token") {
                assert_eq!(token, "synthetic-device-token");
                device_count.fetch_add(1, Ordering::SeqCst);
            }
            assert!(req.headers()["user-agent"]
                .to_str()
                .expect("UA")
                .starts_with("WorkBuddy/"));
            let (code, body) = match req.uri().path() {
                STATUS_PATH => (200, status),
                CLAIM_PATH => (claim_status, claim),
                SUMMARY_PATH => (summary_status, json!({"code":0,"data":{}})),
                _ => panic!("unexpected billing path"),
            };
            let body_bytes = axum::body::to_bytes(req.into_body(), 1024)
                .await
                .expect("body");
            assert_eq!(&body_bytes[..], b"{}");
            (
                http::StatusCode::from_u16(code).expect("status"),
                Json(body),
            )
                .into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    let dir = tempfile::tempdir().expect("temp dir");
    let auth_path = dir.path().join("auth.info");
    std::fs::write(&auth_path, session("synthetic-user").to_string()).expect("auth");
    let mut manager = Manager::with_paths(dir.path().join("checkin.json"), auth_path);
    manager.endpoint = format!("http://{address}");
    Fixture {
        _dir: dir,
        manager,
        calls,
        device_calls,
        task,
    }
}

fn active() -> Value {
    json!({"code":0,"data":{"active":true,"today_checked_in":false}})
}
fn reward() -> Value {
    json!({"code":0,"data":{"credit":"30","streak_days":"3","credit_delayed":true,"message":"private upstream text"}})
}

#[tokio::test]
async fn status_is_read_only_and_manual_claim_is_cached_across_restarts() {
    let f = fixture(active(), reward(), 200, 200).await;
    let before = std::fs::read(&f.manager.auth_path).expect("original auth");
    let status = f.manager.status().await.expect("status");
    assert!(!status.auto_enabled);
    assert_eq!(status.outcome, Outcome::NotChecked);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.device_calls.load(Ordering::SeqCst), 0);
    let (a, b) = tokio::join!(f.manager.check_in(true), f.manager.check_in(true));
    let a = a.expect("claim");
    assert_eq!(a.outcome, Outcome::Claimed);
    assert_eq!(b.expect("concurrent claim").outcome, Outcome::Claimed);
    assert_eq!(a.credit, Some(30.0));
    assert_eq!(a.streak, Some(3));
    assert!(a.credit_delayed);
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    let mut restarted = Manager::with_paths(f.manager.path.clone(), f.manager.auth_path.clone());
    restarted.endpoint = f.manager.endpoint.clone();
    assert_eq!(
        restarted.check_in(true).await.expect("cached").outcome,
        Outcome::Claimed
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&f.manager.auth_path).expect("auth"), before);
    let persisted = std::fs::read_to_string(&f.manager.path).expect("state");
    for private in [
        "synthetic-access-token",
        "synthetic-user",
        "private upstream text",
        "www.workbuddy.cn",
    ] {
        assert!(!persisted.contains(private));
        assert!(!serde_json::to_string(&a)
            .expect("snapshot")
            .contains(private));
    }
}

#[tokio::test]
async fn skips_claim_when_inactive_or_already_checked_and_accepts_camel_case() {
    for (data, outcome) in [
        (json!({"active":false}), Outcome::Inactive),
        (
            json!({"active":true,"todayCheckedIn":true,"todayCredit":"10","streakDays":2}),
            Outcome::AlreadyChecked,
        ),
    ] {
        let f = fixture(json!({"code":"0","data":data}), reward(), 200, 200).await;
        let result = f.manager.check_in(true).await.expect("check in");
        assert_eq!(result.outcome, outcome);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        if outcome == Outcome::AlreadyChecked {
            assert_eq!(result.credit, Some(10.0));
        }
    }
}

#[tokio::test]
async fn distinguishes_business_failures_without_exposing_vendor_messages() {
    for (code, expected) in [
        (1001, Outcome::AlreadyChecked),
        (1002, Outcome::Ineligible),
        (1003, Outcome::Inactive),
        (10000, Outcome::Failed),
    ] {
        let f = fixture(
            active(),
            json!({"code":code,"msg":"synthetic-access-token"}),
            200,
            200,
        )
        .await;
        let result = f.manager.check_in(true).await.expect("check in");
        assert_eq!(result.outcome, expected);
        assert!(!serde_json::to_string(&result)
            .expect("snapshot")
            .contains("synthetic-access-token"));
    }
}

#[tokio::test]
async fn business_answers_are_preserved_on_http_errors() {
    for (code, expected) in [
        (1001, Outcome::AlreadyChecked),
        (1002, Outcome::Ineligible),
        (1003, Outcome::Inactive),
    ] {
        let f = fixture(active(), json!({"code":code}), 400, 200).await;
        assert_eq!(
            f.manager
                .check_in(true)
                .await
                .expect("business answer")
                .outcome,
            expected
        );
    }
}

#[tokio::test]
async fn a_redirect_with_a_location_never_receives_credentials() {
    let mut f = fixture(active(), reward(), 200, 200).await;
    f.task.abort();
    let count = f.calls.clone();
    let app = Router::new().fallback(move |req: Request| {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            if req.uri().path() == STATUS_PATH {
                (
                    http::StatusCode::TEMPORARY_REDIRECT,
                    [("location", "/leak")],
                )
                    .into_response()
            } else {
                Json(json!({"code":0,"data":{"active":false}})).into_response()
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dynamic port");
    f.manager.endpoint = format!("http://{}", listener.local_addr().expect("address"));
    f.task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    assert_eq!(
        f.manager
            .check_in(true)
            .await
            .expect("refused redirect")
            .outcome,
        Outcome::Failed
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn distinguishes_device_requirement_from_expired_login() {
    for (summary, expected) in [
        (200, Outcome::OwnAppRequired),
        (401, Outcome::SignInRequired),
        (503, Outcome::Failed),
    ] {
        let f = fixture(active(), json!({"code":401}), 401, summary).await;
        assert_eq!(
            f.manager.check_in(true).await.expect("check in").outcome,
            expected
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn rejects_malformed_success_and_redirects_without_claiming() {
    for status in [
        json!({}),
        json!({"code":0}),
        json!({"code":0,"data":{}}),
        json!({"code":0,"data":{"active":true,"today_checked_in":"no"}}),
    ] {
        let f = fixture(status, reward(), 200, 200).await;
        assert_eq!(
            f.manager.check_in(true).await.expect("check in").outcome,
            Outcome::Failed
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }
    for (claim, status) in [
        (json!({}), 200),
        (reward(), 307),
        (reward(), 500),
        (json!({"code":0,"data":{"credit":"NaN"}}), 200),
    ] {
        let f = fixture(active(), claim, status, 200).await;
        assert_eq!(
            f.manager.check_in(true).await.expect("check in").outcome,
            Outcome::Failed
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn automatic_checkin_requires_opt_in_and_obeys_backoff() {
    let f = fixture(active(), json!({"code":10000}), 503, 200).await;
    f.manager.check_in(false).await.expect("disabled");
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(
        f.manager
            .set_auto_enabled(true)
            .await
            .expect("enable")
            .auto_enabled
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.manager.check_in(false).await.expect("automatic").outcome,
        Outcome::Failed
    );
    f.manager.check_in(false).await.expect("backoff");
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    f.manager.check_in(true).await.expect("explicit retry");
    assert_eq!(f.calls.load(Ordering::SeqCst), 4);
    assert!(
        !f.manager
            .set_auto_enabled(false)
            .await
            .expect("disable")
            .auto_enabled
    );
}

#[tokio::test]
async fn protected_credentials_without_a_native_runtime_are_reported_without_network_requests() {
    let f = fixture(active(), reward(), 200, 200).await;
    let mut protected = session("synthetic-user");
    protected["auth"]["accessToken"] =
        json!({"$wbEncrypted": 1, "envelope": "synthetic-encrypted-envelope"});
    std::fs::write(&f.manager.auth_path, protected.to_string()).expect("protected session");
    let original = std::fs::read(&f.manager.auth_path).expect("session");
    let result = f.manager.status().await.expect("status");
    assert_eq!(
        serde_json::to_value(&result).expect("status JSON")["outcome"],
        "credentials_protected"
    );
    f.manager.check_in(true).await.expect("manual check-in");
    assert!(f.manager.set_auto_enabled(true).await.is_err());
    f.manager.check_in(false).await.expect("automatic check-in");
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read(&f.manager.auth_path).expect("session"),
        original
    );
    let serialized = serde_json::to_string(&result).expect("status JSON");
    assert!(!serialized.contains("synthetic-encrypted-envelope"));
}

#[cfg(unix)]
#[tokio::test]
async fn native_runtime_resolves_protected_sessions_and_supplies_device_verification() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = fixture(active(), reward(), 200, 200).await;
    let contents = f._dir.path().join("WorkBuddy.app/Contents");
    let runtime = contents.join("MacOS/Electron");
    let framework =
        contents.join("Frameworks/Electron Framework.framework/Versions/A/Electron Framework");
    std::fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("runtime directory");
    std::fs::create_dir_all(framework.parent().expect("framework parent"))
        .expect("framework directory");
    std::fs::write(
        &framework,
        b"synthetic-binary-dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX\x01\x081",
    )
    .expect("enabled Node fuse");
    std::fs::write(&runtime, r##"#!/bin/sh
test "$ELECTRON_RUN_AS_NODE" = 1 || exit 2
if [ "$4" = request ]; then
    printf '\nVARYAI_WORKBUDDY_AUTH:{"uid":"synthetic-user","accessToken":"synthetic-access-token","domain":"www.workbuddy.cn","clientVersion":"5.9.3","deviceToken":"synthetic-device-token"}'
else
    printf '\nVARYAI_WORKBUDDY_AUTH:{"uid":"synthetic-user","accessToken":"synthetic-access-token","domain":"www.workbuddy.cn","clientVersion":"5.9.3"}'
fi
"##).expect("fake runtime");
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).expect("executable");
    f.manager.native_runtime = Some(runtime);
    let mut protected = session("synthetic-user");
    protected["auth"]["accessToken"] = json!({"$wbEncrypted":1,"envelope":"synthetic-envelope"});
    std::fs::write(&f.manager.auth_path, protected.to_string()).expect("session");
    let before = std::fs::read(&f.manager.auth_path).expect("session");
    assert_eq!(
        f.manager.status().await.expect("status").outcome,
        Outcome::NotChecked
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.manager.check_in(true).await.expect("claim").outcome,
        Outcome::Claimed
    );
    assert_eq!(f.device_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read(&f.manager.auth_path).expect("session"),
        before
    );
    let saved = std::fs::read_to_string(&f.manager.path).expect("record");
    assert!(!saved.contains("synthetic-access-token"));
    assert!(!saved.contains("synthetic-device-token"));
    std::fs::write(
        &framework,
        b"synthetic-binary-dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX\x01\x080",
    )
    .expect("disabled Node fuse");
    assert!(matches!(
        f.manager.native_account("session").await,
        Err(Outcome::CredentialsProtected)
    ));
}

#[tokio::test]
async fn unreadable_credentials_and_state_never_send_requests_or_overwrite_files() {
    let f = fixture(active(), reward(), 200, 200).await;
    for auth in [
        json!({}),
        json!({"account":{"uid":"u"},"auth":{"accessToken":{"encrypted":"test"}}}),
        json!({"account":{"uid":"u"},"auth":{"accessToken":"test","expiresAt":1}}),
    ] {
        std::fs::write(&f.manager.auth_path, auth.to_string()).expect("auth");
        let result = f.manager.check_in(true).await.expect("safe outcome");
        assert!(matches!(
            result.outcome,
            Outcome::CredentialsUnavailable | Outcome::SignInRequired
        ));
    }
    std::fs::write(&f.manager.auth_path, session("synthetic-user").to_string()).expect("auth");
    std::fs::write(&f.manager.path, "broken private state").expect("state");
    assert_eq!(
        f.manager.check_in(true).await.expect_err("corrupt store"),
        STORAGE_ERROR
    );
    assert!(f.manager.set_auto_enabled(true).await.is_err());
    assert_eq!(
        std::fs::read_to_string(&f.manager.path).expect("unchanged"),
        "broken private state"
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changing_native_account_does_not_reuse_the_previous_accounts_result() {
    let f = fixture(active(), reward(), 200, 200).await;
    f.manager.check_in(true).await.expect("first account");
    std::fs::write(
        &f.manager.auth_path,
        session("other-synthetic-user").to_string(),
    )
    .expect("switch");
    assert_eq!(
        f.manager.status().await.expect("other account").outcome,
        Outcome::NotChecked
    );
}

#[test]
fn beijing_day_boundary_and_retry_rules() {
    let before = chrono::DateTime::parse_from_rfc3339("2026-11-01T15:59:59Z")
        .expect("time")
        .timestamp();
    assert_eq!(beijing_day(before), "2026-11-01");
    assert_eq!(beijing_day(before + 1), "2026-11-02");
    let early = chrono::DateTime::parse_from_rfc3339("2026-11-02T00:00:00+08:00")
        .expect("early")
        .timestamp();
    let noon = early + 12 * 3600;
    let record = Record {
        day: beijing_day(early),
        checked_at: early,
        outcome: Outcome::Inactive,
        credit: None,
        streak: None,
        credit_delayed: false,
    };
    assert!(!record.should_run(early + 60, false));
    assert!(record.should_run(early + 1800, false));
    let settled = Record {
        checked_at: noon,
        ..record.clone()
    };
    assert!(!settled.should_run(noon + 1800, false));
    assert!(settled.should_run(noon + 1800, true));
    assert!(settled.should_run(noon + 86400, false));
    let claimed = Record {
        outcome: Outcome::Claimed,
        ..record
    };
    assert!(!claimed.should_run(early + 1800, true));
    assert!(!claimed.should_run(early - 1, false));
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "Requires a signed-in macOS WorkBuddy client; reads campaign status without claiming credits"]
async fn live_native_workbuddy_checkin_status() {
    let dir = tempfile::tempdir().expect("temporary local records");
    let manager = Manager::new(dir.path().join("checkin.json"));
    let account = manager
        .native_account("request")
        .await
        .expect("read native account");
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("HTTP client");
    let status = manager
        .call(&client, &account, STATUS_PATH)
        .await
        .map_err(|_| "Official check-in status unavailable")
        .expect("campaign status");
    assert!(status.get("active").is_some_and(Value::is_boolean));
    assert!(!manager.path.exists());
}

#[tokio::test]
async fn saved_native_auto_preference_is_bound_once_and_does_not_follow_account_switches() {
    let f = fixture(active(), reward(), 200, 200).await;
    std::fs::write(
        &f.manager.path,
        json!({"autoEnabled":true,"records":{}}).to_string(),
    )
    .expect("old preference");
    f.manager.run_auto().await.expect("migrate and claim");
    let saved = f.manager.read_store().await.expect("migrated state");
    assert!(!saved.auto_enabled);
    std::fs::write(&f.manager.auth_path, session("different-user").to_string())
        .expect("switch native account");
    f.manager
        .run_auto()
        .await
        .expect("new native account is opt-in");
    assert_eq!(f.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(
        !f.manager
            .status()
            .await
            .expect("current status")
            .auto_enabled
    );
}
