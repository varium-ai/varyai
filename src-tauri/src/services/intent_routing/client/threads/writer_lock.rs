//! Non-blocking coordination with Codex's per-thread writer locks.
//!
//! Codex creates a lock file for every thread it writes. A provider transition
//! must hold the same lock before changing a rollout or its state row; otherwise
//! Codex can continue writing the old provider into a file while the transition
//! is in progress. The lock directory was added after older Codex releases, so
//! an absent directory is intentionally treated as an unlocked home.

use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

const FAILED: &str = "intentRouting.clientFailed";
const CONFLICT: &str = "intentRouting.clientConflict";
const LOCK_DIRECTORY: &str = "thread-writer-locks";
const COORDINATION_LOCK: &str = ".coordination.lock";
const MAX_THREAD_ID_BYTES: usize = 256;

/// A held Codex writer lock.
///
/// The guard owns both descriptors. The coordination descriptor is unlocked
/// after the thread descriptor is acquired, so other threads can be acquired
/// concurrently. Its drop order is deliberate: release the thread lock first,
/// reacquire coordination when possible, remove the lock file while that lock
/// is held, then release coordination.
pub(super) struct Guard {
    coordination: Option<File>,
    thread: Option<File>,
    thread_path: Option<PathBuf>,
}

impl Guard {
    fn noop() -> Self {
        Self {
            coordination: None,
            thread: None,
            thread_path: None,
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            // A no-op guard has no coordination lock or file to release.
            return;
        };

        if let Some(coordination) = self.coordination.take() {
            // The coordination lock is intentionally best effort on drop. If
            // another writer owns it, leaving the regular lock file behind is
            // safe: the next holder can lock and clean it up. Acquire it while
            // the thread lock is still held, so a new Codex writer cannot
            // appear between unlock and cleanup.
            if try_lock(&coordination).is_ok() {
                // unlock() is best effort during Drop. Closing the descriptor
                // also releases the OS lock, including when unlock fails.
                let _ = unlock_file(&thread);
                drop(thread);
                if let Some(path) = self.thread_path.take() {
                    // Never follow a replacement symlink while cleaning up. A
                    // changed or malformed path is left in place for the next
                    // caller to report.
                    if fs::symlink_metadata(&path)
                        .ok()
                        .is_some_and(|metadata| metadata.file_type().is_file())
                    {
                        let _ = fs::remove_file(path);
                    }
                }
                let _ = unlock_file(&coordination);
            } else {
                let _ = unlock_file(&thread);
                drop(thread);
            }
        } else {
            let _ = unlock_file(&thread);
            drop(thread);
        }
    }
}

/// Hold the writer lock for `id` in `home` without waiting.
///
/// A Codex home from before `thread-writer-locks` has no lock directory and
/// returns a no-op guard. Contention, malformed lock paths, and I/O failures
/// are represented by stable client errors; no path or thread identifier is
/// included in the returned error.
pub(super) fn hold(home: &Path, id: &str) -> Result<Guard, String> {
    validate_thread_id(id)?;
    if home.as_os_str().is_empty() {
        return Ok(Guard::noop());
    }

    let locks = home.join(LOCK_DIRECTORY);
    let Some(locks) = existing_lock_directory(&locks)? else {
        return Ok(Guard::noop());
    };

    let coordination_path = locks.join(COORDINATION_LOCK);
    let coordination = open_lock_file(&coordination_path)?;
    try_lock(&coordination)?;

    let thread_path = locks.join(format!("{}.lock", id.to_ascii_lowercase()));
    let thread = match open_lock_file(&thread_path).and_then(|file| try_lock(&file).map(|()| file))
    {
        Ok(file) => file,
        Err(error) => {
            let _ = unlock_file(&coordination);
            return Err(error);
        }
    };

    // Coordination protects creation and acquisition of the per-thread lock;
    // keeping it locked for the rollout/database rewrite would serialize all
    // unrelated Codex threads.
    if unlock_file(&coordination).is_err() {
        let _ = unlock_file(&thread);
        return Err(FAILED.into());
    }

    Ok(Guard {
        coordination: Some(coordination),
        thread: Some(thread),
        thread_path: Some(thread_path),
    })
}

fn validate_thread_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > MAX_THREAD_ID_BYTES
        || matches!(id, "." | "..")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CONFLICT.into());
    }
    Ok(())
}

fn existing_lock_directory(path: &Path) -> Result<Option<PathBuf>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(FAILED.into()),
        Ok(metadata) if metadata.is_dir() => Ok(Some(path.to_path_buf())),
        Ok(_) => Err(FAILED.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FAILED.into()),
    }
}

fn open_lock_file(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(FAILED.into()),
        Ok(metadata) if !metadata.file_type().is_file() => return Err(FAILED.into()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FAILED.into()),
    }

    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| map_io_error(&error))?;
    if !file
        .metadata()
        .map_err(|_| FAILED.to_owned())?
        .file_type()
        .is_file()
    {
        return Err(FAILED.into());
    }
    Ok(file)
}

