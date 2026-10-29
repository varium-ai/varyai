//! Device-local secret copies, separate from portable gateway policy storage.
use crate::error::AppError;
use std::path::{Path, PathBuf};

fn path(directory: &Path, id: &str) -> Result<PathBuf, AppError> {
    uuid::Uuid::parse_str(id).map_err(|_| unavailable())?;
    Ok(directory.join(id))
}

fn unavailable() -> AppError {
    AppError::Config("Gateway secret unavailable".into())
}

pub(super) fn available(directory: &Path, id: &str) -> bool {
    path(directory, id).is_ok_and(|path| path.is_file())
}

pub(super) fn save(directory: &Path, id: &str, secret: &str) -> Result<(), AppError> {
    crate::config::atomic_write_private(&path(directory, id)?, secret.as_bytes())
        .map_err(|_| unavailable())
}

pub(super) fn read(directory: &Path, id: &str) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path(directory, id)?) {
        Ok(secret) => Ok(Some(secret)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(unavailable()),
    }
}

pub(super) fn remove(directory: &Path, id: &str) -> Result<(), AppError> {
    match std::fs::remove_file(path(directory, id)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(unavailable()),
    }
}
