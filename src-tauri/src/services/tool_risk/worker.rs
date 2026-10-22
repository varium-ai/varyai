//! Background tool-observation worker.
//!
//! Parse the bounded queue away from forwarding and evaluate the versioned policy.
//! The forwarding path only uses try_send; this worker handles detection and batched writes.
//! Never persist raw call IDs, full input objects, tool names or rule names.
//! Stored shell summaries retain ordinary content and mask recognized private values;
//! they are not a guarantee of complete anonymity.

use std::sync::Arc;
use std::time::Duration;

use crate::proxy::tool_observation::channel::ObservationReceiver;
use tokio::time::interval;

use crate::database::{Database, ToolRiskEventInput};
use crate::proxy::tool_observation::types::{InputStatus, ToolObservation};

use super::engine::{evaluate, DetectionScope, EngineInput, Verdict};
use super::runtime::get_tool_risk_runtime;

const FLUSH_BATCH: usize = 50;

const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

pub fn project_event(obs: &ToolObservation) -> Option<ToolRiskEventInput> {
    let runtime = get_tool_risk_runtime();
    if !runtime.enabled {
        return None;
    }

    let decision = evaluate(&EngineInput {
        observation: obs,
        rules: &runtime.rules,
        lists: &runtime.command_lists,
    });

    if decision.scope == DetectionScope::OutOfScope {
        return None;
    }

    let (risk_category, severity, reason_code, rule_ids) = if decision.hits.is_empty() {
        (
            None,
            None,
            decision.gray_reason.map(str::to_string),
            Vec::new(),
        )
    } else {
        let mut highest = &decision.hits[0];
        for h in &decision.hits {
            if severity_rank(h.severity) > severity_rank(highest.severity) {
                highest = h;
            }
        }
        (
            Some(highest.risk_category.as_str().to_string()),
            Some(highest.severity.as_str().to_string()),
            Some(highest.reason_code.clone()),
            decision.hits.iter().map(|h| h.rule_id.clone()).collect(),
        )
    };

    let verdict = decision.verdict.map(|v| match v {
        Verdict::Risky => "risky",
        Verdict::Unchecked => "unchecked",
        Verdict::Uncertain => "uncertain",
        Verdict::LowRisk => "low_risk",
    });

    // Only the registered, quality-checked shell adapter may provide a command.
    // Redaction works on this record copy, never on the client payload.
    let command_summary = super::adapters::adapt_observation(obs)
        .operations
        .iter()
        .find(|operation| operation.op_type == super::types::OperationType::ShellExec)
        .and_then(|operation| operation.command.as_deref())
        .map(super::sanitize::summarize_command)
        .filter(|summary| !summary.is_empty());

    // A coverage failure belongs in diagnostics, not an empty per-call audit row.
    // Keep concrete operations and exact-tool risk hits, even without a shell summary.
    if decision.operation_categories.is_empty()
        && command_summary.is_none()
        && decision.hits.is_empty()
    {
        return None;
    }

    Some(ToolRiskEventInput {
        timestamp: now_millis(),
        source: obs.source.as_str().to_string(),
        stage: obs.stage.as_str().to_string(),
        client_category: obs.tool.client.as_deref().and_then(|client| match client {
            "claude" | "claude_code" => Some("claude".into()),
            "codex" | "gemini" | "openai" | "grokbuild" => Some(client.into()),
            _ => None,
        }),
        operation_category: decision
            .operation_categories
            .first()
            .map(|op| op.as_str().to_string()),
        command_summary,
        detection_scope: "in_scope".to_string(),
        integrity_status: integrity_status(obs).to_string(),
        verdict: verdict.map(str::to_string),
        risk_category,
        severity,
        reason_code,
        policy_version: runtime.policy_version as i64,
        rule_ids,
    })
}

