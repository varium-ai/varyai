use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc::{channel, Sender};
use tokio::time::interval;

use super::aggregation::{AggregateState, PendingAuditEvent};
#[cfg(test)]
use crate::database::AuditLogEvent;
use crate::database::{AuditBatchOutcome, AuditWrite, Database};
use std::future::Future;

static AUDIT_LOG_SENDER: OnceLock<Sender<PendingAuditEvent>> = OnceLock::new();
static AUDIT_DB: OnceLock<Arc<Database>> = OnceLock::new();

pub fn init_audit_logger(db: Arc<Database>) {
    let (tx, mut rx) = channel::<PendingAuditEvent>(10_000);
    let _ = AUDIT_DB.set(db.clone());
    if AUDIT_LOG_SENDER.set(tx).is_err() {
        log::warn!("[SecurityAudit] Audit logger 已被初始化，跳过重复初始化");
        return;
    }

    tauri::async_runtime::spawn(async move {
        log::info!("[SecurityAudit] 异步日志落盘 Worker 启动就绪 (有界缓冲容量: 10000)");
        let mut buffer: Vec<PendingAuditEvent> = Vec::with_capacity(50);
        let mut state = AggregateState::new(10_000);
        let mut ticker = interval(Duration::from_millis(500));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_event = rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            buffer.push(event);
                            if buffer.len() >= 50 {
                                flush_buffer_async(&db, &mut state, &mut buffer).await;
                            }
                        }
                        None => {

                            if !buffer.is_empty() {
                                flush_buffer_async(&db, &mut state, &mut buffer).await;
                            }
                            log::info!("[SecurityAudit] 异步日志落盘 Worker 通道已关闭，优雅退出");
                            break;
                        }
                    }
                }
                _ = ticker.tick(), if !buffer.is_empty() => {
                    flush_buffer_async(&db, &mut state, &mut buffer).await;
                }
            }
        }
    });
}

async fn flush_buffer_async(
    db: &Arc<Database>,
    state: &mut AggregateState,
    buffer: &mut Vec<PendingAuditEvent>,
) {
    if buffer.is_empty() {
        return;
    }
    let items = std::mem::take(buffer);
    let db = db.clone();
    if flush_pending_with(
        state,
        &items,
        move |writes| {
            let db = db.clone();
            async move {
                classify_write_result(
                    tokio::task::spawn_blocking(move || db.apply_audit_batch(&writes)).await,
                )
            }
        },
        tokio::time::sleep,
    )
    .await
    {
        crate::security_events::notify_audit_recorded();
    }
}

async fn flush_pending_with<W, WF, S, SF>(
    state: &mut AggregateState,
    items: &[PendingAuditEvent],
    mut write: W,
    mut sleep: S,
) -> bool
where
    W: FnMut(Vec<AuditWrite>) -> WF,
    WF: Future<Output = AuditBatchOutcome>,
    S: FnMut(Duration) -> SF,
    SF: Future<Output = ()>,
{
    if items.is_empty() {
        return false;
    }
    let delays = [
        Duration::from_millis(500),
        Duration::from_secs(1),
        Duration::from_secs(2),
    ];
    for attempt in 0..=delays.len() {
        let plan = state.plan(items);
        match write(plan.writes.clone()).await {
            AuditBatchOutcome::Committed(receipts) if receipts.len() == items.len() => {
                state.commit(plan, &receipts);
                return true;
            }
            AuditBatchOutcome::NotCommitted => {
                if let Some(delay) = delays.get(attempt) {
                    sleep(*delay).await;
                } else {
                    log::error!(
                        "{}",
                        batch_insert_failure_context("retry_exhausted", items.len())
                    );
                }
            }
            _ => {
                state.invalidate(items);
                log::error!(
                    "{}",
                    batch_insert_failure_context("outcome_unknown", items.len())
                );
                return false;
            }
        }
    }
    false
}

fn classify_write_result(
    result: Result<AuditBatchOutcome, tokio::task::JoinError>,
) -> AuditBatchOutcome {
    result.unwrap_or(AuditBatchOutcome::OutcomeUnknown)
}

