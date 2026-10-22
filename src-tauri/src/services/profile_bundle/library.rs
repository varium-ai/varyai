use super::{error, AppError, AppState, AppType};
use crate::services::{McpService, SkillService};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn mcp(app: &AppType) -> bool {
    matches!(
        app,
        AppType::Claude
            | AppType::Codex
            | AppType::Gemini
            | AppType::GrokBuild
            | AppType::OpenCode
            | AppType::Hermes
    )
}
pub fn skills(app: &AppType) -> bool {
    matches!(
        app,
        AppType::Claude
            | AppType::Codex
            | AppType::Gemini
            | AppType::GrokBuild
            | AppType::OpenCode
            | AppType::Hermes
            | AppType::Pi
    )
}
pub fn instructions(app: &AppType) -> bool {
    crate::prompt_files::prompt_file_path(app).is_ok()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySelection {
    pub mcp: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub instructions: Option<String>,
    #[serde(default)]
    pub templates: Option<Vec<crate::services::pi_prompt_files::PiPromptTemplate>>,
}
impl LibrarySelection {
    pub fn capture(state: &AppState, app: &AppType) -> Result<Self, AppError> {
        let prompt_content = if instructions(app) {
            let path = crate::prompt_files::prompt_file_path(app)?;
            match std::fs::read_to_string(path) {
                Ok(s) => Some(s),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(error("readFailed")),
            }
        } else {
            None
        };
        Ok(Self {
            mcp: if mcp(app) {
                Some(
                    state
                        .db
                        .get_all_mcp_servers()?
                        .values()
                        .filter(|s| s.apps.is_enabled_for(app))
                        .map(|s| s.id.clone())
                        .collect(),
                )
            } else {
                None
            },
            skills: if skills(app) {
                Some(
                    SkillService::get_all_installed(&state.db)
                        .map_err(|_| error("readFailed"))?
                        .iter()
                        .filter(|s| s.apps.is_enabled_for(app))
                        .map(|s| s.id.clone())
                        .collect(),
                )
            } else {
                None
            },
            prompts: if instructions(app) {
                Some(
                    state
                        .db
                        .get_prompts(app.as_str())?
                        .values()
                        .filter(|p| p.enabled)
                        .map(|p| p.id.clone())
                        .collect(),
                )
            } else {
                None
            },
            instructions: prompt_content,
            templates: if *app == AppType::Pi {
                Some(crate::services::pi_prompt_files::PiPromptTemplateService::list()?)
            } else {
                None
            },
        })
    }
    pub fn validate(&self, state: &AppState, app: &AppType) -> Result<(), AppError> {
        if self.mcp.is_some() && !mcp(app)
            || self.skills.is_some() && !skills(app)
            || self.prompts.is_some() && !instructions(app)
        {
            return Err(error("invalidConfig"));
        }
        let servers = state.db.get_all_mcp_servers()?;
        let installed = state.db.get_all_installed_skills()?;
        let prompts = state.db.get_prompts(app.as_str())?;
        if self
            .mcp
            .iter()
            .flatten()
            .any(|id| !servers.contains_key(id))
            || self
                .skills
                .iter()
                .flatten()
                .any(|id| !installed.values().any(|s| &s.id == id))
            || self
                .prompts
                .iter()
                .flatten()
                .any(|id| !prompts.contains_key(id))
        {
            return Err(error("missingResource"));
        }
        Ok(())
    }
    pub fn apply(&self, state: &AppState, app: &AppType) -> Result<(), AppError> {
        self.validate(state, app)?;
        if let Some(ids) = &self.mcp {
            for server in state.db.get_all_mcp_servers()?.values() {
                let enabled = ids.contains(&server.id);
                McpService::toggle_app(state, &server.id, app.clone(), enabled)?;
            }
            sync_mcp_restore_backup(state, app)?;
        }
        if let Some(ids) = &self.skills {
            for skill in
                SkillService::get_all_installed(&state.db).map_err(|_| error("readFailed"))?
            {
                let enabled = ids.contains(&skill.id);
                if enabled || skill.apps.is_enabled_for(app) {
                    SkillService::toggle_app(&state.db, &skill.id, app, enabled)
                        .map_err(|_| error("applyFailed"))?;
                }
            }
        }
        if let Some(ids) = &self.prompts {
            for mut prompt in state.db.get_prompts(app.as_str())?.into_values() {
                prompt.enabled = ids.contains(&prompt.id);
                state.db.save_prompt(app.as_str(), &prompt)?;
            }
            if let Some(templates) = &self.templates {
                use crate::services::pi_prompt_files::PiPromptTemplateService as Service;
                let existing = Service::list()?;
                for old in &existing {
                    if !templates.iter().any(|t| t.slug == old.slug) {
                        Service::delete(&old.slug, &old.revision)?;
                    }
                }
                for template in templates {
                    let revision = existing
                        .iter()
                        .find(|t| t.slug == template.slug)
                        .map(|t| t.revision.as_str())
                        .unwrap_or("missing");
                    Service::upsert(&template.slug, None, revision, &template.content)?;
                }
            }
            let path = crate::prompt_files::prompt_file_path(app)?;
            match &self.instructions {
                Some(text) => crate::config::atomic_write_private(&path, text.as_bytes())?,
                None => {
                    if path.exists() {
                        std::fs::remove_file(path).map_err(|_| error("applyFailed"))?;
                    }
                }
            }
        }
        Ok(())
    }
}

// Codex/Grok restore the entire TOML file when takeover stops. Keep managed
// MCP assignments in that backup without copying proxy endpoints or credentials.
fn sync_mcp_restore_backup(state: &AppState, app: &AppType) -> Result<(), AppError> {
    let path = match app {
        AppType::Codex => crate::codex_config::get_codex_config_path(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_path(),
        _ => return Ok(()),
    };
    let Some(backup) = futures::executor::block_on(state.db.get_live_backup(app.as_str()))? else {
        return Ok(());
    };
    let mut saved: serde_json::Value =
        serde_json::from_str(&backup.original_config).map_err(|_| error("invalidConfig"))?;
    let mut doc = saved["config"]
        .as_str()
        .ok_or_else(|| error("invalidConfig"))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| error("invalidConfig"))?;
    let live = std::fs::read_to_string(path)
        .map_err(|_| error("readFailed"))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| error("invalidConfig"))?;
    for id in state.db.get_all_mcp_servers()?.keys() {
        if let Some(item) = live.get("mcp_servers").and_then(|t| t.get(id)) {
            if doc.get("mcp_servers").is_none() {
                doc["mcp_servers"] = toml_edit::table();
            }
            let table = doc
                .get_mut("mcp_servers")
                .and_then(|t| t.as_table_like_mut())
                .ok_or_else(|| error("invalidConfig"))?;
            table.insert(id, item.clone());
        } else if let Some(table) = doc
            .get_mut("mcp_servers")
            .and_then(|t| t.as_table_like_mut())
        {
            table.remove(id);
        }
    }
    saved["config"] = serde_json::json!(doc.to_string());
    futures::executor::block_on(state.db.save_live_backup(app.as_str(), &saved.to_string()))?;
    Ok(())
}

