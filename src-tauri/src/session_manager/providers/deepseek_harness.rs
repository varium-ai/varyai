use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "deepseek-harness";
const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;
const DSH_TURN_KEPT: usize = 4;
const DSH_INDEX_MIN_SCAN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Default)]
pub(crate) struct DshSessionRecord {
    pub session_id: String,
    pub parent_session: Option<String>,
    pub convs: Vec<String>,
    pub seed_convs: Vec<String>,
    pub turn_convs: Vec<String>,
    pub response_ids: Vec<String>,
}

#[derive(Default)]
struct CachedFileRecord {
    len: u64,
    modified: Option<SystemTime>,
    record: Option<DshSessionRecord>,
}

#[derive(Default)]
struct DshIndexCache {
    root: PathBuf,
    last_scan: Option<Instant>,
    files: BTreeMap<PathBuf, CachedFileRecord>,
}

static DSH_INDEX_CACHE: LazyLock<Mutex<DshIndexCache>> = LazyLock::new(Mutex::default);

pub(crate) fn parse_session_filename(name: &str) -> Option<(u32, bool)> {
    let (prefix, is_zstd) = if let Some(stem) = name.strip_suffix(".zstd") {
        (stem, true)
    } else {
        (name, false)
    };
    let stem = prefix.strip_suffix(".jsonl")?;
    if stem == "session" {
        return Some((0, is_zstd));
    }
    let version = stem.strip_prefix("session.v")?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((version.parse().ok()?, is_zstd))
}

pub(crate) fn latest_session_file(session_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(session_dir).ok()?;
    let mut best: Option<(u32, Option<SystemTime>, PathBuf)> = None;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some((version, _)) = parse_session_filename(name) else {
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().ok();
        let replace = match &best {
            None => true,
            Some((best_ver, best_mod, _)) => {
                version > *best_ver || (version == *best_ver && modified > *best_mod)
            }
        };
        if replace {
            best = Some((version, modified, path));
        }
    }
    best.map(|(_, _, path)| path)
}

pub fn dsh_home() -> PathBuf {
    let isolated = std::env::var("CC_SWITCH_TEST_HOME").is_ok_and(|home| !home.trim().is_empty());
    std::env::var_os("DSH_HOME")
        .filter(|value| !isolated && !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::get_home_dir().join(".dsh"))
}

pub fn session_roots() -> Vec<PathBuf> {
    vec![dsh_home().join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = dsh_home().join("sessions");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut sessions = Vec::new();
    let Ok(workspaces) = fs::read_dir(&root) else {
        return Vec::new();
    };
    for workspace in workspaces.flatten() {
        let workspace_path = workspace.path();
        if !workspace_path.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&workspace_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }
            if let Some(session_file) = latest_session_file(&entry_path) {
                if let Some(meta) = parse_session(&session_file) {
                    sessions.push(meta);
                }
            }
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for_each_session_line(path, |line| {
        if !line.contains("\"user/message\"") && !line.contains("\"assistant/message\"") {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let ts = value.get("time").and_then(parse_timestamp_to_ms);
        match kind {
            "user/message" => {
                let text = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "user".to_string(),
                        content: text,
                        ts,
                    });
                }
            }
            "assistant/message" => {
                let text = value
                    .pointer("/data/content")
                    .or_else(|| value.pointer("/data/message/content"))
                    .map(extract_text)
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "assistant".to_string(),
                        content: text,
                        ts,
                    });
                }
            }
            _ => {}
        }
    })?;
    Ok(messages)
}

pub fn delete_session(_root: &Path, source_path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(source_path).ok_or_else(|| {
        format!(
            "Invalid DeepSeek Harness session file: {}",
            source_path.display()
        )
    })?;
    if meta.session_id != session_id && meta.session_id.strip_prefix("session-") != Some(session_id)
    {
        return Err(format!(
            "Session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }
    fs::remove_file(source_path).map_err(|err| format!("Failed to delete session: {err}"))?;
    Ok(true)
}

fn read_lines_from<R: BufRead>(mut reader: R, mut visit: impl FnMut(&str)) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    visit(trimmed);
                }
            }
        }
    }
}

