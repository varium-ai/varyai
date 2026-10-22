use serde::{Deserialize, Serialize};
use tauri::State;

use crate::database::{ToolRiskEventRecord, ToolRiskPolicyChange, ToolRiskSettings};
use crate::services::tool_risk::builtin::BUILTIN_RULES_VERSION;
use crate::services::tool_risk::classifier::{CommandListEntry, CommandMatchKind};
use crate::services::tool_risk::engine::{evaluate, DetectionScope, EngineInput, Verdict};
use crate::services::tool_risk::runtime::get_tool_risk_runtime;
use crate::services::tool_risk::service;
pub use crate::services::tool_risk::service::SaveCustomRuleInput;
pub use crate::services::tool_risk::service::{
    CommandPatternPreview, SaveCommandListInput, ToolRiskMutationReceipt,
};
use crate::services::tool_risk::types::{RuleCondition, RuleOrigin, RuleTarget, ToolRiskRule};
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRiskRuleItem {
    pub id: String,
    pub name: String,
    pub origin: String,
    pub enabled: bool,
    pub risk_category: String,
    pub severity: String,
    pub reason_code: String,
    pub local_review: String,
    /// Legacy list summary, used only when structured selector data is unavailable.
    pub selector_summary: String,
    pub condition_count: usize,
    /// Structured selector scope for semantic presentation without raw summaries.
    pub target_kind: String,
    pub operation_types: Vec<String>,
    pub tool_ref_count: usize,
    /// Structured conditions for the rule editor and list view.
    pub conditions: Vec<RuleCondition>,
    pub revision: u32,

    pub builtin_version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxTestInput {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub client: Option<String>,
    pub definition_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxTestResult {
    pub scope: String,
    pub verdict: Option<String>,
    pub matched_rule_ids: Vec<String>,
    pub risk_categories: Vec<String>,
    pub applicable_rules: usize,
}

pub fn reload_tool_risk_runtime(db: &crate::database::Database) -> Result<(), String> {
    service::reload_runtime(db)
}

fn selector_summary(rule: &ToolRiskRule) -> String {
    match &rule.selector.target {
        RuleTarget::Operation { types } => {
            let names: Vec<&str> = types.iter().map(|t| t.as_str()).collect();
            format!("operation:{}", names.join(","))
        }
        RuleTarget::Tool { .. } => "tool:exact".into(),
    }
}

/// Expose operation types individually; exact-tool selectors expose only a reference count.
fn selector_parts(rule: &ToolRiskRule) -> (String, Vec<String>, usize) {
    match &rule.selector.target {
        RuleTarget::Operation { types } => (
            "operation".into(),
            types.iter().map(|t| t.as_str().to_string()).collect(),
            0,
        ),
        RuleTarget::Tool { tool_refs } => ("tool".into(), Vec::new(), tool_refs.len()),
    }
}

/// Read persisted command lists, including disabled entries and the current revision.
#[tauri::command]
pub async fn get_tool_risk_command_lists(
    state: State<'_, AppState>,
) -> Result<Vec<CommandListEntry>, String> {
    service::get_command_lists(&state.db)
}

#[tauri::command]
pub async fn save_tool_risk_command_list(
    state: State<'_, AppState>,
    input: SaveCommandListInput,
) -> Result<ToolRiskMutationReceipt<String>, String> {
    service::save_command_list(&state.db, input)
}

#[tauri::command]
pub async fn delete_tool_risk_command_list(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<bool>, String> {
    service::delete_command_list(&state.db, &id, expected_revision)
}

#[tauri::command]
pub async fn preview_tool_risk_command_pattern(
    state: State<'_, AppState>,
    match_kind: CommandMatchKind,
    pattern: String,
) -> Result<Vec<CommandPatternPreview>, String> {
    service::preview_command_pattern(&state.db, match_kind, &pattern)
}

#[tauri::command]
pub async fn get_tool_risk_rules(
    state: State<'_, AppState>,
) -> Result<Vec<ToolRiskRuleItem>, String> {
    let rules = service::get_rules(&state.db)?;
    let mut items = Vec::new();

    for rule in &rules {
        let is_builtin = rule.origin == RuleOrigin::Builtin;
        let (target_kind, operation_types, tool_ref_count) = selector_parts(rule);
        items.push(ToolRiskRuleItem {
            id: rule.id.clone(),
            name: rule.name.clone(),
            origin: if is_builtin {
                "builtin".into()
            } else {
                "custom".into()
            },
            enabled: rule.enabled,
            risk_category: rule.risk_category.as_str().to_string(),
            severity: rule.severity.as_str().to_string(),
            reason_code: rule.reason_code.clone(),
            local_review: match rule.local_review {
                crate::services::tool_risk::types::LocalReview::Off => "off".into(),
                crate::services::tool_risk::types::LocalReview::Eligible => "eligible".into(),
            },
            selector_summary: selector_summary(rule),
            condition_count: rule.conditions.len(),
            target_kind,
            operation_types,
            tool_ref_count,
            conditions: rule.conditions.clone(),
            revision: rule.revision,
            builtin_version: if is_builtin {
                Some(BUILTIN_RULES_VERSION)
            } else {
                None
            },
        });
    }
    Ok(items)
}

#[tauri::command]
pub async fn get_tool_risk_rule(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<ToolRiskRule>, String> {
    service::get_rule(&state.db, &id)
}

#[tauri::command]
pub async fn save_custom_tool_risk_rule(
    state: State<'_, AppState>,
    input: SaveCustomRuleInput,
) -> Result<ToolRiskMutationReceipt<String>, String> {
    service::save_custom_rule(&state.db, input)
}

#[tauri::command]
pub async fn delete_custom_tool_risk_rule(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<bool>, String> {
    service::delete_custom_rule(&state.db, &id, expected_revision)
}

#[tauri::command]
pub async fn toggle_tool_risk_rule(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    service::toggle_rule(&state.db, &id, enabled, expected_revision)
}

#[tauri::command]
pub async fn reset_builtin_tool_risk_rule(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u32,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    service::reset_builtin_rule(&state.db, &id, expected_revision)
}

#[tauri::command]
pub async fn test_tool_risk_rule_match(
    _state: State<'_, AppState>,
    input: SandboxTestInput,
) -> Result<SandboxTestResult, String> {
    let observation = sandbox_observation(input);

    let runtime = get_tool_risk_runtime();
    let decision = evaluate(&EngineInput {
        observation: &observation,
        rules: &runtime.rules,
        lists: &runtime.command_lists,
    });

    Ok(SandboxTestResult {
        scope: match decision.scope {
            DetectionScope::InScope => "in_scope".into(),
            DetectionScope::OutOfScope => "out_of_scope".into(),
            DetectionScope::Undetermined => "undetermined".into(),
        },
        verdict: decision.verdict.map(|v| match v {
            Verdict::Risky => "risky".into(),
            Verdict::Unchecked => "unchecked".into(),
            Verdict::Uncertain => "uncertain".into(),
            Verdict::LowRisk => "low_risk".into(),
        }),
        matched_rule_ids: decision.hits.iter().map(|h| h.rule_id.clone()).collect(),
        risk_categories: decision
            .hits
            .iter()
            .map(|h| h.risk_category.as_str().to_string())
            .collect(),
        applicable_rules: decision.applicable_rules,
    })
}

fn sandbox_observation(
    input: SandboxTestInput,
) -> crate::proxy::tool_observation::types::ToolObservation {
    use crate::proxy::tool_observation::types::*;

    ToolObservation {
        schema_version: 1,
        observation_id: "sandbox".into(),
        source: ObservationSource::ModelResponse,
        stage: ObservationStage::Proposed,
        execution_scope: ExecutionScope::Client,
        protocol: "sandbox".into(),
        scope_id: "sandbox".into(),
        correlation: Correlation::default(),
        tool: ToolIdentity {
            client: input.client,
            name: input.tool_name,
            namespace: None,
            server_id: None,
            definition_ref: input.definition_ref,
        },
        input: ToolInput {
            encoding: InputEncoding::Json,
            value: Some(input.input),
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

#[tauri::command]
pub async fn get_tool_risk_settings(
    state: State<'_, AppState>,
) -> Result<ToolRiskSettings, String> {
    state.db.get_tool_risk_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_tool_risk_settings(
    state: State<'_, AppState>,
    settings: ToolRiskSettings,
) -> Result<ToolRiskMutationReceipt<()>, String> {
    service::update_settings(&state.db, &settings)
}

#[tauri::command]
pub async fn get_tool_risk_policy_changes(
    state: State<'_, AppState>,
    limit: u32,
    offset: u32,
) -> Result<Vec<ToolRiskPolicyChange>, String> {
    query_policy_changes(&state.db, limit, offset)
}

fn query_policy_changes(
    db: &crate::database::Database,
    limit: u32,
    offset: u32,
) -> Result<Vec<ToolRiskPolicyChange>, String> {
    db.get_tool_risk_policy_changes(limit, offset)
        .map_err(|_| "工具风险变更记录读取失败".to_string())
}

#[tauri::command]
pub async fn clear_tool_risk_policy_changes(state: State<'_, AppState>) -> Result<usize, String> {
    clear_policy_changes(&state.db)
}

fn clear_policy_changes(db: &crate::database::Database) -> Result<usize, String> {
    db.clear_tool_risk_policy_changes()
        .map_err(|_| "工具风险变更记录清空失败".to_string())
}

#[tauri::command]
pub async fn get_tool_risk_events(
    state: State<'_, AppState>,
    limit: u32,
    offset: u32,
    verdict: Option<Verdict>,
) -> Result<Vec<ToolRiskEventRecord>, String> {
    query_tool_risk_events(&state.db, limit, offset, verdict)
}

fn query_tool_risk_events(
    db: &crate::database::Database,
    limit: u32,
    offset: u32,
    verdict: Option<Verdict>,
) -> Result<Vec<ToolRiskEventRecord>, String> {
    db.get_tool_risk_events_filtered(limit, offset, verdict)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_tool_risk_events_after(
    state: State<'_, AppState>,
    after_id: i64,
    limit: u32,
    policy_version: u64,
) -> Result<Vec<ToolRiskEventRecord>, String> {
    state
        .db
        .get_tool_risk_events_after(after_id, limit, policy_version)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_tool_risk_events(
    state: State<'_, AppState>,
    before_timestamp: Option<i64>,
) -> Result<u64, String> {
    state
        .db
        .clear_tool_risk_events(before_timestamp)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct EventQueryArgs {
        limit: u32,
        offset: u32,
        verdict: Option<Verdict>,
    }

    fn event_query_db() -> crate::database::Database {
        use crate::database::ToolRiskEventInput;

        let db = crate::database::Database::memory().unwrap();
        let events: Vec<_> = [
            Some("risky"),
            Some("unchecked"),
            Some("uncertain"),
            Some("low_risk"),
            None,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, verdict)| ToolRiskEventInput {
            timestamp: index as i64,
            source: "model_response".into(),
            stage: "proposed".into(),
            client_category: None,
            operation_category: None,
            command_summary: Some("public summary".into()),
            detection_scope: if verdict.is_some() {
                "in_scope"
            } else {
                "out_of_scope"
            }
            .into(),
            integrity_status: "complete".into(),
            verdict: verdict.map(str::to_owned),
            risk_category: None,
            severity: None,
            reason_code: None,
            policy_version: 1,
            rule_ids: Vec::new(),
        })
        .collect();
        db.insert_tool_risk_events_batch(&events).unwrap();
        db
    }

    fn run_event_query(
        db: &crate::database::Database,
        payload: serde_json::Value,
    ) -> Result<Vec<ToolRiskEventRecord>, String> {
        let args: EventQueryArgs =
            serde_json::from_value(payload).map_err(|error| error.to_string())?;
        query_tool_risk_events(db, args.limit, args.offset, args.verdict)
    }

    #[test]
    fn event_query_forwards_four_typed_verdicts() {
        let db = event_query_db();
        for (wire, expected, expected_id) in [
            ("risky", Verdict::Risky, 1),
            ("unchecked", Verdict::Unchecked, 2),
            ("uncertain", Verdict::Uncertain, 3),
            ("low_risk", Verdict::LowRisk, 4),
        ] {
            let payload = serde_json::json!({ "limit": 10, "offset": 0, "verdict": wire });
            let args: EventQueryArgs = serde_json::from_value(payload.clone()).unwrap();
            assert_eq!(args.verdict, Some(expected));
            let rows = run_event_query(&db, payload).unwrap();
            assert_eq!(
                rows.iter().map(|event| event.id).collect::<Vec<_>>(),
                vec![expected_id]
            );
            assert_eq!(rows[0].verdict.as_deref(), Some(wire));
        }
    }

    #[test]
    fn event_query_omitted_and_null_verdict_preserve_all_event_pagination() {
        let db = event_query_db();
        for payload in [
            serde_json::json!({ "limit": 2, "offset": 1 }),
            serde_json::json!({ "limit": 2, "offset": 1, "verdict": null }),
        ] {
            let args: EventQueryArgs = serde_json::from_value(payload.clone()).unwrap();
            assert_eq!(args.verdict, None);
            let rows = run_event_query(&db, payload).unwrap();
            assert_eq!(
                rows.iter().map(|event| event.id).collect::<Vec<_>>(),
                vec![4, 3]
            );
            assert_eq!(rows, db.get_tool_risk_events(2, 1).unwrap());
        }
        let all = run_event_query(&db, serde_json::json!({ "limit": 10, "offset": 0 })).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].verdict, None);
    }

    #[test]
    fn event_query_rejects_invalid_verdict_instead_of_querying_all() {
        let db = event_query_db();
        for invalid in [
            serde_json::json!("all"),
            serde_json::json!("Risky"),
            serde_json::json!("lowRisk"),
            serde_json::json!(""),
            serde_json::json!(" risky"),
            serde_json::json!("risky' OR 1=1 --"),
            serde_json::json!(true),
            serde_json::json!(1),
            serde_json::json!({}),
            serde_json::json!([]),
        ] {
            let payload = serde_json::json!({ "limit": 10, "offset": 0, "verdict": invalid });
            assert!(
                run_event_query(&db, payload).is_err(),
                "invalid filter must not fall back to all events"
            );
        }
        assert_eq!(db.get_tool_risk_events(10, 0).unwrap().len(), 5);
    }

    #[test]
    fn sandbox_preserves_explicit_definition_ref_without_name_inference() {
        let input: SandboxTestInput = serde_json::from_value(serde_json::json!({
            "toolName": "Read", "input": {"file_path": "/tmp/example"},
            "client": "claude_code", "definitionRef": "schema:explicit:v1"
        }))
        .unwrap();
        let observation = sandbox_observation(input);
        assert_eq!(
            observation.tool.definition_ref.as_deref(),
            Some("schema:explicit:v1")
        );
        let input: SandboxTestInput = serde_json::from_value(serde_json::json!({
            "toolName": "Read", "input": {"file_path": "/tmp/example"}, "client": "claude_code"
        }))
        .unwrap();
        assert_eq!(sandbox_observation(input).tool.definition_ref, None);
    }
}

#[cfg(test)]
mod command_list_tests {
    #[test]
    fn command_lists_ipc_crud_and_summary_preview_are_registered() {
        let registration = include_str!("../lib.rs");
        for name in [
            "get_tool_risk_command_lists",
            "save_tool_risk_command_list",
            "delete_tool_risk_command_list",
            "preview_tool_risk_command_pattern",
        ] {
            assert!(
                registration.contains(&format!("commands::{name},")),
                "missing IPC registration: {name}"
            );
        }
    }
}

#[cfg(test)]
mod policy_changes_tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PolicyQueryArgs {
        limit: u32,
        offset: u32,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct DeleteArgs {
        id: String,
        expected_revision: u32,
    }

    #[test]
    #[serial_test::serial]
    fn write_inputs_preserve_camel_case_and_serialize_real_receipts() {
        let db = Database::memory().unwrap();
        let input: SaveCommandListInput = serde_json::from_value(json!({
            "listType": "blacklist", "matchKind": "keyword", "pattern": "SECRET_IPC_PATTERN",
            "enabled": true, "severity": "high", "riskCategory": "data_exfiltration"
        }))
        .unwrap();
        let receipt =
            serde_json::to_value(service::save_command_list(&db, input).unwrap()).unwrap();
        let id = receipt["value"]
            .as_str()
            .expect("typed receipt must contain ID");
        assert_eq!(
            receipt,
            json!({"value": id, "persistedPolicyVersion": 1, "auditStatus": "recorded", "runtimeStatus": "applied"})
        );
        assert!(!receipt.to_string().contains("SECRET_IPC_PATTERN"));
        let args: DeleteArgs =
            serde_json::from_value(json!({"id": id, "expectedRevision": 1})).unwrap();
        let deleted = service::delete_command_list(&db, &args.id, args.expected_revision).unwrap();
        assert_eq!(
            serde_json::to_value(deleted).unwrap(),
            json!({"value": true, "persistedPolicyVersion": 2, "auditStatus": "recorded", "runtimeStatus": "applied"})
        );
        let settings: ToolRiskSettings =
            serde_json::from_value(json!({"enabled": true, "policyVersion": 999})).unwrap();
        let saved = service::update_settings(&db, &settings).unwrap();
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            json!({"value": null, "persistedPolicyVersion": 3, "auditStatus": "recorded", "runtimeStatus": "applied"})
        );
    }

    #[test]
    #[serial_test::serial]
    fn history_query_and_clear_preserve_policy_and_runtime_and_wire_shape() {
        let db = Database::memory().unwrap();
        service::update_settings(
            &db,
            &ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
        )
        .unwrap();
        service::toggle_rule(&db, "builtin.sensitive_read", false, 1).unwrap();
        let settings = db.get_tool_risk_settings().unwrap();
        let before = get_tool_risk_runtime();
        let args: PolicyQueryArgs =
            serde_json::from_value(json!({"limit": 1, "offset": 1})).unwrap();
        let rows = query_policy_changes(&db, args.limit, args.offset).unwrap();
        assert_eq!(rows.len(), 1);
        let wire = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(
            wire,
            json!({"id": rows[0].id, "at": rows[0].at, "policyVersion": 1, "actor": "local_ui", "kind": "settings_update", "targetId": null, "changedFields": ["enabled"], "auditStatus": "recorded"})
        );
        assert_eq!(
            serde_json::to_value(clear_policy_changes(&db).unwrap()).unwrap(),
            json!(2)
        );
        assert!(query_policy_changes(&db, 50, 0).unwrap().is_empty());
        assert_eq!(clear_policy_changes(&db).unwrap(), 0);
        assert_eq!(db.get_tool_risk_settings().unwrap(), settings);
        assert!(
            std::sync::Arc::ptr_eq(&before, &get_tool_risk_runtime()),
            "read/clear must not publish a new runtime"
        );
        assert!(
            !service::get_rule(&db, "builtin.sensitive_read")
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn history_ipc_errors_are_fixed_and_never_echo_db_values() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch("INSERT INTO tool_risk_policy_changes (at, policy_version, actor, kind, target_id, changed_fields, audit_status) VALUES (1, 1, 'local_ui', 'settings_update', NULL, '[\"enabled\"]', 'recorded');
            CREATE TRIGGER fail_clear_policy_history BEFORE DELETE ON tool_risk_policy_changes BEGIN SELECT RAISE(ABORT, 'SECRET_CLEAR_FAILURE'); END;").unwrap();
        assert_eq!(
            clear_policy_changes(&db).unwrap_err(),
            "工具风险变更记录清空失败"
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE tool_risk_policy_changes;")
            .unwrap();
        assert_eq!(
            query_policy_changes(&db, 10, 0).unwrap_err(),
            "工具风险变更记录读取失败"
        );
    }

    #[test]
    fn history_ipc_does_not_return_untrusted_raw_library_values() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch("PRAGMA ignore_check_constraints = ON; INSERT INTO tool_risk_policy_changes (at, policy_version, actor, kind, target_id, changed_fields, audit_status)
            VALUES (1, 1, 'SECRET_ACTOR', 'SECRET_KIND', 'SECRET_TARGET', '[\"enabled\",\"SECRET_FIELD\"]', 'SECRET_STATUS');").unwrap();
        let rows = query_policy_changes(&db, 10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        let wire = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(wire["actor"], "unknown");
        assert_eq!(wire["kind"], "unknown");
        assert_eq!(wire["targetId"], "<unknown_target>");
        assert_eq!(wire["changedFields"], json!(["enabled"]));
        assert_eq!(wire["auditStatus"], "unknown");
        assert!(!wire.to_string().contains("SECRET_"));
    }

    #[test]
    fn history_ipcs_are_registered_without_a_second_write_command_set() {
        let registration = include_str!("../lib.rs");
        for name in [
            "get_tool_risk_policy_changes",
            "clear_tool_risk_policy_changes",
        ] {
            assert!(
                registration.contains(&format!("commands::{name},")),
                "missing IPC registration: {name}"
            );
        }
        for name in [
            "update_tool_risk_settings",
            "save_tool_risk_command_list",
            "delete_tool_risk_command_list",
            "save_custom_tool_risk_rule",
            "delete_custom_tool_risk_rule",
            "toggle_tool_risk_rule",
            "reset_builtin_tool_risk_rule",
        ] {
            assert_eq!(
                registration.matches(&format!("commands::{name},")).count(),
                1
            );
        }
    }
}
