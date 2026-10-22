use super::aggregation::{AggregateState, PendingAuditEvent};
use super::identity::fingerprint;
use crate::database::{AuditBatchOutcome, AuditLogEvent, Database};
use std::time::{Duration, Instant};

fn event(at: Instant, wall: i64, epoch: u64, risk: &str) -> PendingAuditEvent {
    PendingAuditEvent {
        log: AuditLogEvent {
            timestamp: wall,
            request_id: None,
            session_id: None,
            app_type: "claude".into(),
            provider_id: String::new(),
            rule_id: "r".into(),
            rule_name: "rule".into(),
            rule_category: "custom".into(),
            risk_level: risk.into(),
            action_taken: "audit".into(),
            matched_snippet_masked: "***".into(),
            location: None,
        },
        identity: fingerprint("sensitive-content"),
        detected_at: at,
        risk_epoch: epoch,
        config_revision: epoch,
    }
}
fn persist(state: &mut AggregateState, db: &Database, items: &[PendingAuditEvent]) {
    let plan = state.plan(items);
    match db.apply_audit_batch(&plan.writes) {
        AuditBatchOutcome::Committed(receipts) => state.commit(plan, &receipts),
        _ => panic!("expected a committed batch"),
    }
}
#[test]
fn one_minute_sliding_window_boundary_and_reversed_clocks() {
    let db = Database::memory().unwrap();
    let mut state = AggregateState::new(10000);
    let at = Instant::now();
    persist(
        &mut state,
        &db,
        &[
            event(at, 1000, 1, "LOW"),
            event(at + Duration::from_secs(59), 2000, 1, "LOW"),
        ],
    );
    persist(
        &mut state,
        &db,
        &[
            event(at + Duration::from_secs(50), 1500, 1, "LOW"),
            event(at + Duration::from_secs(118), 3000, 1, "LOW"),
        ],
    );
    let rows = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.items[0].trigger_count, 4);
    assert_eq!(rows.items[0].timestamp, 1000);
    assert_eq!(rows.items[0].last_seen_at, 3000);
    persist(
        &mut state,
        &db,
        &[event(at + Duration::from_secs(178), 4000, 1, "LOW")],
    );
    assert_eq!(
        db.get_security_audit_logs(&Default::default(), 1, 100)
            .unwrap()
            .total,
        2
    );
}
#[test]
fn late_versions_never_reopen_or_close_the_current_segment() {
    let db = Database::memory().unwrap();
    let mut state = AggregateState::new(10000);
    let at = Instant::now();
    persist(&mut state, &db, &[event(at, 1000, 1, "LOW")]);
    persist(
        &mut state,
        &db,
        &[
            event(at, 2000, 2, "HIGH"),
            event(at, 1500, 1, "LOW"),
            event(at, 2500, 2, "HIGH"),
        ],
    );
    let rows = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap();
    assert_eq!(rows.total, 2);
    assert!(rows.items.iter().all(|r| r.trigger_count == 2));
    persist(
        &mut state,
        &db,
        &[
            event(at, 3000, 3, "LOW"),
            event(at, 2700, 2, "HIGH"),
            event(at, 3100, 3, "LOW"),
        ],
    );
    let rows = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap();
    assert_eq!(rows.total, 3);
    assert_eq!(rows.items[0].trigger_count, 2);
    assert_eq!(
        rows.items
            .iter()
            .find(|r| r.timestamp == 1000)
            .unwrap()
            .trigger_count,
        2
    );
}
#[test]
fn missing_history_clear_eviction_restart_and_identity_failure_create_fresh_rows() {
    let db = Database::memory().unwrap();
    let mut state = AggregateState::new(2);
    let at = Instant::now();
    persist(
        &mut state,
        &db,
        &[
            event(at, 2000, 2, "HIGH"),
            event(at, 1000, 1, "LOW"),
            event(at, 2100, 2, "HIGH"),
        ],
    );
    assert_eq!(
        db.get_security_audit_logs(&Default::default(), 1, 100)
            .unwrap()
            .total,
        2
    );
    db.clear_security_audit_logs(None).unwrap();
    persist(
        &mut state,
        &db,
        &[event(at, 2200, 2, "HIGH"), event(at, 2300, 2, "HIGH")],
    );
    let row = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap()
        .items
        .remove(0);
    assert_eq!(row.trigger_count, 2);
    assert_eq!(row.timestamp, 2200);
    persist(
        &mut state,
        &db,
        &[event(at, 3000, 3, "LOW"), event(at, 4000, 4, "HIGH")],
    );
    assert!(state.segment_count() <= 2);
    persist(&mut state, &db, &[event(at, 5000, 1, "LOW")]);
    let mut fresh = AggregateState::new(10000);
    persist(&mut fresh, &db, &[event(at, 6000, 1, "LOW")]);
    let mut no_identity = event(at, 7000, 1, "LOW");
    no_identity.identity = None;
    let before = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap()
        .total;
    persist(&mut fresh, &db, &[no_identity.clone(), no_identity]);
    assert_eq!(
        db.get_security_audit_logs(&Default::default(), 1, 100)
            .unwrap()
            .total,
        before + 2
    );
}
#[test]
fn rollback_does_not_publish_a_candidate_or_increment_committed_rows() {
    let db = Database::memory().unwrap();
    let mut state = AggregateState::new(1);
    let at = Instant::now();
    persist(&mut state, &db, &[event(at, 1000, 1, "LOW")]);
    db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_high BEFORE INSERT ON security_audit_logs WHEN NEW.risk_level = 'HIGH' BEGIN SELECT RAISE(ABORT, 'private-error'); END;").unwrap();
    let items = [event(at, 1500, 1, "LOW"), event(at, 2000, 2, "HIGH")];
    let plan = state.plan(&items);
    assert!(matches!(
        db.apply_audit_batch(&plan.writes),
        AuditBatchOutcome::NotCommitted
    ));
    assert_eq!(state.segment_count(), 1);
    assert_eq!(
        db.get_security_audit_logs(&Default::default(), 1, 100)
            .unwrap()
            .items[0]
            .trigger_count,
        1
    );
    db.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_high")
        .unwrap();
    persist(&mut state, &db, &items);
    let rows = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap();
    assert_eq!(rows.total, 2);
    assert_eq!(
        rows.items
            .iter()
            .find(|r| r.timestamp == 1000)
            .unwrap()
            .trigger_count,
        2
    );
}
#[test]
fn all_base_key_dimensions_remain_independent() {
    let db = Database::memory().unwrap();
    let mut state = AggregateState::new(10000);
    let at = Instant::now();
    let first = event(at, 1000, 1, "LOW");
    let mut events = vec![first.clone()];
    for dimension in 0..4 {
        let mut other = first.clone();
        match dimension {
            0 => other.log.app_type = "codex".into(),
            1 => other.log.rule_id = "other".into(),
            2 => other.log.action_taken = "block".into(),
            _ => other.identity = fingerprint("other-content"),
        }
        events.push(other);
    }
    persist(&mut state, &db, &events);
    persist(&mut state, &db, &events);
    let rows = db
        .get_security_audit_logs(&Default::default(), 1, 100)
        .unwrap();
    assert_eq!(rows.total, 5);
    assert!(rows.items.iter().all(|r| r.trigger_count == 2));
}

