use super::ToolLifecycleAction;
use std::collections::HashSet;
use std::sync::Mutex;
use tokio::sync::{Semaphore, SemaphorePermit};

pub(super) struct LifecycleCoordinator {
    tools: Mutex<HashSet<String>>,
    packages: Semaphore,
}

impl Default for LifecycleCoordinator {
    fn default() -> Self {
        Self {
            tools: Mutex::new(HashSet::new()),
            packages: Semaphore::new(1),
        }
    }
}

impl LifecycleCoordinator {
    pub(super) fn reserve(&self, tools: &[&str]) -> Result<ToolReservation<'_>, String> {
        let mut active = self.tools.lock().unwrap_or_else(|error| error.into_inner());
        if tools.iter().any(|tool| active.contains(*tool)) {
            return Err("An installation or update for this tool is already running.".into());
        }
        let tools: Vec<String> = tools.iter().map(|tool| (*tool).to_owned()).collect();
        active.extend(tools.iter().cloned());
        Ok(ToolReservation {
            coordinator: self,
            tools,
        })
    }

    pub(super) async fn package_permit(&self, needed: bool) -> Option<SemaphorePermit<'_>> {
        if needed {
            // This private semaphore is never closed. Waiting yields the async runtime.
            self.packages.acquire().await.ok()
        } else {
            None
        }
    }
}

pub(super) struct ToolReservation<'a> {
    coordinator: &'a LifecycleCoordinator,
    tools: Vec<String>,
}

impl Drop for ToolReservation<'_> {
    fn drop(&mut self) {
        let mut active = self
            .coordinator
            .tools
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for tool in &self.tools {
            active.remove(tool);
        }
    }
}

/// Unknown installers and commands with package-manager fallbacks share one writer slot.
/// Only recognized native updates can bypass it, using the same installation as the command.
pub(super) fn uses_shared_packages(
    tool: &str,
    action: ToolLifecycleAction,
    installation: Option<(&str, &str)>,
) -> bool {
    if matches!(action, ToolLifecycleAction::Install) {
        return true;
    }
    let Some((path, real)) = installation else {
        return true;
    };
    let path = path.replace('\\', "/").to_ascii_lowercase();
    let real = real.replace('\\', "/").to_ascii_lowercase();
    match tool {
        "claude" => !(real.contains("/.local/share/claude/") || real.contains("/claude/versions/")),
        "opencode" => !(path.contains("/.opencode/bin/") && real.contains("/.opencode/bin/")),
        "hermes" => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_tools_can_run_together_but_overlapping_batches_are_rejected() {
        let coordinator = LifecycleCoordinator::default();
        let claude = coordinator.reserve(&["claude"]).unwrap();
        let codex = coordinator.reserve(&["codex"]).unwrap();
        assert!(coordinator.reserve(&["claude", "gemini"]).is_err());
        assert!(coordinator.reserve(&["gemini"]).is_ok());
        drop(claude);
        assert!(coordinator.reserve(&["claude"]).is_ok());
        assert!(coordinator.reserve(&["codex"]).is_err());
        drop(codex);
        assert!(coordinator.reserve(&["codex"]).is_ok());
    }

    #[tokio::test]
    async fn package_writers_wait_while_native_updates_can_proceed() {
        let coordinator = LifecycleCoordinator::default();
        let first = coordinator.package_permit(true).await;
        assert!(coordinator.package_permit(false).await.is_none());
        let waiting = coordinator.package_permit(true);
        tokio::pin!(waiting);
        tokio::select! {
            _ = &mut waiting => panic!("shared package writers must not overlap"),
            _ = tokio::task::yield_now() => {}
        }
        drop(first);
        assert!(waiting.await.is_some());
    }
}
