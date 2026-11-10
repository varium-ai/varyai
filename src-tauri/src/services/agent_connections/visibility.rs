use super::*;

/// IDs identify hidden models, or explicitly visible models when `only` is set.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelVisibility {
    pub only: bool,
    pub ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
}
impl ModelVisibility {
    pub fn visible(&self, id: &str) -> bool {
        self.ids.iter().any(|known| known == id) == self.only
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.ids.len() > 256
            || self.order.len() > 256
            || self
                .ids
                .iter()
                .chain(&self.order)
                .any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
        {
            return Err(FAILED.into());
        }
        let unique = self.order.iter().collect::<std::collections::HashSet<_>>();
        if unique.len() != self.order.len() {
            return Err(FAILED.into());
        }
        Ok(())
    }
    pub(super) fn apply_order(&self, models: &mut [Model]) {
        models.sort_by_key(|model| {
            self.order
                .iter()
                .position(|id| id == &model.id)
                .unwrap_or(usize::MAX)
        });
    }
    pub(super) fn desktop_models(&self, catalog: &[Model], selected: &str) -> Vec<Model> {
        let mut models = catalog
            .iter()
            .filter(|m| self.visible(&m.id))
            .cloned()
            .collect::<Vec<_>>();
        self.apply_order(&mut models);
        // Desktop uses the first entry as the default. Keep the remaining menu order stable.
        if let Some(index) = models.iter().position(|m| m.id == selected) {
            let model = models.remove(index);
            models.insert(0, model);
        }
        models
    }
}
impl Manager {
    pub(crate) fn profile_paths(&self, app: &str) -> Result<Vec<PathBuf>, String> {
        let mut paths = vec![self.path(app)?];
        if app == "claude-desktop" {
            paths.push(self.visibility_path(app)?);
        }
        Ok(paths)
    }
    fn visibility_path(&self, app: &str) -> Result<PathBuf, String> {
        if app != "claude-desktop" {
            return Err(FAILED.into());
        }
        Ok(self.root.join("claude-desktop-model-visibility.json"))
    }
    pub fn model_visibility(&self, app: &str) -> Result<ModelVisibility, String> {
        let policy: ModelVisibility = read(&self.visibility_path(app)?)?
            .map(|text| serde_json::from_str(&text).map_err(|_| FAILED.to_owned()))
            .transpose()?
            .unwrap_or_default();
        policy.validate()?;
        Ok(policy)
    }
    pub(super) fn visibility_for(&self, app: &str) -> Result<ModelVisibility, String> {
        if app == "claude-desktop" {
            self.model_visibility(app)
        } else {
            Ok(ModelVisibility::default())
        }
    }
    pub fn write_model_visibility(
        &self,
        app: &str,
        policy: &ModelVisibility,
    ) -> Result<(), String> {
        policy.validate()?;
        atomic_write_private(
            &self.visibility_path(app)?,
            &serde_json::to_vec(policy).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }

    pub fn set_model_visibility(
        &self,
        app: &str,
        policy: &ModelVisibility,
        menu: &Menu,
        origin: &str,
    ) -> Result<Option<Receipt>, String> {
        policy.validate()?;
        let previous = self.model_visibility(app)?;
        let old = self.load(app)?;
        if old
            .as_ref()
            .is_some_and(|r| !r.suspended && !r.files.iter().all(|f| f.intact().unwrap_or(false)))
        {
            return Err(CONFLICT.into());
        }
        self.write_model_visibility(app, policy)?;
        if old.is_none_or(|r| r.suspended) {
            return Ok(None);
        }
        match self.refresh(app, menu, origin) {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                // Failed writes retain their recovery journal; never report a saved selection.
                self.write_model_visibility(app, &previous)?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_default_precedes_the_visible_order_and_hidden_defaults_fall_back() {
        let policy = ModelVisibility {
            only: false,
            ids: vec!["hidden".into()],
            order: vec!["second".into(), "first".into()],
        };
        let catalog = ["first", "hidden", "second"].map(|id| Model {
            id: id.into(),
            name: id.into(),
            group: String::new(),
            context_window: None,
            output_tokens: None,
            cost: None,
            compact_at: None,
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: false,
            icon: None,
        });
        let ids = |selected| {
            policy
                .desktop_models(&catalog, selected)
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("first"), ["first", "second"]);
        assert_eq!(ids("hidden"), ["second", "first"]);
        assert_eq!(ids("removed"), ["second", "first"]);
    }

    #[test]
    fn desktop_model_order_is_persistent_and_rejects_duplicates() -> Result<(), String> {
        let policy: ModelVisibility = serde_json::from_value(json!({
            "only": false, "ids": [], "order": ["second", "first"]
        }))
        .map_err(|_| FAILED)?;
        policy.validate()?;
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        manager.write_model_visibility("claude-desktop", &policy)?;
        assert_eq!(
            serde_json::to_value(manager.model_visibility("claude-desktop")?)
                .map_err(|_| FAILED)?["order"],
            json!(["second", "first"])
        );
        let duplicate: ModelVisibility = serde_json::from_value(json!({
            "only":false,"ids":[],"order":["first","first"]
        }))
        .map_err(|_| FAILED)?;
        assert!(duplicate.validate().is_err());
        Ok(())
    }

    #[test]
    fn desktop_visibility_defaults_to_all_and_handles_new_models() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().join("receipts"),
        };
        assert!(manager.model_visibility("claude-desktop")?.visible("new"));
        let hidden = ModelVisibility {
            only: false,
            ids: vec!["hidden".into()],
            order: vec![],
        };
        assert!(!hidden.visible("hidden"));
        assert!(hidden.visible("new"));
        let picked = ModelVisibility {
            only: true,
            ids: vec!["picked".into()],
            order: vec![],
        };
        assert!(picked.visible("picked"));
        assert!(!picked.visible("new"));
        manager.write_model_visibility("claude-desktop", &picked)?;
        assert_eq!(manager.model_visibility("claude-desktop")?, picked);
        assert!(manager.model_visibility("claude").is_err());
        Ok(())
    }

    #[test]
    fn visibility_is_bounded_and_malformed_settings_fail_closed() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        let oversized = ModelVisibility {
            only: true,
            ids: vec!["id".into(); 257],
            order: vec![],
        };
        assert!(manager
            .write_model_visibility("claude-desktop", &oversized)
            .is_err());
        fs::write(manager.visibility_path("claude-desktop")?, "malformed").map_err(|_| FAILED)?;
        assert_eq!(
            manager.model_visibility("claude-desktop"),
            Err(FAILED.into())
        );
        Ok(())
    }
}
