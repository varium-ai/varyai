//! Paginated Codex display metadata is authoritative in SQLite. Preserve the
//! rollout bytes and offsets, and journal the row before changing its provider.
use super::*;

const BATCH_SIZE: usize = 64;

struct Row {
    provider: String,
    rollout: String,
}

struct Pending {
    index: usize,
    id: String,
    row: Row,
    _writer_lock: writer_lock::Guard,
}

fn read_row(conn: &Connection, id: &str) -> Result<Row, String> {
    conn.query_row(
        "SELECT model_provider, rollout_path FROM threads WHERE id = ?1 AND history_mode = 'paginated'",
        [id],
        |row| Ok(Row { provider: row.get(0)?, rollout: row.get(1)? }),
    ).map_err(|_| CONFLICT.into())
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

fn update_batch(
    conn: &mut Connection,
    pending: &[Pending],
    from: &str,
    to: &str,
) -> Result<(), String> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| CONFLICT)?;
    {
        let mut update = tx.prepare_cached(
            "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3 AND history_mode = 'paginated' AND rollout_path = ?4",
        ).map_err(|_| FAILED)?;
        for entry in pending {
            if entry.row.provider == from {
                let changed = update
                    .execute(params![to, entry.id, from, entry.row.rollout])
                    .map_err(|_| CONFLICT)?;
                if changed != 1 {
                    return Err(CONFLICT.into());
                }
            } else {
                let current = read_row(&tx, &entry.id)?;
                if current.provider != to || current.rollout != entry.row.rollout {
                    return Err(CONFLICT.into());
                }
            }
        }
    }
    tx.commit().map_err(|_| FAILED.into())
}

