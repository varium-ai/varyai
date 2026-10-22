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
    let _ = app;
    Ok(unknown_status(std::env::consts::OS, tauri::is_dev()))
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
}
