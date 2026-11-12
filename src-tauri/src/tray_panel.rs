//! Tray webview lifecycle and metadata-only cross-window coordination.
use std::sync::Mutex;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::{PhysicalPosition, PhysicalSize};

pub const LABEL: &str = "tray-panel";
const HEIGHT: f64 = 520.0;
#[cfg(target_os = "macos")]
#[path = "tray_panel/macos.rs"]
mod macos;
#[derive(Default)]
pub struct PanelState {
    navigation: Mutex<Option<String>>,
    hidden: Mutex<Option<std::time::Instant>>,
}

fn destination(value: &str) -> bool {
    matches!(
        value,
        "agents" | "routing" | "security" | "settings" | "providers" | "stats"
    )
}
fn shared_root(value: &str) -> bool {
    matches!(
        value,
        "agent-connections"
            | "agent-native-models"
            | "agent-model-catalog"
            | "agent-installations"
            | "proxyStatus"
            | "intent-routing-groups"
            | "routing-trace"
            | "security"
            | "securityEvents"
            | "profiles"
            | "routing-upstreams"
            | "settings"
    )
}

// All coordinates are physical pixels, including negative monitor origins.
#[cfg(any(not(target_os = "macos"), test))]
fn placement(work: [f64; 4], anchor: [f64; 4], scale: f64) -> [f64; 4] {
    let gap = 6.0 * scale;
    let width = (440.0 * scale).min((work[2] - 2.0 * gap).max(1.0));
    let height = (HEIGHT * scale).min((work[3] - 2.0 * gap).max(1.0));
    let x = (anchor[0] + anchor[2] / 2.0 - width / 2.0).clamp(
        work[0] + gap,
        (work[0] + work[2] - width - gap).max(work[0] + gap),
    );
    let below = anchor[1] + anchor[3] + gap;
    let y = if below + height <= work[1] + work[3] - gap {
        below
    } else {
        anchor[1] - height - gap
    };
    [
        x,
        y.clamp(
            work[1] + gap,
            (work[1] + work[3] - height - gap).max(work[1] + gap),
        ),
        width,
        height,
    ]
}

#[cfg(not(target_os = "macos"))]
fn position(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window(LABEL).ok_or("trayPanel.failed")?;
    let rect = app
        .tray_by_id(crate::tray::TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten());
    let monitor = if let Some(ref rect) = rect {
        let point = rect
            .position
            .to_physical::<f64>(window.scale_factor().unwrap_or(1.0));
        app.monitor_from_point(point.x, point.y).ok().flatten()
    } else {
        None
    }
    .or_else(|| app.primary_monitor().ok().flatten())
    .ok_or("trayPanel.failed")?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let anchor = rect
        .map(|rect| {
            let p = rect.position.to_physical::<f64>(scale);
            let s = rect.size.to_physical::<f64>(scale);
            [p.x, p.y, s.width, s.height]
        })
        .unwrap_or([
            f64::from(work.position.x + work.size.width as i32) - 32.0 * scale,
            f64::from(work.position.y),
            0.0,
            0.0,
        ]);
    let [x, y, w, h] = placement(
        [
            f64::from(work.position.x),
            f64::from(work.position.y),
            f64::from(work.size.width),
            f64::from(work.size.height),
        ],
        anchor,
        scale,
    );
    window
        .set_size(PhysicalSize::new(w as u32, h as u32))
        .map_err(|_| "trayPanel.failed")?;
    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|_| "trayPanel.failed".into())
}

pub fn toggle(app: &tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if toggle_on_main(&handle).is_err() {
            log::warn!("Unable to show tray panel");
        }
    })
    .map_err(|_| "trayPanel.failed".into())
}

