use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

pub(crate) fn load_task_titles(path: &Path) -> Vec<(String, String)> {
    let Ok(connection) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    if connection.busy_timeout(Duration::from_millis(100)).is_err() {
        return Vec::new();
    }

    // The desktop task index keeps the sess_ prefix that ZCode drops from x-session-id.
    let Ok(mut query) = connection.prepare(
        "WITH local_tasks AS (
            SELECT CASE WHEN SUBSTR(task_id, 1, 5) = 'sess_'
                        THEN SUBSTR(task_id, 6) ELSE task_id END AS session_id,
                   title
            FROM tasks JOIN hosts USING (host_id)
            WHERE hosts.kind = 'local' AND deleted = 0 AND COALESCE(cli_deleted, 0) = 0
        )
        SELECT session_id, SUBSTR(TRIM(title), 1, 256) FROM local_tasks
        GROUP BY session_id
        HAVING COUNT(*) = 1 AND TRIM(title) <> '' AND session_id <> ''",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?))) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zcode_task_index_exposes_only_unique_local_titles() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tasks-index.sqlite");
        assert!(load_task_titles(&path).is_empty());
        assert!(!path.exists());
        let connection = Connection::open(&path)?;
        assert!(load_task_titles(&path).is_empty());
        connection.execute_batch(
            "CREATE TABLE hosts (host_id TEXT PRIMARY KEY, kind TEXT);
             INSERT INTO hosts VALUES ('desktop', 'local'), ('other-machine', 'remote');
             CREATE TABLE tasks (
                 task_id TEXT, title TEXT, host_id TEXT, deleted INTEGER, cli_deleted INTEGER
             );
             INSERT INTO tasks VALUES
                 ('sess_current', ' Synthetic title ', 'desktop', 0, NULL),
                 ('sess_current', 'Remote title', 'other-machine', 0, 0),
                 ('sess_deleted', 'Deleted task', 'desktop', 1, 0),
                 ('sess_cli-deleted', 'Deleted session', 'desktop', 0, 1),
                 ('sess_blank', '  ', 'desktop', 0, 0),
                 ('sess_duplicate', 'First title', 'desktop', 0, 0),
                 ('duplicate', 'Second title', 'desktop', 0, 0),
                 ('sess_remote-only', 'Remote title', 'other-machine', 0, 0);",
        )?;
        assert_eq!(
            load_task_titles(&path),
            vec![("current".to_owned(), "Synthetic title".to_owned())]
        );
        Ok(())
    }
}
