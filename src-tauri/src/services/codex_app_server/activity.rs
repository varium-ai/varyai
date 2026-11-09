use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::SystemTime,
};

const TURN_WINDOW_US: u128 = 30 * 60 * 1_000_000;

fn modified_us(path: &Path) -> Result<u128, ()> {
    match fs::metadata(path) {
        Ok(meta) => meta
            .modified()
            .map_err(|_| ())?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|time| time.as_micros())
            .map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(()),
    }
}

pub(super) fn busy(home: &Path, since_us: u64) -> Result<bool, ()> {
    let now_us = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| ())?
        .as_micros();
    let db = home.join("thread_history_1.sqlite");
    // A long-lived server can leave abandoned inProgress rows behind. Only
    // known file inactivity expires them; an active WAL keeps them protected.
    let last_db_activity =
        modified_us(&db)?.max(modified_us(&home.join("thread_history_1.sqlite-wal"))?);
    if db.exists() && now_us.saturating_sub(last_db_activity) <= TURN_WINDOW_US {
        let conn = Connection::open_with_flags(
            &db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| ())?;
        conn.busy_timeout(std::time::Duration::from_millis(50))
            .map_err(|_| ())?;
        // Codex stores started_at in Unix seconds. Old abandoned turns belong to a prior server.
        let active: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM thread_turns WHERE status = 'inProgress' AND (started_at IS NULL OR started_at >= ?1))",
            [(since_us / 1_000_000).saturating_sub(2)], |row| row.get(0),
        ).map_err(|_| ())?;
        if active {
            return Ok(true);
        }
    }
    sessions(&home.join("sessions"), since_us, now_us)
}

fn sessions(root: &Path, since_us: u64, now_us: u128) -> Result<bool, ()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(()),
    };
    for entry in entries {
        let entry = entry.map_err(|_| ())?;
        // Windows caches directory-entry timestamps while Codex keeps its
        // rollout open. Query the file itself so an active turn cannot look
        // abandoned after the directory's cached timestamp ages out.
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(()),
        };
        if meta.file_type().is_symlink() {
            return Err(());
        }
        if meta.is_dir() {
            if sessions(&entry.path(), since_us, now_us)? {
                return Ok(true);
            }
        } else if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
            let modified = meta
                .modified()
                .map_err(|_| ())?
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|_| ())?
                .as_micros();
            if modified + 2_000_000 < u128::from(since_us)
                || now_us.saturating_sub(modified) > TURN_WINDOW_US
            {
                continue;
            }
            if turn_open(&entry.path(), since_us)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[derive(Deserialize)]
struct Line<'a> {
    #[serde(default, borrow)]
    timestamp: Option<&'a str>,
    #[serde(rename = "type", borrow)]
    kind: &'a str,
    #[serde(borrow)]
    payload: Option<Event<'a>>,
}

#[derive(Deserialize)]
struct Event<'a> {
    #[serde(rename = "type", default, borrow)]
    kind: Option<&'a str>,
}

fn turn_open(path: &Path, since_us: u64) -> Result<bool, ()> {
    let mut file = fs::File::open(path).map_err(|_| ())?;
    let len = file.metadata().map_err(|_| ())?.len();
    let skip = len.saturating_sub(512 * 1024);
    file.seek(SeekFrom::Start(skip)).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.take(512 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let mut open = None;
    for (index, bytes) in bytes.split(|b| *b == b'\n').enumerate() {
        if bytes.is_empty() || (skip > 0 && index == 0) {
            continue;
        }
        let line: Line<'_> = serde_json::from_slice(bytes).map_err(|_| ())?;
        if line.kind != "event_msg" {
            continue;
        }
        match line.payload.and_then(|payload| payload.kind) {
            Some("task_started") => {
                let at = line
                    .timestamp
                    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                    .ok_or(())?;
                open = Some(at.timestamp_micros() >= since_us.saturating_sub(2_000_000) as i64);
            }
            Some("task_complete" | "turn_aborted") => open = Some(false),
            _ => {}
        }
    }
    // A bounded suffix may omit the start of a long tool execution.
    open.or((skip == 0).then_some(false)).ok_or(())
}
