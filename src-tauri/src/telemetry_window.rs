use std::{borrow::Cow, sync::Weak};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use super::{AnalyticsReport, TelemetryManager};

pub(crate) const ANALYTICS_WINDOW_LABEL: &str = "analytics";
const ACK_COMMAND: &str = "acknowledge_analytics_report";

pub(crate) fn allows_command(label: &str, command: &str) -> bool {
    label != ANALYTICS_WINDOW_LABEL || command == ACK_COMMAND
}

// App commands are available by default in local WebViews even without a plugin
// capability. Explicitly deny every command except the token-scoped dispatch ack.
pub(crate) fn guard_invoke_handler<R: tauri::Runtime, F>(
    handler: F,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
where
    F: Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        if !allows_command(
            invoke.message.webview_ref().label(),
            invoke.message.command(),
        ) {
            invoke
                .resolver
                .reject("Command unavailable in analytics context");
            return true;
        }
        handler(invoke)
    }
}

fn allowed_url(url: &url::Url, dev_url: Option<&url::Url>) -> bool {
    if url.path() != "/analytics.html"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    if cfg!(debug_assertions) && dev_url.is_some_and(|dev| dev.origin() == url.origin()) {
        return true;
    }
    url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
        )
}

fn isolated_csp(original: &str) -> String {
    let scripts = original
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("script-src "))
        .unwrap_or("script-src 'self'");
    format!("default-src 'none'; {scripts} https://www.googletagmanager.com; connect-src 'self' ipc: http://ipc.localhost https://*.google-analytics.com https://*.analytics.google.com https://www.googletagmanager.com; img-src https://*.google-analytics.com https://*.analytics.google.com; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'")
}

fn isolate_response(response: &mut http::Response<Cow<'static, [u8]>>) {
    let original = response
        .headers()
        .get("Content-Security-Policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Ok(policy) = http::HeaderValue::from_str(&isolated_csp(original)) else {
        return;
    };
    response
        .headers_mut()
        .insert("Content-Security-Policy", policy);
    // Linux may embed the original app policy in a meta element. Replace it only
    // in this WebView's local analytics asset; the main app policy stays intact.
    if let Ok(meta) = regex::Regex::new(
        r#"(?i)<meta\b[^>]*http-equiv\s*=\s*["']Content-Security-Policy["'][^>]*>"#,
    ) {
        let body = String::from_utf8_lossy(response.body());
        if meta.is_match(&body) {
            let body = meta.replace_all(&body, "").into_owned().into_bytes();
            *response.body_mut() = Cow::Owned(body);
        }
    }
}

fn dispatch_script(report: &AnalyticsReport) -> Result<String, String> {
    let json = serde_json::to_string(report).map_err(|_| "Statistics report unavailable")?;
    Ok(format!("window.varyAnalytics?.report({json})"))
}

pub(super) fn show_report(
    app: &AppHandle,
    manager: Weak<TelemetryManager>,
    report: &AnalyticsReport,
) -> Result<(), String> {
    if let Some(view) = app.get_webview_window(ANALYTICS_WINDOW_LABEL) {
        return view
            .eval(dispatch_script(report)?)
            .map_err(|_| "Statistics dispatch unavailable".into());
    }
    let dev_url = app.config().build.dev_url.clone();
    let page_dev_url = dev_url.clone();
    WebviewWindowBuilder::new(
        app,
        ANALYTICS_WINDOW_LABEL,
        WebviewUrl::App("analytics.html".into()),
    )
    .title("Vary AI statistics")
    .visible(false)
    .focused(false)
    .skip_taskbar(true)
    .decorations(false)
    .resizable(false)
    .inner_size(1.0, 1.0)
    .incognito(true)
    .devtools(false)
    .disable_drag_drop_handler()
    .on_navigation(move |url| allowed_url(url, dev_url.as_ref()))
    .on_web_resource_request(|request, response| {
        if request.uri().path() == "/analytics.html" {
            isolate_response(response);
        }
    })
    .on_page_load(move |view, payload| {
        if payload.event() != tauri::webview::PageLoadEvent::Finished
            || !allowed_url(payload.url(), page_dev_url.as_ref())
        {
            return;
        }
        let Some(owner) = manager.upgrade() else {
            return;
        };
        if let Some(report) = owner.pending_report() {
            if let Ok(script) = dispatch_script(&report) {
                let _ = view.eval(script);
            }
        }
    })
    .build()
    .map(|_| ())
    .map_err(|_| "Statistics WebView unavailable".into())
}

