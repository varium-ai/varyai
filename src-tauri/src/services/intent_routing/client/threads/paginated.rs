//! Paginated Codex display metadata is authoritative in SQLite. Preserve the
//! rollout bytes and offsets, and journal the row before changing its provider.
use super::*;

struct Row {
    provider: String,
    rollout: String,
}

fn read_row(home: &Path, db: &Path, id: &str) -> Result<Row, String> {
    let db = safe_db_path(home, db).ok_or(CONFLICT)?;
    let conn =
        Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| FAILED)?;
    let row = conn.query_row(
        "SELECT model_provider, rollout_path FROM threads WHERE id = ?1 AND history_mode = 'paginated'",
        [id],
        |row| Ok(Row { provider: row.get(0)?, rollout: row.get(1)? }),
    ).map_err(|_| CONFLICT)?;
    Ok(row)
}

fn ensure_quiet(home: &Path, row: &Row) -> Result<(), String> {
    if !row.rollout.is_empty() {
        let path = Path::new(&row.rollout);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            home.join(path)
        };
        match fs::symlink_metadata(path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                return Err(CONFLICT.into())
            }
            Ok(meta) if is_active(&meta.modified().map_err(|_| FAILED)?) => {
                return Err(CONFLICT.into())
            }
            Ok(_) => {}
            // A paginated thread may have no local segment. Its row still
            // identifies its provider; migration does not need to open it.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(FAILED.into()),
        }
    }
    Ok(())
}

fn update(db: &Path, id: &str, row: &Row, to: &str) -> Result<(), String> {
    validate_db_path(db)?;
    let conn =
        Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|_| FAILED)?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|_| FAILED)?;
    let changed = conn.execute(
        "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3 AND history_mode = 'paginated' AND rollout_path = ?4",
        params![to, id, row.provider, row.rollout],
    ).map_err(|_| CONFLICT)?;
    if changed != 1 {
        return Err(CONFLICT.into());
    }
    Ok(())
}

pub(super) fn transition(
    home: &Path,
    recovery: &Path,
    row: &ThreadRow,
    receipt: &mut Receipt,
) -> Result<bool, String> {
    let current = read_row(home, &row.db, &row.id)?;
    let existing = receipt
        .entries
        .iter()
        .position(|entry| entry.paginated && entry.db == row.db && entry.id == row.id);
    if current.provider == receipt.to && existing.is_none() {
        return Ok(false);
    }
    if current.provider != receipt.from && current.provider != receipt.to {
        return Err(CONFLICT.into());
    }
    if existing.is_some_and(|index| {
        receipt.entries[index].state == ReceiptState::Moved && current.provider == receipt.to
    }) {
        return Ok(false);
    }
    ensure_quiet(home, &current)?;
    let index = if let Some(index) = existing {
        index
    } else {
        let index = receipt.entries.len();
        receipt.entries.push(ReceiptEntry {
            id: row.id.clone(),
            db: row.db.clone(),
            rollout: row.rollout.clone(),
            backup: PathBuf::new(),
            state: ReceiptState::Pending,
            paginated: true,
        });
        persist_receipt(recovery, &receipt.from, &receipt.to, receipt)?;
        index
    };
    if current.provider == receipt.from {
        update(&row.db, &row.id, &current, &receipt.to)?;
    }
    receipt.entries[index].state = ReceiptState::Moved;
    persist_receipt(recovery, &receipt.from, &receipt.to, receipt)?;
    Ok(true)
}

