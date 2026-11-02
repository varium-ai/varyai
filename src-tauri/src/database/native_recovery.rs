use super::{
    native::APPLICATION_ID, Database, NATIVE_EXTENSION_SCHEMA_VERSION, NATIVE_MAIN_SCHEMA_VERSION,
};
use crate::{config::path_is_within, error::AppError};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

fn invalid() -> AppError {
    AppError::Message("storage.invalidRecovery".into())
}
fn same_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.canonicalize()
            .or_else(|_| {
                let parent = path
                    .parent()
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                Ok::<_, std::io::Error>(
                    parent.canonicalize()?.join(
                        path.file_name()
                            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?,
                    ),
                )
            })
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let left = normalize(left);
    let right = normalize(right);
    path_is_within(&left, &right) && path_is_within(&right, &left)
}

/// SQLite super-journals contain absolute paths. Recover only this owned pair.
pub(super) fn recover_owned_journals(path: &Path, initializing: bool) -> Result<(), AppError> {
    let extension = Database::extension_path(path);
    let pair = [path, extension.as_path()];
    let journals: Vec<PathBuf> = pair
        .iter()
        .map(|path| PathBuf::from(format!("{}-journal", path.to_string_lossy())))
        .collect();
    if !journals
        .iter()
        .any(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
    {
        return Ok(());
    }
    let root = path
        .parent()
        .ok_or_else(invalid)?
        .canonicalize()
        .map_err(|_| invalid())?;
    for (file, supported) in pair
        .iter()
        .zip([NATIVE_MAIN_SCHEMA_VERSION, NATIVE_EXTENSION_SCHEMA_VERSION])
    {
        let metadata = fs::symlink_metadata(file).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid());
        }
        let mut header = [0u8; 100];
        fs::File::open(file)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|_| invalid())?;
        let version = i32::from_be_bytes(header[60..64].try_into().map_err(|_| invalid())?);
        let application = i32::from_be_bytes(header[68..72].try_into().map_err(|_| invalid())?);
        if &header[..16] != b"SQLite format 3\0"
            || !(application == APPLICATION_ID && version == supported
                || initializing && application == 0 && (0..supported).contains(&version))
        {
            return Err(invalid());
        }
    }
    for journal in &journals {
        if !journal.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(journal).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid());
        }
        let size = metadata.len();
        if size < 16 {
            continue;
        }
        let mut file = fs::File::open(journal).map_err(|_| invalid())?;
        file.seek(SeekFrom::End(-16)).map_err(|_| invalid())?;
        let mut tail = [0u8; 16];
        file.read_exact(&mut tail).map_err(|_| invalid())?;
        if tail[8..] != [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7] {
            continue;
        }
        let length = u32::from_be_bytes(tail[..4].try_into().map_err(|_| invalid())?) as u64;
        if length == 0 {
            continue;
        }
        if length > 4096 || length > size - 16 {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(size - 16 - length))
            .map_err(|_| invalid())?;
        let mut name = vec![0; length as usize];
        file.read_exact(&mut name).map_err(|_| invalid())?;
        let master = PathBuf::from(std::str::from_utf8(&name).map_err(|_| invalid())?);
        if !master
            .parent()
            .is_some_and(|parent| same_path(parent, &root))
        {
            return Err(invalid());
        }
        if master.exists() {
            let metadata = fs::symlink_metadata(&master).map_err(|_| invalid())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 65536 {
                return Err(invalid());
            }
            let contents = fs::read(&master).map_err(|_| invalid())?;
            for child in contents
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
            {
                let child = Path::new(std::str::from_utf8(child).map_err(|_| invalid())?);
                if !journals.iter().any(|owned| same_path(owned, child)) {
                    return Err(invalid());
                }
            }
        }
    }
    for file in pair {
        let conn = rusqlite::Connection::open_with_flags(
            file,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Database::get_user_version(&conn)?;
    }
    Ok(())
}
