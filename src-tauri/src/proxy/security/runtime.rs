use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use super::types::{AuditAction, RiskLevel};
use crate::database::Database;
use crate::error::AppError;

/// A compiled custom rule.
#[derive(Debug, Clone)]
pub struct CompiledCustomRule {
    pub id: String,
    pub name: String,
    pub rule_type: String, // "keyword" | "regex"
    pub pattern: String,
    pub regex: Option<regex::Regex>,
    pub risk_level: RiskLevel,
    pub action: AuditAction,
    pub enabled: bool,
}

/// Immutable runtime security configuration.
#[derive(Debug, Clone)]
pub struct SecurityRuntimeConfig {
    pub(crate) config_revision: u64,
    pub(crate) aggregation_enabled: bool,
    pub(crate) epoch_counter: u64,
    pub(crate) rule_versions: HashMap<String, (RiskLevel, u64)>,
    pub enabled: bool,
    pub custom_rules: Vec<CompiledCustomRule>,
    pub builtin_action_overrides: HashMap<String, AuditAction>,
}

impl Default for SecurityRuntimeConfig {
    fn default() -> Self {
        Self {
            config_revision: 0,
            aggregation_enabled: true,
            epoch_counter: 0,
            rule_versions: HashMap::new(),
            enabled: true,
            custom_rules: Vec::new(),
            builtin_action_overrides: HashMap::new(),
        }
    }
}

impl SecurityRuntimeConfig {
    pub(crate) fn risk_epoch(&self, id: &str) -> u64 {
        self.rule_versions
            .get(id)
            .map(|(_, epoch)| *epoch)
            .unwrap_or(0)
    }

    fn assign_versions(&mut self, previous: &Self, levels: HashMap<String, RiskLevel>) {
        self.epoch_counter = previous.epoch_counter;
        self.aggregation_enabled = previous.aggregation_enabled;
        let Some(revision) = previous.config_revision.checked_add(1) else {
            self.aggregation_enabled = false;
            return;
        };
        self.config_revision = revision;
        for (id, level) in levels {
            let epoch = if let Some((old_level, epoch)) = previous.rule_versions.get(&id) {
                if *old_level == level {
                    Some(*epoch)
                } else {
                    self.epoch_counter.checked_add(1)
                }
            } else {
                self.epoch_counter.checked_add(1)
            };
            let Some(epoch) = epoch else {
                self.aggregation_enabled = false;
                self.rule_versions.clear();
                return;
            };
            self.epoch_counter = self.epoch_counter.max(epoch);
            self.rule_versions.insert(id, (level, epoch));
        }
    }
}
fn parse_risk(value: &str) -> RiskLevel {
    match value.to_ascii_uppercase().as_str() {
        "CRITICAL" => RiskLevel::Critical,
        "HIGH" => RiskLevel::High,
        "LOW" => RiskLevel::Low,
        _ => RiskLevel::Medium,
    }
}

static RUNTIME_CONFIG: OnceLock<RwLock<Arc<SecurityRuntimeConfig>>> = OnceLock::new();

fn runtime_cell() -> &'static RwLock<Arc<SecurityRuntimeConfig>> {
    RUNTIME_CONFIG.get_or_init(|| RwLock::new(Arc::new(SecurityRuntimeConfig::default())))
}

/// Read the current configuration snapshot without acquiring a database lock.
pub fn get_security_runtime() -> Arc<SecurityRuntimeConfig> {
    runtime_cell()
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_else(|_| Arc::new(SecurityRuntimeConfig::default()))
}

/// Set a test-only runtime configuration.
pub fn set_security_runtime_for_test(config: SecurityRuntimeConfig) {
    if let Ok(mut guard) = runtime_cell().write() {
        *guard = Arc::new(config);
    }
}