pub(super) fn restore(
    home: &Path,
    entry: &ReceiptEntry,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let current = read_row(home, &entry.db, &entry.id)?;
    if current.provider == from {
        return Ok(());
    }
    if current.provider != to {
        return Err(CONFLICT.into());
    }
    ensure_quiet(home, &current)?;
    update(&entry.db, &entry.id, &current, from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(
        home: &Path,
        suffix: &str,
    ) -> Result<(PathBuf, Connection), Box<dyn std::error::Error>> {
        fs::create_dir_all(home.join("sessions"))?;
        let path = home
            .join("sessions")
            .join(format!("rollout-synthetic{suffix}"));
        fs::write(&path, b"synthetic suffix; no metadata or padding\n")?;
        File::options().write(true).open(&path)?.set_times(
            FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)),
        )?;
        let conn = Connection::open(home.join("state_5.sqlite"))?;
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL, history_mode TEXT NOT NULL)")?;
        conn.execute(
            "INSERT INTO threads VALUES ('synthetic', ?1, 'magpie', 'paginated')",
            [path.to_string_lossy().as_ref()],
        )?;
        Ok((path, conn))
    }

    fn provider(conn: &Connection) -> Result<String, rusqlite::Error> {
        conn.query_row("SELECT model_provider FROM threads", [], |row| row.get(0))
    }

    #[test]
    fn paginated_rows_do_not_need_rollout_decoding_or_file_budget(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for suffix in [".jsonl", ".jsonl.zst", ".missing", ".oversized"] {
            let dir = tempfile::tempdir()?;
            let home = dir.path().join("codex");
            let recovery = dir.path().join("recovery");
            let (path, conn) = fixture(&home, suffix)?;
            if suffix == ".missing" {
                fs::remove_file(&path)?;
            }
            if suffix == ".oversized" {
                File::options()
                    .write(true)
                    .open(&path)?
                    .set_len(MAX_SYNC_BYTES + 1)?;
                File::options().write(true).open(&path)?.set_times(
                    FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)),
                )?;
            }
            let before = fs::metadata(&path)
                .ok()
                .map(|m| (m.len(), m.modified().ok()));
            let moved = super::super::transition(&home, &recovery, "magpie", "varyai")?;
            assert_eq!(moved.moved, 1, "{suffix}");
            assert!(!moved.incomplete);
            assert_eq!(provider(&conn)?, "varyai");
            assert_eq!(
                fs::metadata(&path)
                    .ok()
                    .map(|m| (m.len(), m.modified().ok())),
                before
            );
            let restored = super::super::restore(&home, &recovery, "magpie", "varyai")?;
            assert_eq!(restored.restored, 1);
            assert_eq!(provider(&conn)?, "magpie");
            assert!(!restored.incomplete);
            if suffix == ".jsonl" || suffix == ".jsonl.zst" {
                assert_eq!(
                    fs::read(path)?,
                    b"synthetic suffix; no metadata or padding\n"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn paginated_rows_wait_for_writer_locks_and_recent_writes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        let (path, conn) = fixture(&home, ".jsonl")?;
        fs::create_dir(home.join("thread-writer-locks"))?;
        let held = writer_lock::hold(&home, "synthetic")?;
        assert!(super::super::transition(&home, &recovery, "magpie", "varyai")?.incomplete);
        assert_eq!(provider(&conn)?, "magpie");
        drop(held);
        File::options()
            .write(true)
            .open(&path)?
            .set_times(FileTimes::new().set_modified(SystemTime::now()))?;
        assert!(super::super::transition(&home, &recovery, "magpie", "varyai")?.incomplete);
        assert_eq!(provider(&conn)?, "magpie");
        File::options().write(true).open(&path)?.set_times(
            FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)),
        )?;
        assert_eq!(
            super::super::transition(&home, &recovery, "magpie", "varyai")?.moved,
            1
        );
        assert_eq!(provider(&conn)?, "varyai");
        Ok(())
    }

    #[test]
    fn paginated_restore_refuses_changed_provider_or_history_mode(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        let (_path, conn) = fixture(&home, ".jsonl")?;
        super::super::transition(&home, &recovery, "magpie", "varyai")?;
        conn.execute("UPDATE threads SET model_provider='external'", [])?;
        assert!(super::super::restore(&home, &recovery, "magpie", "varyai")?.incomplete);
        assert_eq!(provider(&conn)?, "external");
        conn.execute(
            "UPDATE threads SET model_provider='varyai', history_mode='legacy'",
            [],
        )?;
        assert!(super::super::restore(&home, &recovery, "magpie", "varyai")?.incomplete);
        assert_eq!(provider(&conn)?, "varyai");
        conn.execute("UPDATE threads SET history_mode='paginated'", [])?;
        assert_eq!(
            super::super::restore(&home, &recovery, "magpie", "varyai")?.restored,
            1
        );
        assert_eq!(provider(&conn)?, "magpie");
        Ok(())
    }

    #[test]
    fn paginated_pending_journal_recovers_on_either_side_of_the_row_write(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for current in ["magpie", "varyai"] {
            let dir = tempfile::tempdir()?;
            let home = dir.path().join("codex");
            let recovery = dir.path().join("recovery");
            let (_path, conn) = fixture(&home, ".jsonl")?;
            super::super::transition(&home, &recovery, "magpie", "varyai")?;
            let mut receipt = read_receipt(&receipt_path(&recovery, "magpie", "varyai"))?
                .ok_or("missing journal")?;
            receipt.entries[0].state = ReceiptState::Pending;
            persist_receipt(&recovery, "magpie", "varyai", &receipt)?;
            conn.execute("UPDATE threads SET model_provider=?1", [current])?;
            let report = super::super::transition(&home, &recovery, "magpie", "varyai")?;
            assert_eq!(report.moved, 1);
            assert!(!report.incomplete);
            assert_eq!(provider(&conn)?, "varyai");
            assert_eq!(
                super::super::restore(&home, &recovery, "magpie", "varyai")?.restored,
                1
            );
            assert_eq!(provider(&conn)?, "magpie");
        }
        Ok(())
    }
}
