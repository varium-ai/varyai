use crate::services::diagnostics::{
    self,
    operation::{Action, Operation},
    ExportReport, FAILED,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri_plugin_dialog::DialogExt;

static EXPORTING: AtomicBool = AtomicBool::new(false);
struct ExportGuard;
impl Drop for ExportGuard {
    fn drop(&mut self) {
        EXPORTING.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub(crate) async fn export_application_log(
    app: tauri::AppHandle,
) -> Result<Option<ExportReport>, String> {
    EXPORTING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| FAILED)?;
    let guard = ExportGuard;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let mut operation = Operation::start("application", Action::ExportLog);
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("Log", &["log"])
            .set_file_name("varyai.log")
            .blocking_save_file()
        else {
            operation.cancelled();
            return Ok(None);
        };
        let result = (|| {
            let mut path = selected.into_path().map_err(|_| FAILED)?;
            if path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("log"))
            {
                let corrected = path.with_extension("log");
                // Never overwrite a different path than the native dialog approved.
                if corrected.exists() {
                    return Err(FAILED.into());
                }
                path = corrected;
            }
            log::logger().flush();
            diagnostics::export_application_log(&path)
        })();
        operation.finish(&result);
        if let Ok(report) = &result {
            log::info!(
                "[Diagnostics] Application log saved source_bytes={}",
                report.source_bytes
            );
        }
        result.map(Some)
    })
    .await
    .map_err(|_| FAILED.to_owned())?
}