fn toggle_on_main(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if macos::hide_if_open()? {
        return Ok(());
    }
    let state = app.state::<PanelState>();
    if state
        .hidden
        .lock()
        .map_err(|_| "trayPanel.failed")?
        .is_some_and(|time| time.elapsed().as_millis() < 200)
    {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        #[cfg(target_os = "macos")]
        let _ = window;
        #[cfg(not(target_os = "macos"))]
        if window.is_visible().unwrap_or(false) {
            return window.hide().map_err(|_| "trayPanel.failed".into());
        }
    } else {
        let builder =
            WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?panel=tray".into()))
                .title("Vary AI")
                .inner_size(440.0, HEIGHT)
                .visible(false)
                .focused(false)
                .decorations(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .shadow(true);
        #[cfg(target_os = "macos")]
        let builder = builder
            .transparent(true)
            .background_color(tauri::window::Color(0, 0, 0, 0));
        #[cfg(not(target_os = "macos"))]
        let builder = {
            let theme = app
                .get_webview_window("main")
                .and_then(|window| window.theme().ok());
            builder.background_color(fallback_background(theme))
        };
        builder.build().map_err(|_| "trayPanel.failed")?;
    }
    #[cfg(target_os = "macos")]
    return macos::show(app);
    #[cfg(not(target_os = "macos"))]
    {
        position(app)?;
        let window = app.get_webview_window(LABEL).ok_or("trayPanel.failed")?;
        window.show().map_err(|_| "trayPanel.failed")?;
        let _ = window.set_focus();
        let _ = window.emit("tray-panel-refresh", ());
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
fn fallback_background(theme: Option<tauri::Theme>) -> tauri::window::Color {
    match theme {
        Some(tauri::Theme::Dark) => tauri::window::Color(24, 32, 44, 255),
        _ => tauri::window::Color(244, 246, 249, 255),
    }
}

pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != LABEL {
        return;
    }
    // The native NSPanel owns its own focus lifecycle, not the hidden Tauri host.
    #[cfg(target_os = "macos")]
    {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let tauri::WindowEvent::ThemeChanged(theme) = event {
            let _ = window.set_background_color(Some(fallback_background(Some(*theme))));
        }
        if matches!(event, tauri::WindowEvent::Focused(false)) {
            let app = window.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                if let Some(panel) = app.get_webview_window(LABEL) {
                    if !panel.is_focused().unwrap_or(true) {
                        if let Ok(mut time) = app.state::<PanelState>().hidden.lock() {
                            *time = Some(std::time::Instant::now());
                        }
                        let _ = panel.hide();
                    }
                }
            });
        }
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

#[tauri::command]
pub async fn tray_panel_resize(window: tauri::Window, height: f64) -> Result<(), String> {
    if window.label() != LABEL || !height.is_finite() {
        return Err("trayPanel.failed".into());
    }
    let app = window.app_handle().clone();
    let (send, recv) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        // Retain the IPC entry point for older renderers, but enforce fixed geometry.
        let result = macos::resize();
        #[cfg(not(target_os = "macos"))]
        let result = position(&handle);
        #[cfg(target_os = "macos")]
        let _ = handle;
        let _ = send.send(result);
    })
    .map_err(|_| "trayPanel.failed")?;
    recv.await.map_err(|_| "trayPanel.failed")?
}
#[tauri::command]
pub fn tray_panel_hide(window: tauri::Window) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("trayPanel.failed".into());
    }
    hide(window.app_handle())
}

fn hide(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return app
        .run_on_main_thread(|| {
            let _ = macos::hide_if_open();
        })
        .map_err(|_| "trayPanel.failed".into());
    #[cfg(not(target_os = "macos"))]
    app.get_webview_window(LABEL)
        .ok_or("trayPanel.failed")?
        .hide()
        .map_err(|_| "trayPanel.failed".into())
}
#[tauri::command]
pub fn tray_panel_open(window: tauri::Window, view: String) -> Result<(), String> {
    if window.label() != LABEL || !destination(&view) {
        return Err("trayPanel.failed".into());
    }
    let app = window.app_handle();
    *app.state::<PanelState>()
        .navigation
        .lock()
        .map_err(|_| "trayPanel.failed")? = Some(view);
    crate::tray::show_main(app)?;
    // The pending slot also covers a main webview recreated after lightweight mode.
    let _ = app.emit_to("main", "workspace-navigation", ());
    let _ = hide(app);
    Ok(())
}
#[tauri::command]
pub fn take_workspace_navigation(window: tauri::Window) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("trayPanel.failed".into());
    }
    let value = window
        .app_handle()
        .state::<PanelState>()
        .navigation
        .lock()
        .map_err(|_| "trayPanel.failed")?
        .take();
    Ok(value)
}
#[tauri::command]
pub fn broadcast_workspace_change(window: tauri::Window, root: String) -> Result<(), String> {
    if !matches!(window.label(), "main" | LABEL) || !shared_root(&root) {
        return Err("trayPanel.failed".into());
    }
    let other = if window.label() == "main" {
        LABEL
    } else {
        "main"
    };
    let _ = window
        .app_handle()
        .emit_to(other, "workspace-invalidated", root);
    Ok(())
}
#[tauri::command]
pub fn tray_panel_quit(window: tauri::Window) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("trayPanel.failed".into());
    }
    window.app_handle().exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fits_retina_and_negative_monitor_origins() {
        let p = placement(
            [-2560.0, 0.0, 2560.0, 1440.0],
            [-25.0, 0.0, 22.0, 24.0],
            2.0,
        );
        assert!(p[0] >= -2548.0 && p[0] + p[2] <= -12.0);
        assert!(p[1] >= 12.0 && p[1] + p[3] <= 1428.0);
        assert_eq!(p[2], 880.0);
        assert_eq!(p[3], HEIGHT * 2.0);
        assert_eq!(p[0] + p[2], -12.0);
    }
    #[test]
    fn opens_above_bottom_tray_and_clamps_small_work_area() {
        let p = placement([0.0, 0.0, 800.0, 600.0], [780.0, 580.0, 20.0, 20.0], 1.0);
        assert!(p[0] + p[2] <= 794.0 && p[1] + p[3] <= 594.0);
        assert!(p[1] >= 6.0);
        assert_eq!(p[3], HEIGHT);
        let small = placement([0.0, 0.0, 360.0, 400.0], [340.0, 0.0, 20.0, 20.0], 1.0);
        assert_eq!(small[3], 388.0);
    }
    #[test]
    fn coordination_accepts_only_metadata_and_known_destinations() {
        assert!(destination("security"));
        assert!(!destination("https://example.invalid"));
        assert!(shared_root("security"));
        assert!(!shared_root("arbitrary-data"));
    }
}
