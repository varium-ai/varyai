//! Keep Codex rollout files and state rows in the same provider bucket.
//!
//! Legacy history needs matching rollout metadata and SQLite provider rows.
//! Paginated history uses the SQLite row alone; its files may contain only a
//! suffix and retain byte offsets that must not change. The transition is
//! scoped to a caller supplied Codex home and a private recovery directory.

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::{
    collections::HashSet,
    fs::{self, FileTimes, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const FAILED: &str = "intentRouting.clientFailed";
const CONFLICT: &str = "intentRouting.clientConflict";
const RECEIPT_VERSION: u32 = 2;
const MAX_ROLLOUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SYNC_BYTES: u64 = 256 * 1024 * 1024;
const ACTIVE_WINDOW: Duration = Duration::from_secs(60);

mod paginated;
mod writer_lock;

/// A short result intended for the connection status surface. It contains no
/// rollout paths, message data, or session identifiers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadTransitionReport {
    pub moved: usize,
    pub restored: usize,
    pub skipped: usize,
    pub incomplete: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Receipt {
    version: u32,
    from: String,
    to: String,
    entries: Vec<ReceiptEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReceiptEntry {
    id: String,
    db: PathBuf,
    rollout: PathBuf,
    backup: PathBuf,
    state: ReceiptState,
    #[serde(default)]
    paginated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReceiptState {
    Pending,
    Moved,
    Restored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RolloutProvider {
    Source,
    Target,
    Other,
    Missing,
    Unknown,
}

#[derive(Clone, Debug)]
struct ThreadRow {
    db: PathBuf,
    id: String,
    rollout: PathBuf,
    paginated: bool,
}

#[derive(Clone, Debug)]
struct FileSnapshot {
    raw: Vec<u8>,
    decoded: Vec<u8>,
    modified: SystemTime,
    #[cfg(unix)]
    mode: u32,
}

#[derive(Debug, Deserialize)]
struct RolloutSessionMeta<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    #[serde(borrow)]
    payload: Option<RolloutSessionPayload<'a>>,
}

#[derive(Debug, Deserialize)]
struct RolloutSessionPayload<'a> {
    id: Option<&'a str>,
    #[serde(borrow)]
    model_provider: Option<&'a RawValue>,
    history_mode: Option<&'a str>,
}

/// Move every safe, quiet thread from `from` to `to`.
///
/// The receipt is updated after every row, so an interrupted transition can
/// be resumed by calling this function again. Rollouts are changed only when
/// their state row and first metadata event identify the same source provider.
pub(crate) fn transition(
    home: &Path,
    recovery_dir: &Path,
    from: &str,
    to: &str,
) -> Result<ThreadTransitionReport, String> {
    validate_provider_pair(from, to)?;
    let mut receipt = load_or_create_receipt(home, recovery_dir, from, to)?;
    let mut report = ThreadTransitionReport::default();
    let (rows, rows_incomplete) = collect_rows(home, from, to);
    report.incomplete = rows_incomplete;
    let mut seen = HashSet::new();
    let mut scanned_bytes = 0_u64;

    for row in rows {
        // Completed and unrelated paginated target rows need no lock or file
        // inspection. Their writers must not keep other migrations pending.
        if row.paginated
            && read_row_provider(&row.db, &row.id)
                .ok()
                .flatten()
                .as_deref()
                == Some(to)
            && !receipt.entries.iter().any(|entry| {
                entry.paginated
                    && entry.db == row.db
                    && entry.id == row.id
                    && entry.state != ReceiptState::Moved
            })
        {
            continue;
        }
        let _writer_lock = match writer_lock::hold(home, &row.id) {
            Ok(guard) => guard,
            Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if row.paginated {
            match paginated::transition(home, recovery_dir, &row, &mut receipt) {
                Ok(true) => report.moved += 1,
                Ok(false) => {}
                Err(_) => {
                    report.skipped += 1;
                    report.incomplete = true;
                }
            }
            continue;
        }
        let Some(rollout) = safe_rollout_path(home, &row.rollout) else {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        };
        if !seen.insert((row.db.clone(), row.id.clone(), rollout.clone())) {
            continue;
        }

        // A target row already recorded as moved has no work left for this
        // rollout. Check the small state row before charging its file against
        // the scan budget, while still allowing a later source/target cycle
        // to revisit a row that has been moved back to `from`.
        let entry_index_option = receipt
            .entries
            .iter()
            .position(|entry| entry.db == row.db && entry.id == row.id && entry.rollout == rollout);
        let has_thread_receipt = receipt
            .entries
            .iter()
            .any(|entry| entry.db == row.db && entry.id == row.id);
        let db_provider = match read_row_provider(&row.db, &row.id) {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if db_provider == to && !has_thread_receipt {
            // The journal is the ownership proof for target rows. An
            // unrelated target session must not be opened, budgeted, or
            // reported as incomplete while a retry rediscovers state files.
            continue;
        }
        if entry_index_option.is_some_and(|index| {
            receipt.entries[index].state == ReceiptState::Moved && db_provider == to
        }) {
            continue;
        }

        let file_size = match fs::metadata(&rollout) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) | Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if scanned_bytes.saturating_add(file_size) > MAX_SYNC_BYTES {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        scanned_bytes = scanned_bytes.saturating_add(file_size);

        let Some(snapshot) = read_snapshot(&rollout) else {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        };
        if !session_meta_matches_thread(&snapshot.decoded, &row.id) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        if is_active(&snapshot.modified) || has_packing_twin(&rollout) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }

        let provider = provider_for_thread(&snapshot.decoded, &row.id, from, to);
        if matches!(provider, RolloutProvider::Unknown | RolloutProvider::Other) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        let (entry_index, backup) = if let Some(index) = entry_index_option {
            (index, receipt.entries[index].backup.clone())
        } else {
            let index = receipt.entries.len();
            let backup = recovery_dir
                .join("files")
                .join(format!("entry-{index}.rollout"));
            receipt.entries.push(ReceiptEntry {
                id: row.id.clone(),
                db: row.db.clone(),
                rollout: rollout.clone(),
                backup: backup.clone(),
                state: ReceiptState::Pending,
                paginated: false,
            });
            persist_receipt(recovery_dir, from, to, &receipt)?;
            (index, backup)
        };

        if provider == RolloutProvider::Target {
            // A target row is normally already complete. A broad scan of all
            // state databases may include unrelated target sessions, so only
            // a thread already represented in this receipt can rediscover
            // another related segment. A source DB row is still reconciled.
            if db_provider != from && db_provider != to {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
            if db_provider == from {
                match update_row(&row.db, &row.id, from, to) {
                    Ok(1) => {}
                    Ok(0) => {
                        let current = read_row_provider(&row.db, &row.id)?;
                        if current.as_deref() != Some(to) {
                            report.skipped += 1;
                            report.incomplete = true;
                            continue;
                        }
                    }
                    Ok(_) | Err(_) => {
                        report.skipped += 1;
                        report.incomplete = true;
                        continue;
                    }
                }
            }
            if read_snapshot(&rollout)
                .filter(|current| session_meta_matches_thread(&current.decoded, &row.id))
                .map(|current| provider_for_thread(&current.decoded, &row.id, from, to))
                != Some(RolloutProvider::Target)
            {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
            receipt.entries[entry_index].state = ReceiptState::Moved;
            persist_receipt(recovery_dir, from, to, &receipt)?;
            report.moved += 1;
            continue;
        }

        if db_provider != from && db_provider != to {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }

        if !regular_file_exists(&backup) && !copy_backup(&snapshot.raw, &backup) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        let rewritten = if provider == RolloutProvider::Source {
            match rewrite_rollout(&rollout, &snapshot, &row.id, from, to) {
                Ok(written) => Some(written),
                Err(_) => {
                    report.skipped += 1;
                    report.incomplete = true;
                    continue;
                }
            }
        } else {
            None
        };

        if db_provider == from {
            match update_row(&row.db, &row.id, from, to) {
                Ok(1) => {}
                Ok(0) => {
                    let current = read_row_provider(&row.db, &row.id)?;
                    if current.as_deref() != Some(to) {
                        if let Some(written) = rewritten.as_deref() {
                            let _ = restore_raw(
                                &rollout,
                                written,
                                &snapshot.raw,
                                snapshot.modified,
                                #[cfg(unix)]
                                snapshot.mode,
                            );
                        }
                        report.skipped += 1;
                        report.incomplete = true;
                        continue;
                    }
                }
                Ok(_) | Err(_) => {
                    if let Some(written) = rewritten.as_deref() {
                        let _ = restore_raw(
                            &rollout,
                            written,
                            &snapshot.raw,
                            snapshot.modified,
                            #[cfg(unix)]
                            snapshot.mode,
                        );
                    }
                    report.skipped += 1;
                    report.incomplete = true;
                    continue;
                }
            }
        }

        if read_snapshot(&rollout)
            .filter(|current| session_meta_matches_thread(&current.decoded, &row.id))
            .map(|current| provider_for_thread(&current.decoded, &row.id, from, to))
            .is_none_or(|provider| {
                provider != RolloutProvider::Target && provider != RolloutProvider::Missing
            })
        {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }

        receipt.entries[entry_index].state = ReceiptState::Moved;
        persist_receipt(recovery_dir, from, to, &receipt)?;
        report.moved += 1;
    }

    Ok(report)
}

/// Restore rows recorded by a previous transition from `from` to `to`.
///
/// The current rollout is re-edited in place, preserving any messages written
/// after the transition. A file changed to a different provider by the user is
/// left untouched and counted as incomplete.
pub(crate) fn restore(
    home: &Path,
    recovery_dir: &Path,
    from: &str,
    to: &str,
) -> Result<ThreadTransitionReport, String> {
    validate_provider_pair(from, to)?;
    let receipt_path = receipt_path(recovery_dir, from, to);
    let Some(mut receipt) = read_receipt(&receipt_path)? else {
        return Ok(ThreadTransitionReport::default());
    };
    validate_receipt(&receipt, from, to)?;
    receipt.version = RECEIPT_VERSION;
    let mut report = ThreadTransitionReport::default();
    let mut scanned_bytes = 0_u64;
    for index in 0..receipt.entries.len() {
        if !matches!(
            receipt.entries[index].state,
            ReceiptState::Pending | ReceiptState::Moved
        ) {
            continue;
        }
        let entry = receipt.entries[index].clone();
        let _writer_lock = match writer_lock::hold(home, &entry.id) {
            Ok(guard) => guard,
            Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if entry.paginated {
            match paginated::restore(home, &entry, from, to) {
                Ok(()) => {
                    receipt.entries[index].state = ReceiptState::Restored;
                    persist_receipt(recovery_dir, from, to, &receipt)?;
                    report.restored += 1;
                }
                Err(_) => {
                    report.skipped += 1;
                    report.incomplete = true;
                }
            }
            continue;
        }
        let Some(rollout) = safe_rollout_path(home, &entry.rollout) else {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        };
        let file_size = match fs::metadata(&rollout) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) | Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if scanned_bytes.saturating_add(file_size) > MAX_SYNC_BYTES {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        scanned_bytes = scanned_bytes.saturating_add(file_size);
        let Some(snapshot) = read_snapshot(&rollout) else {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        };
        if !session_meta_matches_thread(&snapshot.decoded, &entry.id) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        if is_active(&snapshot.modified) || has_packing_twin(&rollout) {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        let provider = provider_for_thread(&snapshot.decoded, &entry.id, from, to);
        if provider != RolloutProvider::Source
            && provider != RolloutProvider::Target
            && provider != RolloutProvider::Missing
        {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }

        let Some(db) = safe_db_path(home, &entry.db) else {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        };
        let db_provider = match read_row_provider(&db, &entry.id) {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                report.incomplete = true;
                continue;
            }
        };
        if db_provider != from && db_provider != to {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }

        let rewritten = if provider == RolloutProvider::Target {
            match rewrite_rollout(&rollout, &snapshot, &entry.id, to, from) {
                Ok(written) => Some(written),
                Err(_) => {
                    report.skipped += 1;
                    report.incomplete = true;
                    continue;
                }
            }
        } else {
            None
        };
        if db_provider == to {
            match update_row(&db, &entry.id, to, from) {
                Ok(1) => {}
                Ok(0) => {
                    let current = read_row_provider(&db, &entry.id)?;
                    if current.as_deref() != Some(from) {
                        if let Some(written) = rewritten.as_deref() {
                            let _ = restore_raw(
                                &rollout,
                                written,
                                &snapshot.raw,
                                snapshot.modified,
                                #[cfg(unix)]
                                snapshot.mode,
                            );
                        }
                        report.skipped += 1;
                        report.incomplete = true;
                        continue;
                    }
                }
                Ok(_) | Err(_) => {
                    if let Some(written) = rewritten.as_deref() {
                        let _ = restore_raw(
                            &rollout,
                            written,
                            &snapshot.raw,
                            snapshot.modified,
                            #[cfg(unix)]
                            snapshot.mode,
                        );
                    }
                    report.skipped += 1;
                    report.incomplete = true;
                    continue;
                }
            }
        }
        if read_snapshot(&rollout)
            .filter(|current| session_meta_matches_thread(&current.decoded, &entry.id))
            .map(|current| provider_for_thread(&current.decoded, &entry.id, from, to))
            .is_none_or(|provider| {
                provider != RolloutProvider::Source && provider != RolloutProvider::Missing
            })
        {
            report.skipped += 1;
            report.incomplete = true;
            continue;
        }
        receipt.entries[index].state = ReceiptState::Restored;
        persist_receipt(recovery_dir, from, to, &receipt)?;
        report.restored += 1;
    }
    Ok(report)
}

fn validate_provider_pair(from: &str, to: &str) -> Result<(), String> {
    if from.is_empty()
        || to.is_empty()
        || from == to
        || from.len() > 64
        || to.len() > 64
        || !from
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !to
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CONFLICT.into());
    }
    Ok(())
}

fn collect_rows(home: &Path, from: &str, to: &str) -> (Vec<ThreadRow>, bool) {
    let mut incomplete = false;
    let config_text = match fs::read_to_string(home.join("config.toml")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(_) => {
            incomplete = true;
            String::new()
        }
    };
    let mut dbs = state_db_paths(home, &config_text);
    let roots = dbs
        .iter()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .chain(std::iter::once(home.to_path_buf()))
        .collect::<Vec<_>>();
    for root in roots {
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_state_db_filename)
                && !dbs.contains(&path)
            {
                dbs.push(path);
            }
        }
    }

    let mut rows = Vec::new();
    for db in dbs {
        match fs::symlink_metadata(&db) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                incomplete = true;
                continue;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                incomplete = true;
                continue;
            }
        }
        let conn = match Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        let has_history_mode = match conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name = 'history_mode')",
            [],
            |row| row.get::<_, bool>(0),
        ) {
            Ok(present) => present,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        let query = if has_history_mode {
            "SELECT id, rollout_path, COALESCE(history_mode = 'paginated', 0) FROM threads WHERE model_provider = ?1 OR model_provider = ?2"
        } else {
            "SELECT id, rollout_path, 0 FROM threads WHERE model_provider = ?1 OR model_provider = ?2"
        };
        let mut statement = match conn.prepare(query) {
            Ok(statement) => statement,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        let mapped = match statement.query_map(params![from, to], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
            ))
        }) {
            Ok(mapped) => mapped,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        for item in mapped {
            match item {
                Ok((id, raw_rollout, paginated)) if is_safe_thread_id(&id) => {
                    let raw_rollout = PathBuf::from(raw_rollout);
                    let related = if paginated {
                        Vec::new()
                    } else {
                        related_rollout_paths(home, &raw_rollout, &id)
                    };
                    if related.is_empty() {
                        rows.push(ThreadRow {
                            db: db.clone(),
                            id,
                            rollout: raw_rollout,
                            paginated,
                        });
                    } else {
                        rows.extend(related.into_iter().map(|rollout| ThreadRow {
                            db: db.clone(),
                            id: id.clone(),
                            rollout,
                            paginated,
                        }));
                    }
                }
                Ok(_) => {}
                Err(_) => incomplete = true,
            }
        }
    }
    (rows, incomplete)
}

