//! Update one owned catalog while preserving failed configuration publications.
use super::{replace_checked, Path, Value, FAILED};

pub(super) fn publish(
    path: &Path,
    previous: Option<&str>,
    next: &str,
    commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if previous == Some(next) {
        return commit();
    }
    // Preserve an unreadable model list before replacing its contents, matching
    // Magpie's writeOwnList. Never include its contents in diagnostic messages.
    if let Some(previous) = previous.filter(|text| {
        !serde_json::from_str::<Value>(text)
            .ok()
            .is_some_and(|value| value.get("models").is_some_and(Value::is_array))
    }) {
        let backup = path.parent().ok_or(FAILED)?.join(format!(
            "codex-catalog-recovery-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        crate::config::atomic_write_private(&backup, previous.as_bytes()).map_err(|_| FAILED)?;
    }
    replace_checked(path, previous, Some(next))?;
    let result = commit();
    if result.is_err() && replace_checked(path, Some(next), previous).is_err() {
        // A newer writer wins; its file must not be replaced during rollback.
        log::warn!("Codex catalog rollback could not complete; check connection status");
    }
    result
}