/// The fingerprint includes definitions as well as assignments, without exposing them to UI.
pub fn fingerprint(state: &AppState, app: &AppType) -> Result<serde_json::Value, AppError> {
    let m = if mcp(app) {
        serde_json::to_value(state.db.get_all_mcp_servers()?).map_err(|_| error("invalidConfig"))?
    } else {
        serde_json::Value::Null
    };
    let s = if skills(app) {
        serde_json::to_value(state.db.get_all_installed_skills()?)
            .map_err(|_| error("invalidConfig"))?
    } else {
        serde_json::Value::Null
    };
    Ok(serde_json::json!([
        m,
        s,
        state.db.get_prompts(app.as_str())?
    ]))
}

pub struct LibraryBefore {
    mcp: BTreeMap<String, bool>,
    skills: BTreeMap<String, bool>,
    prompts: Vec<crate::prompt::Prompt>,
}
impl LibraryBefore {
    pub fn capture(state: &AppState, app: &AppType) -> Result<Self, AppError> {
        Ok(Self {
            mcp: if mcp(app) {
                state
                    .db
                    .get_all_mcp_servers()?
                    .values()
                    .map(|s| (s.id.clone(), s.apps.is_enabled_for(app)))
                    .collect()
            } else {
                BTreeMap::new()
            },
            skills: if skills(app) {
                state
                    .db
                    .get_all_installed_skills()?
                    .values()
                    .map(|s| (s.id.clone(), s.apps.is_enabled_for(app)))
                    .collect()
            } else {
                BTreeMap::new()
            },
            prompts: state.db.get_prompts(app.as_str())?.into_values().collect(),
        })
    }
    pub fn restore(&self, state: &AppState, app: &AppType) -> Result<(), AppError> {
        for (id, enabled) in &self.mcp {
            state.db.update_mcp_server_app_enabled(id, app, *enabled)?;
        }
        for mut skill in state.db.get_all_installed_skills()?.into_values() {
            if let Some(enabled) = self.skills.get(&skill.id) {
                skill.apps.set_enabled_for(app, *enabled);
                state.db.update_skill_apps(&skill.id, &skill.apps)?;
            }
        }
        for prompt in &self.prompts {
            state.db.save_prompt(app.as_str(), prompt)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPreview {
    pub mcp: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub instructions: bool,
    pub templates: Vec<String>,
}
impl LibrarySelection {
    pub fn preview(&self, state: &AppState, app: &AppType) -> Result<LibraryPreview, AppError> {
        let servers = state.db.get_all_mcp_servers()?;
        let skills = state.db.get_all_installed_skills()?;
        let prompts = state.db.get_prompts(app.as_str())?;
        Ok(LibraryPreview {
            mcp: self.mcp.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| {
                        servers
                            .get(id)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| id.clone())
                    })
                    .collect()
            }),
            skills: self.skills.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| {
                        skills
                            .get(id)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| id.clone())
                    })
                    .collect()
            }),
            prompts: self.prompts.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| {
                        prompts
                            .get(id)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| id.clone())
                    })
                    .collect()
            }),
            instructions: self.instructions.is_some(),
            templates: self
                .templates
                .as_ref()
                .map(|ts| ts.iter().map(|t| t.slug.clone()).collect())
                .unwrap_or_default(),
        })
    }
}
