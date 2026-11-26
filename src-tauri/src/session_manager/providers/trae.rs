use crate::services::trae_bridge::shared::Edition;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::SystemTime,
};

pub(crate) fn session_titles(edition: Edition) -> Vec<(String, String)> {
    with_logs(edition, |cache, logs| cache.read(logs))
}

pub(crate) fn session_turns(edition: Edition) -> Vec<NativeTurn> {
    with_logs(edition, |cache, logs| {
        cache.refresh(logs);
        cache
            .0
            .values()
            .flat_map(|log| &log.turns)
            .filter(|turn| turn.edition == edition)
            .cloned()
            .collect()
    })
}

fn with_logs<T: Default>(edition: Edition, read: impl FnOnce(&mut TitleLogs, &Path) -> T) -> T {
    static CN: LazyLock<Mutex<TitleLogs>> = LazyLock::new(Mutex::default);
    static INTL: LazyLock<Mutex<TitleLogs>> = LazyLock::new(Mutex::default);
    let (product, cache) = match edition {
        Edition::China => ("Trae CN", &CN),
        Edition::International => ("Trae", &INTL),
    };
    let Some(base) = dirs::data_dir() else {
        return T::default();
    };
    cache
        .lock()
        .map(|mut cache| read(&mut cache, &base.join(product).join("logs")))
        .unwrap_or_default()
}

#[derive(Clone)]
pub(crate) struct NativeTurn {
    edition: Edition,
    pub session_id: String,
    pub started_at: i64,
    pub prompt: [u8; 32],
}

pub(crate) fn turn_digest(text: &str) -> Option<[u8; 32]> {
    static SALT: LazyLock<uuid::Uuid> = LazyLock::new(uuid::Uuid::new_v4);
    let text = text.trim();
    if text.is_empty() || text.len() > 16 * 1024 {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(SALT.as_bytes());
    hash.update(text.as_bytes());
    Some(hash.finalize().into())
}

#[derive(Default)]
pub(crate) struct TitleLogs(BTreeMap<PathBuf, WindowLog>);

#[derive(Default)]
struct WindowLog {
    offset: u64,
    created: Option<SystemTime>,
    titles: BTreeMap<String, (i64, String)>,
    turns: VecDeque<NativeTurn>,
}

impl TitleLogs {
    fn refresh(&mut self, logs: &Path) {
        let mut seen = std::collections::BTreeSet::new();
        let Ok(runs) = std::fs::read_dir(logs) else {
            self.0.clear();
            return;
        };
        for run in runs.flatten() {
            let name = run.file_name();
            let Some(name) = name.to_str() else { continue };
            if chrono::NaiveDateTime::parse_from_str(name, "%Y%m%dT%H%M%S").is_err()
                || !run.file_type().is_ok_and(|kind| kind.is_dir())
            {
                continue;
            }
            let Ok(windows) = std::fs::read_dir(run.path()) else {
                continue;
            };
            for window in windows.flatten() {
                let name = window.file_name();
                if !name
                    .to_str()
                    .and_then(|s| s.strip_prefix("window"))
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    || !window.file_type().is_ok_and(|kind| kind.is_dir())
                {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(window.path()) else {
                    continue;
                };
                for log in files.flatten() {
                    if !log.file_name().to_str().is_some_and(renderer_log)
                        || !log.file_type().is_ok_and(|kind| kind.is_file())
                    {
                        continue;
                    }
                    let path = log.path();
                    if let Ok(file) = File::open(&path) {
                        self.0.entry(path.clone()).or_default().refresh(file);
                        seen.insert(path);
                    }
                }
            }
        }
        self.0.retain(|path, _| seen.contains(path));
    }

    pub(crate) fn read(&mut self, logs: &Path) -> Vec<(String, String)> {
        self.refresh(logs);
        let mut latest = BTreeMap::new();
        for window in self.0.values() {
            for (id, update) in &window.titles {
                let entry = latest.entry(id).or_insert(update);
                if update.0 > entry.0 {
                    *entry = update;
                }
            }
        }
        latest
            .into_iter()
            .filter(|(_, (_, title))| !title.is_empty())
            .map(|(id, (_, title))| (id.clone(), title.clone()))
            .collect()
    }
}

fn renderer_log(name: &str) -> bool {
    name == "renderer.log"
        || name
            .strip_prefix("renderer.")
            .and_then(|suffix| suffix.strip_suffix(".log"))
            .is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            })
}

