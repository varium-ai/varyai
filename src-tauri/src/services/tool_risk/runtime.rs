//! Versioned runtime rule snapshots.
//!
//! Use immutable Arc snapshots independently from the DLP runtime.
//! Workers read compiled policies; forwarding never queries SQLite or compiles rules.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

#[cfg(test)]
use super::builtin::builtin_rules;
#[cfg(test)]
use super::types::RuleOrigin;
use super::types::ToolRiskRule;

/// Runtime policy snapshot.
#[derive(Debug, Clone, Default)]
pub struct ToolRiskRuntime {
    /// Feature enablement.
    pub enabled: bool,
    /// Monotonically increasing policy version.
    pub policy_version: u64,
    /// Enabled built-in and custom rules after overrides.
    pub rules: Vec<ToolRiskRule>,
    /// Command lists from the same policy version, including disabled entries.
    pub command_lists: Vec<super::classifier::CommandListEntry>,
}

impl ToolRiskRuntime {
    /// Test fixture; production snapshots come from one database transaction.
    #[cfg(test)]
    pub fn build(
        enabled: bool,
        policy_version: u64,
        custom_rules: Vec<ToolRiskRule>,
        builtin_enabled_overrides: &std::collections::HashMap<String, bool>,
    ) -> Self {
        let mut rules = builtin_rules();
        for r in rules.iter_mut() {
            if let Some(&enabled) = builtin_enabled_overrides.get(&r.id) {
                r.enabled = enabled;
            }
        }

        rules.extend(
            custom_rules
                .into_iter()
                .filter(|r| r.origin == RuleOrigin::Custom),
        );
        rules.retain(|rule| {
            let valid = rule.validate().is_ok();
            if !valid {
                log::warn!("[ToolRisk] 无效规则未进入运行时快照");
            }
            valid
        });
        Self {
            enabled,
            policy_version,
            rules,
            command_lists: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn enabled_rules(&self) -> Vec<ToolRiskRule> {
        self.rules.iter().filter(|r| r.enabled).cloned().collect()
    }

    #[allow(dead_code)]
    pub fn has_enabled_rules(&self) -> bool {
        self.rules.iter().any(|r| r.enabled)
    }
}

#[derive(Default)]
struct RuntimeState {
    snapshot: RwLock<Arc<ToolRiskRuntime>>,
    enabled: AtomicBool,
    policy_version: AtomicU64,
}

impl RuntimeState {
    fn publish(&self, runtime: ToolRiskRuntime) -> bool {
        // Serialize publication under the snapshot lock; disable collection on failure.
        let mut snapshot = match self.snapshot.write() {
            Ok(guard) => guard,
            Err(_) => {
                self.enabled.store(false, Ordering::Release);
                log::warn!("[ToolRisk] 运行时发布锁不可用，已关闭采集");
                return false;
            }
        };
        self.enabled.store(false, Ordering::Release);
        if runtime.rules.iter().any(|rule| rule.validate().is_err())
            || runtime.command_lists.len() > super::classifier::MAX_COMMAND_LISTS
            || runtime
                .command_lists
                .iter()
                .any(|entry| entry.validate().is_err())
        {
            *snapshot = Arc::new(ToolRiskRuntime::default());
            log::warn!("[ToolRisk] 拒绝发布未验证的规则或名单快照，已关闭采集");
            return false;
        }
        let enabled = runtime.enabled;
        let version = runtime.policy_version;
        *snapshot = Arc::new(runtime);
        self.policy_version.store(version, Ordering::Release);
        self.enabled.store(enabled, Ordering::Release);
        true
    }
}

static RUNTIME: OnceLock<RuntimeState> = OnceLock::new();

fn runtime_state() -> &'static RuntimeState {
    RUNTIME.get_or_init(RuntimeState::default)
}

fn runtime_cell() -> &'static RwLock<Arc<ToolRiskRuntime>> {
    &runtime_state().snapshot
}

/// Forwarding hot path: atomic reads only, with no snapshot lock or database access.
pub fn collection_enabled() -> bool {
    runtime_state().enabled.load(Ordering::Acquire)
}

/// Bind observation to this version so workers reject content from another policy.
pub fn collection_policy_version() -> u64 {
    runtime_state().policy_version.load(Ordering::Acquire)
}

/// Get the worker snapshot; return a disabled snapshot if locking fails.
pub fn get_tool_risk_runtime() -> Arc<ToolRiskRuntime> {
    runtime_cell()
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| Arc::new(ToolRiskRuntime::default()))
}

/// Publish a validated snapshot; disable collection and return false on failure.
pub fn publish_tool_risk_runtime(runtime: ToolRiskRuntime) -> bool {
    runtime_state().publish(runtime)
}