fn state_db_paths(home: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = crate::codex_state_db::codex_state_db_paths(home, config_text);
    let actual_codex_home = crate::codex_config::get_codex_config_dir();
    if home == actual_codex_home {
        return paths;
    }

    // The shared resolver also honors CODEX_SQLITE_HOME. A caller supplied
    // temporary home must not inherit that process-global path, because a
    // test or an isolated account could otherwise rewrite the real database.
    let config_db = configured_sqlite_db(config_text);
    paths.retain(|path| path == &home.join(crate::codex_state_db::CODEX_STATE_DB_FILENAME));
    if let Some(config_db) = config_db {
        if !paths.contains(&config_db) {
            paths.push(config_db);
        }
    }
    paths
}

fn configured_sqlite_db(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<toml_edit::DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let home = crate::config::get_home_dir();
    let dir = if raw == "~" {
        home
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if let Some(rest) = raw.strip_prefix("~\\") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    };
    Some(dir.join(crate::codex_state_db::CODEX_STATE_DB_FILENAME))
}

fn safe_db_path(home: &Path, raw: &Path) -> Option<PathBuf> {
    let raw = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        home.join(raw)
    };
    let metadata = fs::symlink_metadata(&raw).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let config_text = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let raw_canonical = fs::canonicalize(&raw).ok()?;
    let file_name = raw_canonical.file_name()?.to_str()?;
    if !is_state_db_filename(file_name) {
        return None;
    }
    let parent = raw_canonical.parent()?;
    state_db_paths(home, &config_text)
        .into_iter()
        .filter_map(|candidate| candidate.parent().map(Path::to_path_buf))
        .filter_map(|candidate_parent| {
            let candidate_parent = if candidate_parent.is_absolute() {
                candidate_parent
            } else {
                home.join(candidate_parent)
            };
            fs::canonicalize(candidate_parent).ok()
        })
        .any(|candidate_parent| candidate_parent == parent)
        .then_some(raw)
}

