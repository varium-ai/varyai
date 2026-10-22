use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const EVENT_SECURITY_AUDIT_RECORDED: &str = "security-audit-recorded";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static EMIT_SCHEDULED: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static MISSING_HANDLE_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::warn!("[SecurityAudit] event=security-audit-recorded stage=init category=duplicate");
    }
}

pub fn notify_audit_recorded() {
    #[cfg(test)]
    TEST_NOTIFY_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let Some(handle) = APP_HANDLE.get() else {
        #[cfg(not(test))]
        if !MISSING_HANDLE_REPORTED.swap(true, Ordering::AcqRel) {
            log::warn!(
                "[SecurityAudit] event=security-audit-recorded stage=init category=missing_handle"
            );
        }
        return;
    };
    let handle = handle.clone();
    schedule_with(
        &EMIT_SCHEDULED,
        |job| {
            std::thread::Builder::new()
                .name("security-audit-event".into())
                .spawn(move || {
                    std::thread::sleep(DEBOUNCE_WINDOW);
                    job();
                })
                .map(|_| ())
        },
        move |name| handle.emit(name, ()).map_err(|_| ()),
    );
}

type EmitJob = Box<dyn FnOnce() + Send>;

fn schedule_with(
    scheduled: &'static AtomicBool,
    spawn: impl FnOnce(EmitJob) -> std::io::Result<()>,
    emit: impl FnOnce(&'static str) -> Result<(), ()> + Send + 'static,
) {
    if scheduled.swap(true, Ordering::AcqRel) {
        return;
    }
    if spawn(Box::new(move || {
        scheduled.store(false, Ordering::Release);
        if emit(EVENT_SECURITY_AUDIT_RECORDED).is_err() {
            log::warn!("[SecurityAudit] event=security-audit-recorded stage=emit category=failed");
        }
    }))
    .is_err()
    {
        scheduled.store(false, Ordering::Release);
        log::warn!("[SecurityAudit] event=security-audit-recorded stage=spawn category=failed");
    }
}

#[cfg(test)]
thread_local! {
    static TEST_NOTIFY_COUNT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_test_notify_count() -> u32 {
    TEST_NOTIFY_COUNT.with(|count| count.replace(0))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn uninitialized_call_counts_attempt_without_emitting() {
        notify_audit_recorded();
        assert_eq!(take_test_notify_count(), 1);
    }

    #[test]
    fn scheduled_emit_is_payload_free_debounced_and_recovers_after_failure() {
        static SCHEDULED: AtomicBool = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let scheduler = |job| tx.send(job).map_err(|_| std::io::Error::other("closed"));
        schedule_with(&SCHEDULED, scheduler, move |name| {
            event_tx.send(name).map_err(|_| ())
        });
        schedule_with(&SCHEDULED, |_| panic!("debounced"), |_| Ok(()));
        let job: Box<dyn FnOnce() + Send> = rx.try_recv().expect("one job");
        job();
        assert_eq!(event_rx.try_recv().unwrap(), EVENT_SECURITY_AUDIT_RECORDED);
        assert!(event_rx.try_recv().is_err());
        assert!(!SCHEDULED.load(Ordering::Acquire));

        schedule_with(
            &SCHEDULED,
            |_| Err(std::io::Error::other("secret")),
            |_| Ok(()),
        );
        assert!(!SCHEDULED.load(Ordering::Acquire));
        let (tx, rx) = mpsc::channel();
        schedule_with(
            &SCHEDULED,
            |job| tx.send(job).map_err(|_| std::io::Error::other("closed")),
            |_| Err(()),
        );
        rx.recv().unwrap()();
        assert!(!SCHEDULED.load(Ordering::Acquire));
        let (tx, rx) = mpsc::channel();
        schedule_with(
            &SCHEDULED,
            |job| tx.send(job).map_err(|_| std::io::Error::other("closed")),
            |_| Ok(()),
        );
        rx.recv().unwrap()();
        assert!(!SCHEDULED.load(Ordering::Acquire));
    }
}