#[allow(dead_code)]
pub fn set_tool_risk_runtime_for_test(runtime: ToolRiskRuntime) {
    publish_tool_risk_runtime(runtime);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn build_applies_builtin_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("builtin.sensitive_read".to_string(), false);
        let rt = ToolRiskRuntime::build(true, 1, Vec::new(), &overrides);
        let sensitive = rt
            .rules
            .iter()
            .find(|r| r.id == "builtin.sensitive_read")
            .unwrap();
        assert!(!sensitive.enabled);
    }

    #[test]
    fn build_keeps_defaults_without_overrides() {
        let rt = ToolRiskRuntime::build(true, 1, Vec::new(), &HashMap::new());
        let destructive = rt
            .rules
            .iter()
            .find(|r| r.id == "builtin.destructive_change")
            .unwrap();
        assert!(destructive.enabled);
    }

    #[test]
    fn enabled_rules_filters_disabled() {
        let rt = ToolRiskRuntime::build(true, 1, Vec::new(), &HashMap::new());

        assert!(!rt
            .enabled_rules()
            .iter()
            .any(|r| r.id == "builtin.destructive_database_change"));
        assert!(rt.has_enabled_rules());
    }

    #[test]
    #[serial_test::serial]
    fn publish_and_get_roundtrip() {
        let rt = ToolRiskRuntime::build(true, 42, Vec::new(), &HashMap::new());
        publish_tool_risk_runtime(rt);
        let got = get_tool_risk_runtime();
        assert_eq!(got.policy_version, 42);
        assert!(got.enabled);
    }

    #[test]
    fn custom_rules_appended() {
        use super::super::types::*;
        let custom = ToolRiskRule {
            schema_version: 1,
            id: "custom-1".into(),
            revision: 1,
            origin: RuleOrigin::Custom,
            name: "c".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                sources: None,
                clients: None,
                target: RuleTarget::Operation {
                    types: vec![OperationType::FileRead],
                },
            },
            conditions: vec![RuleCondition {
                field: "/target_path".into(),
                op: ConditionOp::Detector,
                value: Some("sensitive_path".into()),
                values: None,
            }],
            risk_category: RiskCategory::SensitiveRead,
            severity: Severity::High,
            reason_code: "sensitive_target".into(),
            local_review: LocalReview::Off,
        };
        let rt = ToolRiskRuntime::build(true, 1, vec![custom], &HashMap::new());
        assert!(rt.rules.iter().any(|r| r.id == "custom-1"));
    }

    #[test]
    fn build_excludes_unvalidated_custom_rules() {
        let mut invalid = builtin_rules().remove(0);
        invalid.id = "custom-invalid".into();
        invalid.origin = RuleOrigin::Custom;
        invalid.conditions.clear();
        let rt = ToolRiskRuntime::build(true, 1, vec![invalid], &HashMap::new());
        assert!(rt.rules.iter().all(|rule| rule.validate().is_ok()));
        assert!(!rt.rules.iter().any(|rule| rule.id == "custom-invalid"));
    }

    #[test]
    #[serial_test::serial]
    fn publish_rejects_unvalidated_snapshot() {
        let mut runtime = ToolRiskRuntime::build(true, 43, Vec::new(), &HashMap::new());
        runtime.rules[0].conditions.clear();
        publish_tool_risk_runtime(runtime);
        let current = get_tool_risk_runtime();
        assert!(!current.enabled || current.rules.iter().all(|rule| rule.validate().is_ok()));
    }

    #[test]
    #[serial_test::serial]
    fn collection_getters_follow_publish_without_snapshot_lock() {
        publish_tool_risk_runtime(ToolRiskRuntime::build(
            true,
            50,
            Vec::new(),
            &HashMap::new(),
        ));
        let _guard = runtime_cell().write().unwrap();
        assert!(collection_enabled());
        assert_eq!(collection_policy_version(), 50);
        drop(_guard);
        publish_tool_risk_runtime(ToolRiskRuntime::build(
            false,
            51,
            Vec::new(),
            &HashMap::new(),
        ));
        assert!(!collection_enabled());
        assert_eq!(collection_policy_version(), 51);
    }

    #[test]
    fn poisoned_publish_is_fail_safe() {
        let state = RuntimeState::default();
        assert!(state.publish(ToolRiskRuntime::build(true, 1, Vec::new(), &HashMap::new())));
        let _ = std::panic::catch_unwind(|| {
            let _guard = state.snapshot.write().unwrap();
            panic!("poison local runtime lock");
        });
        assert!(!state.publish(ToolRiskRuntime::build(true, 2, Vec::new(), &HashMap::new())));
        assert!(!state.enabled.load(std::sync::atomic::Ordering::Acquire));
    }
}

#[cfg(test)]
mod command_list_tests {
    use super::*;
    use crate::services::tool_risk::classifier::{
        CommandListEntry, CommandListType, CommandMatchKind, MAX_COMMAND_LISTS,
    };
    use crate::services::tool_risk::types::{RiskCategory, Severity};
    use std::collections::HashMap;

    fn entry() -> CommandListEntry {
        CommandListEntry {
            id: "shell-list-test".into(),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Program,
            pattern: "curl".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::DataExfiltration,
        }
    }

    #[test]
    fn command_lists_runtime_build_stays_compatible_and_publishes_valid_lists() {
        let state = RuntimeState::default();
        let mut rt = ToolRiskRuntime::build(true, 1, vec![], &HashMap::new());
        assert!(rt.command_lists.is_empty());
        rt.command_lists.push(entry());
        assert!(state.publish(rt));
        assert_eq!(state.snapshot.read().unwrap().command_lists, vec![entry()]);
    }

    #[test]
    fn command_lists_runtime_rejects_invalid_or_oversized_lists_and_closes_collection() {
        let state = RuntimeState::default();
        for oversized in [false, true] {
            assert!(state.publish(ToolRiskRuntime::build(true, 1, vec![], &HashMap::new())));
            let mut invalid = ToolRiskRuntime::build(true, 2, vec![], &HashMap::new());
            if oversized {
                invalid.command_lists = vec![entry(); MAX_COMMAND_LISTS + 1];
            } else {
                let mut item = entry();
                item.pattern.clear();
                invalid.command_lists.push(item);
            }
            assert!(!state.publish(invalid));
            assert!(!state.enabled.load(Ordering::Acquire));
            assert!(state.snapshot.read().unwrap().command_lists.is_empty());
        }
    }
}