fn is_state_db_filename(name: &str) -> bool {
    name.strip_prefix("state_")
        .and_then(|suffix| suffix.strip_suffix(".sqlite"))
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn is_safe_thread_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn safe_rollout_path(home: &Path, raw: &Path) -> Option<PathBuf> {
    let mut candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        home.join(raw)
    };
    if matches!(
        fs::symlink_metadata(&candidate),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    ) && candidate
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("jsonl")
    {
        let name = candidate.file_name()?.to_str()?;
        candidate.set_file_name(format!("{name}.zst"));
    }
    let canonical_home = fs::canonicalize(home).ok()?;
    let canonical = fs::canonicalize(&candidate).ok()?;
    let sessions = canonical_home.join("sessions");
    let archived = canonical_home.join("archived_sessions");
    if !canonical.starts_with(&sessions) && !canonical.starts_with(&archived) {
        return None;
    }
    if !canonical
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.contains('.') && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
        })
    {
        return None;
    }
    Some(canonical)
}

fn related_rollout_paths(home: &Path, raw: &Path, id: &str) -> Vec<PathBuf> {
    let Some(main) = safe_rollout_path(home, raw) else {
        return Vec::new();
    };
    let Some(parent) = main.parent() else {
        return vec![main];
    };
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(parent) else {
        return vec![main];
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("rollout-") || !name.contains(id) {
            continue;
        }
        if let Some(path) = safe_rollout_path(home, &path) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if !paths.contains(&main) {
        paths.insert(0, main);
    }
    paths
}

fn read_snapshot(path: &Path) -> Option<FileSnapshot> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ROLLOUT_BYTES {
        return None;
    }
    let raw = fs::read(path).ok()?;
    let decoded = if path.extension().and_then(|value| value.to_str()) == Some("zst") {
        let mut decoder = zstd::stream::read::Decoder::new(raw.as_slice()).ok()?;
        let mut decoded = Vec::new();
        decoder
            .by_ref()
            .take(MAX_ROLLOUT_BYTES.saturating_add(1))
            .read_to_end(&mut decoded)
            .ok()?;
        if decoded.len() as u64 > MAX_ROLLOUT_BYTES {
            return None;
        }
        decoded
    } else {
        raw.clone()
    };
    if decoded.len() as u64 > MAX_ROLLOUT_BYTES {
        return None;
    }
    Some(FileSnapshot {
        raw,
        decoded,
        modified: metadata.modified().ok()?,
        #[cfg(unix)]
        mode: permissions_mode(&metadata),
    })
}

