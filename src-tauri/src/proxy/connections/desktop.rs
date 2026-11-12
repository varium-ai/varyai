//! Desktop's automatic model choices follow the user's lead model, as in Magpie.
use super::Connection;
use crate::proxy::intent::Menu;
use http::HeaderMap;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, RwLock},
};

const TITLE: &str = "You write short session titles.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Title,
    Probe,
}
impl Purpose {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Probe => "provider_test",
        }
    }
}

pub(super) struct Decision {
    pub alias: String,
    pub purpose: Option<Purpose>,
}

#[derive(Default)]
struct Memory {
    last: Option<String>,
    sessions: VecDeque<([u8; 32], String)>,
}

enum Save {
    Model(String),
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

pub(crate) struct Desktop {
    memory: RwLock<Memory>,
    salt: String,
    writer: Option<mpsc::SyncSender<Save>>,
    selectors: Vec<PathBuf>,
}
impl Default for Desktop {
    fn default() -> Self {
        Self {
            memory: RwLock::default(),
            salt: uuid::Uuid::new_v4().to_string(),
            writer: None,
            selectors: vec![],
        }
    }
}

fn bounded_text(path: &Path, limit: u64) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    (text.len() as u64 <= limit).then_some(text)
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

impl Desktop {
    // Called during service setup, never while forwarding.
    pub(crate) fn load(path: PathBuf, selectors: Vec<PathBuf>) -> std::io::Result<Self> {
        let last = bounded_text(&path, 1024).filter(|value| valid_alias(value));
        let (sender, receiver) = mpsc::sync_channel(32);
        std::thread::Builder::new()
            .name("desktop-model-preference".into())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    match message {
                        Save::Model(alias) => {
                            // Persistence is auxiliary; forwarding does not wait for it.
                            let _ = crate::config::atomic_write_private(&path, alias.as_bytes());
                        }
                        #[cfg(test)]
                        Save::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })?;
        Ok(Self {
            memory: RwLock::new(Memory {
                last,
                ..Default::default()
            }),
            writer: Some(sender),
            selectors,
            ..Default::default()
        })
    }

    fn session(&self, headers: &HeaderMap) -> Option<[u8; 32]> {
        let value = [
            "session_id",
            "session-id",
            "thread-id",
            "thread_id",
            "x-conversation-id",
            "x-session-id",
            "x-claude-code-session-id",
        ]
        .into_iter()
        .find_map(|key| {
            headers
                .get(key)?
                .to_str()
                .ok()
                .filter(|value| !value.is_empty() && value.len() <= 256)
        })?;
        let mut hash = Sha256::new();
        hash.update(self.salt.as_bytes());
        hash.update(value.as_bytes());
        Some(hash.finalize().into())
    }

    fn picked(&self, headers: &HeaderMap) -> Option<String> {
        let session = self.session(headers);
        let memory = self.memory.read().ok()?;
        match session {
            Some(key) => memory
                .sessions
                .iter()
                .find(|(id, _)| *id == key)
                .map(|(_, id)| id.clone()),
            None => memory.last.clone(),
        }
    }

    pub(super) fn remember(&self, headers: &HeaderMap, alias: &str) {
        if !valid_alias(alias) {
            return;
        }
        let session = self.session(headers);
        let Ok(mut memory) = self.memory.write() else {
            return;
        };
        if let Some(key) = session {
            memory.sessions.retain(|(id, _)| *id != key);
            memory.sessions.push_back((key, alias.into()));
            while memory.sessions.len() > 128 {
                memory.sessions.pop_front();
            }
        }
        if memory.last.as_deref() != Some(alias) {
            memory.last = Some(alias.into());
            if let Some(writer) = &self.writer {
                let _ = writer.try_send(Save::Model(alias.into()));
            }
        }
    }

    // Run on a blocking worker only for connection probes. Read just model choices.
    pub(super) fn chosen(&self) -> Vec<String> {
        let mut newest = None;
        let mut visited = 0usize;
        for root in &self.selectors {
            let Ok(accounts) = fs::read_dir(root.join("local-agent-mode-sessions")) else {
                continue;
            };
            for account in accounts.flatten().take(128) {
                if !account.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let Ok(orgs) = fs::read_dir(account.path()) else {
                    continue;
                };
                for org in orgs.flatten().take(128) {
                    visited += 1;
                    if visited > 512 {
                        break;
                    }
                    if !org.file_type().is_ok_and(|kind| kind.is_dir()) {
                        continue;
                    }
                    let path = org.path().join("cowork_account_settings.json");
                    let Ok(meta) = fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024
                    {
                        continue;
                    }
                    let Ok(time) = meta.modified() else { continue };
                    if newest.as_ref().is_none_or(|(previous, _)| time > *previous) {
                        newest = Some((time, path));
                    }
                }
                if visited > 512 {
                    break;
                }
            }
        }
        let Some((_, path)) = newest else {
            return vec![];
        };
        let Some(text) = bounded_text(&path, 1024 * 1024) else {
            return vec![];
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return vec![];
        };
        ["code", "cowork", "chat"]
            .into_iter()
            .filter_map(|surface| {
                let model = value["__model_selector_state"][surface]["model"]
                    .as_str()?
                    .trim();
                valid_alias(model).then(|| model.to_owned())
            })
            .collect()
    }

