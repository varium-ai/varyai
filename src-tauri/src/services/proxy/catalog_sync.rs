//! Coalesce resource edits without putting client configuration I/O on the save path.
use super::ProxyService;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
struct Pending {
    dirty: bool,
    running: bool,
}

#[derive(Default)]
struct Job {
    state: Mutex<Pending>,
    idle: Notify,
}

impl Job {
    async fn request<F, Fut>(self: &Arc<Self>, run: F)
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let mut state = self.state.lock().await;
        state.dirty = true;
        if state.running {
            return;
        }
        state.running = true;
        let job = self.clone();
        tokio::spawn(async move {
            loop {
                // A fixed window collapses bursts without starving continuous edits.
                tokio::time::sleep(Duration::from_millis(25)).await;
                {
                    let mut state = job.state.lock().await;
                    if !state.dirty {
                        state.running = false;
                        job.idle.notify_waiters();
                        return;
                    }
                    state.dirty = false;
                }
                run().await;
            }
        });
    }

    #[cfg(test)]
    async fn drain(&self) {
        loop {
            let idle = self.idle.notified();
            if !self.state.lock().await.running {
                return;
            }
            idle.await;
        }
    }
}

pub(super) struct CatalogSync {
    global: Arc<Job>,
    clients: HashMap<&'static str, Arc<Job>>,
    clients_paused: std::sync::atomic::AtomicBool,
}

impl Default for CatalogSync {
    fn default() -> Self {
        Self {
            global: Arc::default(),
            clients_paused: std::sync::atomic::AtomicBool::new(false),
            clients: crate::services::agent_connections::APPS
                .iter()
                .copied()
                .chain(["trae", "trae-intl"])
                .map(|app| (app, Arc::default()))
                .collect(),
        }
    }
}

impl CatalogSync {
    pub(super) fn pause_clients(&self, paused: bool) {
        self.clients_paused
            .store(paused, std::sync::atomic::Ordering::Release);
    }

    pub(super) fn clients_paused(&self) -> bool {
        self.clients_paused
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) async fn drain(&self) {
        self.global.drain().await;
        futures::future::join_all(self.clients.values().map(|job| job.drain())).await;
    }
}

impl ProxyService {
    pub(crate) async fn schedule_routing_refresh(&self) {
        let service = self.clone();
        self.catalog_sync.global.request(move || {
            let service = service.clone();
            async move {
                if service.refresh_global_routing().await.is_err() {
                    log::warn!("Global routing catalog refresh failed; stale targets were revoked");
                }
                for (&app, job) in &service.catalog_sync.clients {
                    let service = service.clone();
                    job.request(move || {
                        let service = service.clone();
                        async move {
                            let started = std::time::Instant::now();
                            match app {
                                "codex" => {
                                    service.sync_codex_configuration().await;
                                    if service.intent_runtime.menu_active()
                                        && service.connect_codex_menu(false, false).await.is_err()
                                    {
                                        log::warn!("Codex model menu refresh failed; reconnect is required");
                                    }
                                }
                                "trae" => service.sync_shared_trae(false).await,
                                "trae-intl" => service.sync_shared_trae_intl(false).await,
                                _ => service.sync_shared_agent_catalog(app).await,
                            }
                            log::info!(
                                "Routing background sync completed: client={app} elapsed_ms={}",
                                started.elapsed().as_millis()
                            );
                        }
                    }).await;
                }
            }
        }).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn coalesces_bursts_and_keeps_the_last_edit_during_a_blocked_sync() {
        let job = Arc::new(Job::default());
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let revision = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let run = {
            let entered = entered.clone();
            let release = release.clone();
            let revision = revision.clone();
            let seen = seen.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                let revision = revision.clone();
                let seen = seen.clone();
                async move {
                    let current = revision.load(Ordering::SeqCst);
                    seen.lock().await.push(current);
                    if current == 0 {
                        entered.notify_one();
                        release.notified().await;
                    }
                }
            }
        };
        job.request(run.clone()).await;
        entered.notified().await;
        for next in 1..=100 {
            revision.store(next, Ordering::SeqCst);
            job.request(run.clone()).await;
        }
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), job.drain())
            .await
            .expect("worker drains");
        assert_eq!(*seen.lock().await, vec![0, 100]);
        revision.store(101, Ordering::SeqCst);
        job.request(run).await;
        tokio::time::timeout(Duration::from_secs(2), job.drain())
            .await
            .expect("worker restarts");
        assert_eq!(*seen.lock().await, vec![0, 100, 101]);
    }
}