fn provider_for_thread(decoded: &[u8], id: &str, from: &str, to: &str) -> RolloutProvider {
    let mut matched = false;
    let mut missing = false;
    let mut source = false;
    let mut target = false;
    visit_session_meta(decoded, |meta| {
        let Some(payload) = meta.payload.as_ref() else {
            return;
        };
        if payload.id != Some(id) {
            return;
        }
        matched = true;
        match payload
            .model_provider
            .and_then(|provider| serde_json::from_str::<String>(provider.get()).ok())
            .as_deref()
        {
            Some(provider) if provider == from => source = true,
            Some(provider) if provider == to => target = true,
            Some(_) => {}
            None => missing = true,
        }
    });
    if !matched {
        RolloutProvider::Unknown
    } else if source {
        RolloutProvider::Source
    } else if target {
        RolloutProvider::Target
    } else if missing {
        RolloutProvider::Missing
    } else {
        RolloutProvider::Other
    }
}

fn session_meta_matches_thread(decoded: &[u8], id: &str) -> bool {
    first_session_meta(decoded)
        .and_then(|meta| meta.payload)
        .and_then(|payload| payload.id)
        == Some(id)
}

fn rewrite_rollout(
    path: &Path,
    snapshot: &FileSnapshot,
    id: &str,
    from: &str,
    to: &str,
) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::with_capacity(snapshot.decoded.len());
    let mut changed = false;
    for line in snapshot.decoded.split_inclusive(|byte| *byte == b'\n') {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        let replacement = parse_session_meta(body)
            .filter(|meta| meta.payload.as_ref().and_then(|payload| payload.id) == Some(id))
            .and_then(|meta| {
                let paginated = meta
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.history_mode)
                    == Some("paginated");
                let replacement = meta
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.model_provider)
                    .and_then(|provider| replace_payload_provider_field(line, provider, from, to));
                replacement.map(|replacement| (replacement, paginated))
            });
        if let Some((replacement, paginated)) = replacement {
            let Some(fitted) = fit_rollout_line(line, &replacement, paginated) else {
                return Err(CONFLICT.into());
            };
            decoded.extend_from_slice(&fitted);
            changed = true;
        } else {
            decoded.extend_from_slice(line);
        }
    }
    if !changed {
        return Err(CONFLICT.into());
    }
    let encoded = if path.extension().and_then(|value| value.to_str()) == Some("zst") {
        zstd::stream::encode_all(decoded.as_slice(), 3).map_err(|_| FAILED)?
    } else {
        decoded
    };
    checked_atomic_write(path, &snapshot.raw, &encoded)?;
    restore_metadata(
        path,
        snapshot.modified,
        #[cfg(unix)]
        snapshot.mode,
    )?;
    Ok(encoded)
}

fn parse_session_meta(line: &[u8]) -> Option<RolloutSessionMeta<'_>> {
    let meta = serde_json::from_slice::<RolloutSessionMeta>(line).ok()?;
    (meta.kind == Some("session_meta")).then_some(meta)
}

fn first_session_meta(decoded: &[u8]) -> Option<RolloutSessionMeta<'_>> {
    for line in decoded.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(meta) = parse_session_meta(line) {
            return Some(meta);
        }
    }
    None
}

fn visit_session_meta(decoded: &[u8], mut visit: impl FnMut(&RolloutSessionMeta<'_>)) {
    for line in decoded.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(meta) = parse_session_meta(line) {
            visit(&meta);
        }
    }
}

fn replace_payload_provider_field(
    line: &[u8],
    provider: &RawValue,
    from: &str,
    to: &str,
) -> Option<Vec<u8>> {
    let current = serde_json::from_str::<String>(provider.get()).ok()?;
    if current != from {
        return None;
    }
    let provider_bytes = provider.get().as_bytes();
    let line_start = line.as_ptr() as usize;
    let provider_start = provider_bytes.as_ptr() as usize;
    let offset = provider_start.checked_sub(line_start)?;
    if offset > line.len()
        || provider_bytes.len() > line.len().saturating_sub(offset)
        || &line[offset..offset + provider_bytes.len()] != provider_bytes
    {
        return None;
    }
    let replacement = serde_json::to_vec(to).ok()?;
    let mut output = Vec::with_capacity(
        line.len()
            .saturating_sub(provider_bytes.len())
            .saturating_add(replacement.len()),
    );
    output.extend_from_slice(&line[..offset]);
    output.extend_from_slice(&replacement);
    output.extend_from_slice(&line[offset + provider_bytes.len()..]);
    Some(output)
}

fn fit_rollout_line(line: &[u8], replacement: &[u8], paginated: bool) -> Option<Vec<u8>> {
    let body_len = line
        .iter()
        .rposition(|byte| !matches!(byte, b'\r' | b'\n'))
        .map_or(0, |index| index + 1);
    let suffix = &line[body_len..];
    let replacement_body_len = replacement
        .iter()
        .rposition(|byte| !matches!(byte, b'\r' | b'\n' | b' '))
        .map_or(0, |index| index + 1);
    if replacement_body_len > body_len {
        return (!paginated).then(|| replacement.to_vec());
    }
    let mut fitted = Vec::with_capacity(body_len.max(replacement_body_len) + suffix.len());
    fitted.extend_from_slice(&replacement[..replacement_body_len]);
    fitted.extend(std::iter::repeat_n(b' ', body_len - replacement_body_len));
    fitted.extend_from_slice(suffix);
    Some(fitted)
}

