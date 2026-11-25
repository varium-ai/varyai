use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, PartialEq, Eq)]
enum NotificationBackend {
    Modern,
    Plugin,
    Unavailable,
}

fn notification_backend(
    platform: &str,
    _is_dev: bool,
    bundle_matches: bool,
) -> NotificationBackend {
    if platform != "macos" {
        NotificationBackend::Plugin
    } else if bundle_matches {
        NotificationBackend::Modern
    } else {
        // Never switch a macOS app back to NSUserNotificationCenter after a
        // modern permission query, even if its bundle identity is invalid.
        NotificationBackend::Unavailable
    }
}

/// All frontend notification senders use this command so querying permissions
/// and submitting notifications cannot select different macOS API families.
#[tauri::command]
pub async fn send_system_notification(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let bundle_matches = macos::can_query(&app);
    #[cfg(not(target_os = "macos"))]
    let bundle_matches = false;

    match notification_backend(std::env::consts::OS, tauri::is_dev(), bundle_matches) {
        NotificationBackend::Modern => {
            #[cfg(target_os = "macos")]
            return macos::send(&title, &body).await;
            #[cfg(not(target_os = "macos"))]
            Err("native notifications are unavailable".into())
        }
        NotificationBackend::Plugin => {
            use tauri_plugin_notification::NotificationExt;
            app.notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|_| "notification submission failed".into())
        }
        NotificationBackend::Unavailable => Err("notification app identity is unavailable".into()),
    }
}

