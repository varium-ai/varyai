use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

pub(crate) fn load_titles(path: &Path) -> Vec<(String, String)> {
    let Ok(db) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    if db.busy_timeout(Duration::ZERO).is_err() {
        return Vec::new();
    }
    let Ok(mut query) = db.prepare(
        "SELECT id, COALESCE(NULLIF(TRIM(custom_title), ''), NULLIF(TRIM(title), ''))
         FROM sessions WHERE deleted_at IS NULL ORDER BY id",
    ) else {
        return Vec::new();
    };
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}