/// Fixed states only: never format an observation, event, name, input or call ID.
fn audit_diagnostic(obs: &ToolObservation, event: Option<&ToolRiskEventInput>) -> String {
    let scope = obs
        .scope_id
        .parse::<u64>()
        .map_or_else(|_| "unknown".into(), |id| id.to_string());
    let verdict = match event.and_then(|event| event.verdict.as_deref()) {
        Some("risky") => "risky",
        Some("unchecked") => "unchecked",
        Some("uncertain") => "uncertain",
        Some("low_risk") => "low_risk",
        _ => "none",
    };
    format!("[ToolRiskDiag] scope={scope} phase=audit input={:?} value={} identity={:?} schema={} fidelity={:?} termination={:?} projected={} verdict={verdict} operation={} summary={}",
        obs.input.status, obs.input.value.is_some(), obs.quality.tool_identity,
        obs.tool.definition_ref.is_some(), obs.quality.input_fidelity, obs.quality.source_termination,
        event.is_some(), event.is_some_and(|e| e.operation_category.is_some()),
        event.is_some_and(|e| e.command_summary.is_some()))
}
fn project_event_with_diagnostic(obs: &ToolObservation) -> Option<ToolRiskEventInput> {
    let event = project_event(obs);
    if obs.tool.client.as_deref() == Some("codex") && log::log_enabled!(log::Level::Info) {
        log::info!("{}", audit_diagnostic(obs, event.as_ref()));
    }
    event
}

/// Aggregated ingress losses include rejected first pieces with no worker session.
/// This is not a per-scope first cause; do not infer ordering from these totals.
fn ingress_diagnostic(
    snapshot: &crate::proxy::tool_observation::channel::CounterSnapshot,
    previous: &mut (u64, u64, u64),
) -> Option<String> {
    let current = (
        snapshot.dropped_queue_full,
        snapshot.dropped_closed,
        snapshot.dropped_budget,
    );
    if current == *previous {
        return None;
    }
    *previous = current;
    Some(format!("[ToolRiskDiag] phase=ingress delivered={} dropped_queue_full={} dropped_closed={} dropped_budget={}",
        snapshot.delivered, current.0, current.1, current.2))
}

fn severity_rank(s: super::types::Severity) -> u8 {
    use super::types::Severity::*;
    match s {
        None => 0,
        Medium => 1,
        High => 2,
    }
}