#[cfg(any(target_os = "macos", test))]
async fn await_submission(
    mut receiver: tokio::sync::mpsc::Receiver<bool>,
    timeout: std::time::Duration,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, receiver.recv()).await {
        Ok(Some(true)) => Ok(()),
        Ok(Some(false)) => Err("notification submission failed".into()),
        Ok(None) => Err("notification submission did not complete".into()),
        Err(_) => Err("notification submission timed out".into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Authorization {
    NotDetermined,
    Denied,
    Authorized,
    Provisional,
    Ephemeral,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPermissionStatus {
    authorization: Authorization,
    platform: String,
    is_dev: bool,
    can_request: bool,
    can_open_settings: bool,
    alerts_enabled: Option<bool>,
    sound_enabled: Option<bool>,
}

fn settings_url(platform: &str) -> Option<&'static str> {
    match platform {
        "macos" => Some("x-apple.systempreferences:com.apple.Notifications-Settings.extension"),
        "windows" => Some("ms-settings:notifications"),
        _ => None,
    }
}

fn unknown_status(platform: &str, is_dev: bool) -> NotificationPermissionStatus {
    NotificationPermissionStatus {
        authorization: Authorization::Unknown,
        platform: platform.into(),
        is_dev,
        can_request: false,
        can_open_settings: settings_url(platform).is_some(),
        alerts_enabled: None,
        sound_enabled: None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn authorization_from_native(value: isize) -> Authorization {
    match value {
        0 => Authorization::NotDetermined,
        1 => Authorization::Denied,
        2 => Authorization::Authorized,
        3 => Authorization::Provisional,
        4 => Authorization::Ephemeral,
        _ => Authorization::Unknown,
    }
}

/// Windows notification switches are opt-out DWORDs: an absent value keeps the
/// Windows default of allowed delivery, `0` disables, `1` enables. Any other
/// value is unreadable rather than guessed.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsSwitch {
    Enabled,
    Disabled,
    Unknown,
}

#[cfg(any(target_os = "windows", test))]
fn switch_from_dword(value: Option<u32>) -> WindowsSwitch {
    match value {
        None | Some(1) => WindowsSwitch::Enabled,
        Some(0) => WindowsSwitch::Disabled,
        Some(_) => WindowsSwitch::Unknown,
    }
}

/// Combines the global toast switch and the per-app switch. An explicit
/// disable at either level denies delivery; otherwise an unreadable switch
/// leaves the authorization undetermined instead of claiming either side.
#[cfg(any(target_os = "windows", test))]
fn windows_authorization(global: WindowsSwitch, app: WindowsSwitch) -> Authorization {
    use WindowsSwitch::{Disabled, Enabled, Unknown};
    match (global, app) {
        (Disabled, _) | (_, Disabled) => Authorization::Denied,
        (Unknown, _) | (_, Unknown) => Authorization::Unknown,
        (Enabled, Enabled) => Authorization::Authorized,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_status(
    global: WindowsSwitch,
    app: WindowsSwitch,
    is_dev: bool,
) -> NotificationPermissionStatus {
    let mut status = unknown_status("windows", is_dev);
    status.authorization = windows_authorization(global, app);
    // Windows has no authorization prompt to request.
    status.can_request = false;
    // Banner and sound sub-toggles are not readable through a stable API.
    status.alerts_enabled = None;
    status.sound_enabled = None;
    status
}

#[cfg(any(target_os = "macos", test))]
fn setting_from_native(value: isize) -> Option<bool> {
    match value {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// The desktop notification plugin reports Granted unconditionally. Query the OS instead.
/// Development apps use the same bundle identity and native API as release apps.
#[tauri::command]
pub async fn get_notification_permission_status(
    app: AppHandle,
) -> Result<NotificationPermissionStatus, String> {
    #[cfg(target_os = "macos")]
    if macos::can_query(&app) {
        return macos::query().await;
    }
    #[cfg(target_os = "windows")]
    {
        Ok(windows::query(&app))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(unknown_status(std::env::consts::OS, tauri::is_dev()))
    }
}

#[tauri::command]
pub async fn request_notification_permission(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Never request for the wrong identity, an unbundled executable, or a denied app.
        let status = get_notification_permission_status(app).await?;
        if status.can_request {
            return macos::request().await;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Err("notification authorization cannot be requested in this environment".into())
}

#[tauri::command]
pub async fn open_notification_settings(app: AppHandle) -> Result<(), String> {
    let url = settings_url(std::env::consts::OS)
        .ok_or_else(|| "notification settings are not supported on this platform".to_string())?;
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|_| "failed to open notification settings".into())
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// A missing value keeps the Windows default of allowed delivery; a value
    /// that cannot be read or interpreted stays unknown instead of guessed.
    fn read_switch(root: &RegKey, path: &str, name: &str) -> WindowsSwitch {
        let key = match root.open_subkey(path) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return switch_from_dword(None);
            }
            Err(_) => return WindowsSwitch::Unknown,
        };
        match key.get_value::<u32, _>(name) {
            Ok(value) => switch_from_dword(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => switch_from_dword(None),
            Err(_) => WindowsSwitch::Unknown,
        }
    }

    /// Windows has no authorization prompt: the notification plugin posts
    /// toasts under the Tauri identifier, so the per-app Settings switch lives
    /// under that same AUMID and the global switch gates every toast.
    pub fn query(app: &AppHandle) -> NotificationPermissionStatus {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let global = read_switch(
            &root,
            r"Software\Microsoft\Windows\CurrentVersion\PushNotifications",
            "ToastEnabled",
        );
        let app_path = format!(
            r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\{}",
            app.config().identifier
        );
        let per_app = read_switch(&root, &app_path, "Enabled");
        windows_status(global, per_app, tauri::is_dev())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::{
        define_class, msg_send,
        rc::Retained,
        runtime::{Bool, ProtocolObject},
        AnyThread,
    };
    use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSettings,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use std::{ptr::NonNull, sync::OnceLock, time::Duration};
    use tokio::sync::mpsc;

    define_class!(
        // SAFETY: NSObject imposes no subclassing requirements. This delegate
        // has no ivars or Drop implementation and only calls the completion block.
        #[unsafe(super = NSObject)]
        struct VaryNotificationDelegate;

        unsafe impl NSObjectProtocol for VaryNotificationDelegate {}

        unsafe impl UNUserNotificationCenterDelegate for VaryNotificationDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }
        }
    );

    // SAFETY: The delegate is stateless, never accesses UI or thread-local
    // objects, and is retained for the process lifetime. Callbacks can arrive
    // on Apple's notification queue and only invoke their supplied completion.
    unsafe impl Send for VaryNotificationDelegate {}
    unsafe impl Sync for VaryNotificationDelegate {}
    static DELEGATE: OnceLock<Retained<VaryNotificationDelegate>> = OnceLock::new();

    fn install_delegate(center: &UNUserNotificationCenter) {
        let delegate = DELEGATE.get_or_init(|| {
            // SAFETY: This stateless NSObject subclass uses NSObject's init.
            unsafe { msg_send![VaryNotificationDelegate::alloc(), init] }
        });
        center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
    }

    fn build_request(title: &str, body: &str) -> Retained<UNNotificationRequest> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let identifier = NSString::from_str(&uuid::Uuid::new_v4().to_string());
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None)
    }

    pub async fn send(title: &str, body: &str) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel(1);
        {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            install_delegate(&center);
            let request = build_request(title, body);
            let callback = RcBlock::new(move |error: *mut NSError| {
                // Copy only success/failure; NSError can contain notification
                // content and must never be returned to JS or written to logs.
                let _ = sender.try_send(error.is_null());
            });
            center.addNotificationRequest_withCompletionHandler(&request, Some(&callback));
        }
        await_submission(receiver, Duration::from_secs(5)).await
    }

    pub fn can_query(app: &AppHandle) -> bool {
        // UNUserNotificationCenter can raise an ObjC exception for bare executables.
        let bundle = NSBundle::mainBundle();
        bundle.bundlePath().to_string().ends_with(".app")
            && bundle
                .bundleIdentifier()
                .is_some_and(|id| id.to_string() == app.config().identifier)
    }

    pub async fn query() -> Result<NotificationPermissionStatus, String> {
        let is_dev = tauri::is_dev();
        let (sender, mut receiver) = mpsc::channel(1);
        {
            let callback = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                // Apple's completion handler supplies a non-null settings object valid
                // for this callback. Copy scalar values; no ObjC objects cross await.
                let settings = unsafe { settings.as_ref() };
                let authorization = authorization_from_native(settings.authorizationStatus().0);
                let _ = sender.try_send(NotificationPermissionStatus {
                    authorization,
                    platform: "macos".into(),
                    is_dev,
                    can_request: authorization == Authorization::NotDetermined,
                    can_open_settings: true,
                    alerts_enabled: setting_from_native(settings.alertSetting().0),
                    sound_enabled: setting_from_native(settings.soundSetting().0),
                });
            });
            UNUserNotificationCenter::currentNotificationCenter()
                .getNotificationSettingsWithCompletionHandler(&callback);
        }
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| "notification permission query timed out".to_string())?
            .ok_or_else(|| "notification permission query did not complete".into())
    }

    pub async fn request() -> Result<(), String> {
        let (sender, mut receiver) = mpsc::channel(1);
        {
            let callback = RcBlock::new(move |_granted: Bool, error: *mut NSError| {
                // A user denial is a valid result; the caller re-queries the OS status.
                let _ = sender.try_send(if error.is_null() {
                    Ok(())
                } else {
                    Err("notification authorization request failed".to_string())
                });
            });
            UNUserNotificationCenter::currentNotificationCenter()
                .requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                    &callback,
                );
        }
        tokio::time::timeout(Duration::from_secs(120), receiver.recv())
            .await
            .map_err(|_| "notification authorization request timed out".to_string())?
            .ok_or_else(|| "notification authorization request did not complete".to_string())?
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn request_preserves_localized_copy_and_has_unique_immediate_delivery_identity() {
            // Construct requests only: no notification center, system permission
            // prompt, or real notification from an unbundled unit-test binary.
            let first = build_request("敏感信息提醒", "检测到手机号码");
            let second = build_request("敏感信息提醒", "检测到手机号码");
            assert_eq!(first.content().title().to_string(), "敏感信息提醒");
            assert_eq!(first.content().body().to_string(), "检测到手机号码");
            assert_ne!(
                first.identifier().to_string(),
                second.identifier().to_string()
            );
            assert!(first.trigger().is_none());
            assert!(first.content().sound().is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_macos_never_falls_back_to_the_legacy_sender() {
        assert_eq!(
            notification_backend("macos", false, true),
            NotificationBackend::Modern
        );
        assert_eq!(
            notification_backend("macos", false, false),
            NotificationBackend::Unavailable
        );
        assert_eq!(
            notification_backend("macos", true, true),
            NotificationBackend::Modern
        );
        assert_eq!(
            notification_backend("macos", true, false),
            NotificationBackend::Unavailable
        );
        assert_eq!(
            notification_backend("windows", false, false),
            NotificationBackend::Plugin
        );
        assert_eq!(
            notification_backend("linux", false, false),
            NotificationBackend::Plugin
        );
    }

    #[tokio::test]
    async fn submission_requires_callback_success_and_reports_failure_or_missing_callback() {
        use std::time::Duration;
        for accepted in [true, false] {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            sender.try_send(accepted).unwrap();
            let result = await_submission(receiver, Duration::from_secs(1)).await;
            assert_eq!(result.is_ok(), accepted);
        }
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(sender);
        assert_eq!(
            await_submission(receiver, Duration::from_secs(1)).await,
            Err("notification submission did not complete".into())
        );
        let (_sender, receiver) = tokio::sync::mpsc::channel(1);
        assert_eq!(
            await_submission(receiver, Duration::ZERO).await,
            Err("notification submission timed out".into())
        );
    }

    #[test]
    fn native_authorization_preserves_denial_and_unknown_future_values() {
        assert_eq!(authorization_from_native(0), Authorization::NotDetermined);
        assert_eq!(authorization_from_native(1), Authorization::Denied);
        assert_eq!(authorization_from_native(2), Authorization::Authorized);
        assert_eq!(authorization_from_native(3), Authorization::Provisional);
        assert_eq!(authorization_from_native(4), Authorization::Ephemeral);
        assert_eq!(authorization_from_native(99), Authorization::Unknown);
    }

    #[test]
    fn unsupported_notification_settings_are_not_reported_as_disabled() {
        assert_eq!(setting_from_native(0), None);
        assert_eq!(setting_from_native(1), Some(false));
        assert_eq!(setting_from_native(2), Some(true));
        assert_eq!(setting_from_native(99), None);
    }

    #[test]
    fn dev_and_unsupported_platforms_never_claim_permission() {
        let dev = unknown_status("macos", true);
        assert_eq!(dev.authorization, Authorization::Unknown);
        assert!(!dev.can_request);
        assert!(dev.can_open_settings);
        assert_eq!(dev.alerts_enabled, None);
        let linux = unknown_status("linux", false);
        assert!(!linux.can_open_settings);
        assert!(!linux.can_request);
        assert_eq!(settings_url("windows"), Some("ms-settings:notifications"));
    }

    #[test]
    fn windows_switch_values_treat_absent_as_enabled_and_refuse_unrecognized_dwords() {
        assert_eq!(switch_from_dword(None), WindowsSwitch::Enabled);
        assert_eq!(switch_from_dword(Some(1)), WindowsSwitch::Enabled);
        assert_eq!(switch_from_dword(Some(0)), WindowsSwitch::Disabled);
        assert_eq!(switch_from_dword(Some(2)), WindowsSwitch::Unknown);
        assert_eq!(switch_from_dword(Some(u32::MAX)), WindowsSwitch::Unknown);
    }

    #[test]
    fn windows_authorization_denies_on_disable_and_never_guesses_unreadable_switches() {
        use WindowsSwitch::{Disabled, Enabled, Unknown};
        // A global toast disable denies delivery even when the per-app switch is readable.
        assert_eq!(
            windows_authorization(Disabled, Enabled),
            Authorization::Denied
        );
        assert_eq!(
            windows_authorization(Disabled, Unknown),
            Authorization::Denied
        );
        // A per-app disable denies delivery even when the global switch is unreadable.
        assert_eq!(
            windows_authorization(Unknown, Disabled),
            Authorization::Denied
        );
        assert_eq!(
            windows_authorization(Enabled, Disabled),
            Authorization::Denied
        );
        // Absent switches keep the Windows default of allowed delivery.
        assert_eq!(
            windows_authorization(Enabled, Enabled),
            Authorization::Authorized
        );
        // Unreadable switches are never reported as authorized or disabled.
        assert_eq!(
            windows_authorization(Unknown, Enabled),
            Authorization::Unknown
        );
        assert_eq!(
            windows_authorization(Enabled, Unknown),
            Authorization::Unknown
        );
    }

    #[test]
    fn windows_status_never_claims_a_request_step_or_unverifiable_alert_settings() {
        let authorized = windows_status(WindowsSwitch::Enabled, WindowsSwitch::Enabled, true);
        assert_eq!(authorized.authorization, Authorization::Authorized);
        assert_eq!(authorized.platform, "windows");
        assert!(authorized.is_dev);
        assert!(!authorized.can_request);
        assert!(authorized.can_open_settings);
        assert_eq!(authorized.alerts_enabled, None);
        assert_eq!(authorized.sound_enabled, None);
        let denied = windows_status(WindowsSwitch::Enabled, WindowsSwitch::Disabled, false);
        assert_eq!(denied.authorization, Authorization::Denied);
        assert!(!denied.can_request);
        assert!(denied.can_open_settings);
        assert_eq!(denied.alerts_enabled, None);
        assert_eq!(denied.sound_enabled, None);
    }
}
