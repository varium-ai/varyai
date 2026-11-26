//! Fixed lifecycle events; never serialize native errors or user-supplied values.
use crate::app_config::AppType;
use futures::FutureExt;
use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const OPERATION_FAILED: &str = "agentConnections.operationFailed";

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Connect,
    Disconnect,
    Reapply,
    Restart,
    AuthorizeModelControl,
    ExportLog,
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::Reapply => "reapply",
            Self::Restart => "restart",
            Self::AuthorizeModelControl => "authorize_model_control",
            Self::ExportLog => "export_log",
        }
    }
}

pub(crate) struct Operation {
    id: u64,
    agent: Option<AppType>,
    action: Action,
    start: Instant,
    finished: bool,
}
impl Operation {
    pub(crate) fn start(app: &str, action: Action) -> Self {
        let event = Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            agent: app.parse().ok(),
            action,
            start: Instant::now(),
            finished: false,
        };
        log::info!("{}", event.message("started", "none"));
        event
    }

    fn message(&self, outcome: &str, reason: &str) -> String {
        format!(
            "[Diagnostics] operation_id={} agent={} operation={} outcome={} reason={} elapsed_ms={}",
            self.id,
            self.agent.as_ref().map_or("application", AppType::as_str),
            self.action.name(), outcome, reason, self.start.elapsed().as_millis()
        )
    }

    pub(crate) fn finish<T>(&mut self, result: &Result<T, String>) {
        self.finished = true;
        match result {
            Ok(_) => log::info!("{}", self.message("succeeded", "none")),
            Err(error) => log::warn!("{}", self.message("failed", safe_reason(error))),
        }
    }

    pub(crate) fn cancelled(&mut self) {
        self.finished = true;
        log::info!("{}", self.message("cancelled", "none"));
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            log::warn!("{}", self.message("interrupted", "incomplete"));
        }
    }
}

fn safe_reason(error: &str) -> &'static str {
    match error {
        "agentConnections.conflict" | "intentRouting.clientConflict" => "configuration_conflict",
        "agentConnections.noModels" => "no_models",
        "agentConnections.restartUnsupported" => "unsupported",
        "agentConnections.restartConfirmationRequired" => "confirmation_required",
        "intentRouting.cancelled" => "cancelled",
        OPERATION_FAILED => "operation_failed",
        super::MISSING => "log_missing",
        super::EMPTY => "log_empty",
        super::SAME_SOURCE => "source_destination_conflict",
        super::PRIVACY => "privacy_unavailable",
        _ => "operation_failed",
    }
}

/// Runs `work` and records the outcome as a fixed lifecycle event.
///
/// Callers must pass `work` as `Box::pin(...)`: the connection commands build
/// very large async state machines, and Tauri's release `respond_async_serialized`
/// moves the command future by value across several main-thread stack frames.
/// Keeping the state machine behind a pointer prevents that copy chain from
/// overflowing the 1 MB main-thread stack (STATUS_STACK_OVERFLOW, 0xc00000fd)
/// while a switch is being used.
pub(crate) async fn record<T>(
    app: &str,
    action: Action,
    work: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let mut operation = Operation::start(app, action);
    let outcome = AssertUnwindSafe(work).catch_unwind().await;
    // A malformed native configuration must produce a safe command error,
    // never a panic that closes the application while a switch is being used.
    let result = outcome.unwrap_or_else(|_| Err(OPERATION_FAILED.to_string()));
    operation.finish(&result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn operation_diagnostics_preserve_success_and_private_errors_for_every_agent() {
        let frontend = include_str!("../../../../src/config/appConfig.tsx");
        let inventory = frontend
            .split("export const APP_IDS: AppId[] = [")
            .nth(1)
            .unwrap()
            .split("];")
            .next()
            .unwrap();
        let mut ids: Vec<_> = inventory
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix('"')
                    .and_then(|line| line.strip_suffix("\","))
            })
            .collect();
        ids.sort_unstable();
        let app_types: Vec<_> = AppType::all().collect();
        let mut backend_ids: Vec<_> = app_types.iter().map(AppType::as_str).collect();
        backend_ids.sort_unstable();
        assert_eq!(ids, backend_ids);
        for app in ids {
            let private_error =
                "synthetic private configuration /Users/private api_key=synthetic-secret";
            for action in [
                Action::Connect,
                Action::Disconnect,
                Action::Reapply,
                Action::Restart,
                Action::AuthorizeModelControl,
            ] {
                assert_eq!(record(app, action, async { Ok(42) }).await, Ok(42));
                let result: Result<(), String> =
                    record(app, action, async { Err(private_error.into()) }).await;
                assert_eq!(result.as_ref().map_err(String::as_str), Err(private_error));
                let mut operation = Operation::start(app, action);
                assert_eq!(operation.agent.as_ref().map(AppType::as_str), Some(app));
                let event = operation.message("failed", safe_reason(private_error));
                assert!(!event.contains("synthetic"));
                assert!(!event.contains("/Users"));
                assert!(event.contains("reason=operation_failed"));
                assert!(event.contains("elapsed_ms="));
                operation.cancelled();
            }
        }
    }

    #[tokio::test]
    async fn operation_diagnostics_convert_panics_into_generic_errors() {
        let result: Result<(), String> = record("deepseek-harness", Action::Connect, async {
            panic!("synthetic private panic payload")
        })
        .await;
        assert_eq!(
            result.as_ref().map_err(String::as_str),
            Err(OPERATION_FAILED)
        );
    }

    #[test]
    fn operation_diagnostics_never_echo_invalid_agent_ids_or_error_values() {
        let mut operation = Operation::start("synthetic-private-agent\nsecret", Action::Connect);
        let text = operation.message("failed", safe_reason("secret"));
        assert!(!text.contains("synthetic-private-agent"));
        assert!(!text.contains("secret"));
        assert_eq!(
            safe_reason("agentConnections.conflict"),
            "configuration_conflict"
        );
        operation.cancelled();
    }
}
