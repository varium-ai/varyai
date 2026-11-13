use std::{future::Future, sync::Mutex};
use tokio::sync::watch;

struct ActiveAttempt {
    id: String,
    cancel: watch::Sender<bool>,
    installing: bool,
}

pub struct UpdateInstallState {
    active: Mutex<Option<ActiveAttempt>>,
}

impl Default for UpdateInstallState {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateInstallState {
    pub const fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn begin(&self, id: &str) -> Result<(Attempt<'_>, watch::Receiver<bool>), String> {
        let mut active = self.active.lock().map_err(|_| "Update state unavailable")?;
        if active.is_some() {
            return Err("An update is already in progress".into());
        }
        let (cancel, receiver) = watch::channel(false);
        *active = Some(ActiveAttempt {
            id: id.into(),
            cancel,
            installing: false,
        });
        Ok((
            Attempt {
                state: self,
                id: id.into(),
            },
            receiver,
        ))
    }

    pub fn cancel(&self, id: &str) -> Result<bool, String> {
        let active = self.active.lock().map_err(|_| "Update state unavailable")?;
        if let Some(attempt) = active.as_ref().filter(|a| a.id == id && !a.installing) {
            attempt.cancel.send_replace(true);
            return Ok(true);
        }
        Ok(false)
    }
}

pub struct Attempt<'a> {
    state: &'a UpdateInstallState,
    id: String,
}

impl Attempt<'_> {
    // Commit under the same lock as cancellation so only one can win.
    pub fn start_install(&self) -> Result<bool, String> {
        let mut active = self
            .state
            .active
            .lock()
            .map_err(|_| "Update state unavailable")?;
        let Some(attempt) = active.as_mut().filter(|a| a.id == self.id) else {
            return Ok(false);
        };
        if *attempt.cancel.borrow() {
            return Ok(false);
        }
        attempt.installing = true;
        Ok(true)
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            if active.as_ref().is_some_and(|a| a.id == self.id) {
                *active = None;
            }
        }
    }
}

pub async fn run_cancellable<T>(
    cancelled: &mut watch::Receiver<bool>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = cancelled.wait_for(|cancelled| *cancelled) => None,
        result = work => Some(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn cancellation_drops_pending_network_work_and_allows_retry() {
        let state = UpdateInstallState::default();
        let (attempt, mut cancelled) = state.begin("first").unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        struct PendingDownload(Arc<AtomicBool>);
        impl Drop for PendingDownload {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let pending = PendingDownload(dropped.clone());
        let network = async move {
            let _pending = pending;
            std::future::pending::<()>().await;
        };
        assert!(state.cancel("first").unwrap());
        assert!(run_cancellable(&mut cancelled, network).await.is_none());
        assert!(dropped.load(Ordering::SeqCst));
        assert!(!attempt.start_install().unwrap());
        drop(attempt);
        assert!(state.begin("retry").is_ok());
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_already_polled_network_request() {
        let state = UpdateInstallState::default();
        let (_attempt, mut cancelled) = state.begin("active").unwrap();
        let started = tokio::sync::Notify::new();
        let network = async {
            started.notify_one();
            std::future::pending::<()>().await;
        };
        let (result, _) = tokio::join!(run_cancellable(&mut cancelled, network), async {
            started.notified().await;
            assert!(state.cancel("active").unwrap());
        });
        assert!(result.is_none());
    }

    #[test]
    fn cancellation_is_scoped_and_cannot_interrupt_installation() {
        let state = UpdateInstallState::default();
        let (attempt, _) = state.begin("active").unwrap();
        assert!(state.begin("duplicate").is_err());
        assert!(!state.cancel("stale").unwrap());
        assert!(attempt.start_install().unwrap());
        assert!(!state.cancel("active").unwrap());
        drop(attempt);
        assert!(state.begin("next").is_ok());
    }

    #[tokio::test]
    async fn completion_passes_through_without_cancellation() {
        let state = UpdateInstallState::default();
        let (_attempt, mut cancelled) = state.begin("active").unwrap();
        assert_eq!(
            run_cancellable(&mut cancelled, async { 42 }).await,
            Some(42)
        );
    }
}
