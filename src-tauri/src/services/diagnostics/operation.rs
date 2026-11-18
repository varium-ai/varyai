//! Fixed lifecycle events; never serialize native errors or user-supplied values.
use crate::app_config::AppType;
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
        super::MISSING => "log_missing",
        super::EMPTY => "log_empty",
        super::SAME_SOURCE => "source_destination_conflict",
        super::PRIVACY => "privacy_unavailable",
        _ => "operation_failed",
    }
}

pub(crate) async fn record<T>(
    app: &str,
    action: Action,
    work: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let mut operation = Operation::start(app, action);
    let result = work.await;
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
        let ids: Vec<_> = inventory
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix('"')
                    .and_then(|line| line.strip_suffix("\","))
            })
            .collect();
        assert_eq!(ids.len(), AppType::all().count());
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