fn try_lock(file: &File) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            Ok(())
        } else {
            Err(map_io_error(&io::Error::last_os_error()))
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY},
            System::IO::OVERLAPPED,
        };

        let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
        let result = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };
        if result != 0 {
            Ok(())
        } else {
            Err(map_windows_error())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(FAILED.into())
    }
}

fn unlock_file(file: &File) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        if result == 0 {
            Ok(())
        } else {
            Err(map_io_error(&io::Error::last_os_error()))
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Storage::FileSystem::UnlockFileEx, System::IO::OVERLAPPED};

        let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
        let result = unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
        if result != 0 {
            Ok(())
        } else {
            Err(map_windows_error())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(FAILED.into())
    }
}

#[cfg(windows)]
fn map_windows_error() -> String {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_LOCK_VIOLATION};

    if unsafe { GetLastError() } == ERROR_LOCK_VIOLATION {
        CONFLICT.into()
    } else {
        FAILED.into()
    }
}

fn map_io_error(error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::WouldBlock {
        CONFLICT.into()
    } else {
        FAILED.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };

    fn lock_directory(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let locks = dir.join(LOCK_DIRECTORY);
        fs::create_dir(&locks)?;
        Ok(locks)
    }

    #[test]
    fn missing_lock_directory_is_a_noop() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let _guard = hold(dir.path(), "synthetic-thread")?;
        assert!(!dir.path().join(LOCK_DIRECTORY).exists());
        Ok(())
    }

    #[test]
    fn thread_contention_is_nonblocking_and_sanitized() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        lock_directory(dir.path())?;
        let first = hold(dir.path(), "synthetic-thread")?;
        let started = Instant::now();
        let result = hold(dir.path(), "synthetic-thread");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(result, Err(error) if error == CONFLICT));
        drop(first);
        Ok(())
    }

    #[test]
    fn different_thread_guards_can_be_held_concurrently() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        lock_directory(dir.path())?;
        let first = hold(dir.path(), "first-thread")?;
        let second = hold(dir.path(), "second-thread")?;
        assert!(dir
            .path()
            .join(LOCK_DIRECTORY)
            .join("first-thread.lock")
            .is_file());
        assert!(dir
            .path()
            .join(LOCK_DIRECTORY)
            .join("second-thread.lock")
            .is_file());
        drop(second);
        drop(first);
        Ok(())
    }

    #[test]
    fn coordination_contention_is_nonblocking_and_sanitized(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let locks = lock_directory(dir.path())?;
        let coordination = open_lock_file(&locks.join(COORDINATION_LOCK))?;
        try_lock(&coordination)?;

        let started = Instant::now();
        let result = hold(dir.path(), "synthetic-thread");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(result, Err(error) if error == CONFLICT));
        unlock_file(&coordination)?;
        Ok(())
    }

    #[test]
    fn dropping_guard_releases_and_removes_thread_lock() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let locks = lock_directory(dir.path())?;
        let path = locks.join("synthetic-thread.lock");
        {
            let _guard = hold(dir.path(), "synthetic-thread")?;
            assert!(path.is_file());
        }
        assert!(!path.exists());
        let _guard = hold(dir.path(), "synthetic-thread")?;
        Ok(())
    }

    #[test]
    fn busy_coordination_on_drop_leaves_a_safe_retryable_lock_file(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let locks = lock_directory(dir.path())?;
        let path = locks.join("synthetic-thread.lock");
        let guard = hold(dir.path(), "synthetic-thread")?;
        let coordination = open_lock_file(&locks.join(COORDINATION_LOCK))?;
        try_lock(&coordination)?;

        drop(guard);
        assert!(path.is_file());
        unlock_file(&coordination)?;

        let _guard = hold(dir.path(), "synthetic-thread")?;
        Ok(())
    }

    #[test]
    fn unsafe_thread_id_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        lock_directory(dir.path())?;
        for id in ["", ".", "..", "../outside", "nested/id", "thread name"] {
            assert!(matches!(hold(dir.path(), id), Err(error) if error == CONFLICT));
        }
        Ok(())
    }

    #[test]
    fn non_regular_lock_file_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let locks = lock_directory(dir.path())?;
        fs::create_dir(locks.join("synthetic-thread.lock"))?;
        assert!(matches!(
            hold(dir.path(), "synthetic-thread"),
            Err(error) if error == FAILED
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_lock_file_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let locks = lock_directory(dir.path())?;
        let target = dir.path().join("outside.lock");
        fs::write(&target, b"synthetic")?;
        std::os::unix::fs::symlink(&target, locks.join("synthetic-thread.lock"))?;
        assert!(matches!(
            hold(dir.path(), "synthetic-thread"),
            Err(error) if error == FAILED
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_lock_directory_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("real-locks");
        fs::create_dir(&target)?;
        std::os::unix::fs::symlink(&target, dir.path().join(LOCK_DIRECTORY))?;
        assert!(matches!(
            hold(dir.path(), "synthetic-thread"),
            Err(error) if error == FAILED
        ));
        Ok(())
    }
}
