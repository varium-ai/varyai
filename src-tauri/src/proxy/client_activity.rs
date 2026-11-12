//! Keep Codex background-service reloads away from active and recently completed requests.
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
#[cfg(any(target_os = "macos", windows, test))]
use std::time::Duration;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Instant,
};

#[derive(Default)]
pub(crate) struct Activity(Mutex<Sample>);
#[derive(Default)]
struct Sample {
    active: usize,
    completed: Option<Instant>,
}
impl Activity {
    #[cfg(any(target_os = "macos", windows, test))]
    pub(crate) fn busy(&self) -> bool {
        self.busy_at(Instant::now())
    }
    #[cfg(any(target_os = "macos", windows, test))]
    fn busy_at(&self, now: Instant) -> bool {
        self.0.lock().map_or(true, |sample| {
            sample.active > 0
                || sample
                    .completed
                    .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(10))
        })
    }
    fn begin(self: &Arc<Self>) -> Option<Guard> {
        let mut sample = self.0.lock().ok()?;
        sample.active = sample.active.saturating_add(1);
        Some(Guard(self.clone()))
    }
}
struct Guard(Arc<Activity>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut sample) = self.0 .0.lock() {
            sample.active = sample.active.saturating_sub(1);
            sample.completed = Some(Instant::now());
        }
    }
}
struct TrackedBody {
    body: Body,
    guard: Option<Guard>,
}
impl http_body::Body for TrackedBody {
    type Data = axum::body::Bytes;
    type Error = axum::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if matches!(frame, Poll::Ready(None | Some(Err(_)))) {
            self.guard.take();
        }
        frame
    }
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}
fn execution(method: &http::Method, path: &str) -> bool {
    if method != http::Method::POST {
        return false;
    }
    if matches!(
        path,
        "/intent/v1/responses"
            | "/backend-api/codex/responses"
            | "/backend-api/codex/responses/compact"
            | "/connections/codex/v1/responses"
    ) {
        return true;
    }
    let path = path.strip_prefix("/codex").unwrap_or(path);
    let path = path.strip_prefix("/v1").unwrap_or(path);
    let path = path.strip_prefix("/v1").unwrap_or(path);
    matches!(
        path,
        "/responses"
            | "/responses/compact"
            | "/chat/completions"
            | "/images/generations"
            | "/images/edits"
            | "/alpha/search"
    )
}
pub(crate) async fn observe(
    State(activity): State<Arc<Activity>>,
    request: Request,
    next: Next,
) -> Response {
    let guard = execution(request.method(), request.uri().path())
        .then(|| activity.begin())
        .flatten();
    let response = next.run(request).await;
    if guard.is_none() {
        return response;
    }
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, Body::new(TrackedBody { body, guard }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use http_body_util::{BodyExt, StreamBody};

    #[test]
    fn overlapping_requests_remain_busy_until_the_last_response_and_quiet_period() {
        let activity = Arc::new(Activity::default());
        let start = Instant::now();
        assert!(!activity.busy_at(start));
        let first = activity.begin();
        let second = activity.begin();
        assert!(activity.busy_at(start + Duration::from_secs(60)));
        drop(first);
        assert!(activity.busy_at(start + Duration::from_secs(60)));
        drop(second);
        let completed = Instant::now();
        assert!(activity.busy_at(completed));
        assert!(!activity.busy_at(completed + Duration::from_secs(11)));
    }

    #[tokio::test]
    async fn response_owner_preserves_frames_and_releases_on_completion_or_cancel(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let activity = Arc::new(Activity::default());
        let mut trailers = http::HeaderMap::new();
        trailers.insert("x-synthetic", http::HeaderValue::from_static("complete"));
        let frames = futures::stream::iter([
            Ok::<_, std::io::Error>(http_body::Frame::data(Bytes::from_static(b"synthetic"))),
            Ok(http_body::Frame::trailers(trailers.clone())),
        ]);
        let mut body = TrackedBody {
            body: Body::new(StreamBody::new(frames)),
            guard: activity.begin(),
        };
        assert_eq!(
            body.frame()
                .await
                .ok_or("missing data")??
                .into_data()
                .map_err(|_| "expected data")?,
            "synthetic"
        );
        assert!(activity.busy_at(Instant::now() + Duration::from_secs(60)));
        assert_eq!(
            body.frame()
                .await
                .ok_or("missing trailers")??
                .into_trailers()
                .map_err(|_| "expected trailers")?,
            trailers
        );
        assert!(body.frame().await.is_none());
        assert!(!activity.busy_at(Instant::now() + Duration::from_secs(11)));
        let abandoned = TrackedBody {
            body: Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>()),
            guard: activity.begin(),
        };
        assert!(activity.busy_at(Instant::now() + Duration::from_secs(60)));
        drop(abandoned);
        assert!(activity.busy());
        assert!(!activity.busy_at(Instant::now() + Duration::from_secs(11)));
        Ok(())
    }

    #[tokio::test]
    async fn gateway_stream_stays_busy_after_headers_until_the_client_receives_its_body(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{routing::post, Router};
        let activity = Arc::new(Activity::default());
        let release = Arc::new(tokio::sync::Notify::new());
        let handler_release = release.clone();
        let router = Router::new()
            .route(
                "/intent/v1/responses",
                post(move || {
                    let release = handler_release.clone();
                    async move {
                        Body::from_stream(async_stream::stream! {
                            yield Ok::<_, std::io::Error>(Bytes::from_static(b"first"));
                            release.notified().await;
                            yield Ok(Bytes::from_static(b"second"));
                        })
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                activity.clone(),
                observe,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        struct Abort(tokio::task::AbortHandle);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _abort = Abort(task.abort_handle());
        let mut response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?
            .post(format!("http://{address}/intent/v1/responses"))
            .send()
            .await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(
            response.chunk().await?.ok_or("missing first chunk")?,
            "first"
        );
        assert!(
            activity.busy_at(Instant::now() + Duration::from_secs(60)),
            "headers do not finish a streaming request"
        );
        release.notify_one();
        assert_eq!(response.bytes().await?, "second");
        assert!(
            activity.busy(),
            "the tool interval after completion still defers reload"
        );
        assert!(!activity.busy_at(Instant::now() + Duration::from_secs(11)));
        task.abort();
        let _ = task.await;
        Ok(())
    }

    #[test]
    fn codex_routes_count_execution_but_exclude_catalogs_and_other_agents() {
        for path in [
            "/intent/v1/responses",
            "/backend-api/codex/responses",
            "/backend-api/codex/responses/compact",
            "/connections/codex/v1/responses",
            "/v1/responses",
            "/codex/v1/responses/compact",
            "/v1/images/edits",
            "/v1/alpha/search",
        ] {
            assert!(execution(&http::Method::POST, path), "{path}");
        }
        for path in [
            "/intent/v1/models",
            "/backend-api/codex/models",
            "/health",
            "/grokbuild/v1/responses",
            "/connections/claude/v1/messages",
            "/workbuddy/v1/chat/completions",
        ] {
            assert!(!execution(&http::Method::POST, path), "{path}");
        }
        assert!(!execution(&http::Method::GET, "/v1/responses"));
    }
}