fn batch_insert_failure_context(category: &'static str, count: usize) -> String {
    format!("[SecurityAudit] stage=batch_insert category={category} count={count}")
}

pub fn emit_audit_event(event: PendingAuditEvent) {
    if let Some(sender) = AUDIT_LOG_SENDER.get() {
        if let Err(error) = sender.try_send(event) {
            let category = match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => "full",
                tokio::sync::mpsc::error::TrySendError::Closed(_) => "closed",
            };
            log::warn!("[SecurityAudit] stage=queue category={category}");
        }
    } else {
        log::debug!("[SecurityAudit] 安全事件记录器尚未初始化，丢弃事件");
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    pub(super) fn event() -> AuditLogEvent {
        AuditLogEvent {
            timestamp: 123,
            request_id: None,
            session_id: None,
            app_type: "claude".into(),
            provider_id: "provider".into(),
            rule_id: "rule".into(),
            rule_name: "sensitive-value".into(),
            rule_category: "test".into(),
            risk_level: "HIGH".into(),
            action_taken: "audit".into(),
            matched_snippet_masked: "secret-request-body".into(),
            location: None,
        }
    }

    #[tokio::test]
    async fn only_committed_nonempty_batches_notify() {
        let db = Arc::new(Database::memory().unwrap());
        let _ = crate::security_events::take_test_notify_count();
        let mut state = AggregateState::new(10000);
        flush_buffer_async(&db, &mut state, &mut Vec::new()).await;
        assert_eq!(crate::security_events::take_test_notify_count(), 0);
        flush_buffer_async(
            &db,
            &mut state,
            &mut vec![PendingAuditEvent {
                log: event(),
                identity: None,
                detected_at: std::time::Instant::now(),
                risk_epoch: 1,
                config_revision: 1,
            }],
        )
        .await;
        assert_eq!(crate::security_events::take_test_notify_count(), 1);
        assert_eq!(
            db.get_security_audit_logs(&Default::default(), 1, 10)
                .unwrap()
                .total,
            1
        );

        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch("DROP TABLE security_audit_logs")
                .unwrap();
        }
        flush_buffer_async(
            &db,
            &mut state,
            &mut vec![PendingAuditEvent {
                log: event(),
                identity: None,
                detected_at: std::time::Instant::now(),
                risk_epoch: 1,
                config_revision: 1,
            }],
        )
        .await;
        assert_eq!(crate::security_events::take_test_notify_count(), 0);
    }

    #[test]
    fn database_and_join_errors_never_notify_or_expose_error_text() {
        let _ = crate::security_events::take_test_notify_count();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let join_error = runtime.block_on(async {
            tokio::spawn(async { panic!("test join failure") })
                .await
                .unwrap_err()
        });
        assert!(matches!(
            classify_write_result(Err(join_error)),
            AuditBatchOutcome::OutcomeUnknown
        ));
        assert_eq!(crate::security_events::take_test_notify_count(), 0);
        let message = batch_insert_failure_context("database", 1);
        assert!(message.contains("stage=batch_insert category=database count=1"));
        assert!(!message.contains("secret-request-body"));
    }
}