fn for_each_session_line(path: &Path, visit: impl FnMut(&str)) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|err| format!("Failed to stat session: {err}"))?;
    if meta.len() > MAX_SESSION_BYTES {
        return Err(format!("Session file exceeds max size: {}", path.display()));
    }
    let file = File::open(path).map_err(|err| format!("Failed to open session: {err}"))?;
    let is_zstd = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".zstd"));
    if is_zstd {
        let decoder = zstd::stream::read::Decoder::new(BufReader::new(file))
            .map_err(|err| format!("Failed to decode zstd session: {err}"))?;
        read_lines_from(BufReader::new(decoder), visit);
    } else {
        read_lines_from(BufReader::new(file), visit);
    }
    Ok(())
}

fn magpie_conv_hash(raw_message: &str) -> String {
    let digest = Sha256::digest(raw_message.as_bytes());
    let mut key = String::from("magpie-");
    use std::fmt::Write;
    for byte in &digest[..12] {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

/// Compute Magpie `dshConvs` conversation IDs for a DSH user message content payload.
pub(crate) fn dsh_convs(content: &Value) -> Vec<String> {
    let text = if let Some(raw) = content.as_str() {
        (!raw.is_empty()).then_some(raw)
    } else if let Some(parts) = content.as_array() {
        if parts.len() == 1 && parts[0]["type"].as_str() == Some("text") {
            parts[0]["text"].as_str().filter(|text| !text.is_empty())
        } else {
            None
        }
    } else {
        None
    };
    let Some(text) = text else {
        return Vec::new();
    };
    let Ok(serialized) = serde_json::to_string(text) else {
        return Vec::new();
    };
    [
        format!(r#"{{"role":"user","content":{serialized}}}"#),
        format!(r#"{{"role":"user","content":[{{"type":"text","text":{serialized}}}]}}"#),
        format!(
            r#"{{"role":"user","content":[{{"type":"text","text":{serialized},"cache_control":{{"type":"ephemeral"}}}}]}}"#
        ),
    ]
    .into_iter()
    .map(|spelling| magpie_conv_hash(&spelling))
    .collect()
}

fn parse_correlation_record(path: &Path) -> Option<DshSessionRecord> {
    let mut session_id = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let mut is_seeded = false;
    let mut parent_session: Option<String> = None;
    let mut main = true;
    let mut convs = Vec::new();
    let mut seed_convs = Vec::new();
    let mut turn_convs = Vec::new();
    let mut response_ids = Vec::new();
    let mut saw_opening_user = false;

    for_each_session_line(path, |line| {
        const WANTED: [&str; 6] = [
            "\"session\"",
            "\"user/message\"",
            "\"turn/start\"",
            "\"session/title-llm-request\"",
            "\"session/end-seed\"",
            "\"assistant/message\"",
        ];
        if !WANTED.iter().any(|needle| line.contains(needle)) {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "session" => {
                if let Some(id) = value.get("id").and_then(Value::as_str) {
                    if !id.trim().is_empty() {
                        session_id = Some(id.to_owned());
                    }
                }
                is_seeded = value
                    .get("isSeeded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                parent_session = value
                    .get("parentSession")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|parent| !parent.is_empty())
                    .map(str::to_owned);
                let delegation_depth = value
                    .get("delegationDepth")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                main = parent_session.is_none() && delegation_depth == 0;
            }
            "user/message" if main => {
                let Some(content) = value.pointer("/data/content") else {
                    return;
                };
                if !saw_opening_user
                    && value.pointer("/data/source/kind").and_then(Value::as_str) == Some("user")
                {
                    let hashes = dsh_convs(content);
                    if !hashes.is_empty() {
                        saw_opening_user = true;
                        convs.extend(hashes);
                    }
                }
                if let Some(first) = dsh_convs(content).into_iter().next() {
                    turn_convs.push(first);
                    if turn_convs.len() > DSH_TURN_KEPT {
                        let excess = turn_convs.len() - DSH_TURN_KEPT;
                        turn_convs.drain(..excess);
                    }
                }
            }
            "turn/start" if main => {
                turn_convs.clear();
            }
            "session/title-llm-request" if main => {
                if let Some(messages) = value.pointer("/data/messages").and_then(Value::as_array) {
                    for message in messages {
                        if message.get("role").and_then(Value::as_str) == Some("user") {
                            convs.extend(dsh_convs(&message["content"]));
                            break;
                        }
                    }
                }
            }
            "session/end-seed" => {
                if is_seeded
                    && value
                        .pointer("/data/inherited")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                {
                    seed_convs.extend(std::mem::take(&mut convs));
                    turn_convs.clear();
                    response_ids.clear();
                }
            }
            "assistant/message" if main => {
                if let Some(id) = value
                    .pointer("/data/message/source/replayState/response/responseId")
                    .or_else(|| value.pointer("/data/replayState/response/responseId"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    response_ids.push(id.to_owned());
                }
            }
            _ => {}
        }
    })
    .ok()?;

    let session_id = session_id?;
    Some(DshSessionRecord {
        session_id,
        parent_session,
        convs,
        seed_convs,
        turn_convs,
        response_ids,
    })
}

#[cfg(test)]
pub(crate) fn scan_correlation_index_in(root: &Path) -> Vec<DshSessionRecord> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut records = Vec::new();
    let Ok(workspaces) = fs::read_dir(root) else {
        return Vec::new();
    };
    for workspace in workspaces.flatten() {
        let workspace_path = workspace.path();
        if !workspace_path.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&workspace_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }
            if let Some(session_file) = latest_session_file(&entry_path) {
                if let Some(record) = parse_correlation_record(&session_file) {
                    records.push(record);
                }
            }
        }
    }
    records
}

pub(crate) fn scan_correlation_index_with(force: bool) -> (Vec<DshSessionRecord>, bool) {
    let root = dsh_home().join("sessions");
    if !root.is_dir() {
        return (Vec::new(), false);
    }
    let known_meta = {
        let mut cache = DSH_INDEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if cache.root != root {
            cache.root = root.clone();
            cache.last_scan = None;
            cache.files.clear();
        }
        if !force
            && cache
                .last_scan
                .is_some_and(|at| at.elapsed() < DSH_INDEX_MIN_SCAN_INTERVAL)
        {
            return (
                cache
                    .files
                    .values()
                    .filter_map(|entry| entry.record.clone())
                    .collect(),
                true,
            );
        }
        cache
            .files
            .iter()
            .map(|(path, entry)| (path.clone(), (entry.len, entry.modified)))
            .collect::<HashMap<PathBuf, (u64, Option<SystemTime>)>>()
    };

    let mut seen = HashSet::new();
    let mut updated: Vec<(PathBuf, CachedFileRecord)> = Vec::new();
    if let Ok(workspaces) = fs::read_dir(&root) {
        for workspace in workspaces.flatten() {
            let workspace_path = workspace.path();
            if !workspace_path.is_dir() {
                continue;
            }
            let Ok(entries) = fs::read_dir(&workspace_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if !entry_path.is_dir() {
                    continue;
                }
                let Some(session_file) = latest_session_file(&entry_path) else {
                    continue;
                };
                let Ok(meta) = fs::metadata(&session_file) else {
                    continue;
                };
                let len = meta.len();
                let modified = meta.modified().ok();
                seen.insert(session_file.clone());
                let unchanged = known_meta
                    .get(&session_file)
                    .is_some_and(|&(prev_len, prev_mod)| prev_len == len && prev_mod == modified);
                if !unchanged {
                    let record = parse_correlation_record(&session_file);
                    updated.push((
                        session_file,
                        CachedFileRecord {
                            len,
                            modified,
                            record,
                        },
                    ));
                }
            }
        }
    }

    let mut cache = DSH_INDEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.root == root {
        for (path, entry) in updated {
            cache.files.insert(path, entry);
        }
        cache.files.retain(|path, _| seen.contains(path));
        cache.last_scan = Some(Instant::now());
    }
    (
        cache
            .files
            .values()
            .filter_map(|entry| entry.record.clone())
            .collect(),
        false,
    )
}

pub(crate) struct DshOwners {
    by_response: HashMap<String, Option<String>>,
    by_conv: HashMap<String, Option<String>>,
    by_last: HashMap<String, Option<String>>,
    fork_of: HashMap<String, String>,
    seeds: HashSet<(String, String)>,
    copied: HashSet<String>,
}

impl DshOwners {
    fn insert_unique(map: &mut HashMap<String, Option<String>>, key: &str, session_id: &str) {
        if key.is_empty() {
            return;
        }
        match map.get_mut(key) {
            Some(slot) => {
                if slot.as_deref() != Some(session_id) {
                    *slot = None;
                }
            }
            None => {
                map.insert(key.to_owned(), Some(session_id.to_owned()));
            }
        }
    }

    pub(crate) fn from_sessions(sessions: &[DshSessionRecord]) -> Self {
        let mut by_response = HashMap::new();
        let mut by_conv = HashMap::new();
        let mut by_last = HashMap::new();
        let mut fork_of = HashMap::new();
        let mut seeds = HashSet::new();
        let mut copied = HashSet::new();
        for session in sessions {
            if let Some(parent) = &session.parent_session {
                fork_of.insert(session.session_id.clone(), parent.clone());
            }
            for c in &session.convs {
                Self::insert_unique(&mut by_conv, c, &session.session_id);
            }
            for r in &session.response_ids {
                Self::insert_unique(&mut by_response, r, &session.session_id);
            }
            for c in &session.turn_convs {
                Self::insert_unique(&mut by_last, c, &session.session_id);
            }
            for c in &session.seed_convs {
                seeds.insert((session.session_id.clone(), c.clone()));
                copied.insert(c.clone());
            }
        }
        Self {
            by_response,
            by_conv,
            by_last,
            fork_of,
            seeds,
            copied,
        }
    }

    fn unique<'a>(
        &'a self,
        map: &'a HashMap<String, Option<String>>,
        key: &str,
    ) -> Option<&'a str> {
        map.get(key)?.as_deref()
    }

    fn forked_from<'a>(&'a self, mut id: &'a str, from: &str, opening_conv: Option<&str>) -> bool {
        for _ in 0..64 {
            if id == from {
                return true;
            }
            if let Some(conv) = opening_conv {
                if self.seeds.contains(&(id.to_owned(), conv.to_owned())) {
                    return true;
                }
            }
            let Some(parent) = self.fork_of.get(id) else {
                break;
            };
            id = parent.as_str();
        }
        false
    }

    pub(crate) fn resolve(
        &self,
        response_id: Option<&str>,
        opening_conv: Option<&str>,
        turn_convs: &[String],
        waiting_for_reply: bool,
    ) -> Option<String> {
        let opened = opening_conv.and_then(|c| self.unique(&self.by_conv, c));
        if let Some(id) = response_id.and_then(|r| self.unique(&self.by_response, r)) {
            if opened.is_none_or(|parent| self.forked_from(id, parent, opening_conv)) {
                return Some(id.to_owned());
            }
        }
        for c in turn_convs {
            if let Some(id) = self.unique(&self.by_last, c) {
                if opened.is_none_or(|parent| self.forked_from(id, parent, opening_conv)) {
                    return Some(id.to_owned());
                }
            }
        }
        if let Some(opened) = opened {
            if waiting_for_reply && opening_conv.is_some_and(|c| self.copied.contains(c)) {
                return None;
            }
            return Some(opened.to_owned());
        }
        None
    }
}

