//! Device-local consent and plans contain model IDs only, never client credentials.
use super::*;

pub(crate) const CONSENT: &str = "agentConnections.modelControlConsent";
const UNAVAILABLE: &str = "agentConnections.modelControlUnavailable";
const ADAPTER_REVISION: u32 = 1;

pub(crate) fn supports(app: &str) -> bool {
    matches!(app, "workbuddy" | "workbuddy-ai" | "qoder")
}

#[cfg(any(target_os = "macos", windows))]
pub(super) fn launch(app: &str, executable: &Path) -> Result<bool, String> {
    Manager {
        root: crate::config::get_app_config_dir().join("agent-connections"),
    }
    .launch_model_control(app, executable)
}

#[cfg(any(target_os = "macos", windows))]
pub(super) fn authorized(app: &str) -> bool {
    supports(app)
        && Manager {
            root: crate::config::get_app_config_dir().join("agent-connections"),
        }
        .model_control_status(app)
        .is_ok_and(|s| s.authorized)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub authorized: bool,
    pub ready: bool,
    pub pending: bool,
}

impl Manager {
    pub(super) fn control_dir(&self, app: &str) -> Result<PathBuf, String> {
        if !supports(app) {
            return Err(FAILED.into());
        }
        Ok(self.root.join("model-control").join(app))
    }

    pub(crate) fn model_control_status(&self, app: &str) -> Result<Status, String> {
        let dir = self.control_dir(app)?;
        let authorized = read(&dir.join("consent.json"))?.is_some_and(|text| {
            serde_json::from_str::<Value>(&text).is_ok_and(|v| v["version"] == 1)
        });
        let plan = read(&dir.join("plan.json"))?
            .map(|s| serde_json::from_str::<Value>(&s).map_err(|_| FAILED))
            .transpose()?;
        let health =
            read(&dir.join("status.json"))?.and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let ready = plan
            .as_ref()
            .zip(health.as_ref())
            .is_some_and(|(plan, health)| {
                // This is proof of a persisted default, not a live-bridge heartbeat.
                // A normal client reopen must not make an already saved choice pending again.
                health["revision"] == plan["revision"] && health["ready"] == true
            });
        Ok(Status {
            authorized,
            ready,
            pending: authorized && plan.is_some() && !ready,
        })
    }