#[cfg(test)]
mod retry_tests {
    use super::super::{
        aggregation::{AggregateState, PendingAuditEvent},
        identity::fingerprint,
    };
    use super::*;
    use crate::database::AuditBatchOutcome;
    use std::{future::ready, time::Instant};
    fn pending() -> PendingAuditEvent {
        PendingAuditEvent {
            log: super::notification_tests::event(),
            identity: fingerprint("worker-secret"),
            detected_at: Instant::now(),
            risk_epoch: 1,
            config_revision: 1,
        }
    }
    #[tokio::test]
    async fn confirmed_rollback_retries_with_bounded_backoff_and_counts_once() {
        let db = Database::memory().unwrap();
        let mut state = AggregateState::new(10000);
        let mut attempts = 0;
        let mut waits = Vec::new();
        assert!(
            flush_pending_with(
                &mut state,
                &[pending()],
                |writes| {
                    attempts += 1;
                    ready(if attempts < 4 {
                        AuditBatchOutcome::NotCommitted
                    } else {
                        db.apply_audit_batch(&writes)
                    })
                },
                |delay| {
                    waits.push(delay);
                    ready(())
                }
            )
            .await
        );
        assert_eq!(attempts, 4);
        assert_eq!(
            waits,
            [
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2)
            ]
        );
        assert_eq!(
            db.get_security_audit_logs(&Default::default(), 1, 100)
                .unwrap()
                .items[0]
                .trigger_count,
            1
        );
        attempts = 0;
        assert!(
            !flush_pending_with(
                &mut state,
                &[pending()],
                |_| {
                    attempts += 1;
                    ready(AuditBatchOutcome::NotCommitted)
                },
                |_| ready(())
            )
            .await
        );
        assert_eq!(attempts, 4);
        assert_eq!(state.segment_count(), 1);
    }
    #[tokio::test]
    async fn committed_then_unknown_never_replays_and_only_invalidates_touched_keys() {
        let db = Database::memory().unwrap();
        let mut state = AggregateState::new(10000);
        let item = pending();
        let mut other = item.clone();
        other.log.rule_id = "untouched".into();
        assert!(
            flush_pending_with(
                &mut state,
                &[item.clone(), other.clone()],
                |writes| ready(db.apply_audit_batch(&writes)),
                |_| ready(())
            )
            .await
        );
        let mut attempts = 0;
        assert!(
            !flush_pending_with(
                &mut state,
                &[item.clone()],
                |writes| {
                    attempts += 1;
                    assert!(matches!(
                        db.apply_audit_batch(&writes),
                        AuditBatchOutcome::Committed(_)
                    ));
                    ready(AuditBatchOutcome::OutcomeUnknown)
                },
                |_| -> std::future::Ready<()> {
                    panic!("must not back off or replay an unknown outcome")
                }
            )
            .await
        );
        assert_eq!(attempts, 1);
        assert_eq!(state.segment_count(), 1);
        assert!(
            flush_pending_with(
                &mut state,
                &[item, other],
                |writes| ready(db.apply_audit_batch(&writes)),
                |_| ready(())
            )
            .await
        );
        let rows = db
            .get_security_audit_logs(&Default::default(), 1, 100)
            .unwrap();
        assert_eq!(rows.total, 3);
        assert_eq!(
            rows.items
                .iter()
                .filter(|r| r.rule_id == "untouched")
                .count(),
            1
        );
        assert_eq!(rows.items.iter().map(|r| r.trigger_count).sum::<i64>(), 5);
    }
}

#[cfg(test)]
mod update_notification_tests {
    use super::super::identity::fingerprint;
    use super::*;
    #[tokio::test]
    async fn committed_updates_signal_refresh_without_inserting_new_notification_ids() {
        let db = Arc::new(Database::memory().unwrap());
        let mut state = AggregateState::new(10000);
        let item = PendingAuditEvent {
            log: super::notification_tests::event(),
            identity: fingerprint("update-secret"),
            detected_at: std::time::Instant::now(),
            risk_epoch: 1,
            config_revision: 1,
        };
        let _ = crate::security_events::take_test_notify_count();
        flush_buffer_async(&db, &mut state, &mut vec![item.clone(), item.clone()]).await;
        let rows = db.get_security_audit_notification_batch(0, 0, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trigger_count, 2);
        let id = rows[0].id;
        flush_buffer_async(&db, &mut state, &mut vec![item]).await;
        assert_eq!(crate::security_events::take_test_notify_count(), 2);
        assert!(db
            .get_security_audit_notification_batch(id, 0, 100)
            .unwrap()
            .is_empty());
        assert_eq!(
            db.get_security_audit_logs(&Default::default(), 1, 100)
                .unwrap()
                .items[0]
                .trigger_count,
            3
        );
    }
}
