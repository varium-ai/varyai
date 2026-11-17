//! Passive native shell reports. Never infer execution from ordinary message text.
use crate::proxy::tool_observation::{native::doubao_report_observation, types::ToolObservation};
use serde_json::Value;
use std::collections::HashMap;

const MAX_LINE: usize = 1024 * 1024;
const MAX_BLOCKS: usize = 64;
const MAX_COMMAND: usize = 65536;

#[derive(Default)]
struct Block {
    shell: bool,
    command: Option<String>,
    finished: bool,
    emitted: bool,
}

#[derive(Default)]
pub(super) struct Reports {
    line: Vec<u8>,
    session: Option<String>,
    message: Option<String>,
    blocks: HashMap<(String, String), Block>,
    failed: bool,
}

impl Reports {
    pub(super) fn feed(&mut self, bytes: &[u8]) -> Vec<ToolObservation> {
        let mut out = Vec::new();
        if self.failed {
            return out;
        }
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            if self.line.len() + piece.len() > MAX_LINE {
                self.failed = true;
                self.line.clear();
                self.blocks.clear();
                break;
            }
            self.line.extend_from_slice(piece);
            if piece.last() == Some(&b'\n') {
                let line = std::mem::take(&mut self.line);
                let Ok(text) = std::str::from_utf8(&line) else {
                    continue;
                };
                let Some(data) = text.trim().strip_prefix("data:") else {
                    continue;
                };
                if let Ok(event) = serde_json::from_str::<Value>(data.trim()) {
                    self.event(&event, &mut out);
                    if self.failed {
                        break;
                    }
                }
            }
        }
        out
    }

    fn event(&mut self, event: &Value, out: &mut Vec<ToolObservation>) {
        use crate::proxy::tool_observation::types::normalized_session_id;
        for meta in [&event["ack_client_meta"], &event["meta"]] {
            if let Some(id) = normalized_session_id(meta["conversation_id"].as_str()) {
                if self.session.as_ref().is_some_and(|old| old != &id) {
                    self.failed = true;
                    self.blocks.clear();
                    return;
                }
                self.session = Some(id);
            }
            if let Some(id) = normalized_session_id(meta["message_id"].as_str()) {
                self.message = Some(id);
            }
        }
        if let Some(id) = normalized_session_id(event["message_id"].as_str()) {
            self.message = Some(id);
        }
        // Only live response content/patches, never echoed messages or historical snapshots.
        self.content(&event["content"], 2, out);
        if let Some(patches) = event["patch_op"].as_array() {
            for patch in patches.iter().take(128) {
                if patch["patch_object"] == 1 {
                    if let Some(kind @ 1..=3) = patch["patch_type"].as_u64() {
                        self.content(&patch["patch_value"], kind, out);
                    }
                }
            }
        }
    }

    fn content(&mut self, content: &Value, kind: u64, out: &mut Vec<ToolObservation>) {
        use crate::proxy::tool_observation::types::normalized_session_id;
        let (Some(session), Some(message), Some(blocks)) = (
            self.session.as_deref(),
            self.message.as_deref(),
            content["content_block"].as_array(),
        ) else {
            return;
        };
        for value in blocks.iter().take(MAX_BLOCKS) {
            if value["block_type"] != 10019 {
                continue;
            }
            let Some(id) = normalized_session_id(value["block_id"].as_str()) else {
                continue;
            };
            let key = (message.to_owned(), id.clone());
            if !self.blocks.contains_key(&key) && self.blocks.len() >= MAX_BLOCKS {
                continue;
            }
            let kind = value["patch_type"].as_u64().unwrap_or(kind);
            if kind == 3 {
                self.blocks.remove(&key);
                continue;
            }
            if !matches!(kind, 1 | 2) {
                continue;
            }
            let block = self.blocks.entry(key).or_default();
            if block.emitted {
                continue;
            }
            if kind == 2 {
                *block = Block::default();
            }
            let report = &value["content"]["file_operation_block"];
            // These fields distinguish the native terminal card from file/text/code blocks.
            if report.get("file_type").is_some() {
                block.shell = report["file_type"] == "shell"
                    && report["file_content_type"] == "shell"
                    && report["operation_type"] == 1
                    && report["scene"] == 1;
            }
            if let Some(command) = report["display_content"]["operation"].as_str() {
                block.command = (!command.trim().is_empty() && command.len() <= MAX_COMMAND)
                    .then(|| command.to_owned());
            }
            if ["path", "file_name"].iter().any(|field| {
                report[*field]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            }) {
                block.shell = false;
                block.command = None;
            }
            if value["is_finish"] == true || value["is_finish"] == "1" {
                block.finished = true;
            }
            if block.shell && block.finished {
                if let Some(command) = block.command.take() {
                    if let Some(observation) =
                        doubao_report_observation(session, message, &id, &command)
                    {
                        block.emitted = true;
                        out.push(observation);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn frame(value: Value) -> Vec<u8> {
        format!("data: {value}\n\n").into_bytes()
    }
    fn metadata() -> Value {
        json!({"meta":{"conversation_id":"synthetic-session","message_id":"synthetic-message"}})
    }
    fn report(finish: bool) -> Value {
        json!({"patch_op":[{"patch_object":1,"patch_type":1,"patch_value":{"content_block":[{"block_type":10019,"block_id":"synthetic-call","is_finish":finish,"content":{"file_operation_block":{"file_type":"shell","file_content_type":"shell","operation_type":1,"scene":1,"display_content":{"operation":"kubectl delete pod synthetic --dry-run=client","result":"private result must not escape"}}}}]}}]})
    }

    #[test]
    fn fragmented_native_report_preserves_command_session_and_reported_stage() {
        let bytes = [frame(metadata()), frame(report(true)), frame(report(true))].concat();
        let mut parser = Reports::default();
        let events: Vec<_> = bytes.chunks(7).flat_map(|part| parser.feed(part)).collect();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.source.as_str(), "client_report");
        assert_eq!(event.stage.as_str(), "reported");
        assert_eq!(
            event.correlation.session_id.as_deref(),
            Some("doubaowork_synthetic-session")
        );
        assert_eq!(
            event.input.value,
            Some(json!({"command":"kubectl delete pod synthetic --dry-run=client"}))
        );
        assert!(
            !crate::services::tool_risk::adapters::adapt_observation(event)
                .operations
                .is_empty()
        );
        let mut wrong_stage = event.clone();
        wrong_stage.stage = crate::proxy::tool_observation::types::ObservationStage::PreExecution;
        assert!(wrong_stage.validate().is_err());
    }

    #[test]
    fn waits_for_finish_and_never_uses_prose_or_echoed_history() {
        let mut parser = Reports::default();
        parser.feed(&frame(metadata()));
        assert!(parser.feed(&frame(report(false))).is_empty());
        assert!(parser
            .feed(&frame(
                json!({"message":report(true),"text":"kubectl delete pod synthetic"})
            ))
            .is_empty());
        let finish = json!({"content":{"content_block":[{"block_type":10019,"block_id":"synthetic-call","patch_type":1,"is_finish":true}]}});
        assert_eq!(parser.feed(&frame(finish)).len(), 1);
    }

    #[test]
    fn terminal_finish_marker_is_supported_but_shell_file_views_are_excluded() {
        let mut parser = Reports::default();
        parser.feed(&frame(metadata()));
        let mut value = report(true);
        value["patch_op"][0]["patch_value"]["content_block"][0]["is_finish"] = json!("1");
        assert_eq!(parser.feed(&frame(value)).len(), 1);
        for field in ["path", "file_name"] {
            let mut parser = Reports::default();
            parser.feed(&frame(metadata()));
            let mut value = report(true);
            value["patch_op"][0]["patch_value"]["content_block"][0]["content"]
                ["file_operation_block"][field] = json!("synthetic.sh");
            assert!(parser.feed(&frame(value)).is_empty());
        }
    }

    #[test]
    fn replacement_and_deletion_cannot_reuse_an_earlier_command() {
        for kind in [2, 3] {
            let mut parser = Reports::default();
            parser.feed(&frame(metadata()));
            parser.feed(&frame(report(false)));
            let update = json!({"patch_op":[{"patch_object":1,"patch_type":kind,"patch_value":{"content_block":[{"block_type":10019,"block_id":"synthetic-call","is_finish":true}]}}]});
            assert!(parser.feed(&frame(update)).is_empty());
        }
    }

    #[test]
    fn malformed_unsupported_and_oversized_reports_produce_no_observation() {
        for field in ["file_type", "scene", "file_content_type", "operation_type"] {
            let mut parser = Reports::default();
            parser.feed(&frame(metadata()));
            let mut value = report(true);
            value["patch_op"][0]["patch_value"]["content_block"][0]["content"]
                ["file_operation_block"][field] = Value::Null;
            assert!(parser.feed(&frame(value)).is_empty());
        }
        let mut parser = Reports::default();
        assert!(parser.feed(&frame(report(true))).is_empty());
        parser.feed(&frame(metadata()));
        assert!(parser.feed(&vec![b'x'; MAX_LINE + 1]).is_empty());
        assert!(parser.feed(&frame(report(true))).is_empty());
        let mut parser = Reports::default();
        parser.feed(&frame(metadata()));
        parser.feed(&frame(
            json!({"meta":{"conversation_id":"different-session"}}),
        ));
        assert!(parser.feed(&frame(report(true))).is_empty());
    }
}