fn integrity_status(obs: &ToolObservation) -> &'static str {
    match obs.input.status {
        InputStatus::Complete => "complete",
        InputStatus::Incomplete => "incomplete",
        InputStatus::Invalid => "invalid",
        InputStatus::Unsupported => "unsupported",
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// One background consumer: decoding, evaluation and bounded batch persistence.
pub fn spawn_worker(db: Arc<Database>, mut rx: ObservationReceiver) {
    tauri::async_runtime::spawn(async move {
        let mut buffer = Vec::with_capacity(FLUSH_BATCH);
        let mut parser = super::response_parser::ResponseProcessor::default();
        let mut ticker = interval(FLUSH_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut policy = super::runtime::collection_policy_version();
        let mut previous_drops = (0, 0, 0);
        loop {
            tokio::select! {
                piece = rx.recv() => {
                    let version = super::runtime::collection_policy_version();
                    if version != policy || !super::runtime::collection_enabled() {
                        parser.clear(); buffer.clear(); policy = version;
                    }
                    match piece {
                        Some(piece) if super::runtime::collection_enabled() && piece.state.policy_version == policy => {
                            for obs in parser.ingest(&piece) {
                                if let Some(event) = project_event_with_diagnostic(&obs) {
                                    if event.policy_version == policy as i64 { buffer.push(event); }
                                }
                                if buffer.len() >= FLUSH_BATCH { flush(&db, &mut buffer).await; }
                            }
                        }
                        Some(piece) => {
                            piece.state.faulted.store(true, std::sync::atomic::Ordering::Release);
                            piece.state.release_reservation();
                        }
                        None => { parser.clear(); flush(&db, &mut buffer).await; break; }
                    }
                }
                _ = ticker.tick() => {
                    let version = super::runtime::collection_policy_version();
                    if version != policy || !super::runtime::collection_enabled() {
                        parser.clear(); buffer.clear(); policy = version;
                    } else {
                        for obs in parser.sweep() {
                            if let Some(event) = project_event_with_diagnostic(&obs) {
                                if event.policy_version == policy as i64 { buffer.push(event); }
                            }
                            if buffer.len() >= FLUSH_BATCH { flush(&db, &mut buffer).await; }
                        }
                        flush(&db, &mut buffer).await;
                    }
                    if let Some(line) = ingress_diagnostic(&rx.counters.snapshot(), &mut previous_drops) {
                        log::info!("{line}");
                    }
                    rx.counters.coverage_fault.store(parser.coverage_faults, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    });
}
async fn flush(db: &Arc<Database>, buffer: &mut Vec<ToolRiskEventInput>) {
    if buffer.is_empty() {
        return;
    }
    let items = std::mem::take(buffer);
    let version = super::runtime::collection_policy_version();
    if !super::runtime::collection_enabled() {
        return;
    }
    let db = db.clone();
    let result =
        tokio::task::spawn_blocking(move || db.insert_current_tool_risk_events_batch(&items)).await;
    match result {
        Ok(Ok(count))
            if count > 0
                && super::runtime::collection_enabled()
                && super::runtime::collection_policy_version() == version =>
        {
            crate::tool_risk_events::notify_tool_risk_recorded()
        }
        Ok(Ok(_)) => {}
        Ok(Err(_)) => log::warn!("[ToolRisk] batch persistence failed (details omitted)"),
        Err(_) => log::warn!("[ToolRisk] persistence task failed (details omitted)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::tool_observation::types::*;
    use crate::services::tool_risk::runtime::{set_tool_risk_runtime_for_test, ToolRiskRuntime};
    use serial_test::serial;
    use std::collections::HashMap;

    pub(super) fn obs(tool: &str, input: serde_json::Value) -> ToolObservation {
        ToolObservation {
            schema_version: 1,
            observation_id: "o1".into(),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: ExecutionScope::Client,
            protocol: "anthropic_messages".into(),
            scope_id: "s".into(),
            correlation: Correlation::default(),
            tool: ToolIdentity {
                client: Some("claude".into()),
                name: tool.into(),
                namespace: None,
                server_id: None,
                definition_ref: Some("builtin:claude:tools:v1".into()),
            },
            input: ToolInput {
                encoding: InputEncoding::Json,
                value: Some(input),
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

    #[test]
    fn diagnostic_ingress_reports_first_piece_loss_only_when_counters_change() {
        use crate::proxy::tool_observation::channel::*;
        let (sender, _receiver) = create_channel(1);
        let mut tap = sender
            .start(
                ResponseMetadata {
                    protocol: "openai_chat",
                    client: "codex",
                    transport: Transport::Sse,
                    quality: OutputQuality::Native,
                    definitions: Arc::new(Vec::new()),
                },
                1,
            )
            .unwrap();
        let mut previous = (0, 0, 0);
        assert!(ingress_diagnostic(&sender.counters().snapshot(), &mut previous).is_none());
        tap.push(&bytes::Bytes::from(vec![0; MAX_PIECE_BYTES + 1]));
        assert_eq!(tap.state.sent.load(std::sync::atomic::Ordering::Acquire), 0);
        let line = ingress_diagnostic(&sender.counters().snapshot(), &mut previous).unwrap();
        assert!(line.contains("phase=ingress"));
        assert!(line.contains("dropped_budget=1"));
        assert!(!line.contains("scope="));
        assert!(ingress_diagnostic(&sender.counters().snapshot(), &mut previous).is_none());
    }

    pub(super) fn enable_runtime() {
        set_tool_risk_runtime_for_test(ToolRiskRuntime::build(
            true,
            7,
            Vec::new(),
            &HashMap::new(),
        ));
    }

    #[test]
    #[serial]
    fn diagnostic_audit_status_never_formats_payload_or_upstream_identity() {
        enable_runtime();
        let mut o = obs(
            "exec_command",
            serde_json::json!({"cmd":"echo PRIVATE_TOKEN"}),
        );
        o.tool.client = Some("codex".into());
        o.tool.definition_ref = Some("builtin:codex:tools:v1".into());
        o.scope_id = "42".into();
        o.correlation.call_id = Some("PRIVATE_CALL_ID".into());
        let before = o.clone();
        let event = project_event(&o);
        let line = audit_diagnostic(&o, event.as_ref());
        assert!(line.contains("[ToolRiskDiag]"));
        assert!(line.contains("scope=42"));
        assert!(line.contains("input=Complete"));
        assert!(line.contains("projected=true"));
        for secret in [
            "PRIVATE_TOKEN",
            "PRIVATE_CALL_ID",
            "echo",
            "exec_command",
            "builtin:codex",
        ] {
            assert!(!line.contains(secret));
        }
        assert_eq!(o, before);
        o.scope_id = "PRIVATE_SCOPE\nINJECTED_LOG".into();
        assert!(!audit_diagnostic(&o, None).contains("PRIVATE_SCOPE"));
        o.scope_id = "42".into();
        o.input.value = None;
        o.input.status = InputStatus::Incomplete;
        o.quality.input_fidelity = InputFidelity::RepairedOrDropped;
        let incomplete = project_event(&o);
        let line = audit_diagnostic(&o, incomplete.as_ref());
        assert!(line.contains("input=Incomplete"));
        assert!(incomplete.is_none());
        assert!(line.contains("projected=false"));
        assert!(line.contains("verdict=none"));
        assert!(line.contains("operation=false"));
        assert!(line.contains("summary=false"));
    }

    #[test]
    #[serial]
    fn regression_empty_shell_observations_do_not_project_audit_rows() {
        enable_runtime();
        for client in ["claude", "codex"] {
            for value in [
                None,
                Some(serde_json::Value::Null),
                Some(serde_json::json!({})),
            ] {
                let mut o = obs("Bash", serde_json::json!({}));
                if client == "codex" {
                    o.tool.client = Some("codex".into());
                    o.tool.name = "exec_command".into();
                    o.tool.definition_ref = Some("builtin:codex:tools:v1".into());
                }
                o.input.value = value;
                o.input.status = InputStatus::Incomplete;
                let decision = evaluate(&EngineInput {
                    observation: &o,
                    rules: &get_tool_risk_runtime().rules,
                    lists: &[],
                });
                assert_eq!(decision.scope, DetectionScope::InScope);
                assert_eq!(decision.verdict, Some(Verdict::Unchecked));
                assert!(
                    project_event(&o).is_none(),
                    "no operation or risk evidence must not create an audit row"
                );
            }
        }
    }

    #[test]
    #[serial]
    fn regression_chat_carrier_fault_preserves_meaningful_shell_audits() {
        use crate::proxy::tool_observation::stream::{StreamAssembler, StreamContext};
        enable_runtime();
        for (command, verdict) in [("echo hi", "unchecked"), ("rm -rf /tmp/example/*", "risky")] {
            for carrier in [
                serde_json::Value::Null,
                serde_json::json!({}),
                serde_json::json!("bad"),
                serde_json::json!(42),
                serde_json::json!(false),
            ] {
                let mut a = StreamAssembler::new(
                    "openai_chat",
                    StreamContext {
                        scope_id: "42".into(),
                        client: Some("codex".into()),
                        execution_scope: ExecutionScope::Client,
                    },
                );
                let event = serde_json::json!({"choices":[{"delta":{"tool_calls":carrier}}]});
                assert!(a.feed(&event).is_empty());
                a.feed(&serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                    "function":{"name":"exec_command","arguments":serde_json::json!({"cmd":command}).to_string()}}]}}]}));
                let mut observations =
                    a.feed(&serde_json::json!({"choices":[{"finish_reason":"tool_calls"}]}));
                assert_eq!(observations.len(), 1);
                let o = &mut observations[0];
                o.tool.definition_ref = Some("builtin:codex:tools:v1".into());
                let event = project_event(o).expect("real shell input must remain auditable");
                assert_eq!(event.command_summary.as_deref(), Some(command));
                assert_eq!(event.operation_category.as_deref(), Some("shell_exec"));
                let expected = if carrier.is_null() && verdict == "unchecked" {
                    "low_risk"
                } else {
                    verdict
                };
                assert_eq!(event.verdict.as_deref(), Some(expected));
            }
        }
    }

    #[test]
    #[serial]
    fn regression_exact_tool_risk_hit_survives_without_operation_or_summary() {
        use super::super::types::{ConditionOp, RuleCondition, RuleOrigin, RuleTarget};
        let o = obs("custom_tool", serde_json::json!({"action":"danger"}));
        let mut rule = super::super::builtin::builtin_rules().remove(1);
        rule.id = "custom.explicit-hit".into();
        rule.origin = RuleOrigin::Custom;
        rule.selector.target =
            serde_json::from_value::<RuleTarget>(serde_json::json!({"kind":"tool","tool_refs":[{
                "client":o.tool.client,"name":o.tool.name,"namespace":o.tool.namespace,
                "definitionRef":o.tool.definition_ref,"serverIdentity":o.tool.server_id
            }]}))
            .unwrap();
        rule.conditions = vec![RuleCondition {
            field: "/action".into(),
            op: ConditionOp::Eq,
            value: Some("danger".into()),
            values: None,
        }];
        rule.validate().unwrap();
        set_tool_risk_runtime_for_test(ToolRiskRuntime::build(
            true,
            7,
            vec![rule],
            &HashMap::new(),
        ));
        let event = project_event(&o).expect("exact-tool risk evidence must be retained");
        assert_eq!(event.verdict.as_deref(), Some("risky"));
        assert!(event.operation_category.is_none());
        assert!(event.command_summary.is_none());
        assert_eq!(event.rule_ids, vec!["custom.explicit-hit"]);
    }

    #[test]
    #[serial]
    fn disabled_runtime_projects_nothing() {
        set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        let o = obs("Bash", serde_json::json!({"command": "rm -rf /"}));
        assert!(project_event(&o).is_none());
    }

    #[test]
    #[serial]
    fn risky_call_projects_sanitized_event() {
        enable_runtime();
        let o = obs(
            "Bash",
            serde_json::json!({"command": "rm -rf /PRIVATE_TARGET/*"}),
        );
        let ev = project_event(&o).expect("in-scope risky event");
        assert_eq!(ev.detection_scope, "in_scope");
        assert_eq!(ev.verdict.as_deref(), Some("risky"));
        assert_eq!(ev.policy_version, 7);
        assert!(!ev.rule_ids.is_empty());
        assert_eq!(ev.operation_category.as_deref(), Some("shell_exec"));
        // Ordinary command paths remain readable; sensitive values alone are hidden.
        assert_eq!(
            ev.command_summary.as_deref(),
            Some("rm -rf /PRIVATE_TARGET/*")
        );
    }

    #[test]
    #[serial]
    fn command_summary_hides_credentials_but_preserves_command_structure() {
        enable_runtime();
        let command = format!(
            "{}={} curl -H '{}: {} {}' https://api.x/public",
            "API_KEY", "TOKENVALUE", "Authorization", "Bearer", "OTHERSECRET"
        );
        let expected = format!(
            "{}={} curl -H '{}: {} {}' https://api.x/public",
            "API_KEY", "<redacted>", "Authorization", "Bearer", "<redacted>"
        );
        assert!(command.contains("TOKENVALUE"));
        let o = obs("Bash", serde_json::json!({"command": command}));
        let ev = project_event(&o).expect("in-scope event");
        assert_eq!(ev.command_summary.as_deref(), Some(expected.as_str()));
        let output = serde_json::to_string(&ev).unwrap();
        for secret in ["TOKENVALUE", "OTHERSECRET"] {
            assert!(!output.contains(secret));
        }
        assert_eq!(o.input.value.as_ref().unwrap()["command"], command);
    }

    #[test]
    #[serial]
    fn command_summary_uses_registered_shell_fields_only() {
        enable_runtime();
        let mut codex = obs(
            "exec_command",
            serde_json::json!({"cmd": "ls -la /workspace/project"}),
        );
        codex.tool.client = Some("codex".into());
        codex.tool.definition_ref = Some("builtin:codex:tools:v1".into());
        assert_eq!(
            project_event(&codex).unwrap().command_summary.as_deref(),
            Some("ls -la /workspace/project")
        );
        codex.quality.tool_identity = ToolIdentityQuality::Unknown;
        if let Some(event) = project_event(&codex) {
            assert!(event.command_summary.is_none());
        }
        let read = obs(
            "Read",
            serde_json::json!({"file_path": "/workspace/project", "command": "SHOULD_NOT_BE_USED"}),
        );
        assert!(project_event(&read).unwrap().command_summary.is_none());
    }

    #[test]
    #[serial]
    fn projected_command_survives_database_and_ipc_with_privacy_values_hidden() {
        let db = Database::memory().unwrap();
        db.save_tool_risk_settings(&crate::database::ToolRiskSettings {
            enabled: true,
            policy_version: 0,
        })
        .unwrap();
        let policy = db.get_tool_risk_settings().unwrap().policy_version;
        set_tool_risk_runtime_for_test(ToolRiskRuntime::build(
            true,
            policy,
            Vec::new(),
            &HashMap::new(),
        ));
        let private_command = format!(
            "{}={} curl https://example.com/public | cat",
            "API_KEY", "PIPELINE_PRIVATE_VALUE"
        );
        let hidden_command = format!(
            "{}={} curl https://example.com/public | cat",
            "API_KEY", "<redacted>"
        );
        for (command, expected) in [
            ("ls -la /workspace/project", "ls -la /workspace/project"),
            (
                "unknown-program --public-option=value /workspace/project | cat",
                "unknown-program --public-option=value /workspace/project | cat",
            ),
            (private_command.as_str(), hidden_command.as_str()),
        ] {
            let observation = obs("Bash", serde_json::json!({"command": command}));
            let event = project_event(&observation).expect("registered shell is in scope");
            assert_eq!(event.command_summary.as_deref(), Some(expected));
            assert_eq!(
                db.insert_current_tool_risk_events_batch(&[event]).unwrap(),
                1
            );
            let page = db.get_tool_risk_events(1, 0).unwrap();
            assert_eq!(page[0].command_summary.as_deref(), Some(expected));
            let ipc = serde_json::to_value(&page[0]).unwrap();
            assert_eq!(ipc["commandSummary"], expected);
            assert!(!ipc.to_string().contains("PIPELINE_PRIVATE_VALUE"));
            let notifications = db
                .get_tool_risk_events_after(page[0].id - 1, 1, policy)
                .unwrap();
            assert_eq!(notifications[0].command_summary.as_deref(), Some(expected));
            db.get_tool_risk_configuration().unwrap();
            assert_eq!(
                db.get_tool_risk_events(1, 0).unwrap()[0]
                    .command_summary
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(
                observation.input.value.as_ref().unwrap()["command"],
                command
            );
        }
        set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
    }

    #[test]
    #[serial]
    fn out_of_scope_projects_nothing() {
        enable_runtime();
        let o = obs("TotallyUnknownTool", serde_json::json!({"foo": "bar"}));
        assert!(project_event(&o).is_none());
    }

    #[test]
    #[serial]
    fn incomplete_input_without_static_hit_yields_unchecked() {
        enable_runtime();

        let mut o = obs("Bash", serde_json::json!({"command": "echo hi"}));
        o.input.status = InputStatus::Incomplete;
        let ev = project_event(&o).expect("in-scope event");
        assert_eq!(ev.integrity_status, "incomplete");
        assert_eq!(ev.verdict.as_deref(), Some("unchecked"));
    }

    #[test]
    #[serial]
    fn static_hit_outranks_broken_input() {
        enable_runtime();

        let mut o = obs(
            "Bash",
            serde_json::json!({"command": "rm -rf /PRIVATE_TARGET/*"}),
        );
        o.input.status = InputStatus::Incomplete;
        let ev = project_event(&o).expect("in-scope event");
        assert_eq!(ev.integrity_status, "incomplete");
        assert_eq!(ev.verdict.as_deref(), Some("risky"));
    }
    #[tokio::test]
    #[serial]
    async fn rejected_queued_first_piece_releases_active_budget() {
        use crate::proxy::tool_observation::channel::*;
        use bytes::Bytes;
        for enabled in [false, true] {
            set_tool_risk_runtime_for_test(ToolRiskRuntime {
                enabled,
                policy_version: 8,
                rules: Vec::new(),
                command_lists: Vec::new(),
            });
            let (tx, rx) = create_channel(32);
            let metadata = ResponseMetadata {
                protocol: "anthropic_messages",
                client: "claude",
                transport: Transport::Sse,
                quality: OutputQuality::Native,
                definitions: Arc::new(Vec::new()),
            };
            let mut taps = Vec::new();
            for _ in 0..(MAX_ACTIVE_BYTES / RESPONSE_RESERVATION) {
                let mut tap = tx.start(metadata.clone(), 7).unwrap();
                tap.push(&Bytes::from_static(b": ping\n\n"));
                taps.push(tap);
            }
            assert!(tx.start(metadata.clone(), 8).is_none());
            spawn_worker(Arc::new(Database::memory().unwrap()), rx);
            let mut reclaimed = false;
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if tx.start(metadata.clone(), 8).is_some() {
                    reclaimed = true;
                    break;
                }
            }
            assert!(
                reclaimed,
                "enabled={enabled}: queued stale response kept reservation while tap alive"
            );
            drop(taps);
        }
        set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
    }
}

#[cfg(test)]
mod classification_projection_tests {
    use super::*;
    #[test]
    #[serial_test::serial]
    fn gray_script_reason_is_projected_without_raw_input() {
        super::tests::enable_runtime();
        let o = super::tests::obs(
            "Bash",
            serde_json::json!({"command":"python3 -c 'secret_script()'"}),
        );
        let e = project_event(&o).unwrap();
        assert_eq!(e.verdict.as_deref(), Some("uncertain"));
        assert_eq!(e.reason_code.as_deref(), Some("script_exec"));
    }
}

#[cfg(test)]
mod mixed_hit_regressions {
    use super::super::{classifier::*, runtime::*, types::*};
    use super::tests::obs;
    use super::*;
    use std::collections::HashMap;
    #[test]
    #[serial_test::serial]
    fn event_labels_follow_highest_risk_hit_and_keep_all_rule_ids() {
        let list = CommandListEntry {
            id: "shell-list-medium".into(),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Program,
            pattern: "cat".into(),
            enabled: true,
            severity: Severity::Medium,
            risk_category: RiskCategory::DestructiveChange,
        };
        let mut runtime = ToolRiskRuntime::build(true, 1, vec![], &HashMap::new());
        runtime.command_lists = vec![list];
        set_tool_risk_runtime_for_test(runtime);
        let event = project_event(&obs("Bash", serde_json::json!({"command":"cat .env"}))).unwrap();
        set_tool_risk_runtime_for_test(ToolRiskRuntime::default());
        assert_eq!(event.severity.as_deref(), Some("high"));
        assert_eq!(event.risk_category.as_deref(), Some("sensitive_read"));
        assert_eq!(event.reason_code.as_deref(), Some("sensitive_target"));
        assert!(event.rule_ids.contains(&"shell-list-medium".into()));
        assert!(event.rule_ids.contains(&"builtin.sensitive_read".into()));
    }
}
