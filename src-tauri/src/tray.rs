//! Native fallback menu. Interactive controls live in the shared tray webview.
use crate::{error::AppError, store::AppState};
use tauri::{
    menu::{Menu, MenuBuilder},
    Emitter, Manager,
};
use tauri_plugin_opener::OpenerExt;

pub const TRAY_ID: &str = "cc-switch";

fn language() -> String {
    crate::settings::get_settings()
        .language
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en".into())
}

pub fn create_tray_menu(
    app: &tauri::AppHandle,
    _state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let lang = language().to_lowercase();
    let labels = if lang.starts_with("zh-tw") || lang.starts_with("zh-hk") || lang.contains("hant")
    {
        ["開啟快速面板", "開啟主介面", "開啟官方網站", "退出"]
    } else if lang.starts_with("zh") {
        ["打开快捷面板", "打开主界面", "打开官方网站", "退出"]
    } else if lang.starts_with("ja") {
        [
            "クイックパネル",
            "メインウィンドウを開く",
            "公式サイト",
            "終了",
        ]
    } else {
        [
            "Open quick panel",
            "Open main window",
            "Official website",
            "Quit",
        ]
    };
    MenuBuilder::new(app)
        .text("quick_panel", labels[0])
        .text("show_main", labels[1])
        .text("open_website", labels[2])
        .separator()
        .text("quit", labels[3])
        .build()
        .map_err(|_| AppError::Message("Unable to build tray menu".into()))
}

pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let (Ok(menu), Some(tray)) = (
            create_tray_menu(app, state.inner()),
            app.tray_by_id(TRAY_ID),
        ) {
            if tray.set_menu(Some(menu)).is_err() {
                log::warn!("Unable to refresh tray menu");
            }
        }
    }
    schedule_tray_refresh(app);
}

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    let _ = app.emit("tray-panel-refresh", ());
}

pub fn handle_tray_menu_event(app: &tauri::AppHandle, id: &str) {
    match id {
        "quick_panel" => {
            let _ = crate::tray_panel::toggle(app);
        }
        "show_main" => {
            let _ = show_main(app);
        }
        "open_website" => {
            let _ = app
                .opener()
                .open_url("https://www.varyai.net/", None::<String>);
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

pub(crate) fn show_main(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("main").is_none() {
        crate::lightweight::exit_lightweight_mode(app)?;
    }
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        let _ = window.set_skip_taskbar(false);
        let _ = window.unminimize();
        window.show().map_err(|_| "trayPanel.failed".to_string())?;
        let _ = window.set_focus();
        #[cfg(target_os = "linux")]
        crate::linux_fix::nudge_main_window(window);
        #[cfg(target_os = "macos")]
        apply_tray_policy(app, true);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn security_audit_icon(enabled: bool) -> tauri::Result<tauri::image::Image<'static>> {
    let bytes: &[u8] = if enabled {
        include_bytes!("../icons/tray/macos/statusbar_audit_enabled_3x.png")
    } else {
        include_bytes!("../icons/tray/macos/statusbar_template_3x.png")
    };
    tauri::image::Image::from_bytes(bytes)
}

#[cfg(target_os = "macos")]
pub(crate) fn update_security_audit_icon(app: &tauri::AppHandle, enabled: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        match security_audit_icon(enabled) {
            Ok(icon) => {
                if let Err(error) = tray.set_icon(Some(icon)) {
                    log::warn!("Failed to update macOS status bar icon: {error}");
                }
            }
            Err(error) => log::warn!("Failed to load macOS status bar icon: {error}"),
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn update_security_audit_icon(_app: &tauri::AppHandle, _enabled: bool) {}

#[cfg(target_os = "macos")]
pub fn apply_tray_policy(app: &tauri::AppHandle, dock_visible: bool) {
    use tauri::ActivationPolicy;

    let desired_policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };

    if let Err(err) = app.set_activation_policy(desired_policy) {
        log::warn!("设置激活策略失败: {err}");
    }

    if dock_visible {
        if let Err(err) = app.run_on_main_thread(|| {
            use objc2::{AllocAnyThread, MainThreadMarker};
            use objc2_app_kit::{NSApplication, NSImage};
            use objc2_foundation::NSData;

            let Some(main_thread) = MainThreadMarker::new() else {
                log::warn!("无法在主线程恢复 Dock 图标");
                return;
            };
            let data = NSData::with_bytes(include_bytes!("../icons/icon.png"));
            let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
                log::warn!("无法解码 Dock 图标");
                return;
            };
            unsafe {
                NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&icon));
            }
        }) {
            log::warn!("恢复 Dock 图标失败: {err}");
        }
    }
}
