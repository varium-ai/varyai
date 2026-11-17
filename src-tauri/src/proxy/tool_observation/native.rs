//! Trusted native hooks retain their pre-execution provenance in the bounded worker queue.
use super::types::*;
use serde_json::Value;

pub(crate) fn trae_observation(client: &'static str, value: &Value) -> Option<ToolObservation> {
    use crate::services::tool_risk::adapters::{TRAE_INTL_TOOLS_SCHEMA, TRAE_TOOLS_SCHEMA};
    let schema = match client {
        "trae" => TRAE_TOOLS_SCHEMA,
        "trae-intl" => TRAE_INTL_TOOLS_SCHEMA,
        _ => return None,
    };
    hook_observation(client, schema, "trae_pre_tool_use", "RunCommand", value)
}

pub(crate) fn doubao_observation(value: &Value) -> Option<ToolObservation> {
    hook_observation(
        "doubaowork",
        crate::services::tool_risk::adapters::DOUBAO_TOOLS_SCHEMA,
        "doubao_pre_tool_use",
        "Bash",
        value,
    )
}

fn hook_observation(
    client: &'static str,
    schema: &'static str,
    protocol: &str,
    name: &str,
    value: &Value,
) -> Option<ToolObservation> {
    if value["hook_event_name"] != "PreToolUse" || value["tool_name"] != name {
        return None;
    }
    let input = value.get("tool_input").filter(|input| input.is_object())?;
    let command = input.get("command")?.as_str()?;
    if command.trim().is_empty() || bounded_input_size(input).is_err() {
        return None;
    }
    let call = normalized_session_id(value["tool_use_id"].as_str());
    let scope = uuid::Uuid::new_v4().to_string();
    let observation = ToolObservation {
        schema_version: 1,
        observation_id: scope.clone(),
        source: ObservationSource::ClientHook,
        stage: ObservationStage::PreExecution,
        execution_scope: ExecutionScope::Client,
        protocol: protocol.into(),
        scope_id: scope,
        correlation: Correlation {
            session_id: normalized_session_id(value["session_id"].as_str()).and_then(|id| {
                match crate::services::trae_bridge::shared::Edition::parse(client) {
                    Some(edition) => edition.session_id(&id),
                    None => normalized_session_id(Some(&format!("{client}_{id}"))),
                }
            }),
            identity_quality: call.as_ref().map(|_| IdentityQuality::SourceId),
            call_id: call,
            ..Default::default()
        },
        tool: ToolIdentity {
            client: Some(client.into()),
            name: name.into(),
            namespace: None,
            server_id: None,
            definition_ref: Some(schema.into()),
        },
        input: ToolInput {
            encoding: InputEncoding::Json,
            value: Some(input.clone()),
            status: InputStatus::Complete,
        },
        quality: ObservationQuality {
            origin: QualityOrigin::Native,
            source_termination: SourceTermination::Normal,
            tool_identity: ToolIdentityQuality::Source,
            input_fidelity: InputFidelity::Preserved,
        },
        context: ObservationContext::default(),
    };
    observation.validate().ok()?;
    Some(observation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_provenance_and_supplied_session_are_preserved_for_both_editions() {
        let value = json!({"hook_event_name":"PreToolUse","tool_name":"RunCommand","tool_use_id":"synthetic-call","session_id":"synthetic-session","tool_input":{"command":"kubectl delete pod synthetic --dry-run=client"},"client":"codex"});
        for (client, prefix) in [("trae", "trae_cn"), ("trae-intl", "trae")] {
            let observation = trae_observation(client, &value).unwrap();
            assert_eq!(observation.source, ObservationSource::ClientHook);
            assert_eq!(observation.stage, ObservationStage::PreExecution);
            assert_eq!(observation.tool.client.as_deref(), Some(client));
            assert_eq!(
                observation.correlation.session_id,
                Some(format!("{prefix}_synthetic-session"))
            );
            assert_eq!(
                observation.correlation.call_id.as_deref(),
                Some("synthetic-call")
            );
            assert!(
                crate::services::tool_risk::adapters::adapt_observation(&observation).recognized
            );
        }
    }

    #[test]
    fn post_execution_text_and_unknown_clients_cannot_claim_pre_execution() {
        let mut value = json!({"hook_event_name":"PreToolUse","tool_name":"RunCommand","tool_input":{"command":"echo synthetic"}});
        assert!(trae_observation("codex", &value).is_none());
        value["hook_event_name"] = json!("PostToolUse");
        assert!(trae_observation("trae", &value).is_none());
        value["hook_event_name"] = json!("PreToolUse");
        value["tool_name"] = json!("mcp__synthetic__RunCommand");
        assert!(trae_observation("trae", &value).is_none());
        value["tool_name"] = json!("RunCommand");
        value["tool_input"]["command"] = json!(["echo"]);
        assert!(trae_observation("trae", &value).is_none());
    }

    #[test]
    fn absent_or_invalid_session_is_not_replaced_by_call_or_scope_id() {
        let value = json!({"hook_event_name":"PreToolUse","tool_name":"RunCommand","session_id":"has space","tool_use_id":"synthetic-call","tool_input":{"command":"echo synthetic"}});
        let observation = trae_observation("trae", &value).unwrap();
        assert_eq!(observation.correlation.session_id, None);
        assert_eq!(
            observation.correlation.call_id.as_deref(),
            Some("synthetic-call")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn native_hook_uses_the_bounded_queue_and_sanitized_persistence() {
        use crate::services::tool_risk::{
            response_parser::ResponseProcessor, runtime::*, worker::project_event,
        };
        struct Restore(ToolRiskRuntime);
        impl Drop for Restore {
            fn drop(&mut self) {
                publish_tool_risk_runtime(self.0.clone());
            }
        }
        let _restore = Restore((*get_tool_risk_runtime()).clone());
        let db = crate::database::Database::memory().unwrap();
        crate::services::tool_risk::service::update_settings(
            &db,
            &crate::database::ToolRiskSettings {
                enabled: true,
                policy_version: 0,
            },
        )
        .unwrap();
        let policy = collection_policy_version();
        let (_, mut receiver) = super::super::channel::test_channel().await;
        let mut parser = ResponseProcessor::default();
        for client in ["trae", "trae-intl", "doubaowork"] {
            let name = if client == "doubaowork" {
                "Bash"
            } else {
                "RunCommand"
            };
            let value = json!({"hook_event_name":"PreToolUse","tool_name":name,"session_id":"synthetic-session","tool_use_id":"synthetic-call","tool_input":{"command":"chmod 4755 /synthetic/fixture"}});
            let observation = if client == "doubaowork" {
                doubao_observation(&value)
            } else {
                trae_observation(client, &value)
            };
            super::super::channel::observe_native(observation.unwrap());
            let piece = tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            let observations = parser.ingest(&piece);
            assert_eq!(observations.len(), 1);
            let event = project_event(&observations[0]).unwrap();
            assert_eq!(event.source, "client_hook");
            assert_eq!(event.stage, "pre_execution");
            assert_eq!(event.client_category.as_deref(), Some(client));
            assert_eq!(event.verdict.as_deref(), Some("risky"));
            crate::services::tool_risk::service::persist_events_for_generation(
                &db,
                &[event],
                collection_generation(),
            )
            .unwrap();
        }
        let events = db.get_tool_risk_events_after(0, 10, policy).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event
            .session_id
            .as_deref()
            .is_some_and(|session| session.ends_with("_synthetic-session"))));
        publish_tool_risk_runtime(ToolRiskRuntime::default());
        super::super::channel::observe_native(trae_observation("trae", &json!({"hook_event_name":"PreToolUse","tool_name":"RunCommand","tool_input":{"command":"echo synthetic"}})).unwrap());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), receiver.recv())
                .await
                .is_err()
        );
    }
}
