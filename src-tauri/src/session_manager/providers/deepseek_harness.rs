use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "deepseek-harness";
const SESSION_FILE: &str = "session.v4.jsonl.zstd";
const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;

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
            let session_file = entry_path.join(SESSION_FILE);
            if session_file.is_file() {
                if let Some(meta) = parse_session(&session_file) {
                    sessions.push(meta);
                }
            }
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let lines = read_zstd_lines(path)?;
    let mut messages = Vec::new();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
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
    }
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

fn read_zstd_lines(path: &Path) -> Result<Vec<String>, String> {
    let meta = fs::metadata(path).map_err(|err| format!("Failed to stat session: {err}"))?;
    if meta.len() > MAX_SESSION_BYTES {
        return Err(format!("Session file exceeds max size: {}", path.display()));
    }
    let file = File::open(path).map_err(|err| format!("Failed to open session: {err}"))?;
    let decoder = zstd::stream::read::Decoder::new(BufReader::new(file))
        .map_err(|err| format!("Failed to decode zstd session: {err}"))?;
    let reader = BufReader::new(decoder);
    let mut lines = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    Ok(lines)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let lines = read_zstd_lines(path).ok()?;
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

    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
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
    }

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
        let session_file = session_dir.join(SESSION_FILE);
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
}
