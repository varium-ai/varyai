//! Export only the current application log, with complete sanitized snapshots.
pub(crate) mod operation;
#[cfg(test)]
mod tests;
mod text_logs;

use crate::proxy::security::SecurityRuntimeConfig;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    sync::Arc,
};
use text_logs::LogPrivacy;

pub(crate) const FAILED: &str = "diagnostics.exportFailed";
pub(crate) const MISSING: &str = "diagnostics.logMissing";
pub(crate) const EMPTY: &str = "diagnostics.logEmpty";
pub(crate) const SAME_SOURCE: &str = "diagnostics.sameSource";
pub(crate) const PRIVACY: &str = "diagnostics.privacyUnavailable";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportReport {
    pub source_bytes: u64,
    pub redacted_lines: u64,
}

pub(crate) fn export_application_log(destination: &Path) -> Result<ExportReport, String> {
    export_with_policy(
        &crate::panic_hook::get_log_dir().join("varyai.log"),
        destination,
        crate::proxy::security::runtime::audit_security_runtime(),
    )
}

fn export_with_policy(
    source: &Path,
    destination: &Path,
    policy: Option<Arc<SecurityRuntimeConfig>>,
) -> Result<ExportReport, String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MISSING
        } else {
            FAILED
        }
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FAILED.into());
    }
    validate_destination(source, destination)?;
    let file = File::open(source).map_err(|_| FAILED)?;
    let size = file.metadata().map_err(|_| FAILED)?.len();
    if size == 0 {
        return Err(EMPTY.into());
    }
    let privacy = LogPrivacy::capture(policy);
    if !privacy.ready {
        return Err(PRIVACY.into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or(FAILED)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| FAILED)?;
    let report = write_snapshot(file, size, temporary.as_file_mut(), &privacy)?;
    temporary.as_file_mut().sync_all().map_err(|_| FAILED)?;
    validate_destination(source, destination)?;
    temporary.persist(destination).map_err(|_| FAILED)?;
    Ok(report)
}

fn validate_destination(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if same_file::is_same_file(source, destination).map_err(|_| FAILED)? {
                return Err(SAME_SOURCE.into());
            }
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(FAILED.into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(FAILED.into()),
    }
    Ok(())
}

fn write_snapshot(
    input: impl Read,
    size: u64,
    output: &mut impl Write,
    privacy: &LogPrivacy,
) -> Result<ExportReport, String> {
    let mut input = BufReader::new(input.take(size));
    let mut file = privacy.file();
    let mut report = ExportReport {
        source_bytes: 0,
        redacted_lines: 0,
    };
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = input.read_until(b'\n', &mut line).map_err(|_| FAILED)?;
        if count == 0 {
            break;
        }
        report.source_bytes += count as u64;
        let text = std::str::from_utf8(&line).map_err(|_| FAILED)?;
        if text.contains('\0') {
            return Err(FAILED.into());
        }
        let (body, ending) = if let Some(body) = text.strip_suffix("\r\n") {
            (body, "\r\n")
        } else if let Some(body) = text.strip_suffix('\n') {
            (body, "\n")
        } else {
            (text, "")
        };
        let (safe, changed) = file.redact(body);
        report.redacted_lines += u64::from(changed);
        output.write_all(safe.as_bytes()).map_err(|_| FAILED)?;
        output.write_all(ending.as_bytes()).map_err(|_| FAILED)?;
    }
    if report.source_bytes != size {
        return Err(FAILED.into());
    }
    Ok(report)
}
