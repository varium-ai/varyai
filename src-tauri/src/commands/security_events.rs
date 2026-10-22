//! Unified security-event IPC; all database work runs on the blocking pool.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::State;

use crate::database::{
    Database, SecurityEventError, SecurityEventFilter, SecurityEventPage, SecurityEventSummary,
    SecurityEventsClearResult,
};
use crate::store::AppState;

// Accept JSON at the boundary so serde failures (including unknown enum values)
// share the same safe error as semantic validation, without echoing IPC input.
fn parse<T: DeserializeOwned>(value: Value) -> Result<T, SecurityEventError> {
    serde_json::from_value(value).map_err(|_| SecurityEventError::InvalidParameters)
}

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SecurityEventError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| "SECURITY_EVENTS_TASK_ERROR".to_owned())?
        .map_err(|error| error.to_string())
}

async fn events(
    db: Arc<Database>,
    filter: Value,
    page: Value,
    page_size: Value,
) -> Result<SecurityEventPage, String> {
    blocking(move || {
        let filter: SecurityEventFilter = parse(filter)?;
        db.get_security_events(&filter, parse(page)?, parse(page_size)?)
    })
    .await
}
async fn summary(db: Arc<Database>, filter: Value) -> Result<SecurityEventSummary, String> {
    blocking(move || db.get_security_events_summary(&parse(filter)?)).await
}
async fn clear(db: Arc<Database>, filter: Value) -> Result<SecurityEventsClearResult, String> {
    blocking(move || db.clear_security_events(&parse(filter)?)).await
}

#[tauri::command]
pub async fn get_security_events(
    state: State<'_, AppState>,
    filter: Value,
    page: Value,
    page_size: Value,
) -> Result<SecurityEventPage, String> {
    events(state.db.clone(), filter, page, page_size).await
}
#[tauri::command]
pub async fn get_security_events_summary(
    state: State<'_, AppState>,
    filter: Value,
) -> Result<SecurityEventSummary, String> {
    summary(state.db.clone(), filter).await
}
#[tauri::command]
pub async fn clear_security_events(
    state: State<'_, AppState>,
    filter: Value,
) -> Result<SecurityEventsClearResult, String> {
    clear(state.db.clone(), filter).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn invalid_ipc_shapes_and_enums_return_fixed_errors() {
        let db = Arc::new(Database::memory().unwrap());
        for filter in [
            json!({"view":"secret-unknown-view"}),
            json!({"view":"security_events","riskLevel":"secret-unknown-level"}),
            json!({"view":"tool_calls","toolVerdict":"secret-unknown-verdict"}),
            json!({"view":"security_events","surprise":"secret-input"}),
            json!({"view":"security_events","keyword":"secret".repeat(100)}),
        ] {
            let error = events(db.clone(), filter, json!(1), json!(50))
                .await
                .unwrap_err();
            assert_eq!(error, "SECURITY_EVENTS_INVALID_PARAMETERS");
        }
        for page in [
            json!(-1),
            json!(0),
            json!(1.5),
            json!("secret-page"),
            json!(18446744073709551615_u64),
        ] {
            assert_eq!(
                events(
                    db.clone(),
                    json!({"view":"security_events"}),
                    page,
                    json!(50)
                )
                .await
                .unwrap_err(),
                "SECURITY_EVENTS_INVALID_PARAMETERS"
            );
        }
        assert_eq!(
            summary(db.clone(), json!({"view":"tool_calls"}))
                .await
                .unwrap_err(),
            "SECURITY_EVENTS_INVALID_PARAMETERS"
        );
        assert_eq!(
            clear(
                db,
                json!({"view":"security_events","toolVerdict":"uncertain"})
            )
            .await
            .unwrap_err(),
            "SECURITY_EVENTS_INVALID_PARAMETERS"
        );
    }

    #[tokio::test]
    async fn database_and_join_failures_are_safe_and_not_empty_success() {
        let db = Arc::new(Database::memory().unwrap());
        db.extension_connection()
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE security_audit_logs")
            .unwrap();
        let f = json!({"view":"security_events","keyword":"private-session-or-summary"});
        assert_eq!(
            events(db.clone(), f.clone(), json!(1), json!(10))
                .await
                .unwrap_err(),
            "SECURITY_EVENTS_DATABASE_ERROR"
        );
        assert_eq!(
            summary(db.clone(), f.clone()).await.unwrap_err(),
            "SECURITY_EVENTS_DATABASE_ERROR"
        );
        assert_eq!(
            clear(db, f).await.unwrap_err(),
            "SECURITY_EVENTS_DATABASE_ERROR"
        );
        let error = blocking::<(), _>(|| panic!("private-join-details"))
            .await
            .unwrap_err();
        assert_eq!(error, "SECURITY_EVENTS_TASK_ERROR");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_work_uses_a_different_thread_and_valid_requests_keep_shape() {
        let current = std::thread::current().id();
        let other = blocking(move || Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(current, other);
        let db = Arc::new(Database::memory().unwrap());
        let f = json!({"view":"security_events"});
        let page = events(db.clone(), f.clone(), json!(1), json!(50))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(page).unwrap(),
            json!({"items":[],"total":0,"page":1,"pageSize":50})
        );
        assert_eq!(summary(db.clone(), f.clone()).await.unwrap().total, 0);
        assert_eq!(clear(db, f).await.unwrap().deleted_total, 0);
    }
}
