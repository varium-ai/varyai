//! Stream complete log snapshots, retaining ordinary diagnostic text and order.
use crate::proxy::security::SecurityRuntimeConfig;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::{Arc, LazyLock};

const REDACTED: &str = "[REDACTED]";
static PAYLOAD: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?ix)\b(?:request[_ -]?body|response[_ -]?body|requests?|responses?|body|payload|messages?|prompts?|tool[_ -]?(?:inputs?|calls?|results?|invocations?)|function[_ -]?calls?|arguments?|args?|parameters?|input|content|config(?:uration)?|settings)(?:\\?["'])?\s*[:=]\s*"#).ok()
});
static SECRET: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?ix)\b(?:api[_-]?keys?|access[_-]?keys?|secret[_-]?keys?|private[_-]?keys?|client[_-]?secrets?|auth[_-]?tokens?|access[_-]?tokens?|refresh[_-]?tokens?|id[_-]?tokens?|session[_-]?(?:tokens?|ids?)|authorization|credentials?|passwords?|passwd|bearer|cookies?|secrets?|tokens?|auth|pwd|keys?)(?:\\?["'])?\s*[:=]\s*"#).ok()
});
static URL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://(?:<redacted>|\[REDACTED\]|[^\s<>"'])+"#).ok()
});
static PATH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?:[A-Za-z]:[\\/]|\\\\|(?:^|[\s=("'])/)(?:<redacted>|\[REDACTED\]|[^\s<>"'])*"#)
        .ok()
});

pub(super) struct LogPrivacy {
    policy: Option<Arc<SecurityRuntimeConfig>>,
    pub ready: bool,
}

impl LogPrivacy {
    pub fn capture(policy: Option<Arc<SecurityRuntimeConfig>>) -> Self {
        let ready = policy.is_some()
            && PAYLOAD.is_some()
            && SECRET.is_some()
            && URL.is_some()
            && PATH.is_some();
        Self { policy, ready }
    }

    pub fn file(&self) -> LogFile<'_> {
        LogFile {
            privacy: self,
            private_key: false,
            payload: None,
        }
    }

    fn plain(&self, text: &str) -> Option<String> {
        let policy = self.policy.as_ref()?;
        let text = crate::services::tool_risk::sanitize::sanitize_diagnostic_text(text, policy)?;
        let text = URL.as_ref()?.replace_all(&text, "[URL]");
        Some(PATH.as_ref()?.replace_all(&text, "[PATH]").into_owned())
    }

    fn structured(&self, value: &Value, depth: usize) -> Option<Value> {
        if depth > 64 {
            return Some(json!(REDACTED));
        }
        match value {
            Value::String(text) => {
                let marker = PAYLOAD
                    .as_ref()
                    .and_then(|regex| regex.find(text))
                    .or_else(|| SECRET.as_ref().and_then(|regex| regex.find(text)));
                if let Some(marker) = marker {
                    self.plain(&text[..marker.end()])
                        .map(|prefix| json!(format!("{prefix}{REDACTED}")))
                } else {
                    self.plain(text).map(Value::String)
                }
            }
            Value::Array(_) => Some(json!(REDACTED)),
            Value::Object(values) => {
                let mut result = serde_json::Map::new();
                for (key, value) in values {
                    let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
                    if matches!(
                        normalized.as_str(),
                        "timestamp"
                            | "time"
                            | "level"
                            | "module"
                            | "target"
                            | "logger"
                            | "file"
                            | "line"
                            | "column"
                            | "message"
                            | "msg"
                            | "error"
                            | "code"
                            | "status"
                            | "statuscode"
                            | "attempt"
                            | "durationms"
                            | "method"
                    ) {
                        result.insert(key.clone(), self.structured(value, depth + 1)?);
                    } else {
                        result.insert("private_fields".into(), json!(REDACTED));
                    }
                }
                Some(Value::Object(result))
            }
            _ => Some(value.clone()),
        }
    }
}

#[derive(Default)]
struct PayloadBlock {
    depth: usize,
    quoted: bool,
    escaped: bool,
    structured: bool,
}
impl PayloadBlock {
    fn consume(&mut self, text: &str) -> bool {
        for character in text.chars() {
            if self.escaped {
                self.escaped = false;
                continue;
            }
            if self.quoted && character == '\\' {
                self.escaped = true;
                continue;
            }
            if character == '"' {
                self.quoted = !self.quoted;
                continue;
            }
            if self.quoted {
                continue;
            }
            match character {
                '{' | '[' => {
                    self.depth += 1;
                    self.structured = true;
                }
                '}' | ']' if self.depth > 0 => {
                    self.depth -= 1;
                    if self.depth == 0 {
                        return true;
                    }
                }
                _ => {}
            }
        }
        self.structured && self.depth == 0
    }
}

pub(super) struct LogFile<'a> {
    privacy: &'a LogPrivacy,
    private_key: bool,
    payload: Option<PayloadBlock>,
}

impl LogFile<'_> {
    pub fn redact(&mut self, line: &str) -> (String, bool) {
        if !self.privacy.ready {
            return (REDACTED.into(), true);
        }
        if self.private_key {
            if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                self.private_key = false;
            }
            return (REDACTED.into(), true);
        }
        if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
            self.private_key =
                !(line.contains("-----END ") && line.matches("PRIVATE KEY-----").count() > 1);
            return (REDACTED.into(), true);
        }
        if let Some(block) = self.payload.as_mut() {
            if block.consume(line) {
                self.payload = None;
            }
            return (REDACTED.into(), true);
        }
        if let Ok(value @ (Value::Object(_) | Value::Array(_))) =
            serde_json::from_str::<Value>(line)
        {
            let output = self
                .privacy
                .structured(&value, 0)
                .and_then(|v| serde_json::to_string(&v).ok())
                .unwrap_or_else(|| REDACTED.into());
            let changed = output != line;
            return (output, changed);
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('{')
            || trimmed == "["
            || trimmed.starts_with("[{")
            || trimmed.starts_with("[\"")
        {
            let mut block = PayloadBlock::default();
            if !block.consume(trimmed) {
                self.payload = Some(block);
            }
            return (REDACTED.into(), true);
        }
        if let Some(found) = PAYLOAD.as_ref().and_then(|regex| regex.find(line)) {
            let mut block = PayloadBlock::default();
            let rest = &line[found.end()..];
            if !block.consume(rest) && (rest.trim().is_empty() || block.structured) {
                self.payload = Some(block);
            }
            let prefix = self.privacy.plain(&line[..found.end()]).unwrap_or_default();
            return (format!("{prefix}{REDACTED}"), true);
        }
        if let Some(found) = SECRET.as_ref().and_then(|regex| regex.find(line)) {
            let mut block = PayloadBlock::default();
            let rest = &line[found.end()..];
            if !block.consume(rest) && (rest.trim().is_empty() || block.structured) {
                self.payload = Some(block);
            }
            let prefix = self.privacy.plain(&line[..found.end()]).unwrap_or_default();
            return (format!("{prefix}{REDACTED}"), true);
        }
        let output = self.privacy.plain(line).unwrap_or_else(|| REDACTED.into());
        let changed = output != line;
        (output, changed)
    }
}