impl WindowLog {
    fn refresh(&mut self, mut file: File) {
        let Ok(meta) = file.metadata() else { return };
        let created = meta.created().ok();
        if meta.len() < self.offset || created != self.created {
            *self = Self {
                created,
                ..Self::default()
            };
        }
        if meta.len() == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut reader = BufReader::new(file.take(meta.len() - self.offset));
        let mut line = Vec::new();
        loop {
            line.clear();
            // Other renderer diagnostics can contain arbitrarily long lines.
            let Ok(n) = (&mut reader).take(64 * 1024).read_until(b'\n', &mut line) else {
                break;
            };
            if n == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                if n < 64 * 1024 {
                    break;
                }
                let Ok(skipped) = reader.skip_until(b'\n') else {
                    break;
                };
                self.offset += (n + skipped) as u64;
                continue;
            }
            self.offset += n as u64;
            let Ok(line) = std::str::from_utf8(&line) else {
                continue;
            };
            if let Some(update) = title_update(line) {
                let entry = self.titles.entry(update.chat_session_id).or_default();
                if update.updated_at >= entry.0 {
                    *entry = (
                        update.updated_at,
                        update.title.trim().chars().take(256).collect(),
                    );
                }
            } else if let Some(turn) = native_turn(line) {
                if self.turns.len() == 128 {
                    self.turns.pop_front();
                }
                self.turns.push_back(turn);
            }
        }
    }
}

#[derive(Deserialize)]
struct TurnMetadata {
    #[serde(rename = "sessionId")]
    context_session_id: String,
    session_id: String,
    message_type: String,
    user_message_context: UserContext,
}

#[derive(Deserialize)]
struct UserContext {
    query: String,
    model_info: ModelEndpoint,
}