/// Reload settings and custom rules from SQLite and publish a runtime snapshot.
pub fn reload_security_runtime(db: &Database) -> Result<(), AppError> {
    let settings = db.get_security_settings()?;
    let custom_rules = db.get_custom_security_rules()?;
    let builtin_action_overrides = db
        .get_builtin_security_rule_actions()?
        .into_iter()
        .filter_map(|(id, action)| {
            let action = match action.as_str() {
                "block" => AuditAction::Block,
                "audit" => AuditAction::Audit,
                "redact" => AuditAction::Redact,
                "allow" => AuditAction::Allow,
                _ => return None,
            };
            Some((id, action))
        })
        .collect();

    // Include disabled rules in metadata so toggles preserve their risk epoch.
    let mut levels: HashMap<String, RiskLevel> = super::builtin_rules::BUILTIN_RULES
        .iter()
        .map(|r| (r.id.to_string(), r.risk_level))
        .collect();
    levels.insert("chinese_id_card".into(), RiskLevel::Medium);
    levels.insert("chinese_phone_number".into(), RiskLevel::Medium);
    levels.extend(
        custom_rules
            .iter()
            .map(|r| (r.id.clone(), parse_risk(&r.risk_level))),
    );
    let mut compiled_rules = Vec::new();
    for rule in custom_rules {
        if !rule.enabled {
            continue;
        }
        let (regex, is_valid) = if rule.rule_type == "regex" {
            match regex::Regex::new(&rule.pattern) {
                Ok(re) => (Some(re), true),
                Err(e) => {
                    log::warn!(
                        "[SecurityAudit] 自定义正则规则 '{}' 语法无效: {e}",
                        rule.name
                    );
                    (None, false)
                }
            }
        } else {
            (None, true)
        };

        if is_valid {
            let risk_level = match rule.risk_level.to_ascii_uppercase().as_str() {
                "CRITICAL" => RiskLevel::Critical,
                "HIGH" => RiskLevel::High,
                "LOW" => RiskLevel::Low,
                _ => RiskLevel::Medium,
            };
            let action = match rule.action.to_ascii_lowercase().as_str() {
                "audit" => AuditAction::Audit,
                "redact" => AuditAction::Redact,
                "allow" => AuditAction::Allow,
                _ => AuditAction::Block,
            };
            compiled_rules.push(CompiledCustomRule {
                id: rule.id,
                name: rule.name,
                rule_type: rule.rule_type,
                pattern: rule.pattern,
                regex,
                risk_level,
                action,
                enabled: rule.enabled,
            });
        }
    }

    let rule_count = compiled_rules.len();
    let mut new_cfg = SecurityRuntimeConfig {
        enabled: settings.enabled,
        custom_rules: compiled_rules,
        builtin_action_overrides,
        ..Default::default()
    };

    if let Ok(mut guard) = runtime_cell().write() {
        new_cfg.assign_versions(&guard, levels);
        *guard = Arc::new(new_cfg);
    }
    log::info!(
        "[SecurityAudit] 安全审计运行时配置已热更新 (enabled: {}, rules: {})",
        settings.enabled,
        rule_count
    );
    Ok(())
}

#[cfg(test)]
mod version_tests {
    use super::*;
    fn next(previous: &SecurityRuntimeConfig, level: RiskLevel) -> SecurityRuntimeConfig {
        let mut cfg = SecurityRuntimeConfig::default();
        cfg.assign_versions(previous, HashMap::from([("r".to_string(), level)]));
        cfg
    }
    #[test]
    fn risk_epochs_survive_unrelated_reload_but_not_grade_changes_or_recreation() {
        let first = next(&SecurityRuntimeConfig::default(), RiskLevel::Low);
        let same = next(&first, RiskLevel::Low);
        assert!(same.config_revision > first.config_revision);
        assert_eq!(same.risk_epoch("r"), first.risk_epoch("r"));
        let high = next(&same, RiskLevel::High);
        let low = next(&high, RiskLevel::Low);
        assert!(high.risk_epoch("r") > same.risk_epoch("r"));
        assert!(low.risk_epoch("r") > high.risk_epoch("r"));
        let mut deleted = SecurityRuntimeConfig::default();
        deleted.assign_versions(&low, HashMap::new());
        let recreated = next(&deleted, RiskLevel::Low);
        assert!(recreated.risk_epoch("r") > low.risk_epoch("r"));
        let mut overflow = recreated.clone();
        overflow.config_revision = u64::MAX;
        assert!(!next(&overflow, RiskLevel::Low).aggregation_enabled);
    }
}
