//! Export only the current application log as a direct raw snapshot.
pub(crate) mod operation;
#[cfg(test)]
mod tests;

use serde::Serialize;
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

pub(crate) const FAILED: &str = "diagnostics.exportFailed";
pub(crate) const MISSING: &str = "diagnostics.logMissing";
pub(crate) const EMPTY: &str = "diagnostics.logEmpty";
pub(crate) const SAME_SOURCE: &str = "diagnostics.sameSource";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportReport {
    pub source_bytes: u64,
}

pub(crate) fn export_application_log(destination: &Path) -> Result<ExportReport, String> {
    export_raw_file(
        &crate::panic_hook::get_log_dir().join("varyai.log"),
        destination,
    )
}

fn export_raw_file(source: &Path, destination: &Path) -> Result<ExportReport, String> {
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
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or(FAILED)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| FAILED)?;
    let report = write_snapshot(file, size, temporary.as_file_mut())?;
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
) -> Result<ExportReport, String> {
    let copied = io::copy(&mut input.take(size), output).map_err(|_| FAILED)?;
    if copied != size {
        return Err(FAILED.into());
    }
    Ok(ExportReport {
        source_bytes: copied,
    })
}