#[test]
fn aggregation_merges_known_and_unknown_sessions_and_notifies_once(
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let mut state = AggregateState::new(100);
    let at = Instant::now();
    let scoped = |session: Option<&str>, seconds| {
        let mut item = event(
            at + Duration::from_secs(seconds),
            1000 + seconds as i64 * 1000,
            1,
            "HIGH",
        );
        item.log.session_id = session.map(str::to_owned);
        item
    };
    persist(
        &mut state,
        &db,
        &[
            scoped(Some("session-a"), 0),
            scoped(Some("session-b"), 1),
            scoped(None, 2),
        ],
    );
    let notifications = db.get_security_audit_notification_batch(0, 0, 100)?;
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].session_id.as_deref(), Some("session-a"));
    persist(
        &mut state,
        &db,
        &[
            scoped(Some("session-b"), 59),
            scoped(None, 60),
            scoped(Some("session-a"), 61),
        ],
    );
    let rows = db.get_security_audit_logs(&Default::default(), 1, 100)?;
    assert_eq!(rows.total, 1, "session identity must not split the group");
    assert_eq!(rows.items[0].trigger_count, 6);
    assert_eq!(rows.items[0].last_seen_at, 62000);
    let filter = serde_json::from_value(serde_json::json!({"sessionId": "session-a"}))?;
    let filtered = db.get_security_audit_logs(&filter, 1, 100)?;
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].session_id.as_deref(), Some("session-a"));
    assert!(db
        .get_security_audit_notification_batch(notifications[0].id, 0, 100)?
        .is_empty());

    // After a full idle minute, a new group preserves its own first source.
    persist(&mut state, &db, &[scoped(None, 121)]);
    persist(&mut state, &db, &[scoped(Some("session-b"), 122)]);
    let next = db.get_security_audit_notification_batch(notifications[0].id, 0, 100)?;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].trigger_count, 2);
    assert_eq!(next[0].session_id, None);
    Ok(())
}

#[test]
fn session_filter_is_exact_and_all_audit_read_paths_preserve_identity() {
    let db = Database::memory().unwrap();
    let at = Instant::now();
    let mut a = event(at, 1000, 1, "HIGH");
    a.log.session_id = Some("session-%_a".into());
    let mut b = a.clone();
    b.log.session_id = Some("session-xxa".into());
    // The compatibility insertion path must persist identities too.
    db.insert_audit_logs_batch(&[a.log.clone(), b.log.clone()])
        .unwrap();
    let filter = serde_json::from_value(
        serde_json::json!({"sessionId":"  session-%_a  ", "appType":"claude"}),
    )
    .unwrap();
    let rows = db.get_security_audit_logs(&filter, 1, 100).unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.items[0].session_id, a.log.session_id);
    let recent = db.get_recent_high_risk_alerts(0).unwrap();
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().any(|row| row.session_id == a.log.session_id));
    let blank = serde_json::from_value(serde_json::json!({"sessionId":"  "})).unwrap();
    assert_eq!(db.get_security_audit_logs(&blank, 1, 100).unwrap().total, 2);
}

#[test]
fn failure_invalidation_applies_across_sessions() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::memory()?;
    let mut state = AggregateState::new(100);
    let at = Instant::now();
    let mut a = event(at, 1000, 1, "HIGH");
    a.log.session_id = Some("session-a".into());
    let mut b = a.clone();
    b.log.session_id = Some("session-b".into());
    persist(&mut state, &db, &[a.clone(), b.clone()]);
    // Either session invalidates the shared content group after an unknown outcome.
    state.invalidate(&[b.clone()]);
    assert_eq!(state.segment_count(), 0);
    persist(&mut state, &db, &[b, a]);
    let rows = db.get_security_audit_logs(&Default::default(), 1, 100)?;
    assert_eq!(rows.total, 2);
    assert!(rows.items.iter().all(|row| row.trigger_count == 2));
    assert!(rows
        .items
        .iter()
        .any(|row| row.session_id.as_deref() == Some("session-a")));
    assert!(rows
        .items
        .iter()
        .any(|row| row.session_id.as_deref() == Some("session-b")));
    assert_eq!(state.segment_count(), 1);
    Ok(())
}