/// Reuse each database connection and commit bounded groups of quiet threads.
/// A durable pending journal precedes every transition transaction; a crash
/// before the completion journal leaves an idempotent recovery record.
pub(super) fn process(
    home: &Path,
    recovery: &Path,
    rows: &[ThreadRow],
    receipt: &mut Receipt,
    restoring: bool,
) -> Result<ThreadTransitionReport, String> {
    use std::collections::{BTreeMap, HashMap};

    let from = receipt.from.clone();
    let to = receipt.to.clone();
    let mut indices = HashMap::new();
    for (index, entry) in receipt
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.paginated)
    {
        indices
            .entry((entry.db.clone(), entry.id.clone()))
            .or_insert(index);
    }
    let mut databases = BTreeMap::<&PathBuf, Vec<&ThreadRow>>::new();
    for row in rows.iter().filter(|row| row.paginated) {
        databases.entry(&row.db).or_default().push(row);
    }
    let mut report = ThreadTransitionReport::default();
    for (db, rows) in databases {
        let open = || -> Result<Connection, String> {
            let db = safe_db_path(home, db).ok_or(CONFLICT)?;
            validate_db_path(&db)?;
            let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(|_| FAILED)?;
            conn.busy_timeout(Duration::from_secs(2))
                .map_err(|_| FAILED)?;
            Ok(conn)
        };
        let mut conn = match open() {
            Ok(conn) => conn,
            Err(_) => {
                report.skipped += rows.len();
                report.incomplete = true;
                continue;
            }
        };
        for chunk in rows.chunks(BATCH_SIZE) {
            let mut pending = Vec::new();
            for row in chunk {
                let candidate = (|| -> Result<Option<Pending>, String> {
                    let existing = indices.get(&(row.db.clone(), row.id.clone())).copied();
                    let current = read_row(&conn, &row.id)?;
                    // Completed and unrelated target rows need neither a writer
                    // lock nor a journal entry, even if Codex currently holds them.
                    let completed = |current: &Row| {
                        !restoring
                            && current.provider == to
                            && existing.is_none_or(|index| {
                                receipt.entries[index].state == ReceiptState::Moved
                            })
                    };
                    if completed(&current) {
                        return Ok(None);
                    }
                    let guard = writer_lock::hold(home, &row.id)?;
                    let current = read_row(&conn, &row.id)?;
                    if completed(&current) {
                        return Ok(None);
                    }
                    if current.provider != from && current.provider != to {
                        return Err(CONFLICT.into());
                    }
                    if !restoring || current.provider != from {
                        ensure_quiet(home, &current)?;
                    }
                    let index = match existing {
                        Some(index) => index,
                        None if !restoring => {
                            let index = receipt.entries.len();
                            receipt.entries.push(ReceiptEntry {
                                id: row.id.clone(),
                                db: row.db.clone(),
                                rollout: row.rollout.clone(),
                                backup: PathBuf::new(),
                                state: ReceiptState::Pending,
                                paginated: true,
                            });
                            indices.insert((row.db.clone(), row.id.clone()), index);
                            index
                        }
                        None => return Err(CONFLICT.into()),
                    };
                    if !restoring {
                        receipt.entries[index].state = ReceiptState::Pending;
                    }
                    Ok(Some(Pending {
                        index,
                        id: row.id.clone(),
                        row: current,
                        _writer_lock: guard,
                    }))
                })();
                match candidate {
                    Ok(Some(entry)) => pending.push(entry),
                    Ok(None) => {}
                    Err(_) => {
                        report.skipped += 1;
                        report.incomplete = true;
                    }
                }
            }
            if pending.is_empty() {
                continue;
            }
            if !restoring {
                persist_receipt(recovery, &from, &to, receipt)?;
            }
            let (source, target) = if restoring {
                (&to, &from)
            } else {
                (&from, &to)
            };
            if update_batch(&mut conn, &pending, source, target).is_err() {
                report.skipped += pending.len();
                report.incomplete = true;
                continue;
            }
            for entry in &pending {
                receipt.entries[entry.index].state = if restoring {
                    ReceiptState::Restored
                } else {
                    ReceiptState::Moved
                };
            }
            // Keep writer locks until the committed batch is recorded durably.
            persist_receipt(recovery, &from, &to, receipt)?;
            if restoring {
                report.restored += pending.len();
            } else {
                report.moved += pending.len();
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn many_threads(home: &Path, count: usize) -> Result<Connection, Box<dyn std::error::Error>> {
        fs::create_dir_all(home)?;
        let mut conn = Connection::open(home.join("state_5.sqlite"))?;
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL, history_mode TEXT NOT NULL)")?;
        let tx = conn.transaction()?;
        for index in 0..count {
            tx.execute(
                "INSERT INTO threads VALUES (?1, '', 'magpie', 'paginated')",
                [format!("synthetic-{index:04}")],
            )?;
        }
        tx.commit()?;
        Ok(conn)
    }

    #[test]
    fn paginated_batch_failure_keeps_rows_atomic_and_journal_recoverable(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        let conn = many_threads(&home, 2)?;
        conn.execute_batch("CREATE TRIGGER reject_second BEFORE UPDATE ON threads WHEN OLD.id = 'synthetic-0001' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END")?;
        let report = super::super::transition(&home, &recovery, "magpie", "varyai")?;
        assert!(report.incomplete);
        assert_eq!(report.moved, 0);
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM threads WHERE model_provider='magpie'",
                [],
                |r| r.get::<_, usize>(0)
            )?,
            2
        );
        conn.execute_batch("DROP TRIGGER reject_second")?;
        let report = super::super::transition(&home, &recovery, "magpie", "varyai")?;
        assert_eq!(report.moved, 2);
        assert!(!report.incomplete);
        conn.execute_batch("CREATE TRIGGER reject_second BEFORE UPDATE ON threads WHEN OLD.id = 'synthetic-0001' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END")?;
        let report = super::super::restore(&home, &recovery, "magpie", "varyai")?;
        assert!(report.incomplete);
        assert_eq!(report.restored, 0);
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM threads WHERE model_provider='varyai'",
                [],
                |r| r.get::<_, usize>(0)
            )?,
            2
        );
        conn.execute_batch("DROP TRIGGER reject_second")?;
        assert_eq!(
            super::super::restore(&home, &recovery, "magpie", "varyai")?.restored,
            2
        );
        Ok(())
    }

    #[test]
    fn paginated_recovery_rechecks_completed_rows_inside_transaction(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for change in [
            "model_provider='external'",
            "history_mode='legacy'",
            "rollout_path='external.jsonl'",
        ] {
            let dir = tempfile::tempdir()?;
            let home = dir.path().join("codex");
            let mut conn = many_threads(&home, 2)?;
            conn.execute(
                "UPDATE threads SET model_provider='varyai' WHERE id='synthetic-0001'",
                [],
            )?;
            let pending = (0..2)
                .map(|index| {
                    let id = format!("synthetic-{index:04}");
                    Ok(Pending {
                        index,
                        row: read_row(&conn, &id)?,
                        _writer_lock: writer_lock::hold(&home, &id)?,
                        id,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            conn.execute(
                &format!("UPDATE threads SET {change} WHERE id='synthetic-0001'"),
                [],
            )?;
            assert!(update_batch(&mut conn, &pending, "magpie", "varyai").is_err());
            assert_eq!(
                conn.query_row(
                    "SELECT model_provider FROM threads WHERE id='synthetic-0000'",
                    [],
                    |r| r.get::<_, String>(0)
                )?,
                "magpie"
            );
        }
        Ok(())
    }

    #[test]
    fn paginated_thousand_threads_round_trip_with_busy_and_unowned_rows(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        let conn = many_threads(&home, 1000)?;
        fs::create_dir(home.join("thread-writer-locks"))?;
        let held = writer_lock::hold(&home, "synthetic-0000")?;
        conn.execute(
            "INSERT INTO threads VALUES ('unowned', '', 'varyai', 'paginated')",
            [],
        )?;
        let started = std::time::Instant::now();
        let report = super::super::transition(&home, &recovery, "magpie", "varyai")?;
        let transition_ms = started.elapsed().as_millis();
        assert_eq!(report.moved, 999);
        assert!(report.incomplete);
        assert_eq!(recorded_ids(&recovery, "magpie", "varyai")?.len(), 999);
        drop(held);
        conn.execute(
            "UPDATE threads SET model_provider='external' WHERE id='synthetic-0001'",
            [],
        )?;
        let started = std::time::Instant::now();
        let restored = super::super::restore(&home, &recovery, "magpie", "varyai")?;
        let restore_ms = started.elapsed().as_millis();
        assert_eq!(restored.restored, 998);
        assert!(restored.incomplete);
        assert_eq!(
            conn.query_row(
                "SELECT model_provider FROM threads WHERE id='unowned'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "varyai"
        );
        assert_eq!(
            conn.query_row(
                "SELECT model_provider FROM threads WHERE id='synthetic-0001'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "external"
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM threads WHERE model_provider='magpie'",
                [],
                |r| r.get::<_, usize>(0)
            )?,
            999
        );
        eprintln!("Synthetic 1000-thread workload: transition_ms={transition_ms}, restore_ms={restore_ms}");
        Ok(())
    }

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
    fn paginated_restore_completes_already_restored_rows_with_recent_writes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("codex");
        let recovery = dir.path().join("recovery");
        let (path, conn) = fixture(&home, ".jsonl")?;
        super::super::transition(&home, &recovery, "magpie", "varyai")?;
        // Simulate a restored row whose completion journal was interrupted.
        conn.execute("UPDATE threads SET model_provider='magpie'", [])?;
        File::options()
            .write(true)
            .open(&path)?
            .set_times(FileTimes::new().set_modified(SystemTime::now()))?;
        let report = super::super::restore(&home, &recovery, "magpie", "varyai")?;
        assert_eq!(report.restored, 1);
        assert!(!report.incomplete);
        assert!(recorded_ids(&recovery, "magpie", "varyai")?.is_empty());
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