fn restore_raw(
    path: &Path,
    expected: &[u8],
    raw: &[u8],
    modified: SystemTime,
    #[cfg(unix)] mode: u32,
) -> Result<(), String> {
    checked_atomic_write(path, expected, raw)?;
    restore_metadata(
        path,
        modified,
        #[cfg(unix)]
        mode,
    )
}

fn checked_atomic_write(path: &Path, expected: &[u8], replacement: &[u8]) -> Result<(), String> {
    if !same_raw_bytes(path, expected) {
        return Err(CONFLICT.into());
    }
    durable_write(path, replacement, Some(expected))
}

fn durable_write(path: &Path, replacement: &[u8], expected: Option<&[u8]>) -> Result<(), String> {
    let parent = path.parent().ok_or(FAILED)?;
    ensure_directory(parent)?;
    let staged = parent.join(format!(
        ".vary-thread-{}.staged",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        crate::config::atomic_write_private(&staged, replacement).map_err(|_| FAILED)?;
        let staged_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&staged)
            .map_err(|_| FAILED)?;
        staged_file.sync_all().map_err(|_| FAILED)?;
        if let Some(expected) = expected {
            if !same_raw_bytes(path, expected) {
                return Err(CONFLICT.into());
            }
        }
        fs::rename(&staged, path).map_err(|_| CONFLICT.to_owned())?;
        sync_parent(parent)
    })();
    let _ = fs::remove_file(&staged);
    result
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), String> {
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FAILED.into())
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), String> {
    Ok(())
}

fn restore_metadata(
    path: &Path,
    modified: SystemTime,
    #[cfg(unix)] mode: u32,
) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| FAILED)?;
    file.set_times(FileTimes::new().set_modified(modified))
        .map_err(|_| FAILED)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|_| FAILED)?;
    }
    Ok(())
}

#[cfg(unix)]
fn permissions_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

fn is_active(modified: &SystemTime) -> bool {
    SystemTime::now()
        .duration_since(*modified)
        .map(|age| age < ACTIVE_WINDOW)
        .unwrap_or(true)
}

fn has_packing_twin(path: &Path) -> bool {
    match path.extension().and_then(|value| value.to_str()) {
        Some("jsonl") => path
            .with_file_name(format!(
                "{}.zst",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
            ))
            .is_file(),
        Some("zst") => path
            .file_stem()
            .map(|stem| path.with_file_name(stem).is_file())
            .unwrap_or(false),
        _ => false,
    }
}

fn same_raw_bytes(path: &Path, expected: &[u8]) -> bool {
    fs::read(path).is_ok_and(|actual| actual == expected)
}

fn copy_backup(raw: &[u8], path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        if ensure_directory(parent).is_err() {
            return false;
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => return false,
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return false,
    }
    durable_write(path, raw, None).is_ok()
}

fn regular_file_exists(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
        .unwrap_or(false)
}

fn read_row_provider(db: &Path, id: &str) -> Result<Option<String>, String> {
    validate_db_path(db)?;
    let conn =
        Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| FAILED)?;
    let mut statement = conn
        .prepare("SELECT model_provider FROM threads WHERE id = ?1")
        .map_err(|_| CONFLICT)?;
    match statement.query_row(params![id], |row| row.get::<_, String>(0)) {
        Ok(provider) => Ok(Some(provider)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(_) => Err(CONFLICT.into()),
    }
}

fn update_row(db: &Path, id: &str, from: &str, to: &str) -> Result<usize, String> {
    validate_db_path(db)?;
    let mut conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| FAILED)?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|_| FAILED)?;
    let tx = conn.transaction().map_err(|_| CONFLICT)?;
    let changed = tx
        .execute(
            "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3",
            params![to, id, from],
        )
        .map_err(|_| CONFLICT)?;
    tx.commit().map_err(|_| CONFLICT)?;
    Ok(changed)
}

fn validate_db_path(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(CONFLICT.into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(CONFLICT.into()),
        Err(_) => Err(FAILED.into()),
    }
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(CONFLICT.into())
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FAILED.into()),
    }
    fs::create_dir_all(path).map_err(|_| FAILED)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => Ok(()),
        _ => Err(CONFLICT.into()),
    }
}

fn load_or_create_receipt(
    _home: &Path,
    recovery_dir: &Path,
    from: &str,
    to: &str,
) -> Result<Receipt, String> {
    let path = receipt_path(recovery_dir, from, to);
    if let Some(mut receipt) = read_receipt(&path)? {
        validate_receipt(&receipt, from, to)?;
        receipt.version = RECEIPT_VERSION;
        return Ok(receipt);
    }
    Ok(Receipt {
        version: RECEIPT_VERSION,
        from: from.into(),
        to: to.into(),
        entries: Vec::new(),
    })
}

fn validate_receipt(receipt: &Receipt, from: &str, to: &str) -> Result<(), String> {
    if !matches!(receipt.version, 1 | RECEIPT_VERSION) || receipt.from != from || receipt.to != to {
        return Err(CONFLICT.into());
    }
    Ok(())
}

fn read_receipt(path: &Path) -> Result<Option<Receipt>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(CONFLICT.into())
        }
        Ok(_) => {
            let bytes = fs::read(path).map_err(|_| FAILED)?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| FAILED.into())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FAILED.into()),
    }
}

fn persist_receipt(
    recovery_dir: &Path,
    from: &str,
    to: &str,
    receipt: &Receipt,
) -> Result<(), String> {
    ensure_directory(recovery_dir)?;
    let bytes = serde_json::to_vec(receipt).map_err(|_| FAILED)?;
    let path = receipt_path(recovery_dir, from, to);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(CONFLICT.into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FAILED.into()),
    }
    durable_write(&path, &bytes, None)
}

