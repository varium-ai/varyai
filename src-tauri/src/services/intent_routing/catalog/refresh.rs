//! Bound catalog network work while retaining independently cached provider results.
use super::{CatalogStatus, Provider};
use futures::{stream, StreamExt};
use std::{future::Future, time::Duration};
use tokio::time::Instant;

pub(super) const TOTAL_BUDGET: Duration = Duration::from_secs(30);
pub(super) const UPSTREAM_BUDGET: Duration = Duration::from_secs(8);
pub(super) const CONCURRENCY: usize = 4;

// A refresh token may rotate before its new generation is persisted. Let an
// already started authentication transaction finish even when its catalog waiter expires.
pub(super) async fn complete_authentication<F, T>(work: F) -> Option<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(work).await.ok()
}

type Prepared = (Provider, Option<CatalogStatus>);

pub(super) async fn collect<F, Fut>(
    providers: impl IntoIterator<Item = Provider>,
    deadline: Option<Instant>,
    upstream_budget: Duration,
    prepare: F,
) -> Result<Vec<Prepared>, String>
where
    F: Fn(Provider, bool) -> Fut + Sync,
    Fut: Future<Output = Result<Prepared, String>> + Send,
{
    let mut results = stream::iter(providers.into_iter().enumerate())
        .map(|(index, provider)| {
            let prepare = &prepare;
            async move {
                let result = if let Some(deadline) = deadline {
                    let cached = provider.clone();
                    let result = if Instant::now() < deadline {
                        tokio::time::timeout_at(
                            deadline.min(Instant::now() + upstream_budget),
                            prepare(provider, true),
                        )
                        .await
                        .ok()
                    } else {
                        None
                    };
                    if let Some(result) = result {
                        result
                    } else {
                        // Re-read after cancellation: successful accounts in a partially
                        // completed pool may already have saved newer identity-scoped data.
                        let (provider, mut status) = prepare(cached, false).await?;
                        if let Some(status) = &mut status {
                            if status.reason != Some("notSignedIn") {
                                status.reason = Some("timeout");
                            }
                        }
                        Ok((provider, status))
                    }
                } else {
                    prepare(provider, false).await
                };
                Ok::<_, String>((index, result?))
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    // Completion order controls scheduling, not menu or status ordering.
    results.sort_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, result)| result).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn provider(index: usize) -> Provider {
        Provider::with_id(
            format!("synthetic-{index}"),
            "Synthetic relay".into(),
            json!({}),
            None,
        )
    }
    fn prepared(provider: Provider, refresh: bool) -> Prepared {
        let status = CatalogStatus {
            provider_id: provider.id.clone(),
            status: if refresh { "updated" } else { "cached" },
            model_count: 2,
            reason: None,
        };
        (provider, Some(status))
    }

    #[tokio::test]
    async fn catalog_cancellation_does_not_interrupt_an_authentication_commit() -> Result<(), String>
    {
        let entered = Arc::new(tokio::sync::Notify::new());
        let commit = Arc::new(tokio::sync::Notify::new());
        let (saved, persisted) = tokio::sync::oneshot::channel();
        let started = entered.clone();
        let finish = commit.clone();
        let waiter = tokio::spawn(complete_authentication(async move {
            started.notify_one();
            finish.notified().await;
            let _ = saved.send(());
        }));
        entered.notified().await;
        waiter.abort();
        assert!(waiter.await.is_err());
        commit.notify_one();
        tokio::time::timeout(Duration::from_secs(1), persisted)
            .await
            .map_err(|_| "Synthetic authentication commit timed out".to_string())?
            .map_err(|_| "Synthetic authentication commit was cancelled".to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn slow_catalog_reuses_cache_without_discarding_a_fast_result() -> Result<(), String> {
        let rows = collect(
            vec![provider(0), provider(1)],
            Some(Instant::now() + Duration::from_secs(2)),
            Duration::from_millis(10),
            |provider, refresh| async move {
                if refresh && provider.id == "synthetic-0" {
                    std::future::pending::<()>().await;
                }
                Ok(prepared(provider, refresh))
            },
        )
        .await?;
        assert_eq!(
            rows[0]
                .1
                .as_ref()
                .map(|s| (s.status, s.reason, s.model_count)),
            Some(("cached", Some("timeout"), 2))
        );
        assert_eq!(
            rows[1].1.as_ref().map(|s| (s.status, s.reason)),
            Some(("updated", None))
        );
        Ok(())
    }

    #[tokio::test]
    async fn total_deadline_prevents_queued_network_requests_from_starting() -> Result<(), String> {
        let started = Arc::new(AtomicUsize::new(0));
        let observed = started.clone();
        let rows = collect(
            (0..8).map(provider),
            Some(Instant::now() + Duration::from_millis(100)),
            Duration::from_secs(2),
            move |provider, refresh| {
                let observed = observed.clone();
                async move {
                    if refresh {
                        observed.fetch_add(1, Ordering::SeqCst);
                        std::future::pending::<()>().await;
                    }
                    Ok(prepared(provider, false))
                }
            },
        )
        .await?;
        assert_eq!(started.load(Ordering::SeqCst), CONCURRENCY);
        assert_eq!(rows.len(), 8);
        assert!(rows.iter().all(|(_, status)| status
            .as_ref()
            .is_some_and(|s| s.status == "cached" && s.reason == Some("timeout"))));
        Ok(())
    }

    #[tokio::test]
    async fn expired_budget_preserves_missing_identity_and_empty_cache_status() -> Result<(), String>
    {
        let rows = collect(
            vec![provider(0), provider(1)],
            Some(Instant::now()),
            UPSTREAM_BUDGET,
            |provider, refresh| async move {
                assert!(!refresh);
                let signed_out = provider.id == "synthetic-0";
                let status = CatalogStatus {
                    provider_id: provider.id.clone(),
                    status: "unavailable",
                    model_count: 0,
                    reason: Some(if signed_out {
                        "notSignedIn"
                    } else {
                        "requestFailed"
                    }),
                };
                Ok((provider, Some(status)))
            },
        )
        .await?;
        assert_eq!(
            rows[0].1.as_ref().and_then(|s| s.reason),
            Some("notSignedIn")
        );
        assert_eq!(
            rows[1]
                .1
                .as_ref()
                .map(|s| (s.status, s.reason, s.model_count)),
            Some(("unavailable", Some("timeout"), 0))
        );
        Ok(())
    }

    #[tokio::test]
    async fn expired_budget_uses_cache_without_polling_network_work() -> Result<(), String> {
        let rows = collect(
            vec![provider(0)],
            Some(Instant::now()),
            UPSTREAM_BUDGET,
            |provider, refresh| async move {
                assert!(!refresh);
                Ok(prepared(provider, refresh))
            },
        )
        .await?;
        assert_eq!(rows[0].1.as_ref().and_then(|s| s.reason), Some("timeout"));
        Ok(())
    }
}