    pub(super) fn resolve(
        &self,
        connection: &Connection,
        menu: &Menu,
        asked: &str,
        body: &Value,
        headers: &HeaderMap,
        chosen: &[String],
    ) -> Decision {
        let alias = connection.resolve(asked);
        let available = |id: &str| {
            menu.groups.contains_key(id) && connection.model_map.values().any(|known| known == id)
        };
        let default = || match &connection.visible_models {
            Some(visible) => visible.iter().find(|id| available(id)).cloned(),
            None => menu.listed_aliases().find(|id| available(id)).cloned(),
        };
        let picked = self.picked(headers).filter(|id| available(id));
        if probe(body) {
            let choices = chosen
                .iter()
                .map(|id| connection.resolve(id))
                .filter(|id| available(id))
                .collect::<Vec<_>>();
            let selected = picked
                .as_ref()
                .filter(|id| choices.contains(id))
                .cloned()
                .or_else(|| choices.first().cloned())
                .or_else(default);
            return Decision {
                alias: selected.unwrap_or(alias),
                purpose: Some(Purpose::Probe),
            };
        }
        let cli = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ua| ua.starts_with("claude-cli/"));
        let tools = body["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty());
        let subagent = headers
            .get("x-claude-code-agent-id")
            .is_some_and(|value| !value.as_bytes().is_empty());
        let small = body["max_tokens"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 4096);
        let title = !tools
            && if cli {
                title(body)
            } else {
                title(body)
                    || (small
                        && (asked.starts_with("claude-")
                            || asked.starts_with("anthropic/")
                            || asked.starts_with("mythos-")))
            };
        let native = !asked.contains('/') && claude_model(asked).is_some() && alias == asked;
        let missing = !menu.groups.contains_key(&alias);
        let target = if title {
            picked.clone().or_else(default)
        } else if native && (missing || !tools || subagent) {
            picked
                .as_deref()
                .and_then(|picked| beside(menu, picked, asked, cli))
                .filter(|id| available(id))
                .or_else(|| picked.clone())
                .or_else(default)
        } else {
            None
        };
        Decision {
            alias: target.unwrap_or(alias),
            purpose: title.then_some(Purpose::Title),
        }
    }

    #[cfg(test)]
    pub(super) fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = &self.writer {
            let (tx, rx) = mpsc::channel();
            writer.send(Save::Flush(tx))?;
            rx.recv_timeout(std::time::Duration::from_secs(3))?;
        }
        Ok(())
    }
}

pub(super) fn probe(body: &Value) -> bool {
    body["max_tokens"] == 1
        && body.get("system").is_none()
        && body.get("tools").is_none()
        && body["messages"].as_array().is_some_and(|messages| {
            messages.len() == 1 && messages[0]["role"] == "user" && messages[0]["content"] == "."
        })
}

fn title(body: &Value) -> bool {
    body["system"]
        .as_str()
        .is_some_and(|text| text.starts_with(TITLE))
        || body["system"].as_array().is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block["type"] == "text"
                    && block["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with(TITLE))
            })
        })
}

fn claude_model(model: &str) -> Option<String> {
    let model = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model)
        .rsplit('/')
        .next()?
        .to_ascii_lowercase()
        .replace('.', "-");
    let model = model
        .rsplit_once('-')
        .filter(|(_, date)| date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(model.as_str(), |(plain, _)| plain);
    let rest = model.strip_prefix("claude-")?;
    let (tier, version) = rest.split_once('-')?;
    let mut parts = version.split('-');
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    (["haiku", "sonnet", "opus", "fable", "mythos"].contains(&tier)
        && parts.next().is_some_and(digits)
        && parts.next().is_none_or(digits)
        && parts.next().is_none())
    .then(|| model.to_owned())
}

fn beside(menu: &Menu, picked: &str, asked: &str, cli: bool) -> Option<String> {
    let requested = claude_model(asked)?;
    let session = menu.groups.get(picked)?;
    let [lead] = session.targets.as_slice() else {
        return None;
    };
    if cli && claude_model(&lead.model).is_none() {
        return None;
    }
    let tier = requested.split('-').nth(1)?;
    let directory = lead.model.rsplit_once('/').map(|(dir, _)| dir);
    let mut candidates = menu
        .models
        .values()
        .filter(|entry| entry.description == "VaryAI direct provider model")
        .filter_map(|entry| {
            let target = menu.groups.get(&entry.alias)?.targets.first()?;
            if target.provider_id != lead.provider_id {
                return None;
            }
            let model = claude_model(&target.model)?;
            if model.split('-').nth(1)? != tier {
                return None;
            }
            Some((
                model == requested,
                target.model.rsplit_once('/').map(|(dir, _)| dir) == directory,
                model,
                entry.alias.clone(),
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop().map(|(_, _, _, alias)| alias)
}

pub(super) fn patch(raw: &[u8], alias: &str, probe: bool) -> Result<Vec<u8>, &'static str> {
    use serde_json::value::{to_raw_value, RawValue};
    const INVALID: &str = "routing_invalid_json";
    let mut fields: std::collections::BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(raw).map_err(|_| INVALID)?;
    fields.insert("model".into(), to_raw_value(alias).map_err(|_| INVALID)?);
    if probe {
        fields.insert("max_tokens".into(), to_raw_value(&16).map_err(|_| INVALID)?);
    }
    serde_json::to_vec(&fields).map_err(|_| INVALID)
}
