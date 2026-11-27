//! Shared catalog control is isolated by desktop edition; audit consent is not shared.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edition {
    China,
    International,
}
impl Edition {
    pub(crate) const ALL: [Self; 2] = [Self::China, Self::International];

    pub(crate) fn parse(app: &str) -> Option<Self> {
        match app {
            "trae" => Some(Self::China),
            "trae-intl" => Some(Self::International),
            _ => None,
        }
    }
    pub(crate) fn app(self) -> &'static str {
        match self {
            Self::China => "trae",
            Self::International => "trae-intl",
        }
    }
    pub(crate) fn consent_key(self) -> &'static str {
        match self {
            Self::China => CONSENT_KEY,
            Self::International => "trae_intl_local_connection_consent",
        }
    }
    pub(crate) fn session_id(self, id: &str) -> Option<String> {
        use crate::proxy::tool_observation::types::normalized_session_id;
        let id = normalized_session_id(Some(id))?;
        let (prefix, previous) = match self {
            Self::China => ("trae_cn_", "trae_"),
            Self::International => ("trae_", "trae-intl_"),
        };
        if id.starts_with(prefix) {
            return Some(id);
        }
        let raw = id.strip_prefix(previous).unwrap_or(&id);
        if raw.is_empty() {
            return None;
        }
        normalized_session_id(Some(&format!("{prefix}{raw}")))
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Client(pub Edition);
impl Client {
    pub(crate) fn directory(self) -> Result<PathBuf, AppError> {
        Ok(crate::config::get_device_config_dir().join(match self.0 {
            Edition::China => "trae-bridge",
            Edition::International => "trae-intl-bridge",
        }))
    }
    pub(crate) fn argv_path(self) -> Result<PathBuf, AppError> {
        #[cfg(test)]
        let home = crate::config::get_home_dir();
        #[cfg(not(test))]
        let home = dirs::home_dir().ok_or_else(unavailable)?;
        Ok(home.join(match self.0 {
            Edition::China => ".trae-cn/argv.json",
            Edition::International => ".trae/argv.json",
        }))
    }
    #[cfg(target_os = "macos")]
    pub(super) fn port_path(self) -> Result<PathBuf, AppError> {
        Ok(dirs::home_dir()
            .ok_or_else(unavailable)?
            .join("Library/Application Support")
            .join(match self.0 {
                Edition::China => "Trae CN",
                Edition::International => "Trae",
            })
            .join("DevToolsActivePort"))
    }
    #[cfg(target_os = "windows")]
    pub(super) fn port_path(self) -> Result<PathBuf, AppError> {
        Ok(dirs::data_dir()
            .ok_or_else(unavailable)?
            .join(match self.0 {
                Edition::China => "Trae CN",
                Edition::International => "Trae",
            })
            .join("DevToolsActivePort"))
    }
    #[cfg(target_os = "windows")]
    pub(crate) fn installed_executable(self) -> Result<PathBuf, AppError> {
        let (folder, name) = match self.0 {
            Edition::China => ("Trae CN", "Trae CN.exe"),
            Edition::International => ("Trae", "Trae.exe"),
        };
        let mut roots = Vec::new();
        if let Some(programs) = std::env::var_os("ProgramFiles") {
            roots.push(PathBuf::from(programs));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Programs"));
        }
        roots
            .into_iter()
            .map(|root| root.join(folder).join(name))
            .find(|path| {
                path.is_file()
                    && !path.is_symlink()
                    && path.canonicalize().is_ok_and(|canonical| {
                        canonical
                            .to_string_lossy()
                            .trim_start_matches(r"\\?\")
                            .eq_ignore_ascii_case(&path.to_string_lossy())
                    })
            })
            .ok_or_else(unavailable)
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn installed_executable(self) -> Result<PathBuf, AppError> {
        // Reuse the presence adapter's on-disk bundle identity checks.
        let bundle =
            crate::commands::find_agent_desktop_bundle(self.0.app()).ok_or_else(unavailable)?;
        let executable = bundle.join("Contents/MacOS/Electron");
        if !executable.is_file() || executable.is_symlink() {
            return Err(unavailable());
        }
        Ok(executable)
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn connection_executable(self) -> Result<PathBuf, AppError> {
        match super::super::agent_connections::desktop::running_executable(self.0.app())
            .map_err(|_| unavailable())?
        {
            Some(executable) => Ok(executable),
            None => self.installed_executable(),
        }
    }
    pub(super) fn request(self, command: &Value) -> Result<Value, AppError> {
        request_in(&self.directory()?, command, false)
    }
    pub(super) fn request_all(self, command: &Value) -> Result<Value, AppError> {
        request_in(&self.directory()?, command, true)
    }
    pub(crate) fn applied_at_ms(self) -> Option<i64> {
        super::freshness::applied_at_ms(&self.directory().ok()?, &self.argv_path().ok()?)
    }
    pub(crate) fn prepare(self) -> Result<(), AppError> {
        if self.0 == Edition::China {
            return prepare_shared();
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.installed_executable()?;
        prepare_connection(&self.directory()?, &self.argv_path()?)?;
        let _ = ensure_ready_for(self.0);
        Ok(())
    }
    pub(crate) fn reconnect(self) -> Result<(), AppError> {
        ensure_ready_for(self.0)
    }
    pub(crate) fn restore(self) -> Result<(), AppError> {
        restore_connection(&self.directory()?, &self.argv_path()?)
    }
    pub(crate) fn apply(self, provider: &Provider) -> Result<(), AppError> {
        super::super::agent_clients::validate(&provider.settings_config)?;
        self.request(&json!({"op":"apply","id":provider.id,"name":provider.name,"config":provider.settings_config}))?;
        Ok(())
    }
    pub(crate) fn reapply(self, provider: &Provider) -> Result<(), AppError> {
        super::super::agent_clients::validate(&provider.settings_config)?;
        self.request(&json!({"op":"replace","id":provider.id,"name":provider.name,"config":provider.settings_config}))?;
        Ok(())
    }
    pub(crate) fn remove(self, id: &str) -> Result<(), AppError> {
        // A stopped client keeps the durable outbox until cleanup is acknowledged.
        self.request(&json!({"op":"remove","id":id}))?;
        Ok(())
    }
    pub(crate) fn selection_pending(self, id: &str, model: &str) -> Option<bool> {
        self.request(&json!({"op":"selection_pending","id":id,"model":model}))
            .ok()?
            .as_bool()
    }
    pub(crate) fn select(self, id: &str, model: &str) -> Result<(), AppError> {
        if self.request(&json!({"op":"select","id":id,"model":model}))? != true {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(crate) fn switch(self, target: &Provider, previous: &[Provider]) -> Result<bool, AppError> {
        if target.category.as_deref() != Some("official") {
            super::super::agent_clients::validate(&target.settings_config)?;
        }
        for provider in previous {
            super::super::agent_clients::validate(&provider.settings_config)?;
        }
        let Ok(health) = self.request(&json!({"op":"health"})) else {
            return Ok(false);
        };
        if health["ready"] != true {
            return Ok(false);
        }
        require_model_replacement(&health)?;
        replace_with(
            target,
            previous,
            |provider| {
                if provider.id != target.id {
                    return self.apply(provider);
                }
                let config = if provider.category.as_deref() == Some("official") {
                    Value::Null
                } else {
                    provider.settings_config.clone()
                };
                self.request(
                    &json!({"op":"replace","id":provider.id,"name":provider.name,"config":config}),
                )?;
                Ok(())
            },
            |id| self.remove(id),
        )?;
        Ok(true)
    }
    pub(crate) fn matches(self, provider: &Provider) -> Option<bool> {
        self.request(&json!({"op":"matches","id":provider.id,"config":provider.settings_config}))
            .ok()?
            .as_bool()
    }
}