pub(crate) fn resolve_session_for_audit(
    opening_conv: &str,
    turn_convs: &[String],
) -> Option<String> {
    let (sessions, from_cache) = scan_correlation_index_with(false);
    if let Some(id) =
        DshOwners::from_sessions(&sessions).resolve(None, Some(opening_conv), turn_convs, true)
    {
        return Some(id);
    }
    if from_cache {
        let (fresh, _) = scan_correlation_index_with(true);
        return DshOwners::from_sessions(&fresh).resolve(
            None,
            Some(opening_conv),
            turn_convs,
            true,
        );
    }
    None
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let mut session_id = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut last_active_at: Option<i64> = None;
    let mut native_title: Option<String> = None;
    let mut first_user_message: Option<String> = None;
    let mut last_message: Option<String> = None;

    for_each_session_line(path, |line| {
        const WANTED: [&str; 4] = [
            "\"session\"",
            "\"session/title\"",
            "\"user/message\"",
            "\"assistant/message\"",
        ];
        if !WANTED.iter().any(|needle| line.contains(needle)) {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = value
            .get("time")
            .or_else(|| value.get("createdAt"))
            .and_then(parse_timestamp_to_ms)
        {
            created_at.get_or_insert(ts);
            last_active_at = Some(last_active_at.map_or(ts, |prev| prev.max(ts)));
        }
        match kind {
            "session" => {
                if let Some(id) = value.get("id").and_then(Value::as_str) {
                    if !id.trim().is_empty() {
                        session_id = Some(id.to_owned());
                    }
                }
                if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                    if !cwd.trim().is_empty() {
                        project_dir = Some(cwd.to_owned());
                    }
                }
            }
            "session/title" => {
                if let Some(title) = value.pointer("/data/title").and_then(Value::as_str) {
                    let trimmed = title.trim();
                    if !trimmed.is_empty() {
                        native_title = Some(truncate_summary(trimmed, TITLE_MAX_CHARS));
                    }
                }
            }
            "user/message" => {
                let text = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    if first_user_message.is_none() {
                        first_user_message = Some(truncate_summary(trimmed, TITLE_MAX_CHARS));
                    }
                    last_message = Some(truncate_summary(trimmed, 160));
                }
            }
            "assistant/message" => {
                let text = value
                    .pointer("/data/content")
                    .or_else(|| value.pointer("/data/message/content"))
                    .map(extract_text)
                    .unwrap_or_default();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    last_message = Some(truncate_summary(trimmed, 160));
                }
            }
            _ => {}
        }
    })
    .ok()?;

    let session_id = session_id?;
    let has_native_title = native_title.is_some();
    let title = native_title.or(first_user_message);
    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title,
        has_native_title,
        summary: last_message,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.display().to_string()),
        resume_command: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_multi_frame_zstd(path: &Path, frames: &[&str]) {
        let mut file = File::create(path).expect("create session file");
        for frame in frames {
            let compressed = zstd::stream::encode_all(frame.as_bytes(), 0).expect("compress frame");
            file.write_all(&compressed).expect("write frame");
        }
    }

    #[test]
    fn scans_multi_frame_zstd_session_and_prefers_latest_native_title() {
        let dir = TempDir::new().expect("temp dir");
        let session_dir = dir
            .path()
            .join("sessions")
            .join("--Users-demo-workspace--")
            .join("session-3a4b6f40-4669-45d6-a1f1-9e995e375293");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let session_file = session_dir.join("session.v4.jsonl.zstd");
        write_multi_frame_zstd(
            &session_file,
            &[
                "{\"type\":\"session\",\"version\":4,\"id\":\"session-3a4b6f40-4669-45d6-a1f1-9e995e375293\",\"createdAt\":1795504573620,\"cwd\":\"/Users/demo/workspace\"}\n",
                "{\"type\":\"session/title\",\"seq\":14,\"time\":1795504576633,\"data\":{\"title\":\"Fallback title\"}}\n{\"type\":\"session/title\",\"seq\":16,\"time\":1795504583807,\"data\":{\"title\":\"Native dsh title\"}}\n",
            ],
        );

        let meta = parse_session(&session_file).expect("parse session");
        assert_eq!(meta.provider_id, "deepseek-harness");
        assert_eq!(
            meta.session_id,
            "session-3a4b6f40-4669-45d6-a1f1-9e995e375293"
        );
        assert!(meta.has_native_title);
        assert_eq!(meta.title.as_deref(), Some("Native dsh title"));
        assert_eq!(meta.project_dir.as_deref(), Some("/Users/demo/workspace"));
    }

    #[test]
    fn latest_session_file_prefers_highest_version_and_plain_or_zstd_formats() {
        let dir = TempDir::new().expect("temp dir");
        assert_eq!(parse_session_filename("session.jsonl"), Some((0, false)));
        assert_eq!(
            parse_session_filename("session.jsonl.zstd"),
            Some((0, true))
        );
        assert_eq!(parse_session_filename("session.v3.jsonl"), Some((3, false)));
        assert_eq!(
            parse_session_filename("session.v4.jsonl.zstd"),
            Some((4, true))
        );
        assert_eq!(parse_session_filename("session.v4.json"), None);

        let v0 = dir.path().join("session.jsonl");
        let v3 = dir.path().join("session.v3.jsonl");
        let v4 = dir.path().join("session.v4.jsonl.zstd");
        fs::write(&v0, "{}\n").unwrap();
        assert_eq!(
            latest_session_file(dir.path()).as_deref(),
            Some(v0.as_path())
        );
        fs::write(&v3, "{}\n").unwrap();
        assert_eq!(
            latest_session_file(dir.path()).as_deref(),
            Some(v3.as_path())
        );
        write_multi_frame_zstd(&v4, &["{}\n"]);
        assert_eq!(
            latest_session_file(dir.path()).as_deref(),
            Some(v4.as_path())
        );
    }

    #[test]
    fn dsh_correlation_index_and_owner_resolution_handle_forks_turns_and_responses() {
        let dir = TempDir::new().expect("temp dir");
        let proj = dir.path().join("--work-proj--");
        let parent_dir = proj.join("session-parent");
        let fork_dir = proj.join("session-fork");
        fs::create_dir_all(&parent_dir).unwrap();
        fs::create_dir_all(&fork_dir).unwrap();

        let opening_text = "Investigate <system> edge case & routing";
        let parent_turn_text = "parent follow-up turn";
        let fork_turn_text = "fork follow-up turn";

        let opening_hashes = dsh_convs(&serde_json::json!(opening_text));
        assert_eq!(opening_hashes.len(), 3);
        let parent_turn_hash = dsh_convs(&serde_json::json!(parent_turn_text))[0].clone();
        let fork_turn_hash = dsh_convs(&serde_json::json!(fork_turn_text))[0].clone();

        let parent_lines = [
            serde_json::json!({"type":"session","version":4,"id":"session-parent","cwd":"/w","createdAt":1000,"delegationDepth":0,"isSeeded":false}).to_string(),
            serde_json::json!({"type":"turn/start","seq":1,"time":1001,"data":{"turn":1}}).to_string(),
            serde_json::json!({"type":"user/message","seq":2,"time":1002,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":opening_text}]}}).to_string(),
            serde_json::json!({"type":"session/title","seq":3,"time":1003,"data":{"title":"Parent Session"}}).to_string(),
            serde_json::json!({"type":"assistant/message","seq":4,"time":1004,"data":{"message":{"source":{"replayState":{"response":{"responseId":"resp-parent"}}}}}}).to_string(),
            serde_json::json!({"type":"turn/start","seq":5,"time":1005,"data":{"turn":2}}).to_string(),
            serde_json::json!({"type":"user/message","seq":6,"time":1006,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":parent_turn_text}]}}).to_string(),
        ].join("\n") + "\n";

        let fork_lines = [
            serde_json::json!({"type":"session","version":4,"id":"session-fork","cwd":"/w","createdAt":2000,"delegationDepth":0,"isSeeded":true}).to_string(),
            serde_json::json!({"type":"turn/start","seq":1,"time":2001,"data":{"turn":1}}).to_string(),
            serde_json::json!({"type":"user/message","seq":2,"time":2002,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":opening_text}]}}).to_string(),
            serde_json::json!({"type":"session/end-seed","seq":3,"time":2003,"data":{"inherited":true}}).to_string(),
            serde_json::json!({"type":"session/title","seq":4,"time":2004,"data":{"title":"Fork Session"}}).to_string(),
            serde_json::json!({"type":"turn/start","seq":5,"time":2005,"data":{"turn":2}}).to_string(),
            serde_json::json!({"type":"user/message","seq":6,"time":2006,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":fork_turn_text}]}}).to_string(),
            serde_json::json!({"type":"assistant/message","seq":7,"time":2007,"data":{"message":{"source":{"replayState":{"response":{"responseId":"resp-fork"}}}}}}).to_string(),
        ].join("\n") + "\n";

        fs::write(parent_dir.join("session.v4.jsonl"), parent_lines).unwrap();
        fs::write(fork_dir.join("session.v4.jsonl"), fork_lines).unwrap();

        let records = scan_correlation_index_in(dir.path());
        assert_eq!(records.len(), 2);
        let fork_record = records
            .iter()
            .find(|record| record.session_id == "session-fork")
            .expect("fork record");
        assert!(
            fork_record.convs.is_empty(),
            "post-seed fork turns must not populate convs"
        );
        let owners = DshOwners::from_sessions(&records);

        assert_eq!(
            owners
                .resolve(Some("resp-fork"), Some(&opening_hashes[0]), &[], false)
                .as_deref(),
            Some("session-fork")
        );
        assert_eq!(
            owners
                .resolve(
                    None,
                    Some(&opening_hashes[1]),
                    &[fork_turn_hash.clone()],
                    true
                )
                .as_deref(),
            Some("session-fork")
        );
        assert_eq!(
            owners.resolve(None, Some(&fork_turn_hash), &[], false),
            None,
            "new session opening matching fork turn text must not match fork via by_conv"
        );
        assert_eq!(
            owners.resolve(None, Some(&opening_hashes[0]), &[], true),
            None,
            "shared opening must fail closed while waiting for reply"
        );
        assert_eq!(
            owners
                .resolve(None, Some(&opening_hashes[0]), &[parent_turn_hash], true)
                .as_deref(),
            Some("session-parent")
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_session_for_audit_rescans_when_cache_misses() {
        struct RestoreHome(Option<std::ffi::OsString>);
        impl Drop for RestoreHome {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(prev) => std::env::set_var("CC_SWITCH_TEST_HOME", prev),
                    None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
                }
            }
        }
        let temp = TempDir::new().expect("temp dir");
        let _restore = RestoreHome(std::env::var_os("CC_SWITCH_TEST_HOME"));
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let sessions_root = temp.path().join(".dsh").join("sessions").join("--proj--");
        fs::create_dir_all(&sessions_root).unwrap();
        let (_, from_cache) = scan_correlation_index_with(false);
        assert!(!from_cache);

        let new_dir = sessions_root.join("session-fresh-after-scan");
        fs::create_dir_all(&new_dir).unwrap();
        let prompt = "fresh session created right after snapshot scan";
        let conv = dsh_convs(&serde_json::json!(prompt))[0].clone();
        let lines = [
            serde_json::json!({"type":"session","version":4,"id":"session-fresh-after-scan","cwd":"/proj","createdAt":1000,"delegationDepth":0,"isSeeded":false}).to_string(),
            serde_json::json!({"type":"turn/start","seq":1,"time":1001,"data":{"turn":1}}).to_string(),
            serde_json::json!({"type":"user/message","seq":2,"time":1002,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":prompt}]}}).to_string(),
        ]
        .join("\n")
            + "\n";
        fs::write(new_dir.join("session.v4.jsonl"), lines).unwrap();

        let (cached, hit) = scan_correlation_index_with(false);
        assert!(hit);
        assert!(cached.is_empty());

        assert_eq!(
            resolve_session_for_audit(&conv, std::slice::from_ref(&conv)).as_deref(),
            Some("session-fresh-after-scan")
        );
    }
}
