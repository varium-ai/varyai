//! Private restoration policy publication. No persistence or forwarding happens
//! while the snapshot lock is held; writer serialization spans save and publish.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};

use super::{runtime::SecurityRuntimeConfig, tokenization::TokenError, types::AuditAction};
use crate::database::{CustomSecurityRule, SecurityAuditSettings};

// Deliberately no Debug or serialization: expressions can contain private values.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct DetectionRule {
    id: String,
    rule_type: String,
    pattern: String,
    action: String,
    enabled: bool,
}

#[derive(PartialEq, Eq)]
pub(super) struct RestorationPolicy {
    enabled: bool,
    rules: Vec<DetectionRule>,
    builtin_actions: Vec<(String, &'static str)>,
    catalog_revision: u64,
}

impl RestorationPolicy {
    pub(super) fn from_sources(
        settings: &SecurityAuditSettings,
        rules: &[CustomSecurityRule],
        overrides: &HashMap<String, AuditAction>,
        catalog_revision: u64,
    ) -> Self {
        let mut rules: Vec<_> = rules
            .iter()
            .map(|rule| DetectionRule {
                id: rule.id.clone(),
                rule_type: rule.rule_type.clone(),
                pattern: rule.pattern.clone(),
                action: rule.action.clone(),
                enabled: rule.enabled,
            })
            .collect();
        rules.sort();
        let mut builtin_actions: Vec<_> = super::builtin_rules::BUILTIN_RULES
            .iter()
            .map(|rule| {
                let configured = overrides
                    .get(rule.id)
                    .copied()
                    .unwrap_or(rule.default_action);
                let effective = if rule.id == "catalog:generic-sk-key-notice"
                    && matches!(
                        configured,
                        AuditAction::Block | AuditAction::Redact | AuditAction::RedactRestore
                    ) {
                    rule.default_action
                } else {
                    configured
                };
                (rule.id.to_string(), effective.as_str())
            })
            .chain(["chinese_id_card", "chinese_phone_number"].map(|id| {
                let action = overrides.get(id).copied().unwrap_or(AuditAction::Redact);
                (id.to_string(), action.as_str())
            }))
            .collect();
        builtin_actions.sort_unstable();
        Self {
            enabled: settings.enabled,
            rules,
            builtin_actions,
            catalog_revision,
        }
    }
}

pub(super) struct DlpPolicyAdmission {
    pub runtime: Arc<SecurityRuntimeConfig>,
    pub restoration_policy_revision: u64,
}

struct PublishedPolicy {
    runtime: Arc<SecurityRuntimeConfig>,
    policy: RestorationPolicy,
    revision: u64,
}

#[derive(Default)]
struct PublicationState {
    published: Option<PublishedPolicy>,
    available: bool,
}

pub(super) struct DlpPolicyGate {
    writer: Mutex<()>,
    state: RwLock<PublicationState>,
}

/// Dropping an unfinished update intentionally leaves admission closed. A caller
/// cannot prove that a failed or interrupted persistence operation did not commit.
pub(super) struct DlpPolicyUpdate<'a> {
    gate: &'a DlpPolicyGate,
    _writer: MutexGuard<'a, ()>,
}

impl DlpPolicyGate {
    pub(super) fn new() -> Self {
        Self {
            writer: Mutex::new(()),
            state: RwLock::new(PublicationState::default()),
        }
    }

    pub(super) fn admit(&self) -> Result<DlpPolicyAdmission, TokenError> {
        let state = self.state.read().map_err(|_| TokenError::Unavailable)?;
        if !state.available {
            return Err(TokenError::Unavailable);
        }
        let published = state.published.as_ref().ok_or(TokenError::Unavailable)?;
        Ok(DlpPolicyAdmission {
            runtime: published.runtime.clone(),
            restoration_policy_revision: published.revision,
        })
    }

    /// Presentation snapshot only, never authorization to forward or restore.
    pub(super) fn runtime_snapshot(
        &self,
    ) -> Result<Option<Arc<SecurityRuntimeConfig>>, TokenError> {
        let state = self.state.read().map_err(|_| TokenError::Unavailable)?;
        Ok(state
            .published
            .as_ref()
            .map(|published| published.runtime.clone()))
    }

    pub(super) fn begin_update(&self) -> Result<DlpPolicyUpdate<'_>, TokenError> {
        // This mutex owns no authorization state. After a cancelled save it may be
        // recovered for a reload, but the independently guarded admission stays shut.
        let writer = match self.writer.lock() {
            Ok(writer) => writer,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.state
            .write()
            .map_err(|_| TokenError::Unavailable)?
            .available = false;
        Ok(DlpPolicyUpdate {
            gate: self,
            _writer: writer,
        })
    }

    #[cfg(test)]
    pub(super) fn set_revision_for_test(&self, revision: u64) -> Result<(), TokenError> {
        self.state
            .write()
            .map_err(|_| TokenError::Unavailable)?
            .published
            .as_mut()
            .ok_or(TokenError::Unavailable)?
            .revision = revision;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn poison_publication_for_test(&self) {
        let _ = std::panic::catch_unwind(|| {
            if let Ok(_state) = self.state.write() {
                panic!("synthetic policy publication poisoning");
            }
        });
    }
}

impl DlpPolicyUpdate<'_> {
    /// Only for deriving runtime metadata during a serialized publication. This
    /// does not admit requests while the gate is closed.
    pub(super) fn previous_runtime(&self) -> Result<Arc<SecurityRuntimeConfig>, TokenError> {
        let state = self
            .gate
            .state
            .read()
            .map_err(|_| TokenError::Unavailable)?;
        Ok(state
            .published
            .as_ref()
            .map(|published| published.runtime.clone())
            .unwrap_or_else(|| Arc::new(SecurityRuntimeConfig::default())))
    }

    pub(super) fn publish(
        self,
        runtime: Arc<SecurityRuntimeConfig>,
        policy: RestorationPolicy,
    ) -> Result<(), TokenError> {
        let mut state = self
            .gate
            .state
            .write()
            .map_err(|_| TokenError::Unavailable)?;
        let revision = match &state.published {
            Some(previous) if previous.policy == policy => previous.revision,
            Some(previous) => previous
                .revision
                .checked_add(1)
                .ok_or(TokenError::Unavailable)?,
            None => 1,
        };
        state.published = Some(PublishedPolicy {
            runtime,
            policy,
            revision,
        });
        state.available = true;
        Ok(())
    }
}