fn receipt_path(recovery_dir: &Path, from: &str, to: &str) -> PathBuf {
    recovery_dir.join(format!("codex-thread-transition-{from}-to-{to}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::Value;
    use tempfile::tempdir;

    const ID: &str = "019a0000-0000-7000-8000-0000000000c1";

    fn fixture(home: &Path, id: &str, provider: &str, suffix: &str) -> (PathBuf, PathBuf) {
        let rollout_dir = home.join("sessions").join("2026").join("11").join("05");
        fs::create_dir_all(&rollout_dir).expect("rollout dir");
        let rollout = rollout_dir.join(format!("rollout-{id}{suffix}"));
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": id, "model_provider": provider, "title": "synthetic"}
        });
        fs::write(
            &rollout,
            format!("{}\n{{\"type\":\"event\",\"value\":1}}\n", meta),
        )
        .expect("rollout");
        (rollout, home.join("state_5.sqlite"))
    }

    fn db(path: &Path, id: &str, rollout: &Path, provider: &str) {
        let conn = Connection::open(path).expect("db");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL)",
            [],
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO threads (id, rollout_path, model_provider) VALUES (?1, ?2, ?3)",
            params![id, rollout.to_string_lossy().to_string(), provider],
        )
        .expect("row");
    }

    fn provider(path: &Path) -> String {
        let line = fs::read_to_string(path)
            .expect("read")
            .lines()
            .next()
            .unwrap()
            .to_owned();
        serde_json::from_str::<Value>(&line).expect("json")["payload"]["model_provider"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn transition_updates_rollout_and_state_and_restores() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        db(&state, ID, &rollout, "openai");
        let old_mtime = SystemTime::now() - Duration::from_secs(120);
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(old_mtime))
            .expect("mtime");

        let moved = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(moved.moved, 1);
        assert_eq!(provider(&rollout), "varyai");
        assert_eq!(
            fs::metadata(&rollout)
                .expect("meta")
                .modified()
                .expect("mtime"),
            old_mtime
        );
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "varyai"
        );
        assert!(recovery.join("files/entry-0.rollout").exists());

        let restored = restore(&home, &recovery, "openai", "varyai").expect("restore");
        assert_eq!(restored.restored, 1);
        assert_eq!(provider(&rollout), "openai");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "openai"
        );
    }

    #[test]
    fn state_database_versions_beyond_default_are_safe_to_restore() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, default_state) = fixture(&home, ID, "openai", ".jsonl");
        db(&default_state, ID, &rollout, "openai");
        let state = home.join("state_6.sqlite");
        fs::rename(&default_state, &state).expect("state version");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");

        assert_eq!(
            transition(&home, &recovery, "openai", "varyai")
                .expect("move")
                .moved,
            1
        );
        assert_eq!(
            restore(&home, &recovery, "openai", "varyai")
                .expect("restore")
                .restored,
            1
        );
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "openai"
        );
    }

    #[test]
    fn transition_skips_active_rollout_and_other_provider() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        db(&state, ID, &rollout, "openai");
        let report = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(report.moved, 0);
        assert_eq!(report.skipped, 1);
        assert!(report.incomplete);
        assert_eq!(provider(&rollout), "openai");
    }

    #[test]
    fn transition_skips_rollout_when_filename_id_does_not_match_metadata() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let replacement_id = "019a0000-0000-7000-8000-0000000000d1";
        let replaced = fs::read_to_string(&rollout)
            .expect("rollout")
            .replace(ID, replacement_id);
        fs::write(&rollout, replaced).expect("replace metadata");
        db(&state, ID, &rollout, "openai");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");

        let report = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(report.moved, 0);
        assert_eq!(report.skipped, 1);
        assert!(report.incomplete);
        assert_eq!(provider(&rollout), "openai");
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "openai"
        );
        assert!(!receipt_path(&recovery, "openai", "varyai").exists());
    }

    #[test]
    fn restore_skips_rollout_replaced_with_another_thread() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        db(&state, ID, &rollout, "openai");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");
        assert_eq!(
            transition(&home, &recovery, "openai", "varyai")
                .expect("transition")
                .moved,
            1
        );

        let replacement_id = "019a0000-0000-7000-8000-0000000000d1";
        let replaced = fs::read_to_string(&rollout)
            .expect("rollout")
            .replace(ID, replacement_id);
        fs::write(&rollout, replaced).expect("replace metadata");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");

        let report = restore(&home, &recovery, "openai", "varyai").expect("restore");
        assert_eq!(report.restored, 0);
        assert_eq!(report.skipped, 1);
        assert!(report.incomplete);
        assert_eq!(provider(&rollout), "varyai");
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "varyai"
        );
    }

    #[test]
    fn transition_rewrites_zstd_rollout() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (plain, state) = fixture(&home, ID, "openai", ".jsonl");
        let decoded = fs::read(&plain).expect("plain");
        let packed = plain.with_file_name(format!(
            "{}.zst",
            plain.file_name().unwrap().to_string_lossy()
        ));
        fs::write(
            &packed,
            zstd::stream::encode_all(decoded.as_slice(), 3).expect("compress"),
        )
        .expect("packed");
        let old_mtime = SystemTime::now() - Duration::from_secs(120);
        OpenOptions::new()
            .write(true)
            .open(&packed)
            .expect("open")
            .set_times(FileTimes::new().set_modified(old_mtime))
            .expect("mtime");
        fs::remove_file(&plain).expect("remove plain");
        db(&state, ID, &packed, "openai");
        let report = transition(&home, &recovery, "openai", "magpie").expect("move");
        assert_eq!(report.moved, 1);
        let packed_decoded =
            zstd::stream::decode_all(fs::read(&packed).unwrap().as_slice()).unwrap();
        let line = String::from_utf8(packed_decoded).unwrap();
        assert!(line.lines().next().unwrap().contains("magpie"));
    }

    #[test]
    fn transition_rewrites_prefixed_metadata_and_thread_segments() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let segment = rollout.with_file_name(format!("rollout-{ID}_child.jsonl"));
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": ID, "model_provider": "openai", "title": "synthetic"}
        });
        fs::write(
            &rollout,
            format!(
                "{{\"type\":\"bootstrap\"}}\n{}\n{{\"type\":\"event\",\"value\":1}}\n",
                meta
            ),
        )
        .expect("prefixed rollout");
        fs::write(
            &segment,
            format!("{}\n{{\"type\":\"event\",\"value\":2}}\n", meta),
        )
        .expect("segment");
        let old_mtime = SystemTime::now() - Duration::from_secs(120);
        for path in [&rollout, &segment] {
            OpenOptions::new()
                .write(true)
                .open(path)
                .expect("open")
                .set_times(FileTimes::new().set_modified(old_mtime))
                .expect("mtime");
        }
        db(&state, ID, &rollout, "openai");

        let report = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(report.moved, 2);
        for path in [&rollout, &segment] {
            let snapshot = read_snapshot(path).expect("snapshot");
            assert_eq!(
                provider_for_thread(&snapshot.decoded, ID, "openai", "varyai"),
                RolloutProvider::Target
            );
        }
    }

    #[test]
    fn rewrite_changes_only_payload_provider_and_all_matching_metadata_lines() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        fs::create_dir_all(&home).expect("home");
        let rollout_dir = home.join("sessions").join("2026").join("11").join("05");
        fs::create_dir_all(&rollout_dir).expect("rollout dir");
        let rollout = rollout_dir.join(format!("rollout-{ID}.jsonl"));
        let first = format!(
            r#"{{"type":"session_meta","metadata":{{"model_provider":"openai"}},"payload":{{"id":"{ID}","model_provider":"openai","title":"first"}}}}"#
        );
        let second = format!(
            r#"{{"type":"session_meta","payload":{{"id":"{ID}","model_provider":"openai","title":"second"}}}}"#
        );
        fs::write(&rollout, format!("{first}\n{second}\n")).expect("rollout");
        let snapshot = read_snapshot(&rollout).expect("snapshot");

        rewrite_rollout(&rollout, &snapshot, ID, "openai", "varyai").expect("rewrite");

        let lines = fs::read_to_string(&rollout).expect("read");
        let values = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("json"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert_eq!(
            values[0]["metadata"]["model_provider"].as_str(),
            Some("openai")
        );
        assert_eq!(
            values[0]["payload"]["model_provider"].as_str(),
            Some("varyai")
        );
        assert_eq!(
            values[1]["payload"]["model_provider"].as_str(),
            Some("varyai")
        );
    }

    #[test]
    fn first_session_meta_identity_must_match_database_thread() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let other_id = "019a0000-0000-7000-8000-0000000000d1";
        let first = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": other_id, "model_provider": "openai"}
        });
        let later = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": ID, "model_provider": "openai"}
        });
        fs::write(&rollout, format!("{first}\n{later}\n")).expect("rollout");
        db(&state, ID, &rollout, "openai");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");

        let report = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(report.moved, 0);
        assert_eq!(report.skipped, 1);
        assert!(report.incomplete);
        assert_eq!(
            fs::read_to_string(&rollout).expect("read"),
            format!("{first}\n{later}\n")
        );
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "openai"
        );
    }

    #[test]
    fn missing_payload_provider_is_preserved_while_state_row_moves_and_restores() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": ID, "title": "legacy"}
        });
        fs::write(&rollout, format!("{meta}\n")).expect("rollout");
        db(&state, ID, &rollout, "openai");
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("mtime");

        let moved = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(moved.moved, 1);
        let moved_value: Value = serde_json::from_str(
            fs::read_to_string(&rollout)
                .expect("read")
                .lines()
                .next()
                .expect("line"),
        )
        .expect("json");
        assert!(moved_value["payload"].get("model_provider").is_none());
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "varyai"
        );

        let restored = restore(&home, &recovery, "openai", "varyai").expect("restore");
        assert_eq!(restored.restored, 1);
        let restored_value: Value = serde_json::from_str(
            fs::read_to_string(&rollout)
                .expect("read")
                .lines()
                .next()
                .expect("line"),
        )
        .expect("json");
        assert!(restored_value["payload"].get("model_provider").is_none());
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "openai"
        );
    }

    #[test]
    fn paginated_provider_growth_is_rejected_without_writing() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        fs::create_dir_all(&home).expect("home");
        let (rollout, _) = fixture(&home, ID, "openai", ".jsonl");
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": ID,
                "model_provider": "openai",
                "history_mode": "paginated"
            }
        });
        fs::write(&rollout, format!("{meta}\n")).expect("rollout");
        let before = fs::read(&rollout).expect("before");
        let snapshot = read_snapshot(&rollout).expect("snapshot");
        let error = rewrite_rollout(
            &rollout,
            &snapshot,
            ID,
            "openai",
            "provider-name-that-does-not-fit",
        )
        .expect_err("paginated growth must fail");
        assert_eq!(error, CONFLICT);
        assert_eq!(fs::read(&rollout).expect("after"), before);
    }

    #[test]
    fn paginated_provider_growth_can_use_existing_padding() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        fs::create_dir_all(&home).expect("home");
        let (rollout, _) = fixture(&home, ID, "openai", ".jsonl");
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": ID,
                "model_provider": "openai",
                "history_mode": "paginated"
            }
        });
        let original = format!("{meta}        \n");
        fs::write(&rollout, &original).expect("rollout");
        let snapshot = read_snapshot(&rollout).expect("snapshot");
        rewrite_rollout(&rollout, &snapshot, ID, "openai", "provider-long").expect("rewrite");
        let rewritten = fs::read_to_string(&rollout).expect("read");
        assert_eq!(rewritten.len(), original.len());
        assert!(rewritten.contains("\"model_provider\":\"provider-long\""));
    }

    #[test]
    fn unrelated_target_thread_is_not_journaled_across_retries_or_restore() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (source_rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let other_id = "019a0000-0000-7000-8000-0000000000d1";
        let (target_rollout, _) = fixture(&home, other_id, "varyai", ".jsonl");
        db(&state, ID, &source_rollout, "openai");
        Connection::open(&state)
            .expect("db")
            .execute(
                "INSERT INTO threads (id, rollout_path, model_provider) VALUES (?1, ?2, ?3)",
                params![
                    other_id,
                    target_rollout.to_string_lossy().to_string(),
                    "varyai"
                ],
            )
            .expect("target row");
        OpenOptions::new()
            .write(true)
            .open(&source_rollout)
            .expect("open source")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("source mtime");
        let target_before = fs::read(&target_rollout).expect("target before");
        let target_mtime = fs::metadata(&target_rollout)
            .expect("target metadata")
            .modified()
            .expect("target mtime");

        assert_eq!(
            transition(&home, &recovery, "openai", "varyai")
                .expect("first move")
                .moved,
            1
        );
        let retry = transition(&home, &recovery, "openai", "varyai").expect("retry move");
        assert_eq!(retry.moved, 0);
        assert!(!retry.incomplete);
        let receipt = read_receipt(&receipt_path(&recovery, "openai", "varyai"))
            .expect("receipt")
            .expect("receipt exists");
        assert_eq!(receipt.entries.len(), 1);
        assert_eq!(
            fs::read(&target_rollout).expect("target retry"),
            target_before
        );
        assert_eq!(
            fs::metadata(&target_rollout)
                .expect("target metadata")
                .modified()
                .expect("target mtime"),
            target_mtime
        );

        assert_eq!(
            restore(&home, &recovery, "openai", "varyai")
                .expect("restore")
                .restored,
            1
        );
        assert_eq!(
            fs::read(&target_rollout).expect("target restore"),
            target_before
        );
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row(
                "SELECT model_provider FROM threads WHERE id = ?1",
                params![other_id],
                |row| row.get::<_, String>(0),
            )
            .expect("target provider"),
            "varyai"
        );
    }

    #[test]
    fn retry_finishes_active_segment_after_the_database_has_moved() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let (segment, _) = fixture(&home, ID, "openai", "-segment.jsonl");
        db(&state, ID, &rollout, "openai");
        let quiet = SystemTime::now() - Duration::from_secs(120);
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("rollout")
            .set_times(FileTimes::new().set_modified(quiet))
            .expect("quiet rollout");
        let first = transition(&home, &recovery, "openai", "varyai").expect("first pass");
        assert!(first.incomplete);
        assert_eq!(first.moved, 1);
        assert_eq!(
            read_row_provider(&state, ID).expect("provider").as_deref(),
            Some("varyai")
        );
        assert_eq!(provider(&segment), "openai");
        OpenOptions::new()
            .write(true)
            .open(&segment)
            .expect("segment")
            .set_times(FileTimes::new().set_modified(quiet))
            .expect("quiet segment");
        let retry = transition(&home, &recovery, "openai", "varyai").expect("retry");
        assert!(!retry.incomplete);
        assert_eq!(retry.moved, 1);
        assert_eq!(provider(&segment), "varyai");
        assert_eq!(
            restore(&home, &recovery, "openai", "varyai")
                .expect("restore")
                .restored,
            2
        );
        assert_eq!(provider(&rollout), "openai");
        assert_eq!(provider(&segment), "openai");
    }

    #[test]
    fn restore_resumes_pending_after_file_write_before_database_update() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        db(&state, ID, &rollout, "openai");
        let old_mtime = SystemTime::now() - Duration::from_secs(120);
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(old_mtime))
            .expect("mtime");
        let snapshot = read_snapshot(&rollout).expect("snapshot");
        let canonical_rollout = fs::canonicalize(&rollout).expect("canonical rollout");
        let backup = recovery.join("files/entry-0.rollout");
        assert!(copy_backup(&snapshot.raw, &backup));
        let written =
            rewrite_rollout(&rollout, &snapshot, ID, "openai", "varyai").expect("rewrite");
        assert!(provider(&rollout) == "varyai");
        persist_receipt(
            &recovery,
            "openai",
            "varyai",
            &Receipt {
                version: RECEIPT_VERSION,
                from: "openai".into(),
                to: "varyai".into(),
                entries: vec![ReceiptEntry {
                    id: ID.into(),
                    db: state.clone(),
                    rollout: canonical_rollout,
                    backup,
                    state: ReceiptState::Pending,
                    paginated: false,
                }],
            },
        )
        .expect("receipt");

        // The argument is deliberately unused here; it documents the point at
        // which a real process could have been interrupted.
        assert!(!written.is_empty());
        let restored = restore(&home, &recovery, "openai", "varyai").expect("restore");
        assert_eq!(restored.restored, 1);
        assert_eq!(provider(&rollout), "openai");
        let conn = Connection::open(&state).expect("db");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "openai"
        );
    }

    #[test]
    fn restore_respects_database_provider_changed_by_user() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        db(&state, ID, &rollout, "openai");
        let old_mtime = SystemTime::now() - Duration::from_secs(120);
        OpenOptions::new()
            .write(true)
            .open(&rollout)
            .expect("open")
            .set_times(FileTimes::new().set_modified(old_mtime))
            .expect("mtime");
        assert_eq!(
            transition(&home, &recovery, "openai", "varyai")
                .expect("transition")
                .moved,
            1
        );
        let conn = Connection::open(&state).expect("db");
        conn.execute(
            "UPDATE threads SET model_provider = 'user-relay' WHERE id = ?1",
            params![ID],
        )
        .expect("user edit");

        let restored = restore(&home, &recovery, "openai", "varyai").expect("restore");
        assert_eq!(restored.restored, 0);
        assert_eq!(restored.skipped, 1);
        assert!(restored.incomplete);
        assert_eq!(provider(&rollout), "varyai");
        assert_eq!(
            conn.query_row("SELECT model_provider FROM threads", [], |row| row
                .get::<_, String>(0))
                .expect("provider"),
            "user-relay"
        );
    }

    #[test]
    fn checked_rollback_does_not_clobber_a_concurrent_append() {
        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        fs::create_dir_all(&home).expect("home");
        let (rollout, _) = fixture(&home, ID, "openai", ".jsonl");
        let snapshot = read_snapshot(&rollout).expect("snapshot");
        let written =
            rewrite_rollout(&rollout, &snapshot, ID, "openai", "varyai").expect("rewrite");
        fs::write(&rollout, b"concurrent append").expect("append");
        let error = restore_raw(
            &rollout,
            &written,
            &snapshot.raw,
            snapshot.modified,
            #[cfg(unix)]
            snapshot.mode,
        )
        .expect_err("concurrent edit must win");
        assert_eq!(error, CONFLICT);
        assert_eq!(fs::read(&rollout).expect("read"), b"concurrent append");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_database_is_reported_incomplete() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        let (rollout, state) = fixture(&home, ID, "openai", ".jsonl");
        let real_db = dir.path().join("real.sqlite");
        db(&real_db, ID, &rollout, "openai");
        symlink(&real_db, &state).expect("db symlink");

        let report = transition(&home, &recovery, "openai", "varyai").expect("move");
        assert_eq!(report.moved, 0);
        assert!(report.incomplete);
        assert_eq!(provider(&rollout), "openai");
        assert!(!receipt_path(&recovery, "openai", "varyai").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_receipt_is_rejected() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("temp");
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&recovery).expect("recovery");
        let target = dir.path().join("receipt-target.json");
        fs::write(&target, b"{}").expect("target");
        symlink(&target, receipt_path(&recovery, "openai", "varyai")).expect("receipt symlink");

        let error = transition(&home, &recovery, "openai", "varyai")
            .expect_err("symlinked receipt must be rejected");
        assert_eq!(error, CONFLICT);
    }
}