#[derive(Deserialize)]
struct ModelEndpoint {
    base_url: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NativeQueryPart {
    Text {
        data: NativeQueryText,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct NativeQueryText {
    content: String,
}

fn native_query_digest(query: &str) -> Option<[u8; 32]> {
    let Ok(parts) = serde_json::from_str::<Vec<NativeQueryPart>>(query) else {
        return turn_digest(query);
    };
    let mut texts = parts.iter().filter_map(|part| match part {
        NativeQueryPart::Text { data } => Some(data.content.as_str()),
        NativeQueryPart::Other => None,
    });
    let text = texts.next()?;
    if texts.next().is_some() {
        return None;
    }
    turn_digest(text)
}

fn native_turn(line: &str) -> Option<NativeTurn> {
    let (prefix, json) = line.split_once(" [MetadataHandler] received metadata ")?;
    let timestamp = prefix
        .strip_suffix(" [info] [ai-chat/v2]")
        .or_else(|| prefix.strip_suffix(" [info] [trae-chat-core]"))?;
    let started_at = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .timestamp_millis();
    let event: TurnMetadata = serde_json::from_str(json).ok()?;
    if event.message_type != "task" || event.session_id != event.context_session_id {
        return None;
    }
    let context = event.user_message_context;
    let url = url::Url::parse(&context.model_info.base_url).ok()?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
        return None;
    }
    let segments = url.path().split('/').collect::<Vec<_>>();
    let app = match segments.as_slice() {
        ["", "connections", app, "v1", "chat", "completions"]
        | ["", "connections", app, "v1", "responses"]
        | ["", "upstream", app, "providers", _, "v1", "responses"] => app,
        _ => return None,
    };
    Some(NativeTurn {
        edition: Edition::parse(app)?,
        session_id: crate::proxy::tool_observation::types::normalized_session_id(Some(
            &event.session_id,
        ))?,
        started_at,
        prompt: native_query_digest(&context.query)?,
    })
}

#[derive(Deserialize)]
struct TitleUpdate {
    chat_session_id: String,
    title: String,
    updated_at: i64,
}

fn title_update(line: &str) -> Option<TitleUpdate> {
    let (prefix, rest) = line.split_once(" [RealtimeEventService] Session updated: ")?;
    let timestamp = prefix
        .strip_suffix(" [info] [ai-chat/v2]")
        .or_else(|| prefix.strip_suffix(" [info] [trae-chat-core]"))?;
    chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let (context, payload) = rest.split_at(rest.find('{')?);
    let event: TitleUpdate = serde_json::from_str(payload).ok()?;
    (context.split_whitespace().next() == Some(event.chat_session_id.as_str())
        && !event.chat_session_id.is_empty())
    .then_some(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn event(id: &str, title: &str, timestamp: i64) -> String {
        let payload =
            serde_json::json!({"chat_session_id":id,"title":title,"updated_at":timestamp});
        format!("2026-11-26T14:00:00.000+08:00 [info] [ai-chat/v2] [RealtimeEventService] Session updated: {id} active-session {payload}\n")
    }

    #[test]
    fn reads_only_consistent_local_turn_metadata() -> Result<(), Box<dyn std::error::Error>> {
        for (app, logger, endpoint) in [
            ("trae", "ai-chat/v2", "chat/completions"),
            ("trae-intl", "trae-chat-core", "responses"),
        ] {
            let metadata = serde_json::json!({
                "sessionId": "synthetic-session", "session_id": "synthetic-session",
                "message_type": "task", "user_message_context": {
                    "query": serde_json::json!([
                        {"type": "text", "data": {"content": "Synthetic prompt"}}
                    ]).to_string(),
                    "model_info": {"base_url": format!("http://127.0.0.1:12345/connections/{app}/v1/{endpoint}")}
                }
            });
            let line = format!("2026-11-26T20:00:00+08:00 [info] [{logger}] [MetadataHandler] received metadata {metadata}");
            let turn = native_turn(&line).ok_or("missing native turn")?;
            assert_eq!(turn.edition.app(), app);
            assert_eq!(turn.session_id, "synthetic-session");
            assert_eq!(Some(turn.prompt), turn_digest("Synthetic prompt"));
            assert!(native_turn(&line.replacen("127.0.0.1", "example.invalid", 1)).is_none());
            assert!(
                native_turn(&line.replacen("synthetic-session", "another-session", 1)).is_none()
            );
        }
        Ok(())
    }

    #[test]
    fn trae_titles_follow_accepted_native_updates_across_windows(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let run = root.path().join("20261126T110000");
        let first = run.join("window1");
        let second = run.join("window2");
        for dir in [&first, &second] {
            std::fs::create_dir_all(dir)?;
        }
        let path = first.join("renderer.log");
        let mut file = File::create(&path)?;
        std::fs::write(
            first.join("renderer.backup.log"),
            event("bad", "Wrong file", 9),
        )?;
        file.write_all(event("one", "First chat", 1).as_bytes())?;
        file.write_all(event("two", "Second chat", 2).as_bytes())?;
        let renamed = event("one", "Renamed chat", 4);
        file.write_all(&renamed.as_bytes()[..35])?;
        let mut logs = TitleLogs::default();
        assert_eq!(
            logs.read(root.path()),
            [
                ("one".into(), "First chat".into()),
                ("two".into(), "Second chat".into())
            ]
        );
        file.write_all(&renamed.as_bytes()[35..])?;
        file.write_all(event("two", "", 5).as_bytes())?;
        let mut other = File::create(second.join("renderer.log"))?;
        other.write_all(event("one", "Stale", 3).as_bytes())?;
        writeln!(other, "{}", "x".repeat(70000))?;
        other.write_all(
            event("bad", "Wrong", 8)
                .replacen("updated: bad", "updated: unrelated", 1)
                .as_bytes(),
        )?;
        other.write_all(
            event("bad", "Wrong", 8)
                .replace("[RealtimeEventService]", "[AnotherService]")
                .as_bytes(),
        )?;
        other.write_all(event("three", "Third", 8).as_bytes())?;
        assert_eq!(
            logs.read(root.path()),
            [
                ("one".into(), "Renamed chat".into()),
                ("three".into(), "Third".into())
            ]
        );
        drop(file);
        std::fs::write(path, event("four", "New file", 9))?;
        assert_eq!(logs.read(root.path()).len(), 3);
        let missing = root.path().join("missing");
        assert!(logs.read(&missing).is_empty());
        assert!(!missing.exists());
        Ok(())
    }
}