pub(crate) fn destroy(app: &AppHandle) {
    if let Some(view) = app.get_webview_window(ANALYTICS_WINDOW_LABEL) {
        let _ = view.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tauri::command]
    fn analytics_isolation_fixture() -> &'static str {
        "fixture result"
    }

    #[test]
    fn invoke_guard_blocks_real_app_dispatch_from_the_analytics_webview() {
        let app = tauri::test::mock_builder()
            .invoke_handler(guard_invoke_handler(tauri::generate_handler![
                analytics_isolation_fixture
            ]))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        for label in [ANALYTICS_WINDOW_LABEL, "main"] {
            let view = WebviewWindowBuilder::new(&app, label, Default::default())
                .build()
                .expect("mock WebView");
            let result = if label == ANALYTICS_WINDOW_LABEL {
                Err("Command unavailable in analytics context")
            } else {
                Ok("fixture result")
            };
            tauri::test::assert_ipc_response(
                &view,
                tauri::webview::InvokeRequest {
                    cmd: "analytics_isolation_fixture".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().expect("mock URL"),
                    body: Default::default(),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
                result,
            );
        }
    }

    #[test]
    fn remote_tag_has_no_app_command_access_except_dispatch_ack() {
        for command in [
            "get_settings",
            "save_settings",
            "get_telemetry_preferences",
            "set_telemetry_enabled",
            "open_external",
            "get_subscription_quota",
            "plugin:fs|read_file",
        ] {
            assert!(!allows_command(ANALYTICS_WINDOW_LABEL, command));
            assert!(allows_command("main", command));
        }
        assert!(allows_command(ANALYTICS_WINDOW_LABEL, ACK_COMMAND));
    }

    #[test]
    fn analytics_navigation_is_limited_to_its_local_asset() {
        for url in [
            "tauri://localhost/analytics.html",
            "http://tauri.localhost/analytics.html",
            "https://tauri.localhost/analytics.html",
        ] {
            assert!(allowed_url(&url.parse().unwrap(), None));
        }
        for url in [
            "https://example.com/analytics.html",
            "tauri://localhost/index.html",
            "http://localhost:15722/analytics.html",
            "tauri://localhost/analytics.html?private=value",
            "http://tauri.localhost:15722/analytics.html",
            "http://user@tauri.localhost/analytics.html",
        ] {
            assert!(!allowed_url(&url.parse().unwrap(), None));
        }
    }

    #[test]
    fn policy_is_confined_to_google_and_preserves_script_nonces() {
        let policy = isolated_csp(
            "default-src 'self'; script-src 'self' 'nonce-fixture'; connect-src https: http:",
        );
        assert!(policy.contains("'nonce-fixture'"));
        assert!(policy.contains("https://www.googletagmanager.com"));
        assert!(!policy.contains("connect-src https: http:"));
        assert!(!policy.contains("'unsafe-inline'"));
        assert!(!policy.contains("'unsafe-eval'"));
        let mut response = http::Response::new(Cow::Borrowed(b"<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'self'\"></head></html>".as_slice()));
        isolate_response(&mut response);
        assert!(!String::from_utf8_lossy(response.body()).contains("<meta"));
        assert!(response.headers()["Content-Security-Policy"]
            .to_str()
            .unwrap()
            .contains("frame-src 'none'"));
    }
}
