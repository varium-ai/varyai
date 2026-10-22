use super::types::{
    ConditionOp, LocalReview, OperationType, RiskCategory, RuleCondition, RuleOrigin, RuleSelector,
    RuleTarget, Severity, ToolRiskRule,
};

pub const BUILTIN_RULES_VERSION: u32 = 1;

fn cond(field: &str, op: ConditionOp, value: &str) -> RuleCondition {
    RuleCondition {
        field: field.into(),
        op,
        value: Some(value.into()),
        values: None,
    }
}

// Keep each built-in rule declaration together in the policy table.
#[allow(clippy::too_many_arguments)]
fn op_rule(
    id: &str,
    name: &str,
    types: Vec<OperationType>,
    conditions: Vec<RuleCondition>,
    category: RiskCategory,
    severity: Severity,
    reason_code: &str,
    enabled: bool,
) -> ToolRiskRule {
    ToolRiskRule {
        schema_version: 1,
        id: id.into(),
        revision: 1,
        origin: RuleOrigin::Builtin,
        name: name.into(),
        description: None,
        enabled,
        selector: RuleSelector {
            sources: None,
            clients: None,
            target: RuleTarget::Operation { types },
        },
        conditions,
        risk_category: category,
        severity,
        reason_code: reason_code.into(),
        local_review: LocalReview::Off,
    }
}

pub fn builtin_rules() -> Vec<ToolRiskRule> {
    vec![
        op_rule(
            "builtin.destructive_change",
            "破坏性删除或覆写",
            vec![
                OperationType::FileDelete,
                OperationType::FileWrite,
                OperationType::ShellExec,
            ],
            vec![cond(
                "/target_path",
                ConditionOp::Detector,
                "destructive_path",
            )],
            RiskCategory::DestructiveChange,
            Severity::High,
            "destructive_target",
            true,
        ),
        op_rule(
            "builtin.sensitive_read",
            "敏感路径读取",
            vec![OperationType::FileRead],
            vec![cond(
                "/target_path",
                ConditionOp::Detector,
                "sensitive_path",
            )],
            RiskCategory::SensitiveRead,
            Severity::High,
            "sensitive_target",
            true,
        ),
        op_rule(
            "builtin.data_exfiltration",
            "敏感数据外发",
            vec![OperationType::DataSend, OperationType::NetworkRequest],
            vec![cond(
                "/destination",
                ConditionOp::Detector,
                "exfil_destination",
            )],
            RiskCategory::DataExfiltration,
            Severity::High,
            "exfil_to_remote",
            true,
        ),
        op_rule(
            "builtin.remote_code_execution",
            "远程代码执行",
            vec![OperationType::ShellExec, OperationType::ProcessExec],
            vec![cond(
                "/command",
                ConditionOp::Detector,
                "remote_download_exec",
            )],
            RiskCategory::RemoteCodeExecution,
            Severity::High,
            "remote_exec_chain",
            true,
        ),
        op_rule(
            "builtin.privilege_change",
            "权限提升",
            vec![OperationType::ShellExec, OperationType::ProcessExec],
            vec![cond(
                "/command",
                ConditionOp::Detector,
                "privilege_escalation",
            )],
            RiskCategory::PrivilegeChange,
            Severity::High,
            "privilege_escalation",
            true,
        ),
        op_rule(
            "builtin.security_config_change",
            "安全配置修改",
            vec![OperationType::FileWrite, OperationType::FileDelete],
            vec![cond(
                "/target_path",
                ConditionOp::Detector,
                "security_config_path",
            )],
            RiskCategory::SecurityConfigChange,
            Severity::High,
            "security_config_target",
            true,
        ),
        op_rule(
            "builtin.destructive_database_change",
            "破坏性数据库变更",
            vec![OperationType::DatabaseOperation],
            vec![cond("/command", ConditionOp::Detector, "destructive_sql")],
            RiskCategory::DestructiveDatabaseChange,
            Severity::High,
            "destructive_sql",
            false,
        ),
    ]
}

#[allow(dead_code)]
pub fn builtin_rule_ids() -> Vec<&'static str> {
    vec![
        "builtin.destructive_change",
        "builtin.sensitive_read",
        "builtin.data_exfiltration",
        "builtin.remote_code_execution",
        "builtin.privilege_change",
        "builtin.security_config_change",
        "builtin.destructive_database_change",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_builtin_rules() {
        assert_eq!(builtin_rules().len(), 7);
    }

    #[test]
    fn all_builtin_rules_validate() {
        for r in builtin_rules() {
            assert!(
                r.validate().is_ok(),
                "rule {} invalid: {:?}",
                r.id,
                r.validate()
            );
        }
    }

    #[test]
    fn default_enabled_states() {
        let rules = builtin_rules();
        let by_id = |id: &str| rules.iter().find(|r| r.id == id).unwrap();
        assert!(by_id("builtin.destructive_change").enabled);
        assert!(by_id("builtin.sensitive_read").enabled);
        assert!(by_id("builtin.data_exfiltration").enabled);
        assert!(by_id("builtin.remote_code_execution").enabled);
        assert!(by_id("builtin.privilege_change").enabled);
        assert!(by_id("builtin.security_config_change").enabled);

        assert!(!by_id("builtin.destructive_database_change").enabled);
    }

    #[test]
    fn all_builtin_ids_stable() {
        let ids: Vec<String> = builtin_rules().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, builtin_rule_ids());
    }

    #[test]
    fn all_builtin_origin_is_builtin() {
        for r in builtin_rules() {
            assert_eq!(r.origin, RuleOrigin::Builtin);
        }
    }
}
