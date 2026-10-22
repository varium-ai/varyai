//! 项目 Profile 管理命令

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::database::Profile;
use crate::services::profile::{ProfilePayload, ProfileScope, ProfileService};
use crate::services::profile_bundle::{ProfileApplyResult, ProfileBundleService, ProfilePreview};
use crate::store::AppState;

async fn run_profile_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| "profiles.bundle.operationFailed".to_owned())?
}

#[cfg(test)]
mod responsiveness_tests {
    #[tokio::test(flavor = "current_thread")]
    async fn filesystem_work_does_not_block_the_async_event_loop() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let task = super::run_profile_task(move || {
            let _ = started.send(());
            wait.recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|_| "event loop was blocked".to_owned())
        });
        let heartbeat = async move {
            ready.await.unwrap();
            let _ = release.send(());
        };
        let (result, ()) = tokio::join!(task, heartbeat);
        assert!(result.is_ok(), "profile I/O blocked the event loop");
    }
}

#[tauri::command]
pub async fn capture_profile_bundle(
    state: State<'_, AppState>,
    name: String,
    apps: Vec<String>,
    id: Option<String>,
) -> Result<ProfileDto, String> {
    let state = state.inner().clone();
    run_profile_task(move || {
        ProfileBundleService::capture(&state, &name, &apps, id.as_deref())
            .map(ProfileDto::from)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn preview_profile_bundle(
    state: State<'_, AppState>,
    id: String,
) -> Result<ProfilePreview, String> {
    let state = state.inner().clone();
    run_profile_task(move || ProfileBundleService::preview(&state, &id).map_err(|e| e.to_string()))
        .await
}

#[tauri::command]
pub async fn apply_profile_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    revision: String,
) -> Result<ProfileApplyResult, String> {
    let state = state.inner().clone();
    run_profile_task(move || {
        let result =
            ProfileBundleService::apply(&state, &id, &revision).map_err(|e| e.to_string())?;
        emit_bundle_events(&app, &state, &result.scopes);
        crate::tray::refresh_tray_menu(&app);
        Ok(result)
    })
    .await
}

#[tauri::command]
pub async fn undo_profile_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    undo_id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_profile_task(move || {
        let scopes = ProfileBundleService::undo(&state, &undo_id).map_err(|e| e.to_string())?;
        emit_bundle_events(&app, &state, &scopes);
        crate::tray::refresh_tray_menu(&app);
        Ok(())
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub payload: ProfilePayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

impl From<Profile> for ProfileDto {
    fn from(profile: Profile) -> Self {
        // 单条 payload 损坏不应拖垮整个列表：降级为默认值并记日志
        let payload = serde_json::from_str(&profile.payload).unwrap_or_else(|e| {
            log::warn!(
                "解析 profile '{}' payload 失败，使用默认值: {e}",
                profile.id
            );
            ProfilePayload::default()
        });
        Self {
            id: profile.id,
            name: profile.name,
            payload,
            created_at: profile.created_at,
            updated_at: profile.updated_at,
        }
    }
}

/// 每个分组当前激活的项目 id（未使用项目时为 null）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProfileIds {
    pub claude: Option<String>,
    pub claude_desktop: Option<String>,
    pub codex: Option<String>,
    pub by_app: std::collections::HashMap<String, Option<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesResponse {
    pub profiles: Vec<ProfileDto>,
    pub current_ids: CurrentProfileIds,
    pub agents: Vec<crate::services::profile_bundle::ProfileAgent>,
    pub undo: Option<crate::services::profile_bundle::ProfileUndo>,
}

/// Profile 应用完成后的统一收尾：发事件 + 重建托盘菜单
///
/// 只对项目所属分组内的应用发 provider-switched。UI 与托盘两个入口必须
/// 共用此函数，保证事件 payload 形状一致（前端 App.tsx 的
/// provider-switched 监听依赖该形状）。
pub fn emit_profile_apply_events(
    app: &tauri::AppHandle,
    state: &AppState,
    profile_id: &str,
    scope: ProfileScope,
) {
    for app_type in scope.apps().iter() {
        let app_str = app_type.as_str();
        let (proxy_enabled, auto_failover_enabled) = state.db.get_proxy_flags_sync(app_str);
        let provider_id = crate::settings::get_effective_current_provider(&state.db, app_type)
            .ok()
            .flatten()
            .unwrap_or_default();
        let event_data = serde_json::json!({
            "appType": app_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": auto_failover_enabled,
            "providerId": provider_id,
        });
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    if let Err(e) = app.emit(
        "profile-applied",
        serde_json::json!({ "profileId": profile_id, "scope": scope.as_str() }),
    ) {
        log::error!("发射 profile-applied 事件失败: {e}");
    }
    crate::tray::refresh_tray_menu(app);
}

fn emit_bundle_events(
    app: &tauri::AppHandle,
    state: &AppState,
    agents: &[crate::app_config::AppType],
) {
    for agent in agents {
        let (proxy_enabled, auto_failover_enabled) = state.db.get_proxy_flags_sync(agent.as_str());
        let provider_id = crate::settings::get_effective_current_provider(&state.db, agent)
            .ok()
            .flatten()
            .unwrap_or_default();
        let _=app.emit("provider-switched",serde_json::json!({"appType":agent.as_str(),"proxyEnabled":proxy_enabled,"autoFailoverEnabled":auto_failover_enabled,"providerId":provider_id}));
    }
    // One scheme is one refresh, even when it covers many agents.
    let first = agents.first();
    let profile_id = first.and_then(|agent| {
        state
            .db
            .get_current_profile_id(agent.as_str())
            .ok()
            .flatten()
    });
    let _ = app.emit(
        "profile-applied",
        serde_json::json!({"profileId":profile_id.unwrap_or_default(),"scope":first.map(|agent| agent.as_str()),"scopes":agents.iter().map(|agent|agent.as_str()).collect::<Vec<_>>()}),
    );
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<ProfilesResponse, String> {
    let profiles = ProfileService::list(&state).map_err(|e| e.to_string())?;
    let by_app = crate::app_config::AppType::all()
        .map(|app| {
            Ok((
                app.as_str().to_owned(),
                state
                    .db
                    .get_current_profile_id(app.as_str())
                    .map_err(|e| e.to_string())?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let current_ids = CurrentProfileIds {
        by_app,
        claude: state
            .db
            .get_current_profile_id(ProfileScope::Claude.as_str())
            .map_err(|e| e.to_string())?,
        claude_desktop: state
            .db
            .get_current_profile_id(ProfileScope::ClaudeDesktop.as_str())
            .map_err(|e| e.to_string())?,
        codex: state
            .db
            .get_current_profile_id(ProfileScope::Codex.as_str())
            .map_err(|e| e.to_string())?,
    };
    Ok(ProfilesResponse {
        profiles: profiles.into_iter().map(ProfileDto::from).collect(),
        current_ids,
        agents: crate::services::profile_bundle::agents(&state).map_err(|e| e.to_string())?,
        undo: ProfileBundleService::last_undo(&state).map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, AppState>,
    name: String,
    scope: String,
) -> Result<ProfileDto, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    ProfileService::create(&state, &name, scope)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
) -> Result<ProfileDto, String> {
    let scope = scope
        .map(|s| ProfileScope::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    ProfileService::update(&state, &id, name, resnapshot.unwrap_or(false), scope)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ProfileService::delete(&state, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_current_profile(state: State<'_, AppState>, scope: String) -> Result<(), String> {
    let scope = scope
        .parse::<crate::app_config::AppType>()
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_current_profile_id(scope.as_str(), None)
        .map_err(|e| e.to_string())
}

/// 应用项目快照（只作用于发起页所属分组内的应用）。
///
/// 注意：必须保持同步命令（跑在 Tauri 线程池）——`ProviderService::switch`
/// 内部使用 block_on 获取切换锁，放进 async 命令会在运行时线程上 panic。
#[tauri::command]
pub fn apply_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    let (warnings, should_stop_proxy) =
        ProfileService::apply(&state, &id, scope).map_err(|e| e.to_string())?;

    if should_stop_proxy {
        // sync 命令线程没有 Tokio runtime，无法直接 await stop()；
        // 把停止服务放到 Tauri async runtime，停止后再补发事件刷新 UI。
        let app_handle = app.clone();
        let profile_id = id.clone();
        let proxy_service = state.proxy_service.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = proxy_service.stop().await {
                log::warn!("切换项目后停止代理服务失败: {e}");
            }
            if let Some(app_state) = app_handle.try_state::<AppState>() {
                emit_profile_apply_events(&app_handle, app_state.inner(), &profile_id, scope);
            }
        });
    } else {
        emit_profile_apply_events(&app, &state, &id, scope);
    }

    Ok(warnings)
}