    pub(crate) fn authorize_model_control(&self, app: &str, confirmed: bool) -> Result<(), String> {
        let dir = self.control_dir(app)?;
        if !confirmed {
            return Err(CONSENT.into());
        }
        if !cfg!(any(target_os = "macos", target_os = "windows"))
            || crate::commands::reward_node_runtime().is_none()
        {
            return Err(UNAVAILABLE.into());
        }
        atomic_write_private(&dir.join("consent.json"), br#"{"version":1}"#)
            .map_err(|_| FAILED.into())
    }

    pub(super) fn sync_model_control(
        &self,
        receipt: &Receipt,
        restore: bool,
    ) -> Result<(), String> {
        if !supports(&receipt.app) || !self.model_control_status(&receipt.app)?.authorized {
            return Ok(());
        }
        use sha2::{Digest, Sha256};
        let model = super::model_wire(
            &receipt.model_map,
            receipt.visible_models.as_deref(),
            &receipt.model,
        );
        let revision = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!([
                    ADAPTER_REVISION,
                    receipt.model,
                    model,
                    receipt.applied_at_ms,
                    restore
                ]))
                .map_err(|_| FAILED)?
            )
        );
        let plan = json!({"app":receipt.app,"revision":revision,"model":model,"restore":restore,"adapterRevision":ADAPTER_REVISION});
        atomic_write_private(
            &self.control_dir(&receipt.app)?.join("plan.json"),
            &serde_json::to_vec(&plan).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED.into())
    }

    /// Called only by an explicitly requested, identity-checked native launch.
    #[cfg(any(target_os = "macos", windows))]
    pub(super) fn launch_model_control(
        &self,
        app: &str,
        executable: &Path,
    ) -> Result<bool, String> {
        if !supports(app) || !self.model_control_status(app)?.authorized {
            return Ok(false);
        }
        let dir = self.control_dir(app)?;
        if read(&dir.join("plan.json"))?.is_none() {
            return Ok(false);
        }
        let node = crate::commands::reward_node_runtime().ok_or(UNAVAILABLE)?;
        for (name, source) in [
            (
                "agent.cjs",
                include_str!("../../../resources/model-default/agent.cjs"),
            ),
            (
                "renderer.cjs",
                include_str!("../../../resources/model-default/renderer.cjs"),
            ),
        ] {
            atomic_write_private(&dir.join(name), source.as_bytes()).map_err(|_| FAILED)?;
        }
        let _ = fs::remove_file(dir.join("status.json"));
        let mut command = std::process::Command::new(node);
        command
            .arg(dir.join("agent.cjs"))
            .arg(executable)
            .arg(&dir)
            .env_remove("NODE_OPTIONS")
            .env_remove("ELECTRON_RUN_AS_NODE")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000 | 0x00000200);
        }
        let mut child = command.spawn().map_err(|_| UNAVAILABLE)?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        // Launch acceptance is distinct from model readback; the UI keeps the pending state.
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_requires_explicit_device_local_consent_and_exact_persisted_readback(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        assert!(!manager.model_control_status("qoder")?.authorized);
        #[cfg(any(target_os = "macos", windows))]
        assert!(!manager.launch_model_control("qoder", Path::new("/synthetic/missing-client"))?);
        assert_eq!(
            manager.authorize_model_control("qoder", false),
            Err(CONSENT.into())
        );
        assert!(manager.control_dir("browser").is_err());
        let control = manager.control_dir("qoder")?;
        atomic_write_private(&control.join("consent.json"), br#"{"version":1}"#)
            .map_err(|_| FAILED)?;
        atomic_write_private(&control.join("plan.json"), br#"{"revision":"new"}"#)
            .map_err(|_| FAILED)?;
        for (revision, time, expected) in [
            ("old", chrono::Utc::now().timestamp_millis(), false),
            ("new", 1, true),
            ("new", chrono::Utc::now().timestamp_millis(), true),
        ] {
            atomic_write_private(
                &control.join("status.json"),
                &serde_json::to_vec(&json!({"revision":revision,"ready":true,"updatedAt":time}))
                    .map_err(|_| FAILED)?,
            )
            .map_err(|_| FAILED)?;
            assert_eq!(manager.model_control_status("qoder")?.ready, expected);
        }
        Ok(())
    }

    #[test]
    fn model_control_plans_exclude_connection_credentials_and_profile_exports() -> Result<(), String>
    {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: dir.path().into(),
        };
        let control = manager.control_dir("qoder")?;
        atomic_write_private(&control.join("consent.json"), br#"{"version":1}"#)
            .map_err(|_| FAILED)?;
        let receipt = Receipt {
            app: "qoder".into(),
            token: "synthetic-connection-secret".into(),
            model: "synthetic-model".into(),
            model_map: BTreeMap::from([
                ("a-previous-name".into(), "synthetic-model".into()),
                ("Zeta Model".into(), "synthetic-model".into()),
            ]),
            visible_models: Some(vec!["Zeta Model".into()]),
            files: vec![],
            suspended: false,
            applied_at_ms: Some(123),
        };
        manager.sync_model_control(&receipt, false)?;
        let plan = read(&control.join("plan.json"))?.ok_or(FAILED)?;
        assert!(!plan.contains(&receipt.token));
        assert_eq!(
            serde_json::from_str::<Value>(&plan).map_err(|_| FAILED)?["model"],
            "Zeta Model"
        );
        assert_eq!(
            manager.profile_paths("qoder")?,
            vec![manager.path("qoder")?]
        );
        Ok(())
    }
}
