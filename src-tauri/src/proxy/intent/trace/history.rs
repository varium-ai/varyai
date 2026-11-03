//! Private, bounded gateway history. The forwarding path only tries a queue send.
use super::{Route, State};
use chrono::{Local, NaiveDate};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
};

const RETENTION_DAYS: i64 = 30;
const BYTE_BUDGET: u64 = 200 * 1024 * 1024;
const MAX_LINE: u64 = 1024 * 1024;
const DAY_LIMIT: usize = 2000;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    pub requests: Vec<Route>,
    pub cut: bool,
}

enum Message {
    Save(Box<Route>),
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

pub(super) struct History {
    root: PathBuf,
    sender: SyncSender<Message>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Routing history unavailable")
}

fn date(value: &str) -> io::Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| invalid())?;
    if parsed.format("%Y-%m-%d").to_string() != value {
        return Err(invalid());
    }
    Ok(parsed)
}

fn private_root(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open(path: &Path, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(!append).append(append).create(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid());
    }
    #[cfg(unix)]
    if append {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn files(root: &Path) -> io::Result<Vec<(String, PathBuf, u64)>> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        _ => return Err(invalid()),
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(day) = name
            .strip_suffix(".jsonl.gz")
            .or_else(|| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if date(day).is_err() {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            result.push((day.to_owned(), entry.path(), metadata.len()));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

fn maintain(root: &Path, today: NaiveDate) -> io::Result<()> {
    for (day, path, _) in files(root)? {
        let age = today.signed_duration_since(date(&day)?).num_days();
        if age >= RETENTION_DAYS {
            fs::remove_file(path)?;
        } else if age > 0 && path.extension().is_some_and(|e| e == "jsonl") {
            let destination = path.with_extension("jsonl.gz");
            // Never append compressed streams or overwrite a previous archive.
            if destination.exists() {
                continue;
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let temporary = root.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
            let compressed = (|| -> io::Result<()> {
                let mut encoder = GzEncoder::new(options.open(&temporary)?, Compression::default());
                io::copy(&mut open(&path, false)?, &mut encoder)?;
                encoder.finish()?.sync_all()?;
                fs::rename(&temporary, &destination)?;
                fs::remove_file(&path)
            })();
            let _ = fs::remove_file(temporary);
            compressed?;
        }
    }
    let inventory = files(root)?;
    let mut bytes: u64 = inventory.iter().map(|(_, _, size)| size).sum();
    for (day, path, size) in inventory {
        if bytes <= BYTE_BUDGET {
            break;
        }
        if date(&day)? >= today {
            continue;
        }
        fs::remove_file(path)?;
        bytes = bytes.saturating_sub(size);
    }
    Ok(())
}

impl History {
    pub(super) fn new(root: PathBuf) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(128);
        let writer_root = root.clone();
        std::thread::Builder::new()
            .name("routing-history".into())
            .spawn(move || {
                let mut maintained = None;
                let mut last_maintenance = std::time::Instant::now();
                let _ = private_root(&writer_root).and_then(|()| {
                    let today = Local::now().date_naive();
                    maintain(&writer_root, today)?;
                    maintained = Some(today);
                    Ok(())
                });
                for message in receiver {
                    match message {
                        Message::Save(row) => {
                            // Disk, serialization and retention failures never reach forwarding.
                            let _ = (|| -> io::Result<()> {
                                private_root(&writer_root)?;
                                let today = Local::now().date_naive();
                                if maintained != Some(today)
                                    || last_maintenance.elapsed().as_secs() >= 60
                                {
                                    maintain(&writer_root, today)?;
                                    maintained = Some(today);
                                    last_maintenance = std::time::Instant::now();
                                }
                                let day = chrono::DateTime::from_timestamp_millis(row.started_at)
                                    .ok_or_else(invalid)?
                                    .with_timezone(&Local)
                                    .date_naive();
                                if today.signed_duration_since(day).num_days() >= RETENTION_DAYS {
                                    return Ok(());
                                }
                                let mut bytes = serde_json::to_vec(&row).map_err(|_| invalid())?;
                                if bytes.len() as u64 >= MAX_LINE {
                                    return Ok(());
                                }
                                bytes.push(b'\n');
                                open(&writer_root.join(format!("{day}.jsonl")), true)?
                                    .write_all(&bytes)
                            })();
                        }
                        #[cfg(test)]
                        Message::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })?;
        Ok(Self { root, sender })
    }

    pub(super) fn save(&self, row: Route) {
        let _ = self.sender.try_send(Message::Save(Box::new(row)));
    }

    #[cfg(test)]
    pub(super) fn flush(&self) -> io::Result<()> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(Message::Flush(sender))
            .map_err(|_| invalid())?;
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| invalid())
    }

    pub(super) fn days(&self) -> io::Result<Vec<String>> {
        let mut days = files(&self.root)?
            .into_iter()
            .map(|(day, _, _)| day)
            .collect::<Vec<_>>();
        days.sort_by(|a, b| b.cmp(a));
        days.dedup();
        Ok(days)
    }

    pub(super) fn read(&self, day: &str) -> io::Result<Page> {
        date(day)?;
        // Validate the directory as well as each leaf before opening history.
        files(&self.root)?;
        let mut rows = VecDeque::new();
        let mut cut = false;
        // A prior-day request can complete after its existing file was compressed.
        for suffix in ["jsonl.gz", "jsonl"] {
            let path = self.root.join(format!("{day}.{suffix}"));
            let mut file = match open(&path, false) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let tail = !suffix.ends_with("gz") && file.metadata()?.len() > BYTE_BUDGET;
            if tail {
                file.seek(SeekFrom::End(-(BYTE_BUDGET as i64)))?;
                cut = true;
            }
            let reader: Box<dyn Read> = if suffix.ends_with("gz") {
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            let mut reader = BufReader::new(reader.take(BYTE_BUDGET));
            if tail {
                reader.by_ref().take(MAX_LINE).skip_until(b'\n')?;
            }
            loop {
                let mut line = Vec::new();
                let n = reader
                    .by_ref()
                    .take(MAX_LINE)
                    .read_until(b'\n', &mut line)?;
                if n == 0 {
                    break;
                }
                if line.last() != Some(&b'\n') {
                    // Ignore an incomplete append; a subsequent read can see its final newline.
                    if n as u64 == MAX_LINE {
                        return Err(invalid());
                    }
                    break;
                }
                let Ok(row) = serde_json::from_slice::<Route>(&line) else {
                    continue;
                };
                if matches!(row.state, State::Routing | State::Streaming)
                    || row.candidates.len() > 256
                    || row.steps.len() > 1024
                {
                    continue;
                }
                rows.push_back(row);
                if rows.len() > DAY_LIMIT {
                    rows.pop_front();
                    cut = true;
                }
            }
            if reader.get_ref().limit() == 0 {
                cut = true;
            }
        }
        let mut requests = rows.into_iter().collect::<Vec<_>>();
        requests.sort_by_key(|row| std::cmp::Reverse(row.started_at));
        Ok(Page { requests, cut })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Instant};

    fn fixture() -> Route {
        let store = Arc::new(super::super::Store::default());
        let request = store.begin("codex", "fixture", "Fixture", "ordered", Instant::now());
        request.finish(200, State::Completed);
        store.snapshot().remove(0)
    }

    #[test]
    fn day_reads_bound_history_and_ignore_corrupt_or_partial_appends() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let day = Local::now().date_naive().to_string();
        let history = History::new(dir.path().to_owned())?;
        history.flush()?;
        let mut file = open(&dir.path().join(format!("{day}.jsonl")), true)?;
        let mut row = fixture();
        for index in 0..DAY_LIMIT + 7 {
            row.id = format!("fixture-{index}");
            row.started_at = index as i64;
            serde_json::to_writer(&mut file, &row)?;
            file.write_all(b"\n")?;
        }
        file.write_all(b"malformed\n{\"unfinished\":")?;
        let page = history.read(&day)?;
        assert!(page.cut);
        assert_eq!(page.requests.len(), DAY_LIMIT);
        assert_eq!(page.requests[0].id, format!("fixture-{}", DAY_LIMIT + 6));
        assert_eq!(
            page.requests.last().map(|row| row.id.as_str()),
            Some("fixture-7")
        );
        Ok(())
    }

    #[test]
    fn full_or_disconnected_history_queue_never_waits_for_a_disk_writer() {
        let (sender, receiver) = mpsc::sync_channel(0);
        let history = History {
            root: PathBuf::new(),
            sender,
        };
        history.save(fixture());
        drop(receiver);
        history.save(fixture());
    }

    #[cfg(unix)]
    #[test]
    fn private_history_files_do_not_follow_symlinks() -> io::Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("history");
        private_root(&root)?;
        let history = History::new(root.clone())?;
        let row = fixture();
        let day = Local::now().date_naive().to_string();
        history.save(row.clone());
        history.flush()?;
        let path = root.join(format!("{day}.jsonl"));
        assert_eq!(fs::metadata(&root)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        fs::remove_file(&path)?;
        let unrelated = dir.path().join("unrelated");
        fs::write(&unrelated, b"synthetic")?;
        symlink(&unrelated, &path)?;
        history.save(row);
        history.flush()?;
        assert!(history.read(&day).is_err());
        assert_eq!(fs::read(&unrelated)?, b"synthetic");
        Ok(())
    }

    #[test]
    fn retention_compresses_previous_days_and_excludes_unrelated_files() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let today = NaiveDate::from_ymd_opt(2026, 11, 3).ok_or_else(invalid)?;
        fs::write(dir.path().join("2026-11-02.jsonl"), b"synthetic\n")?;
        fs::write(dir.path().join("2026-10-01.jsonl"), b"synthetic\n")?;
        fs::write(dir.path().join("unrelated.jsonl"), b"synthetic\n")?;
        maintain(dir.path(), today)?;
        assert!(!dir.path().join("2026-10-01.jsonl").exists());
        assert!(!dir.path().join("2026-11-02.jsonl").exists());
        let mut content = String::new();
        GzDecoder::new(open(&dir.path().join("2026-11-02.jsonl.gz"), false)?)
            .read_to_string(&mut content)?;
        assert_eq!(content, "synthetic\n");
        assert!(dir.path().join("unrelated.jsonl").exists());
        Ok(())
    }
}
