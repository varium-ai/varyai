//! Background-only response transport decoding. No payload-bearing diagnostics.
use crate::proxy::tool_observation::channel::{
    OutputQuality, RawPiece, ResponseState, Transport, MAX_PIECE_BYTES,
};
use crate::proxy::tool_observation::parsers::{parse_response, ParseContext};
use crate::proxy::tool_observation::stream::{StreamAssembler, StreamContext};
use crate::proxy::tool_observation::types::*;
use std::collections::BTreeMap;
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

const IDLE_TTL: Duration = Duration::from_secs(120);
const MAX_TTL: Duration = Duration::from_secs(600);
struct Session {
    state: Arc<ResponseState>,
    expected: u64,
    born: Instant,
    last: Instant,
    bytes: Vec<u8>,
    assembler: StreamAssembler,
}
impl Session {
    fn log_degraded(&self) {
        if let Some(reason) = self.assembler.diagnostics().first_reason {
            log_fault(
                &self.state,
                reason,
                self.expected.saturating_sub(1),
                self.assembler.diagnostics().first_line.unwrap_or(0),
            );
        }
    }
    fn log_end(&self) {
        if self.state.metadata.client == "codex" {
            let diagnostics = self.assembler.diagnostics();
            log::info!("[ToolRiskDiag] scope={} phase=end reason={} degraded={} faulted={} expected={} sent={} invalid_event={} missing_delta={} budget_exceeded={} conflicting_snapshot={}",
                self.state.id, diagnostics.first_reason.unwrap_or("none"), self.assembler.is_degraded(),
                self.state.faulted.load(Ordering::Acquire), self.expected, self.state.sent.load(Ordering::Acquire),
                diagnostics.invalid_event, diagnostics.missing_delta, diagnostics.budget_exceeded, diagnostics.conflicting_snapshot);
        }
    }
}
fn log_fault(state: &ResponseState, reason: &'static str, sequence: u64, line: u32) {
    if state.metadata.client == "codex" && state.take_fault_diagnostic() {
        log::info!(
            "[ToolRiskDiag] scope={} phase=degraded reason={} sequence={} site_line={}",
            state.id,
            reason,
            sequence,
            line
        );
    }
}
#[derive(Default)]
pub struct ResponseProcessor {
    sessions: BTreeMap<u64, Session>,
    pub coverage_faults: u64,
}
impl ResponseProcessor {
    pub fn clear(&mut self) {
        for session in self.sessions.values() {
            session.state.faulted.store(true, Ordering::Release);
            session.state.release_reservation();
        }
        self.sessions.clear();
    }
    pub fn ingest(&mut self, piece: &RawPiece) -> Vec<ToolObservation> {
        let state = &piece.state;
        let expected = self
            .sessions
            .get(&state.id)
            .map_or(0, |session| session.expected);
        if state.faulted.load(Ordering::Acquire) || piece.sequence != expected {
            state.faulted.store(true, Ordering::Release);
            self.coverage_faults += 1;
            let result = if let Some(mut session) = self.sessions.remove(&state.id) {
                session.bytes = Vec::new();
                session
                    .assembler
                    .mark_degraded_with_reason(if piece.sequence != expected {
                        "sequence_gap"
                    } else {
                        state.fault_reason()
                    });
                session.log_degraded();
                let observations = qualify(session.assembler.finish(), state);
                session.log_end();
                observations
            } else {
                log_fault(
                    state,
                    if piece.sequence != expected {
                        "sequence_gap"
                    } else {
                        state.fault_reason()
                    },
                    piece.sequence,
                    0,
                );
                Vec::new()
            };
            state.release_reservation();
            return result;
        }
        if state.metadata.transport == Transport::Json {
            let Ok(body) = serde_json::from_slice(&piece.bytes) else {
                log_fault(state, "invalid_json", piece.sequence, 0);
                self.coverage_faults += 1;
                return Vec::new();
            };
            let ctx = ParseContext {
                scope_id: state.id.to_string(),
                client: Some(state.metadata.client.into()),
                execution_scope: ExecutionScope::Client,
            };
            let result = parse_response(state.metadata.protocol, &body, &ctx);
            if !result.recognized {
                log_fault(state, "unrecognized_json", piece.sequence, 0);
                self.coverage_faults += 1;
            }
            return qualify(result.observations, state);
        }
        let now = Instant::now();
        let session = self.sessions.entry(state.id).or_insert_with(|| {
            if state.metadata.client == "codex" {
                let protocol = match state.metadata.protocol {
                    "openai_chat" => "openai_chat",
                    "openai_responses" => "openai_responses",
                    _ => "other",
                };
                let quality = if state.metadata.quality == OutputQuality::Native {
                    "native"
                } else {
                    "converted_unknown"
                };
                log::info!(
                    "[ToolRiskDiag] scope={} phase=start protocol={} quality={} definitions={}",
                    state.id,
                    protocol,
                    quality,
                    state.metadata.definitions.len()
                );
            }
            Session {
                state: state.clone(),
                expected: 0,
                born: now,
                last: now,
                bytes: Vec::new(),
                assembler: StreamAssembler::new(
                    state.metadata.protocol,
                    StreamContext {
                        scope_id: state.id.to_string(),
                        client: Some(state.metadata.client.into()),
                        execution_scope: ExecutionScope::Client,
                    },
                ),
            }
        });
        session.last = now;
        session.expected = piece.sequence.saturating_add(1);
        let mut observations = Vec::new();
        // Process delimiters as bytes first, so split UTF-8 is never lossy-decoded.
        // Do not allocate more than one bounded SSE event, even for a huge chunk.
        for &byte in piece.bytes.iter() {
            if session.bytes.len() >= MAX_PIECE_BYTES {
                session.state.faulted.store(true, Ordering::Release);
                session
                    .assembler
                    .mark_degraded_with_reason("sse_frame_budget");
                session.bytes = Vec::new();
                session.log_degraded();
                self.coverage_faults += 1;
                break;
            }
            session.bytes.push(byte);
            let delimiter = if session.bytes.ends_with(b"\r\n\r\n") {
                4
            } else if session.bytes.ends_with(b"\n\n") {
                2
            } else {
                0
            };
            if delimiter == 0 {
                continue;
            }
            let len = session.bytes.len() - delimiter;
            match std::str::from_utf8(&session.bytes[..len]) {
                Ok(block) => {
                    let data = block
                        .lines()
                        .filter_map(|line| {
                            line.strip_prefix("data:")
                                .map(|v| v.strip_prefix(' ').unwrap_or(v))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !data.is_empty() && data.trim() != "[DONE]" {
                        match serde_json::from_str(&data) {
                            Ok(event) => {
                                observations.extend(session.assembler.feed(&event));
                                session.log_degraded();
                            }
                            Err(_) => {
                                session.assembler.mark_degraded_with_reason("invalid_json");
                                session.log_degraded();
                                self.coverage_faults += 1;
                            }
                        }
                    }
                }
                Err(_) => {
                    session.assembler.mark_degraded_with_reason("invalid_utf8");
                    session.log_degraded();
                    self.coverage_faults += 1;
                }
            }
            session.bytes = Vec::new();
        }
        qualify(observations, state)
    }
    pub fn sweep(&mut self) -> Vec<ToolObservation> {
        self.sweep_at(Instant::now())
    }
    fn sweep_at(&mut self, now: Instant) -> Vec<ToolObservation> {
        let ids: Vec<u64> = self
            .sessions
            .iter()
            .filter_map(|(&id, session)| {
                let expired = now.duration_since(session.last) >= IDLE_TTL
                    || now.duration_since(session.born) >= MAX_TTL;
                let closed = session.state.closed.load(Ordering::Acquire)
                    && (session.expected >= session.state.sent.load(Ordering::Acquire)
                        || session.state.faulted.load(Ordering::Acquire));
                (expired || closed || session.state.faulted.load(Ordering::Acquire)).then_some(id)
            })
            .collect();
        let mut result = Vec::new();
        for id in ids {
            if let Some(mut session) = self.sessions.remove(&id) {
                if !session.bytes.is_empty()
                    || session.state.faulted.load(Ordering::Acquire)
                    || now.duration_since(session.last) >= IDLE_TTL
                    || now.duration_since(session.born) >= MAX_TTL
                {
                    let reason = if session.state.faulted.load(Ordering::Acquire) {
                        session.state.fault_reason()
                    } else if !session.bytes.is_empty() {
                        "incomplete_sse_frame"
                    } else if now.duration_since(session.last) >= IDLE_TTL {
                        "idle_ttl"
                    } else {
                        "response_ttl"
                    };
                    session.state.faulted.store(true, Ordering::Release);
                    session.assembler.mark_degraded_with_reason(reason);
                    self.coverage_faults += 1;
                }
                session.log_degraded();
                result.extend(qualify(session.assembler.finish(), &session.state));
                session.log_end();
                session.state.release_reservation();
            }
        }
        result
    }
}
fn qualify(mut observations: Vec<ToolObservation>, state: &ResponseState) -> Vec<ToolObservation> {
    for obs in &mut observations {
        if matches!(
            obs.quality.tool_identity,
            ToolIdentityQuality::Source | ToolIdentityQuality::ValidatedMapping
        ) {
            if let Some(def) = state
                .metadata
                .definitions
                .iter()
                .find(|d| d.name == obs.tool.name && d.namespace == obs.tool.namespace)
            {
                obs.tool.definition_ref = Some(def.schema.into());
            }
        }
        if state.metadata.quality == OutputQuality::ConvertedUnknown {
            obs.quality.origin = QualityOrigin::Converted;
            obs.quality.tool_identity = ToolIdentityQuality::Unknown;
            obs.quality.source_termination = SourceTermination::Unknown;
            obs.quality.input_fidelity = InputFidelity::Unknown;
            if obs.input.status == InputStatus::Complete {
                obs.input.status = InputStatus::Incomplete;
            }
        }
        if state.faulted.load(Ordering::Acquire) {
            obs.input.value = None;
            obs.input.status = InputStatus::Incomplete;
            obs.quality.source_termination = SourceTermination::Abnormal;
        }
        if obs.validate().is_err() {
            obs.input.value = None;
            obs.input.status = InputStatus::Invalid;
        }
    }
    observations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::tool_observation::channel::*;
    use bytes::Bytes;
    fn metadata(transport: Transport, quality: OutputQuality) -> ResponseMetadata {
        ResponseMetadata {
            protocol: "anthropic_messages",
            client: "claude",
            transport,
            quality,
            definitions: Arc::new(vec![ToolSchemaRef {
                name: "Bash".into(),
                namespace: None,
                schema: "builtin:claude:tools:v1",
            }]),
        }
    }
    static DIAGNOSTIC_LOGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    struct DiagnosticLogger;
    impl log::Log for DiagnosticLogger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            let message = record.args().to_string();
            if message.starts_with("[ToolRiskDiag]") {
                if let Ok(mut logs) = DIAGNOSTIC_LOGS.lock() {
                    logs.push(message);
                }
            }
        }
        fn flush(&self) {}
    }
    fn enable_diagnostic_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        static LOGGER: DiagnosticLogger = DiagnosticLogger;
        INIT.call_once(|| {
            log::set_logger(&LOGGER).unwrap();
            log::set_max_level(log::LevelFilter::Info);
        });
    }
    #[tokio::test]
    async fn diagnostic_parser_logs_first_reason_once_and_preserves_clean_result() {
        enable_diagnostic_logger();
        for fault in [false, true] {
            let (tx, mut rx) = create_channel(4);
            let metadata = ResponseMetadata {
                protocol: "openai_chat",
                client: "codex",
                transport: Transport::Sse,
                quality: OutputQuality::Native,
                definitions: Arc::new(vec![ToolSchemaRef {
                    name: "exec_command".into(),
                    namespace: None,
                    schema: "builtin:codex:tools:v1",
                }]),
            };
            let mut tap = tx.start(metadata, 1).unwrap();
            let scope = tap.state.id;
            let mut processor = ResponseProcessor::default();
            tap.push(&Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"PRIVATE_CALL_ID\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"echo PRIVATE_TOKEN\\\"}\"}}]}}]}\n\n"));
            assert!(processor.ingest(&rx.recv().await.unwrap()).is_empty());
            if fault {
                tap.push(&Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":null}]}}]}\n\ndata: PRIVATE_INVALID_JSON\n\n"));
                assert!(processor.ingest(&rx.recv().await.unwrap()).is_empty());
            }
            tap.push(&Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            ));
            let result = processor.ingest(&rx.recv().await.unwrap());
            assert_eq!(result.len(), 1);
            assert_eq!(
                result[0].tool.definition_ref.as_deref(),
                Some("builtin:codex:tools:v1")
            );
            if fault {
                assert!(result[0].input.value.is_none());
            } else {
                assert_eq!(
                    result[0].input.value,
                    Some(serde_json::json!({"cmd":"echo PRIVATE_TOKEN"}))
                );
            }
            drop(tap);
            assert!(processor.sweep().is_empty());
            let logs = DIAGNOSTIC_LOGS.lock().unwrap();
            let lines: Vec<_> = logs
                .iter()
                .filter(|s| s.contains(&format!("scope={scope} ")))
                .collect();
            assert!(lines.iter().any(|s| s.contains("phase=start")));
            assert!(lines.iter().any(|s| s.contains("phase=end")));
            assert_eq!(
                lines
                    .iter()
                    .filter(|s| s.contains("phase=degraded"))
                    .count(),
                usize::from(fault)
            );
            if fault {
                assert!(lines
                    .iter()
                    .any(|s| s.contains("reason=chat_function_null")));
            }
            for secret in [
                "PRIVATE_CALL_ID",
                "PRIVATE_TOKEN",
                "PRIVATE_INVALID_JSON",
                "echo",
                "exec_command",
            ] {
                assert!(lines.iter().all(|s| !s.contains(secret)));
            }
            assert!(lines.len() <= 3);
        }
    }

    #[tokio::test]
    async fn diagnostic_parser_distinguishes_queue_fault_and_partial_frame_at_end() {
        enable_diagnostic_logger();
        for reason in ["queue_full", "incomplete_sse_frame"] {
            let (tx, mut rx) = create_channel(1);
            let mut metadata = metadata(Transport::Sse, OutputQuality::Native);
            metadata.client = "codex";
            let mut tap = tx.start(metadata, 1).unwrap();
            let scope = tap.state.id;
            let mut processor = ResponseProcessor::default();
            tap.push(&start_event("PRIVATE_ID"));
            assert!(processor.ingest(&rx.recv().await.unwrap()).is_empty());
            if reason == "queue_full" {
                tap.push(&Bytes::from_static(b": queued\n\n"));
                tap.push(&Bytes::from_static(b"PRIVATE_DROPPED"));
            } else {
                tap.push(&Bytes::from_static(b"data: PRIVATE_PARTIAL"));
            }
            drop(tap);
            let mut observations = processor.ingest(&rx.recv().await.unwrap());
            observations.extend(processor.sweep());
            assert_eq!(observations.len(), 1);
            assert_eq!(observations[0].input.status, InputStatus::Incomplete);
            assert!(observations[0].input.value.is_none());
            let logs = DIAGNOSTIC_LOGS.lock().unwrap();
            let lines: Vec<_> = logs
                .iter()
                .filter(|s| s.contains(&format!("scope={scope} ")))
                .collect();
            assert_eq!(
                lines
                    .iter()
                    .filter(|s| s.contains("phase=degraded"))
                    .count(),
                1
            );
            assert!(lines
                .iter()
                .any(|s| s.contains(&format!("reason={reason}"))));
            for secret in ["PRIVATE_ID", "PRIVATE_DROPPED", "PRIVATE_PARTIAL"] {
                assert!(lines.iter().all(|s| !s.contains(secret)));
            }
        }
    }

    #[tokio::test]
    async fn regression_chat_sse_carrier_shapes_preserve_bytes_and_known_input() {
        for carrier in [
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!("PRIVATE_BAD_CARRIER"),
            serde_json::json!(17),
            serde_json::json!(true),
        ] {
            let (tx, mut rx) = create_channel(8);
            let metadata = ResponseMetadata {
                protocol: "openai_chat",
                client: "codex",
                transport: Transport::Sse,
                quality: OutputQuality::Native,
                definitions: Arc::new(vec![ToolSchemaRef {
                    name: "exec_command".into(),
                    namespace: None,
                    schema: "builtin:codex:tools:v1",
                }]),
            };
            let mut tap = tx.start(metadata, 1).unwrap();
            let events = [
                serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"PRIVATE_ID",
                    "function":{"name":"exec_command","arguments":"{\"cmd\":"}}]}}]}),
                serde_json::json!({"choices":[{"delta":{"tool_calls":carrier}}]}),
                serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                    "function":{"arguments":"\"echo hi\",\"workdir\":\"/workspace\",\"yield_time_ms\":1000}"}}]}}]}),
                serde_json::json!({"choices":[{"finish_reason":"tool_calls"}]}),
            ];
            let wire = events
                .iter()
                .map(|event| format!("data: {event}\r\n\r\n"))
                .collect::<String>();
            let mut processor = ResponseProcessor::default();
            let mut observations = Vec::new();
            for chunk in wire.as_bytes().chunks(41) {
                let bytes = Bytes::copy_from_slice(chunk);
                tap.push(&bytes);
                let piece = rx.recv().await.unwrap();
                assert_eq!(piece.bytes, bytes);
                observations.extend(processor.ingest(&piece));
            }
            drop(tap);
            observations.extend(processor.sweep());
            assert_eq!(observations.len(), 1);
            let observation = &observations[0];
            assert_eq!(
                observation.input.value,
                Some(
                    serde_json::json!({"cmd":"echo hi","workdir":"/workspace","yield_time_ms":1000})
                )
            );
            assert_eq!(
                observation.tool.definition_ref.as_deref(),
                Some("builtin:codex:tools:v1")
            );
            let expected = if carrier.is_null() || carrier.is_array() {
                InputStatus::Complete
            } else {
                InputStatus::Incomplete
            };
            assert_eq!(observation.input.status, expected);
            assert_eq!(observation.quality.input_fidelity, InputFidelity::Preserved);
            assert!(processor.sessions.is_empty());
        }
    }

    fn body() -> Bytes {
        Bytes::from_static(br#"{"content":[{"type":"tool_use","id":"call","name":"Bash","input":{"command":"echo PRIVATE"}}],"stop_reason":"tool_use"}"#)
    }
    #[tokio::test]
    async fn json_is_decoded_in_background_with_verified_definition() {
        let (tx, mut rx) = create_channel(2);
        let mut tap = tx
            .start(metadata(Transport::Json, OutputQuality::Native), 0)
            .unwrap();
        tap.push(&body());
        let mut processor = ResponseProcessor::default();
        let result = processor.ingest(&rx.recv().await.unwrap());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].input.status, InputStatus::Complete);
        assert_eq!(
            result[0].tool.definition_ref.as_deref(),
            Some("builtin:claude:tools:v1")
        );
    }
    #[tokio::test]
    async fn converted_output_does_not_prove_upstream_completion() {
        let (tx, mut rx) = create_channel(2);
        let mut tap = tx
            .start(
                metadata(Transport::Json, OutputQuality::ConvertedUnknown),
                0,
            )
            .unwrap();
        tap.push(&body());
        let mut processor = ResponseProcessor::default();
        let result = processor.ingest(&rx.recv().await.unwrap());
        assert_eq!(result.len(), 1);
        assert_ne!(result[0].input.status, InputStatus::Complete);
        assert_eq!(result[0].quality.input_fidelity, InputFidelity::Unknown);
    }
    #[tokio::test]
    async fn split_utf8_crlf_and_multiline_data_are_one_event() {
        let (tx, mut rx) = create_channel(8);
        let mut tap = tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .unwrap();
        let event = "data: {\"type\":\"content_block_start\",\"index\":0,\r\ndata: \"content_block\":{\"type\":\"tool_use\",\"id\":\"x\",\"name\":\"Bash\",\"input\":{\"command\":\"echo 中文\"}}}\r\n\r\ndata: {\"type\":\"content_block_stop\",\"index\":0}\r\n\r\n";
        let split = event.find('中').unwrap() + 1;
        tap.push(&Bytes::copy_from_slice(&event.as_bytes()[..split]));
        tap.push(&Bytes::copy_from_slice(&event.as_bytes()[split..]));
        drop(tap);
        let mut processor = ResponseProcessor::default();
        let mut result = processor.ingest(&rx.recv().await.unwrap());
        result.extend(processor.ingest(&rx.recv().await.unwrap()));
        result.extend(processor.sweep());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].input.status, InputStatus::Complete);
        assert_eq!(
            result[0].input.value.as_ref().unwrap()["command"],
            "echo 中文"
        );
    }
    #[tokio::test]
    async fn dropped_piece_cannot_produce_complete_call() {
        let (tx, mut rx) = create_channel(1);
        let mut tap = tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .unwrap();
        tap.push(&Bytes::from_static(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"x\",\"name\":\"Bash\",\"input\":{}}}\n\n"));
        let mut processor = ResponseProcessor::default();
        let mut result = processor.ingest(&rx.recv().await.unwrap());
        tap.push(&Bytes::from_static(b"data: {}\n\n"));
        tap.push(&Bytes::from_static(
            b"data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        ));
        drop(tap);
        result.extend(processor.ingest(&rx.recv().await.unwrap()));
        result.extend(processor.sweep());
        assert_eq!(result.len(), 1);
        assert_ne!(result[0].input.status, InputStatus::Complete);
        assert!(result[0].input.value.is_none());
    }
    #[tokio::test]
    async fn sweep_releases_idle_input_and_active_reservation() {
        let (tx, mut rx) = create_channel(2);
        let mut tap = tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .unwrap();
        tap.push(&Bytes::from_static(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"x\",\"name\":\"Bash\",\"input\":{}}}\n\n"));
        let mut processor = ResponseProcessor::default();
        processor.ingest(&rx.recv().await.unwrap());
        let result = processor.sweep_at(Instant::now() + IDLE_TTL);
        assert_eq!(result.len(), 1);
        assert_ne!(result[0].input.status, InputStatus::Complete);
        assert!(processor.sessions.is_empty());
        assert!(tap.state.faulted.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn conversion_unknown_cannot_establish_trusted_identity() {
        let (tx, mut rx) = create_channel(2);
        let mut tap = tx
            .start(
                metadata(Transport::Json, OutputQuality::ConvertedUnknown),
                0,
            )
            .unwrap();
        tap.push(&body());
        let mut processor = ResponseProcessor::default();
        let result = processor.ingest(&rx.recv().await.unwrap());
        assert_eq!(
            result[0].quality.tool_identity,
            ToolIdentityQuality::Unknown
        );
    }
    #[tokio::test]
    async fn invalid_namespace_cannot_become_a_builtin_definition() {
        for namespace in [
            serde_json::json!(123),
            serde_json::json!(""),
            serde_json::json!("x".repeat(MAX_NAME_BYTES + 1)),
        ] {
            let (tx, mut rx) = create_channel(2);
            let mut tap = tx
                .start(metadata(Transport::Json, OutputQuality::Native), 0)
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&body()).unwrap();
            value["content"][0]["namespace"] = namespace;
            tap.push(&Bytes::from(value.to_string()));
            let result = ResponseProcessor::default().ingest(&rx.recv().await.unwrap());
            assert_eq!(result.len(), 1);
            assert!(result[0].tool.definition_ref.is_none());
            assert_eq!(
                result[0].quality.tool_identity,
                ToolIdentityQuality::Unknown
            );
        }
    }
    #[tokio::test]
    async fn ttl_releases_budget_even_while_client_keeps_taps_alive() {
        let (tx, mut rx) = create_channel(32);
        let mut taps = Vec::new();
        let mut processor = ResponseProcessor::default();
        for _ in 0..(MAX_ACTIVE_BYTES / RESPONSE_RESERVATION) {
            let mut tap = tx
                .start(metadata(Transport::Sse, OutputQuality::Native), 0)
                .unwrap();
            tap.push(&Bytes::from_static(b": ping\n\n"));
            processor.ingest(&rx.recv().await.unwrap());
            taps.push(tap);
        }
        assert!(tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .is_none());
        processor.sweep_at(Instant::now() + IDLE_TTL);
        assert!(tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .is_some());
        drop(taps);
    }

    fn start_event(id: &str) -> Bytes {
        Bytes::from(format!("data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{id}\",\"name\":\"Bash\",\"input\":{{}}}}}}\n\n"))
    }
    #[tokio::test]
    async fn faulted_first_piece_cannot_establish_identity_or_session() {
        let (tx, mut rx) = create_channel(2);
        let mut tap = tx
            .start(metadata(Transport::Sse, OutputQuality::Native), 0)
            .unwrap();
        tap.push(&start_event("late"));
        tap.fail();
        let mut processor = ResponseProcessor::default();
        assert!(processor.ingest(&rx.recv().await.unwrap()).is_empty());
        assert!(processor.sessions.is_empty());
        assert!(processor.sweep().is_empty());
    }
    #[tokio::test]
    async fn invalidated_session_cannot_be_recreated_from_queued_piece() {
        for clear in [false, true] {
            let (tx, mut rx) = create_channel(3);
            let mut tap = tx
                .start(metadata(Transport::Sse, OutputQuality::Native), 0)
                .unwrap();
            tap.push(&start_event("known"));
            tap.push(&start_event("late"));
            let mut processor = ResponseProcessor::default();
            processor.ingest(&rx.recv().await.unwrap());
            if clear {
                processor.clear();
            } else {
                assert_eq!(processor.sweep_at(Instant::now() + IDLE_TTL).len(), 1);
            }
            assert!(processor.ingest(&rx.recv().await.unwrap()).is_empty());
            assert!(processor.sessions.is_empty());
            assert!(processor.sweep().is_empty());
        }
    }
    #[tokio::test]
    async fn fault_or_gap_finishes_only_existing_identity_without_reading_piece() {
        for gap in [false, true] {
            let (tx, mut rx) = create_channel(3);
            let mut tap = tx
                .start(metadata(Transport::Sse, OutputQuality::Native), 0)
                .unwrap();
            tap.push(&start_event("known"));
            let mut processor = ResponseProcessor::default();
            processor.ingest(&rx.recv().await.unwrap());
            tap.push(&start_event("late"));
            let mut piece = rx.recv().await.unwrap();
            if gap {
                piece.sequence += 1;
            } else {
                tap.fail();
            }
            let result = processor.ingest(&piece);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].correlation.call_id.as_deref(), Some("known"));
            assert!(result[0].input.value.is_none());
            assert_ne!(result[0].input.status, InputStatus::Complete);
            assert!(tap.state.faulted.load(Ordering::Acquire));
            assert!(processor.sessions.is_empty());
        }
    }

    #[tokio::test]
    async fn invalid_stream_namespace_cannot_become_builtin_definition() {
        for namespace in [
            serde_json::json!(123),
            serde_json::json!(""),
            serde_json::json!(" \t"),
            serde_json::json!("x".repeat(MAX_NAME_BYTES + 1)),
        ] {
            let (tx, mut rx) = create_channel(3);
            let mut tap = tx
                .start(metadata(Transport::Sse, OutputQuality::Native), 0)
                .unwrap();
            let event = serde_json::json!({"type":"content_block_start","index":0,"content_block":{
                "type":"tool_use","id":"call","name":"Bash","namespace":namespace,"input":{"command":"echo hi"}
            }});
            tap.push(&Bytes::from(format!(
                "data: {event}\n\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n"
            )));
            let result = ResponseProcessor::default().ingest(&rx.recv().await.unwrap());
            assert_eq!(result.len(), 1);
            assert!(result[0].tool.definition_ref.is_none(), "{namespace}");
            assert_eq!(
                result[0].quality.tool_identity,
                ToolIdentityQuality::Unknown,
                "{namespace}"
            );
        }
    }
}
