use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const EVENT_TOOL_RISK_RECORDED: &str = "tool-risk-recorded";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static EMIT_SCHEDULED: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static MISSING_HANDLE_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::warn!("[ToolRisk] event=tool-risk-recorded stage=init category=duplicate");
    }
}

pub fn notify_tool_risk_recorded() {
    #[cfg(test)]
    TEST_NOTIFY_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let Some(handle) = APP_HANDLE.get() else {
        #[cfg(not(test))]
        if !MISSING_HANDLE_REPORTED.swap(true, Ordering::AcqRel) {
            log::warn!("[ToolRisk] event=tool-risk-recorded stage=init category=missing_handle");
        }
        return;
    };
    let handle = handle.clone();
    schedule_with(
        &EMIT_SCHEDULED,
        |job| {
            std::thread::Builder::new()
                .name("tool-risk-event".into())
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
    let job: EmitJob = Box::new(move || {
        let _ = emit(EVENT_TOOL_RISK_RECORDED);
        scheduled.store(false, Ordering::Release);
    });
    if spawn(job).is_err() {
        scheduled.store(false, Ordering::Release);
    }
}

#[cfg(test)]
thread_local! {
    static TEST_NOTIFY_COUNT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_without_handle_is_safe() {
        notify_tool_risk_recorded();
    }

    #[test]
    fn schedule_emits_once_and_resets() {
        static SCHEDULED: AtomicBool = AtomicBool::new(false);
        let emitted = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let e = emitted.clone();
        schedule_with(
            &SCHEDULED,
            |job| {
                job();
                Ok(())
            },
            move |name| {
                assert_eq!(name, EVENT_TOOL_RISK_RECORDED);
                e.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        );
        assert_eq!(emitted.load(Ordering::Relaxed), 1);
        assert!(!SCHEDULED.load(Ordering::Acquire));
    }
}
