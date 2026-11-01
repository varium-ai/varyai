use crate::proxy::providers::{
    streaming_codex_anthropic::create_responses_sse_stream_from_anthropic,
    streaming_codex_chat::create_responses_sse_stream_from_chat,
};
use bytes::Bytes;
use futures::{stream, StreamExt};

fn chat_terminal() -> &'static str {
    "data: {\"id\":\"synthetic-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"synthetic\"},\"finish_reason\":\"stop\"}]}\n\n"
}

fn anthropic_without_stop() -> &'static str {
    concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"synthetic-message\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"
    )
}

#[tokio::test]
async fn reversible_completion_legacy_chat_still_ignores_bad_json() {
    let upstream = stream::iter([
        Ok::<_, std::io::Error>(Bytes::from_static(b"data: {bad synthetic JSON}\n\n")),
        Ok(Bytes::from_static(chat_terminal().as_bytes())),
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
    ]);
    let output: Vec<_> = create_responses_sse_stream_from_chat(upstream)
        .collect()
        .await;
    let bytes: Vec<_> = output.into_iter().map(Result::unwrap).collect();
    let text = String::from_utf8(bytes.concat()).unwrap();
    assert!(text.contains("event: response.completed\n"));
    assert!(!text.contains("event: response.failed\n"));
}

#[tokio::test]
async fn reversible_completion_legacy_chat_done_alone_is_unchanged() {
    let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
        b"data: [DONE]\n\n",
    ))]);
    let output: Vec<_> = create_responses_sse_stream_from_chat(upstream)
        .collect()
        .await;
    let bytes: Vec<_> = output.into_iter().map(Result::unwrap).collect();
    assert!(String::from_utf8(bytes.concat())
        .unwrap()
        .contains("event: response.completed\n"));
}

#[tokio::test]
async fn reversible_completion_legacy_anthropic_missing_message_stop_is_unchanged() {
    let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
        anthropic_without_stop().as_bytes(),
    ))]);
    let output: Vec<_> = create_responses_sse_stream_from_anthropic(upstream)
        .collect()
        .await;
    let bytes: Vec<_> = output.into_iter().map(Result::unwrap).collect();
    assert!(String::from_utf8(bytes.concat())
        .unwrap()
        .contains("event: response.completed\n"));
}

#[tokio::test]
async fn reversible_completion_chat_borrowed_observer_preserves_exact_legacy_output() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let input = [chat_terminal(), "data: [DONE]\n\n"];
    let upstream = || {
        stream::iter(input.map(|s| Ok::<_, std::io::Error>(Bytes::copy_from_slice(s.as_bytes()))))
    };
    let legacy: Vec<_> = create_responses_sse_stream_from_chat(upstream())
        .collect()
        .await;
    let mut observations = Vec::new();
    let observed: Vec<_> = create_responses_sse_stream_from_chat_observed(
        upstream(),
        CodexToolContext::default(),
        |event| {
            if matches!(event, CompletionObservation::RawFrame(_)) {
                return;
            }
            observations.push(match event {
                CompletionObservation::ChatChunk(value) => {
                    assert_eq!(value["id"], "synthetic-chat");
                    "chat"
                }
                CompletionObservation::ChatDone => "done",
                CompletionObservation::EntityEnd {
                    unfinished_frame,
                    unfinished_utf8,
                    ..
                } => {
                    assert!(!unfinished_frame && !unfinished_utf8);
                    "end"
                }
                _ => "unexpected",
            });
        },
    )
    .collect()
    .await;
    let legacy: Vec<_> = legacy.into_iter().map(Result::unwrap).collect();
    let observed: Vec<_> = observed.into_iter().map(Result::unwrap).collect();
    assert_eq!(legacy, observed);
    assert_eq!(observations, ["chat", "done", "end"]);
}

#[tokio::test]
async fn reversible_completion_anthropic_observer_preserves_exact_legacy_output() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        transform_codex_chat::CodexToolContext,
    };
    let upstream = || {
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
            anthropic_without_stop().as_bytes(),
        ))])
    };
    let legacy: Vec<_> = create_responses_sse_stream_from_anthropic(upstream())
        .collect()
        .await;
    let mut kinds = Vec::new();
    let observed: Vec<_> = create_responses_sse_stream_from_anthropic_observed(
        upstream(),
        CodexToolContext::default(),
        |event| match event {
            CompletionObservation::AnthropicEvent { event_name, data } => {
                assert_eq!(event_name, data["type"].as_str());
                kinds.push(data["type"].as_str().unwrap().to_owned());
            }
            CompletionObservation::EntityEnd {
                unfinished_frame,
                unfinished_utf8,
                ..
            } => {
                assert!(!unfinished_frame && !unfinished_utf8);
                kinds.push("end".to_owned());
            }
            CompletionObservation::RawFrame(_) => {}
            _ => panic!("unexpected synthetic protocol observation"),
        },
    )
    .collect()
    .await;
    assert_eq!(
        legacy.into_iter().map(Result::unwrap).collect::<Vec<_>>(),
        observed.into_iter().map(Result::unwrap).collect::<Vec<_>>()
    );
    assert_eq!(kinds, ["message_start", "message_delta", "end"]);
}

#[tokio::test]
async fn reversible_completion_bad_json_is_observed_before_and_after_real_terminals() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let mut missing = Vec::new();
    for anthropic in [false, true] {
        for late in [false, true] {
            let bad = "data: {bad synthetic JSON}\n\n";
            let terminal = if anthropic {
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            } else {
                "data: [DONE]\n\n"
            };
            let prefix = if anthropic {
                anthropic_without_stop()
            } else {
                chat_terminal()
            };
            let parts = if late {
                [prefix, terminal, bad]
            } else {
                [bad, prefix, terminal]
            };
            let upstream = stream::iter(
                parts.map(|s| Ok::<_, std::io::Error>(Bytes::copy_from_slice(s.as_bytes()))),
            );
            let mut invalid = 0;
            let observe = |event: CompletionObservation<'_>| {
                if matches!(event, CompletionObservation::InvalidJson) {
                    invalid += 1;
                }
            };
            let output: Vec<_> = if anthropic {
                create_responses_sse_stream_from_anthropic_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            } else {
                create_responses_sse_stream_from_chat_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            };
            assert!(output.iter().all(Result::is_ok));
            if invalid != 1 {
                missing.push((anthropic, late, invalid));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "dropped JSON observations missing (anthropic, late, count): {missing:?}"
    );
}

#[tokio::test]
async fn reversible_completion_utf8_observations_precede_lossy_decoding() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let mut missing = Vec::new();
    for anthropic in [false, true] {
        for (case, fragments, invalid_expected, remainder_expected) in [
            ("invalid", vec![&b":\xff\n\n"[..]], true, false),
            (
                "invalid-split",
                vec![&b":\xe4"[..], &b"X\n\n"[..]],
                true,
                false,
            ),
            (
                "valid-split",
                vec![&b":\xe4"[..], &b"\xbd"[..], &b"\xa0\n\n"[..]],
                false,
                false,
            ),
            (
                "literal-replacement",
                vec![":\u{fffd}\n\n".as_bytes()],
                false,
                false,
            ),
            ("unfinished", vec![&b":\xe4"[..]], false, true),
        ] {
            let prefix = if anthropic {
                anthropic_without_stop()
            } else {
                chat_terminal()
            };
            let terminal = if anthropic {
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            } else {
                "data: [DONE]\n\n"
            };
            let mut input = vec![
                Bytes::copy_from_slice(prefix.as_bytes()),
                Bytes::copy_from_slice(terminal.as_bytes()),
            ];
            input.extend(fragments.into_iter().map(Bytes::copy_from_slice));
            let upstream = stream::iter(input.into_iter().map(Ok::<_, std::io::Error>));
            let mut invalid = false;
            let mut remainder = false;
            let observe = |event: CompletionObservation<'_>| match event {
                CompletionObservation::InvalidUtf8 => invalid = true,
                CompletionObservation::EntityEnd {
                    unfinished_utf8, ..
                } => remainder = unfinished_utf8,
                _ => {}
            };
            let output: Vec<_> = if anthropic {
                create_responses_sse_stream_from_anthropic_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            } else {
                create_responses_sse_stream_from_chat_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            };
            assert!(output.iter().all(Result::is_ok));
            if invalid != invalid_expected || remainder != remainder_expected {
                missing.push((anthropic, case, invalid, remainder));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "original UTF-8 facts lost (anthropic, case, invalid, remainder): {missing:?}"
    );
}

#[tokio::test]
async fn reversible_completion_raw_input_precedes_duplicate_key_loss_and_json_conversion() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let mut missing = Vec::new();
    for (anthropic, raw_json, input) in [
        (false, false, "data: {\"choices\":[],\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"),
        (true, false, "event: message_stop\ndata: {\"type\":\"message_delta\",\"type\":\"message_stop\"}\n\n"),
        (true, true, "{\"id\":\"synthetic\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"stop_reason\":\"max_tokens\",\"stop_reason\":\"end_turn\"}"),
    ] {
        let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::copy_from_slice(input.as_bytes()))]);
        let mut seen_raw = false;
        let mut seen_decoded = false;
        let observe = |event: CompletionObservation<'_>| match event {
            CompletionObservation::RawFrame(raw) if !raw_json => {
                assert_eq!(raw, input.trim_end_matches('\n'));
                assert!(!seen_decoded);
                seen_raw = true;
            }
            CompletionObservation::RawJson(raw) if raw_json => {
                assert_eq!(raw, input);
                assert!(!seen_decoded);
                seen_raw = true;
            }
            CompletionObservation::ChatChunk(_) | CompletionObservation::AnthropicEvent { .. } | CompletionObservation::AnthropicJson(_) => seen_decoded = true,
            _ => {}
        };
        let output: Vec<_> = if anthropic {
            create_responses_sse_stream_from_anthropic_observed(upstream, CodexToolContext::default(), observe).collect().await
        } else {
            create_responses_sse_stream_from_chat_observed(upstream, CodexToolContext::default(), observe).collect().await
        };
        assert!(output.iter().all(Result::is_ok));
        assert!(seen_decoded);
        if !seen_raw { missing.push((anthropic, raw_json)); }
    }
    assert!(
        missing.is_empty(),
        "raw duplicate-key provenance missing (anthropic, json): {missing:?}"
    );
}

#[tokio::test]
async fn reversible_completion_errors_and_cancel_never_report_clean_entity_end() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    for anthropic in [false, true] {
        for io_failure in [false, true] {
            let input = if io_failure {
                Err(std::io::Error::other("synthetic transport error"))
            } else {
                Ok(Bytes::from_static(b"event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"synthetic error\"}}\n\n"))
            };
            let upstream = stream::iter([input]);
            let mut end = None;
            let mut saw_io = false;
            let observe = |event: CompletionObservation<'_>| match event {
                CompletionObservation::EntityEnd { reached_eof, .. } => end = Some(reached_eof),
                CompletionObservation::IoFailure => saw_io = true,
                _ => {}
            };
            let output: Vec<_> = if anthropic {
                create_responses_sse_stream_from_anthropic_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            } else {
                create_responses_sse_stream_from_chat_observed(
                    upstream,
                    CodexToolContext::default(),
                    observe,
                )
                .collect()
                .await
            };
            assert!(output.iter().all(Result::is_ok));
            assert_eq!(end, Some(false));
            assert_eq!(saw_io, io_failure);
        }
        let prefix = if anthropic {
            anthropic_without_stop()
        } else {
            chat_terminal()
        };
        let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::copy_from_slice(
            prefix.as_bytes(),
        ))])
        .chain(stream::pending());
        let mut ends = 0;
        let observe = |event: CompletionObservation<'_>| {
            if matches!(event, CompletionObservation::EntityEnd { .. }) {
                ends += 1;
            }
        };
        if anthropic {
            let converted = create_responses_sse_stream_from_anthropic_observed(
                upstream,
                CodexToolContext::default(),
                observe,
            );
            tokio::pin!(converted);
            assert!(converted.next().await.is_some());
        } else {
            let converted = create_responses_sse_stream_from_chat_observed(
                upstream,
                CodexToolContext::default(),
                observe,
            );
            tokio::pin!(converted);
            assert!(converted.next().await.is_some());
        }
        assert_eq!(
            ends, 0,
            "dropping a pending stream must not report entity completion"
        );
    }
}

#[test]
fn reversible_completion_native_observer_keeps_legacy_aggregation() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::handlers::responses_sse_to_response_value_observed;
    let input = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"synthetic\",\"status\":\"completed\",\"output\":[]}}\n\n";
    let mut kinds = Vec::new();
    let response = responses_sse_to_response_value_observed(input, |fact| match fact {
        CompletionObservation::RawFrame(_) => kinds.push("raw"),
        CompletionObservation::NativeEvent { event_name, data } => {
            assert_eq!(event_name, "response.completed");
            assert_eq!(data["response"]["id"], "synthetic");
            kinds.push("native");
        }
        CompletionObservation::BufferedBodyEnd { unfinished_frame } => {
            assert!(!unfinished_frame);
            kinds.push("buffer-end");
        }
        _ => panic!("unexpected synthetic native fact"),
    })
    .unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(kinds, ["raw", "native", "buffer-end"]);
}

#[test]
fn reversible_completion_native_ignored_tail_and_bad_json_remain_observable() {
    use super::completion_evidence::CompletionObservation;
    use crate::proxy::handlers::responses_sse_to_response_value_observed;
    let complete = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"synthetic\",\"status\":\"completed\",\"output\":[]}}\n\n";
    let failed_tail = "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"synthetic\"}}}\n";
    let mut raw_tail = false;
    let response =
        responses_sse_to_response_value_observed(&format!("{complete}{failed_tail}"), |fact| {
            if let CompletionObservation::RawFrame(raw) = fact {
                raw_tail |= raw.contains("response.failed");
            }
        })
        .unwrap();
    assert_eq!(
        response["status"], "completed",
        "legacy tail handling must stay unchanged"
    );
    let mut invalid = false;
    let result =
        responses_sse_to_response_value_observed("data: {bad synthetic JSON}\n\n", |fact| {
            invalid |= matches!(fact, CompletionObservation::InvalidJson);
        });
    assert!(result.is_err());
    assert!(
        raw_tail && invalid,
        "native provenance lost: tail={raw_tail}, invalid={invalid}"
    );
}

#[test]
fn reversible_completion_chat_reason_extraction_preserves_legacy_status() {
    use super::completion_evidence::ChatCompletionReason;
    use crate::proxy::providers::transform_codex_chat::response_status_from_finish_reason;
    for reason in [
        None,
        Some("stop"),
        Some("tool_calls"),
        Some("function_call"),
        Some("length"),
        Some("content_filter"),
        Some("other"),
    ] {
        let expected = if reason == Some("length") {
            "incomplete"
        } else {
            "completed"
        };
        assert_eq!(ChatCompletionReason::new(reason).legacy_status(), expected);
        assert_eq!(response_status_from_finish_reason(reason), expected);
    }
}

#[tokio::test]
async fn reversible_completion_chat_requires_real_allowed_reason_before_done() {
    use super::completion_evidence::{ChatCompletionReason, CompletionObservation};
    use crate::proxy::providers::{
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let mut mismatches = Vec::new();
    for reason in [
        None,
        Some(""),
        Some("stop"),
        Some("tool_calls"),
        Some("function_call"),
        Some("length"),
        Some("content_filter"),
        Some("other"),
    ] {
        let allowed = matches!(reason, Some("stop" | "tool_calls" | "function_call"));
        let finish = reason
            .map(|r| format!(",\"finish_reason\":\"{r}\""))
            .unwrap_or_default();
        let input = format!("data: {{\"id\":\"synthetic\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"synthetic\"}}{finish}}}]}}\n\ndata: [DONE]\n\n");
        let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from(input))]);
        let mut permitted = ChatCompletionReason::new(None).permits_completion();
        let output: Vec<_> = create_responses_sse_stream_from_chat_observed(
            upstream,
            CodexToolContext::default(),
            |fact| {
                if let CompletionObservation::ChatChunk(chunk) = fact {
                    permitted =
                        ChatCompletionReason::new(chunk["choices"][0]["finish_reason"].as_str())
                            .permits_completion();
                }
            },
        )
        .collect()
        .await;
        assert!(output.into_iter().all(|r| r.is_ok()));
        if permitted != allowed {
            mismatches.push(reason);
        }
    }
    assert!(
        mismatches.is_empty(),
        "unproven Chat reasons admitted: {mismatches:?}"
    );
}

#[test]
fn reversible_completion_transport_errors_are_sticky_and_buffer_end_is_not_entity() {
    use super::completion_evidence::{CompletionObservation as O, CompletionTransport};
    let clean = || O::EntityEnd {
        reached_eof: true,
        unfinished_frame: false,
        unfinished_utf8: false,
    };
    let cases = [
        vec![O::InvalidJson, clean()],
        vec![O::InvalidUtf8, clean()],
        vec![O::EntityEnd {
            reached_eof: true,
            unfinished_frame: true,
            unfinished_utf8: false,
        }],
        vec![O::EntityEnd {
            reached_eof: true,
            unfinished_frame: false,
            unfinished_utf8: true,
        }],
        vec![
            O::EntityEnd {
                reached_eof: false,
                unfinished_frame: false,
                unfinished_utf8: false,
            },
            clean(),
        ],
        vec![clean(), O::InvalidJson, clean()],
        vec![clean(), O::RawFrame("data: synthetic late frame")],
        vec![clean(), clean()],
        vec![O::BufferedBodyEnd {
            unfinished_frame: false,
        }],
        vec![O::IoFailure, clean()],
    ];
    let mut admitted = Vec::new();
    for (index, facts) in cases.into_iter().enumerate() {
        let mut transport = CompletionTransport::default();
        for fact in facts {
            transport.observe(&fact);
        }
        if transport.permits_completion() {
            admitted.push(index);
        }
    }
    assert!(
        admitted.is_empty(),
        "invalid transport sequences admitted: {admitted:?}"
    );
    let mut valid = CompletionTransport::default();
    assert!(!valid.permits_completion());
    valid.observe(&O::RawFrame("data: synthetic"));
    valid.observe(&clean());
    assert!(valid.permits_completion());
}

#[tokio::test]
async fn reversible_completion_transport_rejects_lossy_converter_successes() {
    use super::completion_evidence::CompletionTransport;
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let mut admitted = Vec::new();
    for anthropic in [false, true] {
        let terminal = if anthropic {
            format!(
                "{}event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
                anthropic_without_stop()
            )
        } else {
            format!("{}data: [DONE]\n\n", chat_terminal())
        };
        for position in [0, 1] {
            for invalid in [
                &b"data: {bad synthetic JSON}\n\n"[..],
                &b": synthetic \xff\n\n"[..],
                &b"data: {\"partial\":"[..],
            ] {
                let mut input = Vec::new();
                if position == 0 {
                    input.extend_from_slice(invalid);
                }
                input.extend_from_slice(terminal.as_bytes());
                if position == 1 {
                    input.extend_from_slice(invalid);
                }
                let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from(input))]);
                let mut transport = CompletionTransport::default();
                let observe = |fact: super::completion_evidence::CompletionObservation<'_>| {
                    transport.observe(&fact)
                };
                let output: Vec<_> = if anthropic {
                    create_responses_sse_stream_from_anthropic_observed(
                        upstream,
                        CodexToolContext::default(),
                        observe,
                    )
                    .collect()
                    .await
                } else {
                    create_responses_sse_stream_from_chat_observed(
                        upstream,
                        CodexToolContext::default(),
                        observe,
                    )
                    .collect()
                    .await
                };
                assert!(output.into_iter().all(|r| r.is_ok()));
                if transport.permits_completion() {
                    admitted.push((anthropic, position, invalid.len()));
                }
            }
        }
    }
    assert!(admitted.is_empty(), "lossy streams admitted: {admitted:?}");
}

#[test]
fn reversible_completion_other_terminal_extraction_preserves_legacy() {
    use super::completion_evidence::{AnthropicCompletionReason, NativeCompletionEvent};
    use crate::proxy::providers::transform_codex_anthropic::map_anthropic_stop_reason_to_status;
    for reason in [
        None,
        Some("end_turn"),
        Some("tool_use"),
        Some("stop_sequence"),
        Some("max_tokens"),
        Some("refusal"),
        Some("model_context_window_exceeded"),
        Some("pause_turn"),
        Some("other"),
    ] {
        assert_eq!(
            AnthropicCompletionReason::new(reason).legacy_status(),
            map_anthropic_stop_reason_to_status(reason)
        );
    }
    let data = serde_json::json!({"response":{"status":"incomplete"}});
    assert!(NativeCompletionEvent::new("response.completed", &data).legacy_is_completed());
    assert!(!NativeCompletionEvent::new("response.failed", &data).legacy_is_completed());
}

#[tokio::test]
async fn reversible_completion_anthropic_reason_and_native_status_require_allowlist() {
    use super::completion_evidence::{
        AnthropicCompletionReason, CompletionObservation, NativeCompletionEvent,
    };
    use crate::proxy::{
        handlers::responses_sse_to_response_value_observed,
        providers::{
            streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
            transform_codex_chat::CodexToolContext,
        },
    };
    let mut mismatches = Vec::new();
    for reason in [
        None,
        Some(""),
        Some("end_turn"),
        Some("tool_use"),
        Some("stop_sequence"),
        Some("pause_turn"),
        Some("other"),
        Some("max_tokens"),
        Some("refusal"),
    ] {
        let reason_json = serde_json::to_string(&reason).unwrap();
        let input = format!("event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"synthetic\",\"content\":[]}}}}\n\nevent: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":{reason_json}}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n");
        let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from(input))]);
        let mut permitted = AnthropicCompletionReason::new(None).permits_completion();
        let _: Vec<_> = create_responses_sse_stream_from_anthropic_observed(
            upstream,
            CodexToolContext::default(),
            |fact| {
                if let CompletionObservation::AnthropicEvent { data, .. } = fact {
                    if data["type"] == "message_delta" {
                        permitted = AnthropicCompletionReason::new(
                            data.pointer("/delta/stop_reason").and_then(|r| r.as_str()),
                        )
                        .permits_completion();
                    }
                }
            },
        )
        .collect()
        .await;
        if permitted != matches!(reason, Some("end_turn" | "tool_use" | "stop_sequence")) {
            mismatches.push(format!("anthropic:{reason:?}"));
        }
    }
    for response in [
        serde_json::json!({}),
        serde_json::json!({"status":"incomplete"}),
        serde_json::json!({"status":"failed"}),
        serde_json::json!({"status":"completed"}),
        serde_json::json!({"status":"completed","error":{}}),
    ] {
        let input = format!("event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{response}}}\n\n");
        let mut permitted = false;
        let result = responses_sse_to_response_value_observed(&input, |fact| {
            if let CompletionObservation::NativeEvent { event_name, data } = fact {
                permitted = NativeCompletionEvent::new(event_name, data).permits_completion();
            }
        });
        assert!(result.is_ok(), "legacy native acceptance changed");
        let expected = response["status"] == "completed" && response.get("error").is_none();
        if permitted != expected {
            mismatches.push(format!("native:{response}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "unsupported terminal admitted: {mismatches:?}"
    );
}

async fn strict_sse_candidate(anthropic: bool, input: String) -> bool {
    use super::completion_evidence::{CompletionObservation, CompletionTransport, SseCompletion};
    use crate::proxy::providers::{
        streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_observed,
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::CodexToolContext,
    };
    let upstream = stream::iter([Ok::<_, std::io::Error>(Bytes::from(input))]);
    let mut transport = CompletionTransport::default();
    let mut protocol = if anthropic {
        SseCompletion::anthropic()
    } else {
        SseCompletion::chat()
    };
    let budget = super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024);
    let observe = |fact: CompletionObservation<'_>| {
        transport.observe(&fact);
        protocol.observe(&fact, &budget);
    };
    let output: Vec<_> = if anthropic {
        create_responses_sse_stream_from_anthropic_observed(
            upstream,
            CodexToolContext::default(),
            observe,
        )
        .collect()
        .await
    } else {
        create_responses_sse_stream_from_chat_observed(
            upstream,
            CodexToolContext::default(),
            observe,
        )
        .collect()
        .await
    };
    assert!(output.into_iter().all(|r| r.is_ok()));
    transport.permits_completion() && protocol.permits_completion()
}

fn anthropic_frame(value: serde_json::Value) -> String {
    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap()
    )
}

#[tokio::test]
async fn reversible_completion_sse_requires_real_ordered_selected_terminal() {
    use serde_json::json;
    let start = anthropic_frame(
        json!({"type":"message_start", "message":{"id":"synthetic", "role":"assistant", "content":[]}}),
    );
    let reason =
        anthropic_frame(json!({"type":"message_delta", "delta":{"stop_reason":"end_turn"}}));
    let stop = anthropic_frame(json!({"type":"message_stop"}));
    let open = anthropic_frame(
        json!({"type":"content_block_start", "index":0,"content_block":{"type":"text","text":""}}),
    );
    let delta = anthropic_frame(
        json!({"type":"content_block_delta", "index":0,"delta":{"type":"text_delta","text":"synthetic"}}),
    );
    let close = anthropic_frame(json!({"type":"content_block_stop", "index":0}));
    let mut admitted = Vec::new();
    let cases = [
        ("no-real-stop", format!("{start}{reason}")),
        ("no-reason", format!("{start}{stop}")),
        ("no-start", format!("{reason}{stop}")),
        ("open-block", format!("{start}{open}{reason}{stop}")),
        (
            "duplicate-open",
            format!("{start}{open}{open}{close}{reason}{stop}"),
        ),
        ("orphan-close", format!("{start}{close}{reason}{stop}")),
        ("orphan-delta", format!("{start}{delta}{reason}{stop}")),
        ("late-delta", format!("{start}{reason}{stop}{open}")),
        ("double-stop", format!("{start}{reason}{stop}{stop}")),
        (
            "late-error",
            format!(
                "{start}{reason}{stop}{}",
                anthropic_frame(json!({"type":"error","error":{"message":"synthetic"}}))
            ),
        ),
    ];
    for (name, input) in cases {
        if strict_sse_candidate(true, input).await {
            admitted.push(name);
        }
    }
    let valid = format!("{start}{open}{delta}{close}{reason}{stop}");
    assert!(
        strict_sse_candidate(true, valid).await,
        "valid Anthropic sequence denied"
    );
    let done = "data: [DONE]\n\n";
    let chat_chunk = |choices: serde_json::Value| {
        format!(
            "data: {}\n\n",
            json!({"id":"synthetic-chat","choices":choices})
        )
    };
    let content = chat_chunk(json!([{"index":0,"delta":{"content":"late"}}]));
    let ambiguous = chat_chunk(
        json!([{"index":0,"delta":{},"finish_reason":"stop"},{"index":1,"delta":{},"finish_reason":"stop"}]),
    );
    let changed_choice = chat_chunk(json!([{"index":1,"delta":{},"finish_reason":"stop"}]));
    let denied = chat_chunk(json!([{"index":0,"delta":{},"finish_reason":"content_filter"}]));
    for (name, input) in [
        ("chat-done-alone", done.to_string()),
        ("chat-multiple-choices", format!("{ambiguous}{done}")),
        (
            "chat-changed-choice",
            format!("{content}{changed_choice}{done}"),
        ),
        (
            "chat-late-content",
            format!("{}{done}{content}", chat_terminal()),
        ),
        (
            "chat-duplicate-done",
            format!("{}{done}{done}", chat_terminal()),
        ),
        (
            "chat-filter-then-stop",
            format!("{denied}{}{done}", chat_terminal()),
        ),
    ] {
        if strict_sse_candidate(false, input).await {
            admitted.push(name);
        }
    }
    assert!(
        strict_sse_candidate(false, format!("{}{done}", chat_terminal())).await,
        "valid Chat sequence denied"
    );
    assert!(
        admitted.is_empty(),
        "incomplete or ambiguous SSE sequences admitted: {admitted:?}"
    );
}

#[tokio::test]
async fn reversible_completion_sse_malformed_shapes_cannot_supply_terminal() {
    use serde_json::json;
    let start = anthropic_frame(
        json!({"type":"message_start", "message":{"id":"synthetic", "type":"message", "role":"assistant", "content":[]}}),
    );
    let reason =
        anthropic_frame(json!({"type":"message_delta", "delta":{"stop_reason":"end_turn"}}));
    let stop = anthropic_frame(json!({"type":"message_stop"}));
    let mut admitted = Vec::new();
    for (name, altered_start) in [
        ("missing-message", json!({"type":"message_start"})),
        ("non-message", json!({"type":"message_start", "message":1})),
        (
            "wrong-role",
            json!({"type":"message_start", "message":{"id":"synthetic", "role":"user", "content":[]}}),
        ),
        (
            "prefilled-content",
            json!({"type":"message_start", "message":{"id":"synthetic", "role":"assistant", "content":[{"type":"text","text":"unobserved"}]}}),
        ),
    ] {
        if strict_sse_candidate(
            true,
            format!("{}{reason}{stop}", anthropic_frame(altered_start)),
        )
        .await
        {
            admitted.push(name);
        }
    }
    for (name, open, delta) in [
        (
            "unknown-block",
            json!({"type":"content_block_start", "index":0,"content_block":{"type":"synthetic_unknown"}}),
            None,
        ),
        (
            "missing-block",
            json!({"type":"content_block_start", "index":0}),
            None,
        ),
        (
            "wrong-delta-kind",
            json!({"type":"content_block_start", "index":0,"content_block":{"type":"text","text":""}}),
            Some(
                json!({"type":"content_block_delta", "index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
            ),
        ),
        (
            "missing-delta",
            json!({"type":"content_block_start", "index":0,"content_block":{"type":"text","text":""}}),
            Some(json!({"type":"content_block_delta", "index":0})),
        ),
    ] {
        let delta = delta.map(anthropic_frame).unwrap_or_default();
        let close = anthropic_frame(json!({"type":"content_block_stop","index":0}));
        if strict_sse_candidate(
            true,
            format!(
                "{start}{}{delta}{close}{reason}{stop}",
                anthropic_frame(open)
            ),
        )
        .await
        {
            admitted.push(name);
        }
    }
    let chat = |choice: serde_json::Value| {
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"id":"synthetic-chat","choices":[choice]})
        )
    };
    for (name, choice) in [
        ("missing-delta", json!({"index":0,"finish_reason":"stop"})),
        (
            "bad-delta",
            json!({"index":0,"delta":[],"finish_reason":"stop"}),
        ),
        (
            "structured-content",
            json!({"index":0,"delta":{"content":{}},"finish_reason":"stop"}),
        ),
        (
            "wrong-role",
            json!({"index":0,"delta":{"role":"user"},"finish_reason":"stop"}),
        ),
        (
            "wrong-tool-shape",
            json!({"index":0,"delta":{"tool_calls":{}},"finish_reason":"tool_calls"}),
        ),
    ] {
        if strict_sse_candidate(false, chat(choice)).await {
            admitted.push(name);
        }
    }
    assert!(
        admitted.is_empty(),
        "malformed original shapes admitted: {admitted:?}"
    );
}

#[tokio::test]
async fn reversible_completion_raw_duplicates_and_ambiguous_fields_deny_protocol_evidence() {
    let chat =
        r#"{"id":"synthetic-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
    let mut admitted = Vec::new();
    let duplicates = [
        r#"{"choices":[],"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        r#"{"choices":[],"\u0063hoices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"first","content":"second"},"finish_reason":"stop"}]}"#,
    ];
    for (index, data) in duplicates.into_iter().enumerate() {
        if strict_sse_candidate(false, format!("data: {data}\n\ndata: [DONE]\n\n")).await {
            admitted.push(index);
        }
    }
    for (index, raw) in [
        format!("event: error\ndata: {chat}\n\ndata: [DONE]\n\n"),
        format!("event: synthetic\nevent: message\ndata: {chat}\n\ndata: [DONE]\n\n"),
        format!("unknown-field: synthetic\ndata: {chat}\n\ndata: [DONE]\n\n"),
    ]
    .into_iter()
    .enumerate()
    {
        if strict_sse_candidate(false, raw).await {
            admitted.push(index + 10);
        }
    }
    let duplicate_anthropic = "event: message_start\ndata: {\"type\":\"error\",\"type\":\"message_start\",\"message\":{\"id\":\"synthetic\",\"role\":\"assistant\",\"content\":[]}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    if strict_sse_candidate(true, duplicate_anthropic.to_string()).await {
        admitted.push(20);
    }
    assert!(
        admitted.is_empty(),
        "raw provenance failures admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_raw_validation_is_precharged_and_requires_raw_pairing() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    let chunk = serde_json::json!({"id":"synthetic-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
    let budget = PayloadBudget::with_limit(0);
    let mut protocol = SseCompletion::chat();
    let raw = format!("data: {chunk}");
    protocol.observe(&O::RawFrame(&raw), &budget);
    protocol.observe(&O::ChatChunk(&chunk), &budget);
    protocol.observe(&O::ChatDone, &budget);
    assert_eq!(budget.used(), 0);
    assert!(
        !protocol.permits_completion(),
        "raw validation bypassed an exhausted payload budget"
    );
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let mut protocol = SseCompletion::chat();
    protocol.observe(&O::ChatChunk(&chunk), &budget);
    protocol.observe(&O::ChatDone, &budget);
    assert!(
        !protocol.permits_completion(),
        "parsed facts without original raw provenance were admitted"
    );
}

#[test]
fn reversible_completion_raw_pairing_requires_exact_original_shape_and_protocol() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    let original = serde_json::json!({"id":"synthetic","choices":[{"index":0,"delta":{"content":"synthetic"},"finish_reason":"stop"}],"meta":{"a":-0.0,"b":2}});
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let raw = format!("data: {original}");
    let mut denied = Vec::new();
    for mode in 0..5 {
        let mut protocol = SseCompletion::chat();
        protocol.observe(&O::RawFrame(&raw), &budget);
        let mut changed = original.clone();
        if mode == 0 {
            changed["meta"]["a"] = serde_json::json!(0.0);
        }
        if mode == 1 {
            changed["meta"] = serde_json::json!({"b":2,"a":-0.0});
        }
        protocol.observe(&O::ChatChunk(&changed), &budget);
        protocol.observe(&O::RawFrame("data: [DONE]"), &budget);
        protocol.observe(&O::ChatDone, &budget);
        if mode == 2 {
            protocol.observe(&O::RawJson("{}"), &budget);
        }
        if mode == 3 {
            protocol.observe(
                &O::NativeEvent {
                    event_name: "response.completed",
                    data: &original,
                },
                &budget,
            );
        }
        if mode == 4 {
            protocol.observe(&O::AnthropicJson(&original), &budget);
        }
        if protocol.permits_completion() {
            denied.push(mode);
        }
    }
    assert_eq!(budget.used(), 0, "pairing leaked payload backing");
    assert!(
        denied.is_empty(),
        "nonexact or foreign provenance admitted: {denied:?}"
    );
}

#[test]
fn reversible_completion_raw_multiline_precharge_release_and_cancel() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let raw = ": synthetic heartbeat\r\nid: synthetic\r\nretry: 10\r\ndata: {\"id\":\"synthetic\",\r\ndata: \"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}";
    let chunk = serde_json::json!({"id":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
    let mut protocol = SseCompletion::chat();
    protocol.observe(&O::RawFrame(raw), &budget);
    assert!(
        budget.used() > 0,
        "pending raw must retain its charged strict parse"
    );
    protocol.observe(&O::ChatChunk(&chunk), &budget);
    assert_eq!(
        budget.used(),
        "synthetic".len(),
        "only the selected response identity remains charged"
    );
    protocol.observe(&O::RawFrame("data: [DONE]"), &budget);
    protocol.observe(&O::ChatDone, &budget);
    assert!(protocol.permits_completion());
    drop(protocol);
    assert_eq!(budget.used(), 0);
    let mut canceled = SseCompletion::chat();
    canceled.observe(&O::RawFrame(raw), &budget);
    assert!(budget.used() > 0);
    drop(canceled);
    assert_eq!(budget.used(), 0);
    let mut budget_denied = SseCompletion::chat();
    let exhausted = budget.reserve(budget.limit()).unwrap();
    budget_denied.observe(&O::RawFrame(raw), &budget);
    budget_denied.observe(&O::ChatChunk(&chunk), &budget);
    assert!(!budget_denied.permits_completion());
    assert_eq!(budget.used(), budget.limit());
    drop(exhausted);
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
async fn reversible_completion_selected_response_identity_is_required_and_stable() {
    use serde_json::json;
    let chunk = |id: serde_json::Value, terminal: bool| {
        let choice = if terminal {
            json!({"index":0,"delta":{},"finish_reason":"stop"})
        } else {
            json!({"index":0,"delta":{"content":"synthetic"}})
        };
        let mut value = json!({"choices":[choice]});
        if !id.is_null() {
            value["id"] = id;
        }
        format!("data: {value}\n\n")
    };
    let done = "data: [DONE]\n\n";
    let mut admitted = Vec::new();
    for (index, id) in [
        json!(null),
        json!(""),
        json!("synthetic other"),
        json!(3),
        json!("x".repeat(257)),
    ]
    .into_iter()
    .enumerate()
    {
        if strict_sse_candidate(false, format!("{}{done}", chunk(id, true))).await {
            admitted.push(index);
        }
    }
    let first = chunk(json!("synthetic-first"), false);
    let changed = chunk(json!("synthetic-second"), true);
    if strict_sse_candidate(false, format!("{first}{changed}{done}")).await {
        admitted.push(10);
    }
    let usage = "data: {\"id\":\"synthetic-second\",\"choices\":[],\"usage\":{}}\n\n";
    if strict_sse_candidate(
        false,
        format!("{}{usage}{done}", chunk(json!("synthetic-first"), true)),
    )
    .await
    {
        admitted.push(11);
    }
    assert!(
        strict_sse_candidate(
            false,
            format!("{first}{}{done}", chunk(json!("synthetic-first"), true))
        )
        .await
    );
    assert!(
        admitted.is_empty(),
        "missing/changed selected response identity admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_anthropic_raw_event_name_cannot_be_replaced() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let start = json!({"type":"message_start","message":{"id":"synthetic","role":"assistant","content":[]}});
    let reason = json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}});
    let stop = json!({"type":"message_stop"});
    let mut admitted = Vec::new();
    for mode in 0..3 {
        let mut protocol = SseCompletion::anthropic();
        let name = match mode {
            0 => "error",
            1 => "message_start",
            _ => "",
        };
        let raw = if name.is_empty() {
            format!("data: {start}")
        } else {
            format!("event: {name}\ndata: {start}")
        };
        protocol.observe(&O::RawFrame(&raw), &budget);
        let callback = if mode == 1 {
            None
        } else {
            Some("message_start")
        };
        protocol.observe(
            &O::AnthropicEvent {
                event_name: callback,
                data: &start,
            },
            &budget,
        );
        for (name, data) in [("message_delta", &reason), ("message_stop", &stop)] {
            protocol.observe(
                &O::RawFrame(&format!("event: {name}\ndata: {data}")),
                &budget,
            );
            protocol.observe(
                &O::AnthropicEvent {
                    event_name: Some(name),
                    data,
                },
                &budget,
            );
        }
        if protocol.permits_completion() {
            admitted.push(mode);
        }
    }
    assert_eq!(budget.used(), 0);
    assert!(
        admitted.is_empty(),
        "raw event name replaced by callback: {admitted:?}"
    );
}

#[tokio::test]
async fn reversible_completion_chat_tools_require_original_complete_arguments_and_identity() {
    use serde_json::json;
    let frame = |tools: serde_json::Value, reason: Option<&str>| {
        format!(
            "data: {}\n\n",
            json!({"id":"synthetic-tools","choices":[{"index":0,"delta":{"tool_calls":tools},"finish_reason":reason}]})
        )
    };
    let call = |args: serde_json::Value| json!({"index":0,"id":"synthetic-call","type":"function","function":{"name":"synthetic_tool","arguments":args}});
    let finish="data: {\"id\":\"synthetic-tools\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
    let mut admitted = Vec::new();
    for (index, args) in [
        json!(null),
        json!(""),
        json!(" "),
        json!("{\"x\":"),
        json!("{\"x\":1,\"x\":2}"),
        json!("{\"x\":1,\"\\u0078\":2}"),
        json!({}),
        json!("[]"),
    ]
    .into_iter()
    .enumerate()
    {
        if strict_sse_candidate(
            false,
            format!("{}{finish}", frame(json!([call(args)]), None)),
        )
        .await
        {
            admitted.push(index);
        }
    }
    for (index, field) in ["id", "index", "type"].into_iter().enumerate() {
        let mut tool = call(json!("{}"));
        tool.as_object_mut().unwrap().remove(field);
        if strict_sse_candidate(false, format!("{}{finish}", frame(json!([tool]), None))).await {
            admitted.push(index + 10);
        }
    }
    for (index, mutation) in [
        json!({"name":"","arguments":"{}"}),
        json!({"name":"synthetic_tool"}),
        json!({"name":7,"arguments":"{}"}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut tool = call(json!("{}"));
        tool["function"] = mutation;
        if strict_sse_candidate(false, format!("{}{finish}", frame(json!([tool]), None))).await {
            admitted.push(index + 20);
        }
    }
    if strict_sse_candidate(false, format!("{}{finish}", frame(json!([]), None))).await {
        admitted.push(30);
    }
    for (index,tools) in [
        json!([{"index":0,"id":"synthetic-call","type":"unexpected","function":{"name":"synthetic_tool","arguments":"{}"}}]),
        json!([{"index":1,"id":"synthetic-call","type":"function","function":{"name":"synthetic_tool","arguments":"{}"}}]),
        json!([call(json!("{}")),{"index":1,"id":"synthetic-call","type":"function","function":{"name":"synthetic_tool","arguments":"{}"}}]),
    ].into_iter().enumerate(){
        if strict_sse_candidate(false,format!("{}{finish}",frame(tools,None))).await{admitted.push(index+40);}
    }
    for (index, continuation) in [
        json!({"index":0,"id":"synthetic-other","function":{"arguments":"1}"}}),
        json!({"index":0,"function":{"name":"other_tool","arguments":"1}"}}),
    ]
    .into_iter()
    .enumerate()
    {
        if strict_sse_candidate(
            false,
            format!(
                "{}{}{finish}",
                frame(json!([call(json!("{\"x\":"))]), None),
                frame(json!([continuation]), None)
            ),
        )
        .await
        {
            admitted.push(index + 50);
        }
    }
    let legacy="data: {\"id\":\"synthetic-tools\",\"choices\":[{\"index\":0,\"delta\":{\"function_call\":{\"name\":\"synthetic_tool\",\"arguments\":\"{}\"}},\"finish_reason\":\"function_call\"}]}\n\ndata: [DONE]\n\n";
    if strict_sse_candidate(false, legacy.to_string()).await {
        admitted.push(31);
    }
    assert!(
        strict_sse_candidate(
            false,
            format!(
                "{}{finish}",
                frame(json!([call(json!("{\"synthetic\":1}"))]), None)
            )
        )
        .await,
        "valid original tool denied"
    );
    let first = call(json!("{\"synthetic\":"));
    let continuation = json!({"index":0,"function":{"arguments":"1}"}});
    assert!(
        strict_sse_candidate(
            false,
            format!(
                "{}{}{finish}",
                frame(json!([first]), None),
                frame(json!([continuation]), None)
            )
        )
        .await,
        "valid split tool denied"
    );
    assert!(
        admitted.is_empty(),
        "repaired/dropped/unfinished Chat tools admitted: {admitted:?}"
    );
}

#[tokio::test]
async fn reversible_completion_anthropic_tools_cannot_use_repairs_or_ambiguous_inputs() {
    use serde_json::json;
    let start = anthropic_frame(
        json!({"type":"message_start","message":{"id":"synthetic","role":"assistant","content":[]}}),
    );
    let close = anthropic_frame(json!({"type":"content_block_stop","index":0}));
    let finish = format!(
        "{}{}",
        anthropic_frame(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}})),
        anthropic_frame(json!({"type":"message_stop"}))
    );
    let block = |value| {
        anthropic_frame(json!({"type":"content_block_start","index":0,"content_block":value}))
    };
    let delta = |value| {
        anthropic_frame(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":value}}),
        )
    };
    let tool = json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{}});
    let mut admitted = Vec::new();
    for (index, args) in ["", " ", "{\"x\":", "{\"x\":1,\"x\":2}", "[]"]
        .into_iter()
        .enumerate()
    {
        if strict_sse_candidate(
            true,
            format!(
                "{start}{}{}{close}{finish}",
                block(tool.clone()),
                delta(args)
            ),
        )
        .await
        {
            admitted.push(index);
        }
    }
    for (index, field) in ["id", "name", "input"].into_iter().enumerate() {
        let mut changed = tool.clone();
        changed.as_object_mut().unwrap().remove(field);
        if strict_sse_candidate(true, format!("{start}{}{close}{finish}", block(changed))).await {
            admitted.push(index + 10);
        }
    }
    let mut changed = tool.clone();
    changed["input"] = json!({"x":1});
    if strict_sse_candidate(
        true,
        format!(
            "{start}{}{}{close}{finish}",
            block(changed),
            delta("{\"x\":2}")
        ),
    )
    .await
    {
        admitted.push(20);
    }
    let mut changed = tool.clone();
    changed["name"] = json!("Read");
    changed["input"] = json!({"file_path":"synthetic","pages":""});
    if strict_sse_candidate(true, format!("{start}{}{close}{finish}", block(changed))).await {
        admitted.push(21);
    }
    let second =
        anthropic_frame(json!({"type":"content_block_start","index":1,"content_block":tool}));
    let second_close = anthropic_frame(json!({"type":"content_block_stop","index":1}));
    if strict_sse_candidate(
        true,
        format!(
            "{start}{}{close}{second}{second_close}{finish}",
            block(tool.clone())
        ),
    )
    .await
    {
        admitted.push(23);
    }
    let no_tools = format!("{start}{finish}");
    if strict_sse_candidate(true, no_tools).await {
        admitted.push(22);
    }
    assert!(
        strict_sse_candidate(
            true,
            format!("{start}{}{close}{finish}", block(tool.clone()))
        )
        .await,
        "explicit empty input denied"
    );
    assert!(
        strict_sse_candidate(
            true,
            format!(
                "{start}{}{}{}{close}{finish}",
                block(tool),
                delta("{\"x\":"),
                delta("1}")
            )
        )
        .await,
        "valid split Anthropic input denied"
    );
    assert!(
        admitted.is_empty(),
        "repaired/ambiguous Anthropic tools admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_tool_owners_enforce_limits_and_release_on_cancel_or_failure() {
    use super::{
        completion_tools::{AnthropicTools, ChatTools},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let call = |index| json!({"index":index,"id":format!("synthetic-{index}"),"type":"function","function":{"name":"synthetic_tool","arguments":"{}"}});
    let mut tools = ChatTools::new(budget.clone()).unwrap();
    for index in 0..128 {
        tools.observe(&json!([call(index)]), &budget).unwrap();
    }
    assert!(tools.finish(true, &budget).is_ok());
    assert!(tools.observe(&json!([call(128)]), &budget).is_err());
    assert!(budget.used() > 0);
    drop(tools);
    assert_eq!(budget.used(), 0);
    let arguments = format!("{{\"s\":\"{}\"}}", "x".repeat(1024 * 1024 - 8));
    assert_eq!(arguments.len(), 1024 * 1024);
    let mut tools = ChatTools::new(budget.clone()).unwrap();
    let mut first = call(0);
    first["function"]["arguments"] = json!(&arguments[..512 * 1024]);
    tools.observe(&json!([first]), &budget).unwrap();
    tools
        .observe(
            &json!([{"index":0,"function":{"arguments":&arguments[512*1024..]}}]),
            &budget,
        )
        .unwrap();
    assert!(tools.finish(true, &budget).is_ok());
    assert!(tools
        .observe(&json!([{"index":0,"function":{"arguments":" "}}]), &budget)
        .is_err());
    drop(tools);
    assert_eq!(budget.used(), 0);
    let mut tools = AnthropicTools::new(budget.clone()).unwrap();
    let block = json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{}});
    tools.start(&block, &budget).unwrap();
    tools
        .delta(&json!({"delta":{"partial_json":"{\"synthetic\":"}}))
        .unwrap();
    assert!(budget.used() > 0);
    drop(tools);
    assert_eq!(
        budget.used(),
        0,
        "canceling an unfinished tool releases all backing"
    );
    let mut tools = AnthropicTools::new(budget.clone()).unwrap();
    tools.start(&block, &budget).unwrap();
    tools
        .delta(&json!({"delta":{"partial_json":"{\"synthetic\":"}}))
        .unwrap();
    assert!(tools.close(&budget).is_err());
    assert_eq!(
        budget.used(),
        0,
        "failed strict parse retains no partial tool"
    );
    for anthropic in [false, true] {
        let exhausted = budget.reserve(budget.limit()).unwrap();
        if anthropic {
            let mut tools = AnthropicTools::new(budget.clone()).unwrap();
            assert!(tools.start(&block, &budget).is_err());
        } else {
            let mut tools = ChatTools::new(budget.clone()).unwrap();
            assert!(tools.observe(&json!([call(0)]), &budget).is_err());
        }
        assert_eq!(budget.used(), budget.limit());
        drop(exhausted);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn reversible_completion_native_sse_json_compact_and_normalizations_enforce_strict_rules() {
    use super::{
        completion_evidence::CompletionObservation as O,
        completion_json::{
            permits_chat_argument_normalization, permits_xai_argument_normalization,
            JsonCompletion, JsonCompletionProtocol as P, NativeSseCompletion,
        },
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);

    // 1. Native SSE requires raw pairing, valid event name, single completed response, no residual/incomplete/error/duplicate keys.
    let valid_completed = r#"{"type":"response.completed","response":{"id":"resp_valid","status":"completed","output":[]}}"#;
    let mut native_ok = NativeSseCompletion::new();
    native_ok.observe(
        &O::RawFrame(&format!(
            "event: response.completed\ndata: {valid_completed}"
        )),
        &budget,
    );
    let parsed_completed: serde_json::Value = serde_json::from_str(valid_completed).unwrap();
    native_ok.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &parsed_completed,
        },
        &budget,
    );
    native_ok.observe(
        &O::BufferedBodyEnd {
            unfinished_frame: false,
        },
        &budget,
    );
    assert!(native_ok.permits_completion(), "valid native SSE denied");

    let mut native_admitted = Vec::new();
    // Unfinished trailing frame
    let mut n0 = NativeSseCompletion::new();
    n0.observe(
        &O::RawFrame(&format!(
            "event: response.completed\ndata: {valid_completed}"
        )),
        &budget,
    );
    n0.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &parsed_completed,
        },
        &budget,
    );
    n0.observe(
        &O::BufferedBodyEnd {
            unfinished_frame: true,
        },
        &budget,
    );
    if n0.permits_completion() {
        native_admitted.push("unfinished-frame");
    }
    // Missing RawFrame pairing
    let mut n1 = NativeSseCompletion::new();
    n1.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &parsed_completed,
        },
        &budget,
    );
    if n1.permits_completion() {
        native_admitted.push("no-raw");
    }
    // Duplicate key in raw frame
    let dup_raw = r#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_valid","status":"failed","status":"completed","output":[]}}"#;
    let mut n2 = NativeSseCompletion::new();
    n2.observe(&O::RawFrame(dup_raw), &budget);
    n2.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &parsed_completed,
        },
        &budget,
    );
    if n2.permits_completion() {
        native_admitted.push("dup-key");
    }
    // Late raw frame after completion
    let mut n3 = NativeSseCompletion::new();
    n3.observe(
        &O::RawFrame(&format!(
            "event: response.completed\ndata: {valid_completed}"
        )),
        &budget,
    );
    n3.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &parsed_completed,
        },
        &budget,
    );
    n3.observe(
        &O::RawFrame("event: response.failed\ndata: {\"type\":\"response.failed\"}"),
        &budget,
    );
    if n3.permits_completion() {
        native_admitted.push("late-tail");
    }

    // 2. Complete JSON (Native, Chat, Anthropic) & Conservative Compact JSON
    let mut json_admitted = Vec::new();
    for (name, proto, raw) in [
        (
            "native-dup-key",
            P::NativeResponses,
            r#"{"id":"resp_1","object":"response","status":"failed","status":"completed","output":[]}"#,
        ),
        (
            "native-error-field",
            P::NativeResponses,
            r#"{"id":"resp_1","object":"response","status":"completed","error":{"message":"bad"},"output":[]}"#,
        ),
        (
            "native-bad-id",
            P::NativeResponses,
            r#"{"id":"","object":"response","status":"completed","output":[]}"#,
        ),
        (
            "chat-missing-reason",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"message":{"role":"assistant","content":"hi"}}]}"#,
        ),
        (
            "chat-multi-choice",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"message":{"role":"assistant","content":"a"},"finish_reason":"stop"},{"index":1,"message":{"role":"assistant","content":"b"},"finish_reason":"stop"}]}"#,
        ),
        (
            "chat-bad-tool-args",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"fn1","arguments":""}}]},"finish_reason":"tool_calls"}]}"#,
        ),
        (
            "anthropic-missing-stop",
            P::AnthropicMessages,
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"hi"}]}"#,
        ),
        (
            "anthropic-read-empty-pages",
            P::AnthropicMessages,
            r#"{"id":"msg_1","type":"message","role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"tu_1","name":"Read","input":{"file_path":"a","pages":""}}]}"#,
        ),
        ("compact-empty-obj", P::ResponsesCompact, r#"{}"#),
        (
            "compact-no-compaction-item",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":1,"output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hi"}]}]}"#,
        ),
        (
            "compact-with-tool",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":1,"output":[{"id":"c_1","type":"compaction","encrypted_content":"enc"},{"id":"fc_1","type":"function_call","call_id":"call_1","name":"fn","arguments":"{}"}]}"#,
        ),
        (
            "compact-empty-encrypted",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":1,"output":[{"id":"c_1","type":"compaction","encrypted_content":""}]}"#,
        ),
    ] {
        let mut jc = JsonCompletion::new(proto);
        jc.observe_raw_json(raw, &budget);
        if jc.permits_completion() {
            json_admitted.push(name);
        }
    }

    let valid_compact = r#"{"id":"cmp_1","object":"response.compaction","created_at":0,"output":[{"id":"c_1","type":"compaction","encrypted_content":"opaque_blob"},{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hello"}]}]}"#;
    let mut jc_compact = JsonCompletion::new(P::ResponsesCompact);
    jc_compact.observe_raw_json(valid_compact, &budget);
    assert!(jc_compact.permits_completion(), "valid compact JSON denied");

    // 3. Exact normalization allowlist (Chat & xAI)
    let mut norm_admitted = Vec::new();
    for (name, orig, norm) in [
        ("missing", None, "{}"),
        ("empty", Some(json!("")), "{}"),
        ("whitespace", Some(json!("   ")), "{}"),
        ("structured", Some(json!({"a": 1})), r#"{"a":1}"#),
        ("invalid-json", Some(json!("{bad")), "{bad"),
        ("dup-key", Some(json!(r#"{"a":1,"a":2}"#)), r#"{"a":2}"#),
        (
            "signed-zero-flip",
            Some(json!(r#"{"a":-0.0}"#)),
            r#"{"a":0.0}"#,
        ),
        ("array-primitives", Some(json!("[1,2]")), "[1,2]"),
    ] {
        if permits_chat_argument_normalization(orig.as_ref(), norm, &budget).unwrap_or(false) {
            norm_admitted.push(name);
        }
    }
    assert!(
        permits_chat_argument_normalization(
            Some(&json!(" { \"b\" : 1 , \"a\" : [ -0.0 , \"\\u0068i\" ] } ")),
            r#"{"a":[-0.0,"hi"],"b":1}"#,
            &budget,
        )
        .unwrap(),
        "valid Chat canonicalization denied"
    );

    for (name, orig, norm, applied) in [
        (
            "unrecorded-xai",
            r#"{"n":92116.0}"#,
            r#"{"n":92116}"#,
            false,
        ),
        ("fraction-changed", r#"{"n":1.5}"#, r#"{"n":1}"#, true),
        ("dup-key-xai", r#"{"n":1.0,"n":2.0}"#, r#"{"n":2}"#, true),
        ("neg-zero-to-int", r#"{"n":-0.0}"#, r#"{"n":0}"#, true),
    ] {
        if permits_xai_argument_normalization(orig, norm, applied, &budget).unwrap_or(false) {
            norm_admitted.push(name);
        }
    }
    assert!(
        permits_xai_argument_normalization(
            r#"{"a":92116.0,"b":[1.5,-42.0]}"#,
            r#"{"a":92116,"b":[1.5,-42]}"#,
            true,
            &budget,
        )
        .unwrap(),
        "valid xAI whole-float normalization denied"
    );

    assert_eq!(budget.used(), 0, "completion validators leaked budget");
    assert!(
        native_admitted.is_empty() && json_admitted.is_empty() && norm_admitted.is_empty(),
        "strict completion/normalization rules violated: native={native_admitted:?}, json={json_admitted:?}, norm={norm_admitted:?}"
    );
}

#[test]
fn reversible_completion_native_sse_requires_strict_pairing_status_and_no_late_frames() {
    use super::{
        completion_evidence::{CompletionObservation as O, CompletionTransport},
        completion_json::NativeSseCompletion,
        payload_budget::PayloadBudget,
    };
    use crate::proxy::handlers::responses_sse_to_response_value_observed;

    let run = |body: &str, reached_eof: bool| -> bool {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let mut transport = CompletionTransport::default();
        let mut native = NativeSseCompletion::new();
        let mut tail = false;
        let _ = responses_sse_to_response_value_observed(body, |fact| {
            if let O::BufferedBodyEnd { unfinished_frame } = fact {
                tail = unfinished_frame;
                return;
            }
            transport.observe(&fact);
            native.observe(&fact, &budget);
        });
        let end = O::EntityEnd {
            reached_eof,
            unfinished_frame: tail,
            unfinished_utf8: false,
        };
        transport.observe(&end);
        native.observe(&end, &budget);
        assert_eq!(budget.used(), 0);
        transport.permits_completion() && native.permits_completion()
    };

    let valid = "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"output\":[]}}\n\n";
    assert!(run(valid, true), "valid native SSE denied");

    let mut admitted = Vec::new();
    let cases = [
        ("incomplete-status", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"incomplete\",\"output\":[]}}\n\n"),
        ("error-on-response", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"error\":{\"message\":\"x\"},\"output\":[]}}\n\n"),
        ("missing-id", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"),
        ("duplicate-key-in-raw", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"failed\",\"status\":\"completed\",\"output\":[]}}\n\n"),
        ("ignored-failed-tail", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"late\"}}}\n"),
        ("double-completed", "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"),
        ("event-type-mismatch", "event: response.completed\ndata: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"),
        ("bad-function-args", "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"tool\",\"arguments\":\"{\"}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"),
    ];
    for (name, body) in cases {
        if run(body, true) {
            admitted.push(name);
        }
    }
    if run(valid, false) {
        admitted.push("eof-false");
    }
    assert!(
        admitted.is_empty(),
        "invalid native SSE admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_json_and_compact_require_strict_envelopes_and_reject_repairs() {
    use super::{
        completion_json::{
            permits_chat_argument_normalization, permits_xai_argument_normalization,
            JsonCompletion, JsonCompletionProtocol as P,
        },
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let check_json = |protocol: P, raw: &str| -> bool {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let mut validator = JsonCompletion::new(protocol);
        validator.observe_raw_json(raw, &budget);
        let ok = validator.permits_completion();
        drop(validator);
        assert_eq!(budget.used(), 0);
        ok
    };

    assert!(check_json(
        P::NativeResponses,
        r#"{"id":"resp_1","object":"response","status":"completed","output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"ok"}]}]}"#
    ));
    assert!(check_json(
        P::ChatCompletions,
        r#"{"id":"chat_1","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"ok"}}]}"#
    ));
    assert!(check_json(
        P::AnthropicMessages,
        r#"{"id":"msg_1","type":"message","role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"ok"}]}"#
    ));
    assert!(check_json(
        P::ResponsesCompact,
        r#"{"id":"cmp_1","object":"response.compaction","created_at":10,"output":[{"id":"cmp_item_1","type":"compaction","encrypted_content":"opaque_blob"}]}"#
    ));

    let mut admitted = Vec::new();
    for (name, protocol, raw) in [
        (
            "native-duplicate",
            P::NativeResponses,
            r#"{"id":"resp_1","status":"failed","status":"completed","output":[]}"#,
        ),
        (
            "native-error",
            P::NativeResponses,
            r#"{"id":"resp_1","status":"completed","error":{"message":"bad"},"output":[]}"#,
        ),
        (
            "native-bad-tool",
            P::NativeResponses,
            r#"{"id":"resp_1","status":"completed","output":[{"id":"fc_1","type":"function_call","call_id":"c1","name":"t","arguments":""}]}"#,
        ),
        (
            "chat-missing-reason",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"message":{"role":"assistant","content":"ok"}}]}"#,
        ),
        (
            "chat-multi-choice",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"a"}},{"index":1,"finish_reason":"stop","message":{"role":"assistant","content":"b"}}]}"#,
        ),
        (
            "chat-empty-tool-args",
            P::ChatCompletions,
            r#"{"id":"chat_1","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"t","arguments":""}}]}}]}"#,
        ),
        (
            "anthropic-missing-stop",
            P::AnthropicMessages,
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"ok"}]}"#,
        ),
        (
            "anthropic-read-repair",
            P::AnthropicMessages,
            r#"{"id":"msg_1","type":"message","role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"c1","name":"Read","input":{"file_path":"a","pages":""}}]}"#,
        ),
        (
            "compact-empty-output",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":10,"output":[]}"#,
        ),
        (
            "compact-no-compaction-item",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":10,"output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"ok"}]}]}"#,
        ),
        (
            "compact-tool-denied",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":10,"output":[{"id":"cmp_item_1","type":"compaction","encrypted_content":"opaque"},{"id":"fc_1","type":"function_call","call_id":"c1","name":"t","arguments":"{}"}]}"#,
        ),
        (
            "compact-empty-opaque",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":10,"output":[{"id":"cmp_item_1","type":"compaction","encrypted_content":""}]}"#,
        ),
        (
            "compact-negative-created",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":-1,"output":[{"id":"cmp_item_1","type":"compaction","encrypted_content":"opaque"}]}"#,
        ),
        (
            "compact-error",
            P::ResponsesCompact,
            r#"{"id":"cmp_1","object":"response.compaction","created_at":1,"error":{},"output":[{"id":"cmp_item_1","type":"compaction","encrypted_content":"opaque"}]}"#,
        ),
    ] {
        if check_json(protocol, raw) {
            admitted.push(name);
        }
    }

    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    assert!(permits_chat_argument_normalization(
        Some(&json!(" { \"b\" : 1 , \"a\" : [ -0.0 , \"\\u0061\" ] } ")),
        r#"{"a":[-0.0,"a"],"b":1}"#,
        &budget,
    )
    .unwrap());
    for (name, orig, norm) in [
        ("chat-norm-none", None, "{}"),
        ("chat-norm-empty", Some(json!("")), "{}"),
        ("chat-norm-object", Some(json!({"a":1})), r#"{"a":1}"#),
        (
            "chat-norm-dup",
            Some(json!(r#"{"a":1,"a":2}"#)),
            r#"{"a":2}"#,
        ),
        (
            "chat-norm-signed-zero",
            Some(json!(r#"{"a":-0.0}"#)),
            r#"{"a":0.0}"#,
        ),
        (
            "chat-norm-float-int",
            Some(json!(r#"{"a":1.0}"#)),
            r#"{"a":1}"#,
        ),
    ] {
        if permits_chat_argument_normalization(orig.as_ref(), norm, &budget).unwrap_or(false) {
            admitted.push(name);
        }
    }

    assert!(permits_xai_argument_normalization(
        r#"{"session_id":92116.0,"nested":{"n":-4.0},"keep":1.5}"#,
        r#"{"session_id":92116,"nested":{"n":-4},"keep":1.5}"#,
        true,
        &budget,
    )
    .unwrap());
    for (name, orig, norm, applied) in [
        ("xai-not-applied", r#"{"a":1.0}"#, r#"{"a":1}"#, false),
        ("xai-dup-key", r#"{"a":1.0,"a":2.0}"#, r#"{"a":2}"#, true),
        ("xai-fraction-changed", r#"{"a":1.5}"#, r#"{"a":1}"#, true),
        (
            "xai-key-reordered",
            r#"{"b":1.0,"a":2}"#,
            r#"{"a":2,"b":1}"#,
            true,
        ),
        (
            "xai-negative-zero-to-int",
            r#"{"a":-0.0}"#,
            r#"{"a":0}"#,
            true,
        ),
    ] {
        if permits_xai_argument_normalization(orig, norm, applied, &budget).unwrap_or(false) {
            admitted.push(name);
        }
    }
    assert_eq!(budget.used(), 0);
    assert!(
        admitted.is_empty(),
        "strict JSON/compact/normalization admitted invalid cases: {admitted:?}"
    );
}

#[test]
fn private_native_null_completion_fields_are_absent_but_nonnull_errors_stick() {
    use super::{
        completion_evidence::{CompletionObservation as O, NativeCompletionEvent},
        completion_json::{JsonCompletion, JsonCompletionProtocol as P, NativeSseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let response = json!({"id":"resp_null_fixture","object":"response","status":"completed","error":null,"incomplete_details":null,"output":[]});
    let mut json_evidence = JsonCompletion::new(P::NativeResponses);
    json_evidence.observe_raw_json(&response.to_string(), &budget);
    let event = json!({"type":"response.completed","error":null,"response":response});
    let mut sse = NativeSseCompletion::new();
    sse.observe(
        &O::RawFrame(&format!("event: response.completed\ndata: {event}\n\n")),
        &budget,
    );
    sse.observe(
        &O::NativeEvent {
            event_name: "response.completed",
            data: &event,
        },
        &budget,
    );
    assert!(
        json_evidence.permits_completion()
            && NativeCompletionEvent::new("response.completed", &event).permits_completion()
            && sse.permits_completion(),
        "native JSON, terminal classifier and paired SSE must permit null optional fields"
    );
    for path in ["top", "response", "incomplete"] {
        let mut failed = event.clone();
        match path {
            "top" => failed["error"] = json!({"code":"fixture_error"}),
            "response" => failed["response"]["error"] = json!({"code":"fixture_error"}),
            _ => failed["response"]["incomplete_details"] = json!({"reason":"max_output_tokens"}),
        }
        let mut evidence = NativeSseCompletion::new();
        for value in [&failed, &event] {
            evidence.observe(
                &O::RawFrame(&format!("event: response.completed\ndata: {value}\n\n")),
                &budget,
            );
            evidence.observe(
                &O::NativeEvent {
                    event_name: "response.completed",
                    data: value,
                },
                &budget,
            );
        }
        assert!(
            !evidence.permits_completion(),
            "nonnull failure evidence stays sticky"
        );
    }
    assert_eq!(budget.used(), 0);
}

fn chat_carrier_fixture() -> (super::tokenization::DlpRequestLease, String) {
    use super::{
        payload_budget::PayloadBudget,
        tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    };
    let store = DlpTokenStore::new();
    let mut lease = store
        .admit("codex", Some("synthetic-carrier-session"), 1)
        .unwrap();
    lease
        .prepare_with_budget(
            &[TokenFragment {
                raw: "synthetic-restoration-value",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            PayloadBudget::with_limit(4 * 1024 * 1024),
            |_| Ok(()),
        )
        .unwrap();
    let token = lease.authorized_tokens().unwrap().as_slice()[0].to_owned();
    (lease, token)
}

fn chat_carrier_json(message: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id":"synthetic-carrier-chat",
        "choices":[{"index":0,"finish_reason":"stop","message":message}]
    })
}

fn chat_carrier_json_permitted(body: &serde_json::Value) -> bool {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
    completion.observe_raw_json(
        &body.to_string(),
        &PayloadBudget::with_limit(4 * 1024 * 1024),
    );
    completion.permits_completion()
}

#[test]
fn reversible_completion_chat_carriers_json_accepts_converter_text_and_refusal() {
    use crate::proxy::providers::transform_codex_chat::chat_completion_to_response;
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    for (name, message, field) in [
        (
            "string",
            json!({"role":"assistant","content":token}),
            "text",
        ),
        (
            "text",
            json!({"role":"assistant","content":[{"type":"text","text":token}]}),
            "text",
        ),
        (
            "output-text",
            json!({"role":"assistant","content":[{"type":"output_text","text":token}]}),
            "text",
        ),
        (
            "array-refusal",
            json!({"role":"assistant","content":[{"type":"refusal","refusal":token}]}),
            "refusal",
        ),
        (
            "message-refusal",
            json!({"role":"assistant","content":null,"refusal":token}),
            "refusal",
        ),
    ] {
        let body = chat_carrier_json(message);
        assert!(
            chat_carrier_json_permitted(&body),
            "supported carrier denied: {name}"
        );
        let converted = chat_completion_to_response(body).unwrap();
        assert!(
            converted["output"][0]["content"][0][field]
                .as_str()
                .is_some_and(|v| v == token),
            "converter lost supported carrier: {name}"
        );
    }
    for content in [json!(null), json!(""), json!([])] {
        assert!(chat_carrier_json_permitted(&chat_carrier_json(
            json!({"role":"assistant","content":content})
        )));
    }
    assert!(chat_carrier_json_permitted(&chat_carrier_json(
        json!({"role":"assistant"})
    )));
}

#[test]
fn reversible_completion_chat_carriers_json_rejects_unknown_array_parts() {
    use crate::proxy::providers::transform_codex_chat::chat_completion_to_response;
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    let mut admitted = Vec::new();
    for (name, part) in [
        ("future-text", json!({"type":"future_text","text":token})),
        ("missing-type", json!({"text":token})),
        ("object-text", json!({"type":"text","text":{"value":token}})),
        ("array-text", json!({"type":"output_text","text":[token]})),
        (
            "object-refusal",
            json!({"type":"refusal","refusal":{"value":token}}),
        ),
        ("string-part", json!(token)),
        (
            "extra-text-carrier",
            json!({"type":"text","text":"synthetic","future_text":token}),
        ),
    ] {
        let body = chat_carrier_json(json!({"role":"assistant","content":[part]}));
        let converted = chat_completion_to_response(body.clone()).unwrap();
        assert!(
            !converted.to_string().contains(&token),
            "fixture was not dropped: {name}"
        );
        if chat_carrier_json_permitted(&body) {
            admitted.push(name);
        }
    }
    assert!(
        admitted.is_empty(),
        "dropped array carriers admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_chat_carriers_json_rejects_wrong_content_types() {
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    let mut admitted = Vec::new();
    for (name, message) in [
        ("object-content", json!({"content":{"text":token}})),
        ("number-content", json!({"content":7})),
        ("bool-content", json!({"content":true})),
        ("number-text", json!({"content":[{"type":"text","text":7}]})),
        ("missing-text", json!({"content":[{"type":"output_text"}]})),
        (
            "null-refusal",
            json!({"content":[{"type":"refusal","refusal":null}]}),
        ),
        ("structured-refusal", json!({"refusal":{"text":token}})),
    ] {
        if chat_carrier_json_permitted(&chat_carrier_json(message)) {
            admitted.push(name);
        }
    }
    assert!(
        admitted.is_empty(),
        "wrong Chat content types admitted: {admitted:?}"
    );
}

#[test]
fn reversible_completion_chat_carriers_json_rejects_unknown_message_carrier() {
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    for message in [
        json!({"content":"synthetic","future_text":token}),
        json!({"content":null,"audio":{"transcript":token}}),
    ] {
        assert!(
            !chat_carrier_json_permitted(&chat_carrier_json(message)),
            "unconverted message carrier authorized completion"
        );
    }
}

fn chat_carrier_sse(delta: serde_json::Value, tool: bool) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({
            "id":"synthetic-carrier-chat",
            "choices":[{"index":0,"delta":delta,"finish_reason":if tool { "tool_calls" } else { "stop" }}]
        })
    )
}

#[tokio::test]
async fn reversible_completion_chat_carriers_sse_rejects_unconverted_carriers() {
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    let mut admitted = Vec::new();
    for (name, delta) in [
        ("future-carrier", json!({"future_text":token})),
        ("refusal", json!({"refusal":token})),
        ("structured-refusal", json!({"refusal":{"text":token}})),
        ("audio", json!({"audio":{"transcript":token}})),
        (
            "future-content",
            json!({"content":[{"type":"future_text","text":token}]}),
        ),
        (
            "array-text",
            json!({"content":[{"type":"text","text":token}]}),
        ),
        ("object-content", json!({"content":{"text":token}})),
        ("number-content", json!({"content":7})),
    ] {
        if strict_sse_candidate(false, chat_carrier_sse(delta, false)).await {
            admitted.push(name);
        }
    }
    assert!(
        admitted.is_empty(),
        "dropped SSE carriers admitted: {admitted:?}"
    );
}

#[tokio::test]
async fn reversible_completion_chat_carriers_preserves_reasoning_and_tools() {
    use crate::proxy::providers::transform_codex_chat::chat_completion_to_response;
    use serde_json::json;
    let (_lease, token) = chat_carrier_fixture();
    for (name, reasoning) in [
        ("reasoning-content", json!({"reasoning_content":token})),
        ("reasoning-string", json!({"reasoning":token})),
        ("reasoning-object", json!({"reasoning":{"content":token}})),
        ("reasoning-text", json!({"reasoning":{"text":token}})),
        ("reasoning-summary", json!({"reasoning":{"summary":token}})),
        ("details-string", json!({"reasoning_details":token})),
        (
            "details-array",
            json!({"reasoning_details":[{"text":token}]}),
        ),
        (
            "details-object",
            json!({"reasoning_details":{"parts":[{"content":token}]}}),
        ),
    ] {
        let mut message = reasoning.clone();
        message["role"] = json!("assistant");
        message["content"] = json!("synthetic answer");
        let body = chat_carrier_json(message);
        assert!(
            chat_carrier_json_permitted(&body),
            "JSON reasoning denied: {name}"
        );
        let converted = chat_completion_to_response(body).unwrap();
        assert!(
            converted["output"][0]["summary"][0]["text"]
                .as_str()
                .is_some_and(|v| v == token),
            "converter lost supported reasoning: {name}"
        );
        assert!(
            strict_sse_candidate(false, chat_carrier_sse(reasoning, false)).await,
            "SSE reasoning denied: {name}"
        );
    }
    let tool = json!({"index":0,"id":"synthetic-call","type":"function","function":{
        "name":"synthetic_tool","arguments":json!({"value":token}).to_string()
    }});
    let message = json!({"role":"assistant","content":null,"reasoning_content":"synthetic reasoning","tool_calls":[tool]});
    let mut body = chat_carrier_json(message);
    body["choices"][0]["finish_reason"] = json!("tool_calls");
    assert!(chat_carrier_json_permitted(&body), "JSON tool denied");
    assert!(strict_sse_candidate(false, chat_carrier_sse(json!({"content":null,"reasoning_content":"synthetic reasoning","tool_calls":[tool]}), true)).await,
        "SSE tool denied");
    assert!(
        strict_sse_candidate(
            false,
            chat_carrier_sse(
                json!({"role":"assistant","content":token,"refusal":null}),
                false
            )
        )
        .await,
        "supported SSE text/null refusal denied"
    );
}

#[test]
fn reversible_completion_chat_json_unsupported_classification_matches_carrier_validation() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    for (message, unsupported) in [
        (json!({"role":"assistant","content":"synthetic"}), false),
        (
            json!({"content":[{"type":"text","text":"synthetic"}]}),
            false,
        ),
        (json!({"content":null,"refusal":"synthetic"}), false),
        (json!({"content":"synthetic","future_text":null}), false),
        (json!({"future_text":"synthetic"}), true),
        (json!({"audio":{"transcript":"synthetic"}}), true),
        (
            json!({"content":[{"type":"future_text","text":"synthetic"}]}),
            true,
        ),
        (
            json!({"content":[{"type":"text","text":"synthetic","future_text":"synthetic"}]}),
            true,
        ),
        (json!({"content":{"text":"synthetic"}}), true),
    ] {
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
        assert!(!completion.has_unsupported_carrier());
        completion.observe_raw_json(&chat_carrier_json(message).to_string(), &budget);
        assert_eq!(completion.has_unsupported_carrier(), unsupported);
        assert_eq!(completion.permits_completion(), !unsupported);
        if unsupported {
            completion.observe_raw_json(
                &chat_carrier_json(json!({"content":"synthetic"})).to_string(),
                &budget,
            );
            completion.observe_raw_json("{malformed synthetic JSON}", &budget);
            assert!(completion.has_unsupported_carrier());
            assert!(!completion.permits_completion());
        }
    }
}

#[test]
fn reversible_completion_chat_json_ordinary_failures_are_not_unsupported() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let valid = chat_carrier_json(json!({"content":"synthetic"}));
    for raw in [
        "{malformed synthetic JSON}".to_string(),
        json!({"id":"synthetic","choices":[]}).to_string(),
        json!({"id":"synthetic","choices":[{"finish_reason":"stop","message":null}]}).to_string(),
        chat_carrier_json(json!({"role":"user","content":"synthetic"})).to_string(),
        chat_carrier_json(json!({"tool_calls":7})).to_string(),
        {
            let mut body = valid.clone();
            body["choices"][0]["finish_reason"] = json!("length");
            body.to_string()
        },
    ] {
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
        completion.observe_raw_json(&raw, &budget);
        assert!(!completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
        completion.observe_raw_json(&valid.to_string(), &budget);
        assert!(!completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
    }
}

fn observe_chat_classification_chunk(
    completion: &mut super::completion_evidence::SseCompletion,
    chunk: &serde_json::Value,
    budget: &super::payload_budget::PayloadBudget,
) {
    use super::completion_evidence::CompletionObservation as O;
    completion.observe(&O::RawFrame(&format!("data: {chunk}")), budget);
    completion.observe(&O::ChatChunk(chunk), budget);
}

#[test]
fn reversible_completion_chat_sse_unsupported_classification_is_sticky() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    for delta in [
        json!({"future_text":"synthetic"}),
        json!({"refusal":"synthetic"}),
        json!({"audio":{"transcript":"synthetic"}}),
        json!({"content":[{"type":"text","text":"synthetic"}]}),
        json!({"content":{"text":"synthetic"}}),
    ] {
        let mut completion = SseCompletion::chat();
        assert!(!completion.has_unsupported_carrier());
        let chunk =
            json!({"id":"synthetic","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        observe_chat_classification_chunk(&mut completion, &chunk, &budget);
        assert!(completion.has_unsupported_carrier());
        let terminal = json!({"id":"synthetic","choices":[{"index":0,"delta":{"content":"synthetic"},"finish_reason":"stop"}]});
        observe_chat_classification_chunk(&mut completion, &terminal, &budget);
        completion.observe(&O::RawFrame("data: [DONE]"), &budget);
        completion.observe(&O::ChatDone, &budget);
        completion.observe(
            &O::EntityEnd {
                reached_eof: true,
                unfinished_frame: false,
                unfinished_utf8: false,
            },
            &budget,
        );
        assert!(completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
        completion.observe(&O::IoFailure, &budget);
        assert!(completion.has_unsupported_carrier());
    }
}

#[test]
fn reversible_completion_chat_sse_ordinary_failures_are_not_unsupported() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let terminal = json!({"id":"synthetic","choices":[{"index":0,"delta":{"content":"synthetic"},"finish_reason":"stop"}]});
    for fact in [
        O::InvalidJson,
        O::InvalidUtf8,
        O::IoFailure,
        O::RawFrame("event: unsupported\ndata: {}"),
        O::ChatDone,
    ] {
        let mut completion = SseCompletion::chat();
        completion.observe(&fact, &budget);
        observe_chat_classification_chunk(&mut completion, &terminal, &budget);
        assert!(!completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
    }
    for (delta, reason, permitted) in [
        (
            json!({"content":"synthetic","future_text":null}),
            "stop",
            true,
        ),
        (json!({"role":"user"}), "stop", false),
        (json!({"tool_calls":7}), "stop", false),
        (json!(null), "stop", false),
        (json!({}), "length", false),
    ] {
        let mut completion = SseCompletion::chat();
        let chunk =
            json!({"id":"synthetic","choices":[{"index":0,"delta":delta,"finish_reason":reason}]});
        observe_chat_classification_chunk(&mut completion, &chunk, &budget);
        completion.observe(&O::RawFrame("data: [DONE]"), &budget);
        completion.observe(&O::ChatDone, &budget);
        assert!(!completion.has_unsupported_carrier());
        assert_eq!(completion.permits_completion(), permitted);
    }
}

fn anthropic_carrier_json(blocks: serde_json::Value, tool: bool) -> serde_json::Value {
    serde_json::json!({
        "type":"message", "id":"synthetic-carrier-message", "role":"assistant",
        "content":blocks, "stop_reason":if tool { "tool_use" } else { "end_turn" }
    })
}

fn observe_anthropic_classification_event(
    completion: &mut super::completion_evidence::SseCompletion,
    event: &serde_json::Value,
    budget: &super::payload_budget::PayloadBudget,
) {
    use super::completion_evidence::CompletionObservation as O;
    completion.observe(&O::RawFrame(&anthropic_frame(event.clone())), budget);
    completion.observe(
        &O::AnthropicEvent {
            event_name: event["type"].as_str(),
            data: event,
        },
        budget,
    );
}

fn anthropic_classification_start() -> serde_json::Value {
    serde_json::json!({"type":"message_start","message":{
        "id":"synthetic-carrier-message","type":"message","role":"assistant","content":[]
    }})
}

#[test]
fn reversible_completion_anthropic_carriers_json_unknown_fields_and_types_are_sticky() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let (_lease, token) = chat_carrier_fixture();
    for (block, tool) in [
        (
            json!({"type":"text","text":"synthetic ordinary text","future_text":token}),
            false,
        ),
        (
            json!({"type":"text","text":"synthetic","future_text":null}),
            false,
        ),
        (
            json!({"type":"thinking","thinking":"synthetic","signature":"opaque","future_text":token}),
            false,
        ),
        (
            json!({"type":"redacted_thinking","data":"opaque","future_text":token}),
            false,
        ),
        (
            json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{},"future_text":token}),
            true,
        ),
        (json!({"type":"future_text","text":token}), false),
    ] {
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::AnthropicMessages);
        assert!(!completion.has_unsupported_carrier());
        completion.observe_raw_json(
            &anthropic_carrier_json(json!([block]), tool).to_string(),
            &budget,
        );
        assert!(
            !completion.permits_completion(),
            "unknown original Anthropic carrier admitted"
        );
        assert!(
            completion.has_unsupported_carrier(),
            "unknown original Anthropic carrier not classified"
        );
        completion.observe_raw_json(
            &anthropic_carrier_json(json!([]), false).to_string(),
            &budget,
        );
        completion.observe_raw_json("{malformed synthetic JSON}", &budget);
        assert!(completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
    }
}

#[test]
fn reversible_completion_anthropic_carriers_json_preserves_reasoning_tools_and_opaque_fields() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use crate::proxy::providers::transform_codex_anthropic::anthropic_response_to_responses;
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let input = json!({"future_text":"synthetic tool text","nested":[{"type":"future_text","signature":"synthetic"}]});
    let mut body = anthropic_carrier_json(
        json!([
            {"type":"text","text":"synthetic text"},
            {"type":"thinking","thinking":"synthetic reasoning","signature":"synthetic opaque signature"},
            {"type":"redacted_thinking","data":"synthetic opaque data"},
            {"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":input}
        ]),
        true,
    );
    body["metadata"] = json!({"future_text":"synthetic metadata"});
    body["usage"] = json!({"future_text":"synthetic usage"});
    body["schema"] = json!({"future_text":"synthetic schema"});
    let mut completion = JsonCompletion::new(JsonCompletionProtocol::AnthropicMessages);
    completion.observe_raw_json(&body.to_string(), &budget);
    assert!(completion.permits_completion());
    assert!(!completion.has_unsupported_carrier());
    let converted = anthropic_response_to_responses(body).unwrap();
    assert_eq!(
        converted["output"][0]["content"][0]["text"],
        "synthetic text"
    );
    assert_eq!(
        converted["output"][1]["summary"][0]["text"],
        "synthetic reasoning"
    );
    assert!(converted["output"][1]["encrypted_content"].is_string());
    assert!(converted["output"][2]["encrypted_content"].is_string());
    let arguments: serde_json::Value =
        serde_json::from_str(converted["output"][3]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(arguments, input);
}

#[test]
fn reversible_completion_anthropic_carriers_sse_unknown_blocks_and_deltas_are_sticky() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let (_lease, token) = chat_carrier_fixture();
    let text = json!({"type":"text","text":""});
    let thinking = json!({"type":"thinking","thinking":"","signature":"opaque"});
    let tool = json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{}});
    let mut cases = Vec::new();
    for block in [
        json!({"type":"text","text":"synthetic","future_text":token}),
        json!({"type":"thinking","thinking":"synthetic","signature":"opaque","future_text":token}),
        json!({"type":"redacted_thinking","data":"opaque","future_text":token}),
        json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{},"future_text":token}),
        json!({"type":"future_text","text":token}),
    ] {
        cases.push((block, None));
    }
    for (block, delta) in [
        (
            text.clone(),
            json!({"type":"text_delta","text":"synthetic ordinary text","future_text":token}),
        ),
        (
            text.clone(),
            json!({"type":"text_delta","text":"synthetic","future_text":null}),
        ),
        (
            thinking.clone(),
            json!({"type":"thinking_delta","thinking":"synthetic","future_text":token}),
        ),
        (
            thinking,
            json!({"type":"signature_delta","signature":"opaque","future_text":token}),
        ),
        (
            tool,
            json!({"type":"input_json_delta","partial_json":"{}","future_text":token}),
        ),
        (text, json!({"type":"future_delta","text":token})),
    ] {
        cases.push((block, Some(delta)));
    }
    for (block, delta) in cases {
        let mut completion = SseCompletion::anthropic();
        assert!(!completion.has_unsupported_carrier());
        observe_anthropic_classification_event(
            &mut completion,
            &anthropic_classification_start(),
            &budget,
        );
        observe_anthropic_classification_event(
            &mut completion,
            &json!({"type":"content_block_start","index":0,"content_block":block}),
            &budget,
        );
        if let Some(delta) = delta {
            observe_anthropic_classification_event(
                &mut completion,
                &json!({"type":"content_block_delta","index":0,"delta":delta}),
                &budget,
            );
        }
        assert!(
            completion.has_unsupported_carrier(),
            "unknown original Anthropic SSE carrier not classified"
        );
        for event in [
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ] {
            observe_anthropic_classification_event(&mut completion, &event, &budget);
        }
        completion.observe(
            &O::EntityEnd {
                reached_eof: true,
                unfinished_frame: false,
                unfinished_utf8: false,
            },
            &budget,
        );
        assert!(completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
        completion.observe(&O::InvalidJson, &budget);
        completion.observe(&O::IoFailure, &budget);
        assert!(completion.has_unsupported_carrier());
    }
}

#[test]
fn reversible_completion_anthropic_carriers_sse_message_start_content_is_classified() {
    use super::{completion_evidence::SseCompletion, payload_budget::PayloadBudget};
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    for block in [
        json!({"type":"text","text":"synthetic","future_text":"synthetic"}),
        json!({"type":"future_text","text":"synthetic"}),
    ] {
        let mut start = anthropic_classification_start();
        start["message"]["content"] = json!([block]);
        let mut completion = SseCompletion::anthropic();
        observe_anthropic_classification_event(&mut completion, &start, &budget);
        assert!(completion.has_unsupported_carrier());
        assert!(!completion.permits_completion());
    }
}

#[test]
fn reversible_completion_anthropic_carriers_sse_message_delta_is_classified() {
    use super::{completion_evidence::SseCompletion, payload_budget::PayloadBudget};
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    for field in ["future_text", "content", "thinking"] {
        let mut delta = json!({"stop_reason":"end_turn","stop_sequence":null});
        delta[field] = json!("synthetic dropped text");
        let mut completion = SseCompletion::anthropic();
        for event in [
            anthropic_classification_start(),
            json!({"type":"message_delta","delta":delta}),
            json!({"type":"message_stop"}),
        ] {
            observe_anthropic_classification_event(&mut completion, &event, &budget);
        }
        assert!(
            !completion.permits_completion(),
            "dropped message delta admitted"
        );
        assert!(completion.has_unsupported_carrier());
    }
}

#[test]
fn reversible_completion_anthropic_carriers_sse_preserves_reasoning_tools_and_opaque_fields() {
    use super::{completion_evidence::SseCompletion, payload_budget::PayloadBudget};
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let input = json!({"future_text":"synthetic input","nested":{"type":"future_text","data":[1,null,true]}});
    for streamed_input in [false, true] {
        let mut completion = SseCompletion::anthropic();
        let mut start = anthropic_classification_start();
        start["message"]["model"] = json!("synthetic-model");
        start["message"]["metadata"] = json!({"future_text":"synthetic"});
        let mut events = vec![
            start,
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"synthetic","signature":"opaque"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":" reasoning"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic opaque signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"synthetic opaque data"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":if streamed_input { json!({}) } else { input.clone() }}}),
        ];
        if streamed_input {
            events.push(json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":input.to_string()}}));
        }
        events.extend([
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"future_text":"synthetic"}}),
            json!({"type":"message_stop"}),
        ]);
        for event in events {
            observe_anthropic_classification_event(&mut completion, &event, &budget);
        }
        assert!(completion.permits_completion());
        assert!(!completion.has_unsupported_carrier());
    }
}

#[test]
fn reversible_completion_anthropic_carriers_malformed_known_fields_remain_ordinary_failures() {
    use super::{
        completion_evidence::{CompletionObservation as O, SseCompletion},
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    for block in [
        json!({}),
        json!({"type":"text"}),
        json!({"type":"text","text":7}),
        json!({"type":"thinking","thinking":7}),
        json!({"type":"redacted_thinking","data":7}),
        json!({"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":7}),
    ] {
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::AnthropicMessages);
        completion.observe_raw_json(
            &anthropic_carrier_json(json!([block]), false).to_string(),
            &budget,
        );
        assert!(!completion.permits_completion());
        assert!(!completion.has_unsupported_carrier());
        let mut completion = SseCompletion::anthropic();
        for event in [
            anthropic_classification_start(),
            json!({"type":"content_block_start","index":0,"content_block":block}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ] {
            observe_anthropic_classification_event(&mut completion, &event, &budget);
        }
        assert!(!completion.permits_completion());
        assert!(!completion.has_unsupported_carrier());
    }
    for delta in [
        json!({}),
        json!({"type":"text_delta"}),
        json!({"type":"text_delta","text":7}),
    ] {
        let mut completion = SseCompletion::anthropic();
        for event in [
            anthropic_classification_start(),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":delta}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ] {
            observe_anthropic_classification_event(&mut completion, &event, &budget);
        }
        assert!(!completion.permits_completion());
        assert!(!completion.has_unsupported_carrier());
    }
    let mut completion = SseCompletion::anthropic();
    for event in [
        anthropic_classification_start(),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
    ] {
        observe_anthropic_classification_event(&mut completion, &event, &budget);
    }
    completion.observe(
        &O::EntityEnd {
            reached_eof: true,
            unfinished_frame: false,
            unfinished_utf8: false,
        },
        &budget,
    );
    assert!(!completion.permits_completion());
    assert!(!completion.has_unsupported_carrier());
}

// Direct-wrapper evidence only: these controls do not traverse an HTTP listener.
async fn closure_anthropic_witness_sequence(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    use super::{
        completion_evidence::CompletionObservation as O,
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        response_restoration::ArgumentNormalizationMode,
    };
    use axum::body::Body;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"model":"synthetic-model","input":"13800138000","stream":true});
    let request = match admit_http_request(
        &gateway,
        &crate::app_config::AppType::Codex,
        true,
        false,
        &mut body,
        &http::HeaderMap::new(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic witness request was not admitted".into()),
    };
    let (attempt, _owner) = request.prepare_owned_attempt_for_proxy(
        "synthetic-witness",
        "anthropic",
        "http://127.0.0.1:1/v1/messages",
        Some("synthetic-witness-principal"),
        &mut body,
        Ok,
    )?;
    let tracker = attempt
        .upstream_sse_evidence_tracker()
        .ok_or("missing synthetic witness tracker")?;
    let token = body["input"]
        .as_str()
        .ok_or("missing synthetic witness carrier")?;
    assert!(token.contains("rredact-cn_phone-") && !token.contains("13800138000"));
    let original = json!({"id":"msg_witness","type":"message","role":"assistant","model":"synthetic-model",
        "content":[{"type":"tool_use","id":"call_witness","name":"synthetic_lookup","input":{"phone":token}}],
        "stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}});
    let raw = original.to_string();
    {
        let mut tracker = tracker
            .lock()
            .map_err(|_| "synthetic witness lock poisoned")?;
        match mode {
            "json_control" => tracker.observe_fully_read_anthropic_json(&raw),
            "double_json" => {
                tracker.observe_fully_read_anthropic_json(&raw);
                tracker.observe_fully_read_anthropic_json(&raw);
            }
            "json_then_sse" => {
                tracker.observe_fully_read_anthropic_json(&raw);
                tracker.observe(&O::RawFrame(
                    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                ));
                tracker.observe(&O::AnthropicEvent {
                    event_name: Some("message_stop"),
                    data: &json!({"type":"message_stop"}),
                });
                tracker.observe(&O::EntityEnd {
                    reached_eof: true,
                    unfinished_frame: false,
                    unfinished_utf8: false,
                });
            }
            "sse_then_json" => {
                tracker.observe(&O::RawFrame("event: message_start\ndata: {}\n\n"));
                tracker.observe_fully_read_anthropic_json(&raw);
            }
            "raw_json_without_entity_eof" => {
                tracker.observe(&O::RawJson(&raw));
                tracker.observe(&O::AnthropicJson(&original));
                tracker.observe(&O::BufferedBodyEnd {
                    unfinished_frame: false,
                });
            }
            "io_then_json" => {
                tracker.observe(&O::IoFailure);
                tracker.observe_fully_read_anthropic_json(&raw);
            }
            _ => return Err("unknown synthetic witness mode".into()),
        }
    }
    let item = json!({"type":"function_call","id":"fc_witness","call_id":"call_witness","name":"synthetic_lookup",
        "arguments":json!({"phone":token}).to_string(),"status":"completed"});
    let wire = format!(
        "event: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"resp_witness","object":"response","status":"completed","output":[item]}})
    );
    let response = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(wire))?;
    let restored = attempt
        .restore_client_http_response(response, ArgumentNormalizationMode::Exact, Some(tracker))
        .await;
    let bytes = restored.into_body().collect().await?.to_bytes();
    let text = std::str::from_utf8(&bytes)?;
    let events: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let tools: Vec<_> = events
        .iter()
        .filter(|event| {
            event["type"] == "response.output_item.done" && event["item"]["type"] == "function_call"
        })
        .collect();
    let success = mode == "json_control";
    assert_eq!(
        tools.len(),
        usize::from(success),
        "double/mixed witnesses cannot release buffered tools; mode={mode}"
    );
    assert_eq!(
        events
            .iter()
            .any(|event| event["type"] == "response.completed"),
        success
    );
    assert_eq!(
        events
            .iter()
            .any(|event| event["type"] == "response.failed"),
        !success
    );
    assert_eq!(text.contains("13800138000"), success);
    assert!(!text.contains("rredact-cn_phone-"));
    Ok(())
}

macro_rules! closure_witness_test {
    ($name:ident, $mode:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> Result<(), Box<dyn std::error::Error>> {
            let original = (*super::runtime::get_security_runtime()).clone();
            struct Reset(super::runtime::SecurityRuntimeConfig);
            impl Drop for Reset {
                fn drop(&mut self) {
                    super::runtime::set_security_runtime_for_test(self.0.clone());
                }
            }
            let _reset = Reset(original);
            super::runtime::set_security_runtime_for_test(Default::default());
            closure_anthropic_witness_sequence($mode).await
        }
    };
}
closure_witness_test!(
    reversible_closure_resp7_json_single_witness_control,
    "json_control"
);
closure_witness_test!(
    reversible_closure_resp7_json_double_witness_refused,
    "double_json"
);
closure_witness_test!(
    reversible_closure_resp7_json_then_sse_witness_refused,
    "json_then_sse"
);
closure_witness_test!(
    reversible_closure_resp7_sse_then_json_witness_refused,
    "sse_then_json"
);
closure_witness_test!(
    reversible_closure_resp7_raw_json_without_eof_refused,
    "raw_json_without_entity_eof"
);
closure_witness_test!(
    reversible_closure_resp7_io_then_json_witness_refused,
    "io_then_json"
);

#[test]
fn reversible_closure_resp3_json_known_shapes_and_closed_carriers() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::{json, Value};
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let check = |tools: Value, permits: bool, unsupported: bool, label: &str| {
        let body = json!({"id":"chat_shape_control","choices":[{"index":0,"message":{
            "role":"assistant","content":null,"tool_calls":tools},"finish_reason":"tool_calls"}]});
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
        completion.observe_raw_json(&body.to_string(), &budget);
        assert_eq!(
            completion.permits_completion(),
            permits,
            "completion classification; case={label}"
        );
        assert_eq!(
            completion.has_unsupported_carrier(),
            unsupported,
            "carrier classification; case={label}"
        );
    };
    let tool = json!({"id":"call_shape","type":"function","function":{"name":"synthetic_tool","arguments":"{}"}});
    check(json!([tool]), true, false, "absent_json_index");
    for (label, changed) in [
        ("nonobject", Value::Null),
        (
            "missing_function",
            json!({"id":"call_shape","type":"function"}),
        ),
        (
            "null_function",
            json!({"id":"call_shape","type":"function","function":null}),
        ),
        (
            "string_function",
            json!({"id":"call_shape","type":"function","function":"synthetic"}),
        ),
        (
            "missing_name",
            json!({"id":"call_shape","type":"function","function":{"arguments":"{}"}}),
        ),
        (
            "null_name",
            json!({"id":"call_shape","type":"function","function":{"name":null,"arguments":"{}"}}),
        ),
        (
            "numeric_name",
            json!({"id":"call_shape","type":"function","function":{"name":7,"arguments":"{}"}}),
        ),
        (
            "empty_name",
            json!({"id":"call_shape","type":"function","function":{"name":"","arguments":"{}"}}),
        ),
        (
            "numeric_arguments",
            json!({"id":"call_shape","type":"function","function":{"name":"synthetic_tool","arguments":7}}),
        ),
        (
            "invalid_arguments",
            json!({"id":"call_shape","type":"function","function":{"name":"synthetic_tool","arguments":"{bad}"}}),
        ),
    ] {
        check(json!([changed]), false, false, label);
    }
    let mut both_null = tool.clone();
    both_null["future_text"] = Value::Null;
    both_null["function"]["future_text"] = Value::Null;
    both_null["index"] = json!(0);
    check(json!([both_null]), true, false, "both_unknown_null");
    for name in ["future_text", "function", "index"] {
        let mut ordinary = tool.clone();
        ordinary["function"]["name"] = json!(name);
        ordinary["function"]["arguments"] =
            json!(json!({"future_text":"synthetic ordinary", "function":{"index":1}}).to_string());
        check(json!([ordinary]), true, false, "business_keys_and_names");
    }
    for function_level in [false, true] {
        for sibling in [
            json!(""),
            json!(7),
            json!(false),
            json!({}),
            json!([]),
            json!({"nested":["rredact-cn_phone-0123456789abcdef0123-ai"]}),
        ] {
            let mut changed = tool.clone();
            if function_level {
                changed["function"]["future_text"] = sibling;
            } else {
                changed["future_text"] = sibling;
            }
            check(json!([changed]), false, true, "unknown_nonnull_sibling");
        }
    }
    for index in [Value::Null, json!(-1), json!(1.5), json!("0"), json!({})] {
        let mut changed = tool.clone();
        changed["index"] = index;
        check(json!([changed]), false, true, "non_u64_index");
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn reversible_closure_resp3_sse_known_shapes_and_index_controls() {
    use super::{
        completion_evidence::{CompletionObservation as O, CompletionTransport, SseCompletion},
        payload_budget::PayloadBudget,
    };
    use serde_json::{json, Value};
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let check = |fragments: Vec<Value>, permits: bool, unsupported: bool, label: &str| {
        let mut completion = SseCompletion::chat();
        let mut transport = CompletionTransport::default();
        for (position, tools) in fragments.iter().enumerate() {
            let chunk = json!({"id":"chat_shape_control","choices":[{"index":0,"delta":{"tool_calls":tools},
                "finish_reason":if position + 1 == fragments.len() {Some("tool_calls")} else {None}}]});
            let raw = format!("data: {chunk}\n\n");
            for fact in [O::RawFrame(&raw), O::ChatChunk(&chunk)] {
                transport.observe(&fact);
                completion.observe(&fact, &budget);
            }
        }
        for fact in [
            O::RawFrame("data: [DONE]\n\n"),
            O::ChatDone,
            O::EntityEnd {
                reached_eof: true,
                unfinished_frame: false,
                unfinished_utf8: false,
            },
        ] {
            transport.observe(&fact);
            completion.observe(&fact, &budget);
        }
        assert_eq!(
            transport.permits_completion() && completion.permits_completion(),
            permits,
            "completion classification; case={label}"
        );
        assert_eq!(
            completion.has_unsupported_carrier(),
            unsupported,
            "carrier classification; case={label}"
        );
    };
    let tool = json!({"index":0,"id":"call_shape","type":"function","function":{"name":"synthetic_tool","arguments":"{}"}});
    check(vec![json!([tool])], true, false, "index_zero");
    let mut second = tool.clone();
    second["index"] = json!(1);
    second["id"] = json!("call_shape_second");
    check(
        vec![json!([tool, second])],
        true,
        false,
        "contiguous_two_tools",
    );
    let mut first = tool.clone();
    first["function"]["arguments"] = json!("{\"future_text\":");
    first["future_text"] = Value::Null;
    first["function"]["future_text"] = Value::Null;
    check(
        vec![
            json!([first]),
            json!([{"index":0,"function":{"arguments":"\"synthetic\"}"}}]),
        ],
        true,
        false,
        "split_arguments_null_siblings",
    );
    for index in [1, 128] {
        let mut changed = tool.clone();
        changed["index"] = json!(index);
        check(
            vec![json!([changed])],
            false,
            false,
            "noncontiguous_or_out_of_range",
        );
    }
    for index in [Value::Null, json!(-1), json!("0")] {
        let mut changed = tool.clone();
        changed["index"] = index;
        check(vec![json!([changed])], false, true, "non_u64_index");
    }
    for function_level in [false, true] {
        for sibling in [json!(""), json!(7), json!(false), json!({}), json!([])] {
            let mut changed = tool.clone();
            if function_level {
                changed["function"]["future_text"] = sibling;
            } else {
                changed["future_text"] = sibling;
            }
            check(
                vec![json!([changed])],
                false,
                true,
                "unknown_nonnull_sibling",
            );
        }
    }
    for function in [
        Value::Null,
        json!("synthetic"),
        json!({"name":null,"arguments":"{}"}),
        json!({"name":"synthetic_tool","arguments":7}),
    ] {
        let mut changed = tool.clone();
        changed["function"] = function;
        check(
            vec![json!([changed])],
            false,
            false,
            "known_malformed_function",
        );
    }
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_completion_chat_sse_and_json_permit_null_optional_delta_and_tool_fields() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;

    let sse_stream = [
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": null, "role": "assistant", "content": ""},
                "logprobs": null,
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {"role": null, "content": "\n\n", "reasoning_content": null, "tool_calls": null, "function_call": null},
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null,
            "error": null
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": null,
                    "content": null,
                    "reasoning_content": null,
                    "tool_calls": [{
                        "id": "call_null_delta_1",
                        "index": 0,
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": ""}
                    }]
                },
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": null,
                    "content": null,
                    "reasoning_content": null,
                    "tool_calls": [{
                        "id": null,
                        "index": 0,
                        "type": "function",
                        "function": {"name": null, "arguments": "{"}
                    }]
                },
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": null,
                    "content": null,
                    "reasoning_content": null,
                    "tool_calls": [{
                        "id": null,
                        "index": 0,
                        "type": null,
                        "function": {"name": null, "arguments": "\"city\": \"Tokyo\"}"}
                    }]
                },
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {"role": null, "content": "\n", "reasoning_content": null, "tool_calls": null},
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": null},
                "logprobs": null,
                "finish_reason": "tool_calls"
            }]
        }),
        json!({
            "id": "chatcmpl_null_delta_fixture",
            "object": "chat.completion.chunk",
            "created": 1793280778,
            "model": "synthetic-model",
            "choices": [],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }),
    ]
    .iter()
    .map(|chunk| format!("data: {chunk}\n\n"))
    .collect::<String>()
        + "data: [DONE]\n\n";

    assert!(
        strict_sse_candidate(false, sse_stream.clone()).await,
        "Chat SSE stream with null optional delta and tool fields must permit completion"
    );

    // End-to-end verification through `create_responses_sse_stream_from_chat_observed`
    // and `restore_client_http_response` with active reversible DLP tokens.
    {
        use super::{
            http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
            response_restoration::ArgumentNormalizationMode,
        };
        use crate::proxy::providers::{
            streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
            transform_codex_chat::CodexToolContext,
        };
        use axum::body::Body;
        use http_body_util::BodyExt;

        let original_runtime = (*super::runtime::get_security_runtime()).clone();
        struct Reset(super::runtime::SecurityRuntimeConfig);
        impl Drop for Reset {
            fn drop(&mut self) {
                super::runtime::set_security_runtime_for_test(self.0.clone());
            }
        }
        let _reset = Reset(original_runtime);
        super::runtime::set_security_runtime_for_test(Default::default());

        let gateway =
            RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
        let mut req_body =
            json!({"model": "synthetic-model", "input": "13800138000", "stream": true});
        let admitted = match admit_http_request(
            &gateway,
            &crate::app_config::AppType::Codex,
            true,
            false,
            &mut req_body,
            &http::HeaderMap::new(),
        )
        .unwrap()
        {
            RestorationAdmission::Admitted(req) => req,
            _ => panic!("expected admitted request"),
        };
        let placeholder = req_body["input"].as_str().unwrap().to_string();
        let (attempt, _owner) = admitted
            .prepare_owned_attempt_for_proxy(
                "synthetic-chat-witness",
                "openai_chat",
                "http://127.0.0.1:1/v1/chat/completions",
                Some("synthetic-principal"),
                &mut req_body,
                crate::proxy::providers::transform_codex_chat::responses_to_chat_completions,
            )
            .unwrap();
        let tokenized_sse = sse_stream.replace("Tokyo", &placeholder);
        let tracker = attempt.upstream_sse_evidence_tracker().unwrap();
        let tracker_for_stream = std::sync::Arc::clone(&tracker);
        let upstream =
            futures::stream::iter([Ok::<_, std::io::Error>(bytes::Bytes::from(tokenized_sse))]);
        let converted = create_responses_sse_stream_from_chat_observed(
            upstream,
            CodexToolContext::default(),
            move |fact| {
                if let Ok(mut guard) = tracker_for_stream.lock() {
                    guard.observe(&fact);
                }
            },
        );
        let response = http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(converted))
            .unwrap();
        let restored = attempt
            .restore_client_http_response(
                response,
                ArgumentNormalizationMode::ChatCanonical,
                Some(tracker),
            )
            .await;
        let bytes = restored.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.contains("event: response.completed") && text.contains("13800138000"),
            "expected completed response with restored phone number, got: {text}"
        );
        assert!(
            !text.contains("response.failed") && !text.contains("rredact-cn_phone-"),
            "unexpected restoration failure or unredacted token in: {text}"
        );
    }

    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let json_body = json!({
        "id": "chatcmpl_null_json_fixture",
        "error": null,
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": null,
                "content": "synthetic ok",
                "reasoning_content": null,
                "tool_calls": null,
                "function_call": null
            }
        }]
    });
    let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
    completion.observe_raw_json(&json_body.to_string(), &budget);
    assert!(
        completion.permits_completion() && !completion.has_unsupported_carrier(),
        "Chat JSON response with null optional fields must permit completion"
    );
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_empty_tool_calls_restore_reasoning_and_authorize_tool_followup() {
    use super::{
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        response_restoration::ArgumentNormalizationMode,
    };
    use crate::proxy::providers::{
        streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
        transform_codex_chat::{responses_to_chat_completions, CodexToolContext},
    };
    use axum::body::Body;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};

    let original = (*super::runtime::get_security_runtime()).clone();
    struct Reset(super::runtime::SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            super::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset(original);
    super::runtime::set_security_runtime_for_test(Default::default());

    // The negative control must not release tools or authorize a continuation at EOF.
    for tool in [false, true] {
        for done in [true, false] {
            let gateway =
                RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
            let mut headers = http::HeaderMap::new();
            headers.insert(
                "session-id",
                http::HeaderValue::from_static("synthetic-empty-tools"),
            );
            let mut body = json!({"model":"synthetic-model","input":"13800138000","stream":true});
            let admitted = match admit_http_request(
                &gateway,
                &crate::app_config::AppType::Codex,
                true,
                false,
                &mut body,
                &headers,
            )
            .expect("synthetic admission")
            {
                RestorationAdmission::Admitted(request) => request,
                _ => panic!("synthetic request must be admitted"),
            };
            let token = body["input"].as_str().expect("synthetic token").to_owned();
            assert!(
                token.starts_with("rredact-cn_phone-"),
                "request must be tokenized"
            );
            let (attempt, _owner) = admitted
                .prepare_owned_attempt_for_proxy(
                    "synthetic-empty-tools",
                    "openai_chat",
                    "http://127.0.0.1:1/v1/chat/completions",
                    Some("synthetic-principal"),
                    &mut body,
                    responses_to_chat_completions,
                )
                .expect("synthetic preparation");
            let outbound = attempt
                .encode_outbound_body(&body)
                .expect("synthetic encoding")
                .expect("charged outbound body");
            let outbound = std::str::from_utf8(&outbound).expect("synthetic UTF-8");
            assert!(
                !outbound.contains("13800138000") && outbound.contains(&token),
                "final encoded Chat payload must contain only the authorized placeholder"
            );

            let chunk = |delta: Value, reason: Value| {
                json!({
                    "id":"chatcmpl_empty_tools", "model":"synthetic-model",
                    "choices":[{"index":0,"delta":delta,"finish_reason":reason}]
                })
            };
            // Split an authorized token across reasoning deltas with empty tool arrays.
            let split = 13;
            let mut chunks = vec![
                chunk(
                    json!({"reasoning_content":format!("Need {}", &token[..split]),"tool_calls":[]}),
                    Value::Null,
                ),
                chunk(
                    json!({"reasoning_content":format!("{} context.", &token[split..]),"tool_calls":[]}),
                    Value::Null,
                ),
            ];
            if tool {
                chunks.push(chunk(
                    json!({"tool_calls":[{"index":0,"id":"call_empty_tools","type":"function",
                    "function":{"name":"synthetic_lookup","arguments":null}}]}),
                    Value::Null,
                ));
                chunks.push(chunk(json!({"tool_calls":[{"index":0,"function":{"arguments":json!({"phone":token}).to_string()}}]}), Value::Null));
                chunks.push(chunk(json!({}), json!("tool_calls")));
            } else {
                chunks.push(chunk(
                    json!({"content":format!("Dial {token}"),"tool_calls":[]}),
                    json!("stop"),
                ));
            }
            if !done {
                chunks.last_mut().expect("synthetic terminal chunk")["choices"][0]
                    ["finish_reason"] = Value::Null;
            }
            let mut wire = chunks
                .iter()
                .map(|chunk| format!("data: {chunk}\n\n"))
                .collect::<String>();
            if done {
                wire.push_str("data: [DONE]\n\n");
            }
            assert_eq!(
                strict_sse_candidate(false, wire.clone()).await,
                done,
                "original-protocol completion evidence must remain strict"
            );
            let tracker = attempt
                .upstream_sse_evidence_tracker()
                .expect("synthetic evidence");
            let observer = tracker.clone();
            let converted = create_responses_sse_stream_from_chat_observed(
                stream::iter([Ok::<_, std::io::Error>(Bytes::from(wire))]),
                CodexToolContext::default(),
                move |fact| {
                    observer
                        .lock()
                        .expect("synthetic evidence lock")
                        .observe(&fact)
                },
            );
            let response = http::Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(converted))
                .expect("synthetic response");
            let restored = attempt
                .restore_client_http_response(
                    response,
                    ArgumentNormalizationMode::ChatCanonical,
                    Some(tracker),
                )
                .await;
            let bytes = restored
                .into_body()
                .collect()
                .await
                .expect("synthetic body")
                .to_bytes();
            let text = std::str::from_utf8(&bytes).expect("synthetic UTF-8");
            let events = text
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .map(|data| serde_json::from_str::<Value>(data).expect("synthetic event"))
                .collect::<Vec<_>>();
            let completed = events
                .iter()
                .find(|event| event["type"] == "response.completed");
            assert_eq!(
                completed.is_some(),
                done,
                "completion mismatch: tool={tool} done={done}"
            );
            assert_eq!(
                events
                    .iter()
                    .any(|event| event["type"] == "response.failed"),
                !done,
                "failure mismatch: tool={tool} done={done}"
            );
            if let Some(completed) = completed {
                assert!(
                    completed["response"]["output"][0]["summary"][0]["text"]
                        == format!("Need {token} context."),
                    "reasoning must retain its original reference without inverse restoration"
                );
                let output = &completed["response"]["output"][1];
                if tool {
                    assert_eq!(output["type"], "function_call");
                    let arguments: Value = serde_json::from_str(
                        output["arguments"].as_str().expect("synthetic arguments"),
                    )
                    .expect("synthetic argument object");
                    assert_eq!(arguments["phone"], "13800138000");
                } else {
                    assert_eq!(output["content"][0]["text"], "Dial 13800138000");
                }
            }
            if tool {
                if !done {
                    assert!(
                        !events
                            .iter()
                            .any(|event| event.pointer("/item/type").and_then(Value::as_str)
                                == Some("function_call")),
                        "incomplete evidence must retain the tool barrier"
                    );
                }
                let mut followup = json!({"model":"synthetic-model","stream":false,
                    "previous_response_id":"resp_chatcmpl_empty_tools",
                    "input":[{"type":"function_call_output","call_id":"call_empty_tools","output":"13800138000"}]});
                let continuation = match admit_http_request(
                    &gateway,
                    &crate::app_config::AppType::Codex,
                    true,
                    false,
                    &mut followup,
                    &headers,
                )
                .expect("synthetic continuation admission")
                {
                    RestorationAdmission::Admitted(request) => Some(request),
                    RestorationAdmission::Intercepted(_) => None,
                    RestorationAdmission::Passthrough => {
                        panic!("continuation must retain reversible protection")
                    }
                };
                let resumed = continuation.map(|request| {
                    request.prepare_owned_attempt_for_proxy(
                        "synthetic-empty-tools",
                        "openai_chat",
                        "http://127.0.0.1:1/v1/chat/completions",
                        Some("synthetic-principal"),
                        &mut followup,
                        responses_to_chat_completions,
                    )
                });
                assert_eq!(
                    resumed.as_ref().is_some_and(Result::is_ok),
                    done,
                    "only verified completion may authorize the tool continuation"
                );
                if let Some(Ok((attempt, _owner))) = resumed {
                    let bytes = attempt
                        .encode_outbound_body(&followup)
                        .expect("synthetic followup encoding")
                        .expect("protected continuation");
                    let wire = std::str::from_utf8(&bytes).expect("synthetic UTF-8");
                    assert!(
                        !wire.contains("13800138000") && wire.contains("rredact-cn_phone-"),
                        "tool output must be re-tokenized in the actual outbound Chat payload"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn reversible_restore_diagnostic_stages_are_fixed_and_keep_tool_barrier() {
    use super::{
        http_restoration::wrap_sse_stream_with_restoration,
        payload_budget::PayloadBudget,
        response_restoration::{ArgumentNormalizationMode, SseRestorationState},
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;

    let private_marker = "synthetic-private-diagnostic-marker";
    let event = |kind: &str, fields: serde_json::Value| {
        let mut data = fields;
        data["type"] = json!(kind);
        format!("event: {kind}\ndata: {data}\n\n")
    };
    let tool = event(
        "response.function_call_arguments.delta",
        json!({
            "output_index": 0, "delta": private_marker
        }),
    );
    let cases = vec![
        ("sse_read", vec![Err(std::io::Error::other(private_marker))]),
        ("sse_frame", vec![Ok(Bytes::from_static(b"data: \xff\n\n"))]),
        ("sse_eof", vec![Ok(Bytes::from("data: unfinished"))]),
        (
            "sse_event",
            vec![Ok(Bytes::from(event(
                "response.created",
                json!({"error": private_marker}),
            )))],
        ),
        (
            "sse_snapshot",
            vec![Ok(Bytes::from(format!(
                "{}{}",
                event(
                    "response.output_text.delta",
                    json!({"delta": "synthetic-a"})
                ),
                event("response.output_text.done", json!({"text": "synthetic-b"}))
            )))],
        ),
        (
            "token_restore",
            vec![Ok(Bytes::from(event(
                "response.output_text.delta",
                json!({
                    "delta": "rredact-custom-00000000000000000000-ai"
                }),
            )))],
        ),
        ("completion_evidence", vec![Ok(Bytes::from(tool))]),
    ];
    for (stage, chunks) in cases {
        let (lease, _) = chat_carrier_fixture();
        let binding = lease
            .bind_actual_attempt(
                "synthetic-provider",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                None,
            )
            .unwrap();
        let restorer = SseRestorationState::new(
            lease,
            binding,
            PayloadBudget::with_limit(4 * 1024 * 1024),
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        let mut stream = wrap_sse_stream_with_restoration(futures::stream::iter(chunks), restorer);
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }
        assert!(
            output.contains(&format!("[stage={stage}]"))
                || output.contains(&format!("[stage={stage} reason=")),
            "missing fixed stage: {stage}"
        );
        assert!(output.contains("dlp_restore_failed"));
        assert!(output.contains("event: response.failed"));
        assert!(!output.contains(private_marker));
        assert!(!output.contains("rredact-custom-00000000000000000000-ai"));
        assert!(!output.contains("event: response.function_call_arguments.delta"));
        assert!(!output.contains("event: response.function_call_arguments.done"));
        assert!(!output.contains("event: response.completed"));
    }
}

#[test]
fn reversible_restore_diagnostic_http_keeps_error_contract() {
    use super::http_restoration::DlpForwardError;
    use crate::proxy::{error::ProxyError, response_processor::isolate_codex_dlp_response_error};
    let error = isolate_codex_dlp_response_error(
        ProxyError::Internal("synthetic-private-diagnostic-marker".into()),
        true,
    );
    let ProxyError::DlpForward(error) = error else {
        panic!("expected isolated DLP error")
    };
    assert_eq!(error.status_code(), axum::http::StatusCode::BAD_GATEWAY);
    assert_eq!(error.error_type(), "server_error");
    assert_eq!(error.code(), "dlp_restore_failed");
    assert_eq!(
        error.message(),
        "Reversible DLP response restoration failed. [stage=response_processing]"
    );
    assert_eq!(
        DlpForwardError::RestoreFailed.message(),
        "Reversible DLP response restoration failed. [stage=unknown]"
    );
}

fn assert_fixed_restore_client_error(error: &serde_json::Value, expected: &str) {
    assert!(
        error
            == &serde_json::json!({
                "message": expected,
                "code": "dlp_restore_failed",
                "type": "server_error",
                "param": null,
            }),
        "client restoration error must match the complete fixed envelope"
    );
}

#[test]
fn history_provenance_client_error_assertion_failure_is_content_free() {
    use serde_json::json;
    let expected = "synthetic fixed client restoration message";
    let valid = json!({"message":expected,"code":"dlp_restore_failed",
        "type":"server_error","param":null});
    assert_fixed_restore_client_error(&valid, expected);
    for field in ["message", "code", "type", "param", "synthetic_extra"] {
        let mut invalid = valid.clone();
        invalid[field] = json!("synthetic assertion payload must stay private");
        let panic = std::panic::catch_unwind(|| {
            assert_fixed_restore_client_error(&invalid, expected);
        });
        let payload = panic
            .err()
            .expect("mutated envelope must fail its assertion");
        let fixed = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
        assert!(
            fixed == Some("client restoration error must match the complete fixed envelope"),
            "failed envelope assertions must expose only their fixed failure category"
        );
    }
}

#[tokio::test]
async fn reversible_restore_diagnostic_reasons_are_fixed_and_survive_http_sse() {
    use super::{
        http_restoration::build_codex_restore_error_response_at,
        payload_budget::PayloadBudget,
        payload_metadata::PayloadVec,
        response_restoration::{
            failed_sse_frame_at, restore_json_arguments_string, restore_text_with_lease,
            split_safe_prefix_and_tail, RestoreError,
        },
        restore_diagnostics::RestoreStage,
    };
    use axum::body::to_bytes;
    use serde_json::{json, Value};

    let (lease, token) = chat_carrier_fixture();
    let private_marker = "synthetic-private-reason-marker";
    let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
    let mut references = PayloadVec::new(budget.clone()).unwrap();
    let mut occurrences = 0;
    let mut cases = Vec::new();
    cases.push((
        "token_format",
        split_safe_prefix_and_tail("rredact-custom-invalid").unwrap_err(),
    ));
    for (text, reason) in [
        ("rredact-custom-invalid".to_string(), "token_format"),
        (
            "rredact-custom-00000000000000000000-ai".to_string(),
            "token_context",
        ),
        (token.repeat(4097), "restore_capacity"),
    ] {
        let err =
            restore_text_with_lease(&text, &lease, &budget, &mut references, &mut occurrences)
                .err()
                .unwrap();
        cases.push((reason, err));
    }
    let tiny = PayloadBudget::with_limit(1);
    let mut tiny_references = PayloadVec::new(tiny.clone()).unwrap();
    let err = restore_text_with_lease(
        private_marker,
        &lease,
        &tiny,
        &mut tiny_references,
        &mut occurrences,
    )
    .err()
    .unwrap();
    cases.push(("restore_capacity", err));
    for (args, reason) in [
        (" ".to_string(), "arguments_empty"),
        (" ".repeat(1024 * 1024 + 1) + "{}", "arguments_size"),
        (format!("{{\"{private_marker}\":"), "arguments_json"),
        ("{\"x\":1,\"x\":2}".to_string(), "arguments_json"),
        ("[]".to_string(), "arguments_shape"),
        ("[".repeat(100) + &"]".repeat(100), "restore_capacity"),
    ] {
        let err = restore_json_arguments_string(
            &args,
            &lease,
            &budget,
            &mut references,
            &mut occurrences,
        )
        .err()
        .unwrap();
        cases.push((reason, err));
    }
    // Fault injection uses existing allocation hooks; no live payloads or providers.
    {
        use super::payload_json::test_support::{self, Action, Phase};
        let _hook = test_support::install(|event| {
            if event.phase == Phase::BeforeBuild {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let err =
            restore_json_arguments_string("{}", &lease, &budget, &mut references, &mut occurrences)
                .err()
                .unwrap();
        cases.push(("arguments_rebuild", err));
    }
    {
        use super::payload_buffer::test_support::{self, Action, Phase};
        let _hook = test_support::install(|event| {
            if event.phase == Phase::BeforeAllocation {
                Action::FailAllocation
            } else {
                Action::Continue
            }
        });
        let err =
            restore_json_arguments_string("{}", &lease, &budget, &mut references, &mut occurrences)
                .err()
                .unwrap();
        cases.push(("arguments_serialize", err));
    }
    {
        use super::payload_text::test_support::{self, Action, Phase};
        let _hook = test_support::install(|event| {
            if event.phase == Phase::BeforeCopy {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let err =
            restore_json_arguments_string("{}", &lease, &budget, &mut references, &mut occurrences)
                .err()
                .unwrap();
        cases.push(("restore_capacity", err));
    }
    cases.push(("unknown", RestoreError::RestoreFailed));
    for (reason, err) in cases {
        let response = build_codex_restore_error_response_at(err, RestoreStage::TokenRestore);
        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let http: Value = serde_json::from_slice(&bytes).unwrap();
        let frame = failed_sse_frame_at(err, RestoreStage::TokenRestore);
        let sse_text = std::str::from_utf8(&frame).unwrap();
        let sse: Value = serde_json::from_str(
            sse_text
                .strip_prefix("event: response.failed\ndata: ")
                .unwrap()
                .trim(),
        )
        .unwrap();
        let expected = format!(
            "Reversible DLP response restoration failed. [stage=token_restore reason={reason}]"
        );
        for error in [&http["error"], &sse["response"]["error"]] {
            assert_fixed_restore_client_error(error, &expected);
        }
        let http_text = std::str::from_utf8(&bytes).unwrap();
        for text in [http_text, sse_text] {
            assert!(!text.contains(private_marker));
            assert!(!text.contains(&token));
        }
        assert!(
            sse["response"]["status"] == "failed",
            "client restoration failure must retain its fixed terminal status"
        );
    }
    // Authorized references in object keys remain unsupported, not a restore failure.
    let args = json!({token: private_marker}).to_string();
    let err =
        restore_json_arguments_string(&args, &lease, &budget, &mut references, &mut occurrences)
            .err()
            .unwrap();
    assert_eq!(err, RestoreError::RestoreUnsupported);
    let frame = failed_sse_frame_at(err, RestoreStage::TokenRestore);
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.contains("dlp_restore_unsupported"));
    assert!(!text.contains("reason="));
}

#[test]
fn reversible_restore_diagnostic_reason_stays_sticky_behind_tool_barrier() {
    use super::{
        payload_budget::PayloadBudget,
        response_restoration::{
            failed_sse_frame_at, ArgumentNormalizationMode, SseRestorationState,
        },
    };
    use serde_json::json;
    let (lease, _) = chat_carrier_fixture();
    let binding = lease
        .bind_actual_attempt(
            "synthetic-provider",
            "openai_responses",
            "http://127.0.0.1:1/v1/responses",
            None,
        )
        .unwrap();
    let mut state = SseRestorationState::new(
        lease,
        binding,
        PayloadBudget::with_limit(4 * 1024 * 1024),
        ArgumentNormalizationMode::Exact,
    )
    .unwrap();
    let event = |kind: &str, fields: serde_json::Value| {
        let mut data = fields;
        data["type"] = json!(kind);
        format!("event: {kind}\ndata: {data}\n\n")
    };
    assert!(state
        .process_sse_block(&event(
            "response.function_call_arguments.delta",
            json!({"output_index": 0, "delta": "{}"}),
        ))
        .unwrap()
        .is_empty());
    assert!(state.process_sse_block(&event(
        "response.completed", json!({"response": {
            "id": "synthetic-failed", "status": "completed", "output": [{
                "id": "synthetic-tool", "type": "function_call", "call_id": "synthetic-call",
                "name": "synthetic_tool", "arguments": "{}", "status": "completed"
            }, {
                "id": "synthetic-message", "type": "message", "role": "assistant",
                "status": "completed", "content": [{"type": "refusal", "refusal": "rredact-custom-invalid"}]
            }]
        }}),
    )).unwrap().is_empty());
    let failure = state.finalize_and_flush(true).unwrap_err();
    let stage = state.failure_stage();
    assert_eq!(state.finalize_and_flush(true).unwrap_err(), failure);
    assert_eq!(state.process_sse_block(&event(
        "response.completed", json!({"response": {"id": "synthetic-failed", "status": "completed", "output": []}}),
    )).unwrap_err(), failure);
    assert_eq!(state.failure_stage(), stage);
    let frame = failed_sse_frame_at(failure, stage);
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.contains("[stage=token_restore reason=token_format]"));
    assert!(!text.contains("event: response.function_call_arguments"));
    assert!(!text.contains("event: response.completed"));
}

#[cfg(test)]
mod text_fallback_stream_tests {
    use super::super::{
        payload_budget::PayloadBudget,
        response_restoration::{TextProjectionStream, TEXT_PROJECTION_NOTICE},
        tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    };
    use serde_json::{json, Value};

    fn events(text: &str) -> Vec<Value> {
        let part = json!({"type":"output_text", "text":text, "annotations":[]});
        let item = json!({"id":"msg_projection", "type":"message", "role":"assistant", "status":"completed", "content":[part.clone()]});
        vec![
            json!({"type":"response.output_text.delta", "output_index":0, "content_index":0, "item_id":"msg_projection", "delta":text}),
            json!({"type":"response.output_text.done", "output_index":0, "content_index":0, "item_id":"msg_projection", "text":text}),
            json!({"type":"response.content_part.done", "output_index":0, "content_index":0, "item_id":"msg_projection", "part":part}),
            json!({"type":"response.output_item.done", "output_index":0, "item":item.clone()}),
            json!({"type":"response.completed", "response":{"id":"resp_projection", "object":"response", "status":"completed", "output":[item]}}),
        ]
    }

    fn fixture() -> (DlpTokenStore, TextProjectionStream, PayloadBudget) {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(32 * 1024 * 1024);
        let mut lease = store
            .admit("codex", Some("projection-stream"), 1)
            .expect("fixture lease");
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-stream-value",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("fixture preparation");
        let stream = TextProjectionStream::new(lease, budget.clone()).expect("stream init");
        (store, stream, budget)
    }

    fn push(stream: &mut TextProjectionStream, value: &Value) -> bool {
        let raw = format!(
            "event: {}\ndata: {value}\n\n",
            value["type"].as_str().expect("fixture event")
        );
        stream.push_frame(&raw).is_ok()
    }

    #[test]
    fn text_fallback_sse_buffers_until_eof_and_replays_consistently() {
        let (_store, mut stream, budget) = fixture();
        for event in events("rredact-custom-damaged") {
            assert!(push(&mut stream, &event));
        }
        let frames = stream.finish(true).expect("eligible stream");
        assert_eq!(frames.len(), 5);
        for frame in frames.as_slice() {
            let raw = std::str::from_utf8(frame).expect("fixture utf8");
            assert!(raw.contains(TEXT_PROJECTION_NOTICE));
            assert!(!raw.contains("rredact-custom-"));
        }
        assert!(stream.finish(true).is_err());
        drop(frames);
        drop(stream);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn text_fallback_sse_hard_failure_after_format_never_flushes() {
        for variant in 0..9 {
            let (_store, mut stream, budget) = fixture();
            let mut input = events("rredact-custom-damaged");
            match variant {
                0 => {
                    input[1]["text"] = json!("different");
                }
                1 => {
                    input[2]["item_id"] = json!("other");
                }
                2 => {
                    input[3]["item"]["content"][0]["text"] = json!("different");
                }
                3 => {
                    input.pop();
                }
                4 => {
                    input.push(input[4].clone());
                }
                5 => {
                    input[4]["response"]["output"]
                        .as_array_mut()
                        .expect("fixture output")
                        .push(json!({"type":"function_call"}));
                }
                6 => {
                    input[0]["metadata"] = json!("rredact-custom-damaged");
                }
                7 => {
                    input[4]["response"]["status"] = json!("incomplete");
                }
                _ => {}
            }
            let accepted = input.iter().all(|event| push(&mut stream, event));
            assert!(
                !accepted || stream.finish(variant != 8).is_err(),
                "hard failure must refuse"
            );
            drop(stream);
            assert_eq!(budget.used(), 0);
        }
    }

    #[test]
    fn text_fallback_sse_cross_delta_and_unknown_reference_refuse() {
        let (store, mut stream, _budget) = fixture();
        let mut other = store
            .admit("codex", Some("different-fixture"), 1)
            .expect("other lease");
        other
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "other-synthetic-value",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                PayloadBudget::with_limit(4096),
                |_| Ok(()),
            )
            .expect("other prepare");
        let reference = other
            .authorized_tokens()
            .expect("other authority")
            .as_slice()[0]
            .to_owned();
        let text = format!("rredact-custom-damaged {reference}");
        let mut input = events(&text);
        input[0]["delta"] = json!(&text[..20]);
        let mut second = input[0].clone();
        second["delta"] = json!(&text[20..]);
        input.insert(1, second);
        let accepted = input.iter().all(|event| push(&mut stream, event));
        assert!(!accepted || stream.finish(true).is_err());
    }

    #[test]
    fn text_fallback_budget_cancel_frame_and_event_limits() {
        let (_store, mut stream, budget) = fixture();
        assert!(push(&mut stream, &events("rredact-custom-damaged")[0]));
        assert!(budget.used() > 0);
        drop(stream);
        assert_eq!(budget.used(), 0);
        let (_store, mut stream, budget) = fixture();
        assert!(stream.push_frame(&"x".repeat(1024 * 1024 + 1)).is_err());
        drop(stream);
        assert_eq!(budget.used(), 0);
        let (_store, mut stream, _budget) = fixture();
        for _ in 0..4096 {
            assert!(stream.push_frame(": heartbeat\n\n").is_ok());
        }
        assert!(stream.push_frame(": heartbeat\n\n").is_err());
    }
    #[test]
    fn text_fallback_sse_preserves_undamaged_delta_sequence() {
        let (_store, mut stream, _budget) = fixture();
        let mut input = events("first second");
        input[0]["delta"] = json!("first ");
        let mut second = input[0].clone();
        second["delta"] = json!("second");
        input.insert(1, second);
        input.last_mut().expect("fixture terminal")["response"]["output"][0]["content"]
            .as_array_mut()
            .expect("fixture content")
            .push(json!({"type":"output_text","text":"rredact-custom-damaged"}));
        // The item snapshot must make the same original content claim as the terminal.
        let final_item = input.last().expect("terminal")["response"]["output"][0].clone();
        input[4]["item"] = final_item;
        for event in input {
            assert!(push(&mut stream, &event));
        }
        let frames = stream.finish(true).expect("projection");
        let first = std::str::from_utf8(&frames.as_slice()[0]).expect("utf8");
        let second = std::str::from_utf8(&frames.as_slice()[1]).expect("utf8");
        assert!(first.contains("\"delta\":\"first \""));
        assert!(second.contains("\"delta\":\"second\""));
    }

    #[test]
    fn text_fallback_sse_unexpected_carrier_cannot_hide_unauthorized() {
        let (store, mut stream, _budget) = fixture();
        let mut other = store
            .admit("codex", Some("unexpected-carrier"), 1)
            .expect("other lease");
        other
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "unexpected-synthetic-value",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                PayloadBudget::with_limit(4096),
                |_| Ok(()),
            )
            .expect("other prepare");
        let reference = other.authorized_tokens().expect("authority").as_slice()[0].to_owned();
        let mut input = events("ordinary text");
        input[0]["part"] = json!({"type":"output_text","text":reference});
        input.last_mut().expect("terminal")["response"]["output"][0]["content"]
            .as_array_mut()
            .expect("content")
            .push(json!({"type":"output_text","text":"rredact-custom-damaged"}));
        let final_item = input.last().expect("terminal")["response"]["output"][0].clone();
        input[3]["item"] = final_item;
        let accepted = input.iter().all(|event| push(&mut stream, event));
        assert!(!accepted || stream.finish(true).is_err());
    }
    #[test]
    fn text_fallback_sse_created_carrier_and_response_identity_refuse() {
        let invalid = [
            json!({"id":"other", "object":"response", "status":"in_progress", "output":[]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"function_call"}]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"reasoning"}]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"message", "content":[{"type":"refusal"}]}]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"compaction"}]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"encrypted"}]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress", "output":[{"type":"unknown"}]}),
            json!({"id":"resp_projection", "object":"other", "status":"in_progress", "output":[]}),
            json!({"id":"resp_projection", "object":"response", "status":"completed", "output":[]}),
            json!({"id":"resp_projection", "object":"response", "status":"in_progress"}),
        ];
        for kind in ["response.created", "response.in_progress"] {
            for response in &invalid {
                let (_store, mut stream, budget) = fixture();
                let prefix = json!({"type":kind, "response":response});
                let accepted = push(&mut stream, &prefix)
                    && events("rredact-custom-damaged")
                        .iter()
                        .all(|event| push(&mut stream, event));
                assert!(
                    !accepted || stream.finish(true).is_err(),
                    "invalid lifecycle must refuse"
                );
                drop(stream);
                assert_eq!(budget.used(), 0);
            }
        }
        let (_store, mut stream, _budget) = fixture();
        for kind in ["response.created", "response.in_progress"] {
            assert!(push(
                &mut stream,
                &json!({"type":kind, "response":{"id":"resp_projection", "object":"response", "status":"in_progress", "output":[]}})
            ));
        }
        for event in events("rredact-custom-damaged") {
            assert!(push(&mut stream, &event));
        }
        assert!(stream.finish(true).is_ok());
    }

    #[test]
    fn text_fallback_sse_invalid_explicit_index_refuses() {
        for event_index in [0, 1] {
            for key in ["output_index", "content_index"] {
                for invalid in [json!("0"), json!(-1), json!(0.5), Value::Null] {
                    let (_store, mut stream, _budget) = fixture();
                    let mut input = events("rredact-custom-damaged");
                    input[event_index][key] = invalid;
                    let accepted = input.iter().all(|event| push(&mut stream, event));
                    assert!(
                        !accepted || stream.finish(true).is_err(),
                        "explicit invalid index must refuse"
                    );
                }
            }
        }
    }

    #[test]
    fn text_fallback_sse_preserves_authorized_cross_delta_restoration() {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(32 * 1024 * 1024);
        let mut lease = store
            .admit("codex", Some("projection-split"), 1)
            .expect("lease");
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-restored-text",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("prepare");
        let reference = lease.authorized_tokens().expect("authority").as_slice()[0].to_owned();
        let mut stream = TextProjectionStream::new(lease, budget).expect("stream");
        let text = format!("first {reference} last");
        let mut input = events(&text);
        input[0]["delta"] = json!(&text[..18]);
        let mut second = input[0].clone();
        second["delta"] = json!(&text[18..]);
        input.insert(1, second);
        input.last_mut().expect("terminal")["response"]["output"][0]["content"]
            .as_array_mut()
            .expect("content")
            .push(json!({"type":"output_text","text":"rredact-custom-damaged"}));
        let final_item = input.last().expect("terminal")["response"]["output"][0].clone();
        input[4]["item"] = final_item;
        for event in input {
            assert!(push(&mut stream, &event));
        }
        let frames = stream.finish(true).expect("projection");
        let deltas: Vec<String> = frames.as_slice()[..2]
            .iter()
            .map(|frame| {
                let raw = std::str::from_utf8(frame).expect("utf8");
                let data = raw
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .expect("data");
                serde_json::from_str::<Value>(data).expect("json")["delta"]
                    .as_str()
                    .expect("text")
                    .to_owned()
            })
            .collect();
        assert!(deltas[0] == "first ");
        assert!(deltas[1] == "synthetic-restored-text last");
    }
    #[test]
    fn text_fallback_sse_every_reference_split_restores_authorized_text() {
        fn prepared() -> (DlpTokenStore, TextProjectionStream, PayloadBudget, String) {
            let store = DlpTokenStore::new();
            let budget = PayloadBudget::with_limit(32 * 1024 * 1024);
            let mut lease = store
                .admit("codex", Some("projection-every-split"), 1)
                .expect("fixture lease");
            lease
                .prepare_with_budget(
                    &[TokenFragment {
                        raw: "synthetic-every-split-value",
                        whole_span_categories: &[TokenCategory::Custom],
                    }],
                    &[],
                    budget.clone(),
                    |_| Ok(()),
                )
                .expect("fixture preparation");
            let reference = lease
                .authorized_tokens()
                .expect("fixture authority")
                .as_slice()[0]
                .to_owned();
            let stream = TextProjectionStream::new(lease, budget.clone()).expect("fixture stream");
            (store, stream, budget, reference)
        }

        let reference_len = {
            let (_, _, _, reference) = prepared();
            assert!(reference.is_ascii());
            reference.len()
        };
        let mut checked = 0;
        for split in 0..=reference_len {
            let (_store, mut stream, budget, reference) = prepared();
            assert_eq!(reference.len(), reference_len);
            let mut input = events(&reference);
            input[0]["delta"] = json!(&reference[..split]);
            let mut second = input[0].clone();
            second["delta"] = json!(&reference[split..]);
            input.insert(1, second);
            input.last_mut().expect("fixture terminal")["response"]["output"][0]["content"]
                .as_array_mut()
                .expect("fixture content")
                .push(json!({"type":"output_text","text":"rredact-custom-damaged"}));
            input[4]["item"] =
                input.last().expect("fixture terminal")["response"]["output"][0].clone();
            assert!(input.iter().all(|event| push(&mut stream, event)));
            let frames = stream.finish(true).expect("eligible split projection");
            let mut restored = String::new();
            for (index, frame) in frames.as_slice().iter().enumerate() {
                let raw = std::str::from_utf8(frame).expect("fixture utf8");
                assert!(!raw.contains(&reference));
                let data = raw
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .expect("fixture data");
                let value: Value = serde_json::from_str(data).expect("fixture json");
                if index < 2 {
                    restored.push_str(value["delta"].as_str().expect("projected delta"));
                } else if index == 2 {
                    assert!(value["text"] == "synthetic-every-split-value");
                } else if index == 3 {
                    assert!(value["part"]["text"] == "synthetic-every-split-value");
                } else {
                    let item = if index == 4 {
                        &value["item"]
                    } else {
                        &value["response"]["output"][0]
                    };
                    assert!(item["content"][0]["text"] == "synthetic-every-split-value");
                    assert!(item["content"][1]["text"] == TEXT_PROJECTION_NOTICE);
                }
            }
            assert!(restored == "synthetic-every-split-value");
            assert_eq!(frames.len(), 6);
            drop(frames);
            drop(stream);
            assert_eq!(budget.used(), 0);
            checked += 1;
        }
        assert_eq!(checked, reference_len + 1);
        assert!(checked > 2);
    }

    #[test]
    fn text_fallback_sse_every_reference_split_rejects_unauthorized_before_or_after_damage() {
        let other_store = DlpTokenStore::new();
        let mut other = other_store
            .admit("codex", Some("unrelated-split-fixture"), 1)
            .expect("fixture lease");
        other
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-unrelated-split-value",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                PayloadBudget::with_limit(4096),
                |_| Ok(()),
            )
            .expect("fixture preparation");
        let reference = other
            .authorized_tokens()
            .expect("fixture authority")
            .as_slice()[0]
            .to_owned();
        assert!(reference.is_ascii());
        let mut checked = 0;
        for damage_first in [false, true] {
            let prefix = if damage_first {
                "rredact-custom-damaged "
            } else {
                ""
            };
            let suffix = if damage_first {
                ""
            } else {
                " rredact-custom-damaged"
            };
            let text = format!("{prefix}{reference}{suffix}");
            for split in 0..=reference.len() {
                let (_store, mut stream, budget) = fixture();
                let boundary = prefix.len() + split;
                let mut input = events(&text);
                input[0]["delta"] = json!(&text[..boundary]);
                let mut second = input[0].clone();
                second["delta"] = json!(&text[boundary..]);
                input.insert(1, second);
                let accepted = input.iter().all(|event| push(&mut stream, event));
                assert!(
                    !accepted || stream.finish(true).is_err(),
                    "unauthorized split must refuse"
                );
                drop(stream);
                assert_eq!(budget.used(), 0);
                checked += 1;
            }
        }
        assert_eq!(checked, 2 * (reference.len() + 1));
        assert!(checked > 4);
    }

    #[test]
    fn text_fallback_budget_exact_frame_round_track_and_over_limits() {
        let (_store, mut stream, budget) = fixture();
        let mut heartbeat = String::with_capacity(1024 * 1024);
        heartbeat.push(':');
        heartbeat.push_str(&"x".repeat(1024 * 1024 - 3));
        heartbeat.push_str("\n\n");
        assert_eq!(heartbeat.len(), 1024 * 1024);
        for _ in 0..8 {
            assert!(stream.push_frame(&heartbeat).is_ok());
        }
        assert!(stream.push_frame(":\n\n").is_err());
        drop(stream);
        assert_eq!(budget.used(), 0);

        let (_store, mut stream, budget) = fixture();
        let delta = json!({"type":"response.output_text.delta", "output_index":0,"content_index":0,"item_id":"msg_projection","delta":"x".repeat(128 * 1024)});
        for _ in 0..8 {
            assert!(push(&mut stream, &delta));
        }
        let extra = json!({"type":"response.output_text.delta", "output_index":0,"content_index":0,"item_id":"msg_projection","delta":"x"});
        assert!(!push(&mut stream, &extra));
        drop(stream);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn text_fallback_budget_final_buffer_allocation_failure_never_replays() {
        use super::super::payload_buffer::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};
        let (_store, mut stream, budget) = fixture();
        for event in events("rredact-custom-damaged") {
            assert!(push(&mut stream, &event));
        }
        let count = Rc::new(Cell::new(0));
        let observed = count.clone();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let frames = stream.finish(true).expect("baseline preparation");
        drop(hook);
        let allocations = count.get();
        assert!(allocations > 5);
        drop(frames);
        drop(stream);
        assert_eq!(budget.used(), 0);
        for fail_at in 0..allocations {
            let (_store, mut stream, budget) = fixture();
            for event in events("rredact-custom-damaged") {
                assert!(push(&mut stream, &event));
            }
            let mut seen = 0;
            let hook = test_support::install(move |event| {
                if event.phase == Phase::BeforeAllocation {
                    let index = seen;
                    seen += 1;
                    if index == fail_at {
                        return Action::FailAllocation;
                    }
                }
                Action::Continue
            });
            assert!(
                stream.finish(true).is_err(),
                "allocation failure cannot produce replay"
            );
            assert!(
                stream.finish(true).is_err(),
                "failed terminal remains closed"
            );
            drop(hook);
            drop(stream);
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn text_fallback_native_object_sse_refuses_wrong_or_missing() {
        for object in [Some(json!("other")), Some(Value::Null), None] {
            let (_store, mut stream, _budget) = fixture();
            let mut input = events("rredact-custom-damaged");
            let terminal = input.last_mut().expect("terminal")["response"]
                .as_object_mut()
                .expect("response");
            if let Some(value) = object {
                terminal.insert("object".to_owned(), value);
            } else {
                terminal.remove("object");
            }
            let accepted = input.iter().all(|event| push(&mut stream, event));
            assert!(
                !accepted || stream.finish(true).is_err(),
                "non-native object cannot project"
            );
        }
    }
    #[test]
    fn text_fallback_review_duplicate_and_late_part_item_completion_refuse() {
        for variant in 0..4 {
            let (_store, mut stream, _budget) = fixture();
            let mut input = events("rredact-custom-damaged");
            match variant {
                0 => input.insert(3, input[2].clone()),
                1 => input.insert(4, input[3].clone()),
                2 => {
                    input.remove(1);
                    let part = input.remove(1);
                    input.insert(0, part);
                }
                _ => {
                    input.remove(1);
                    let item = input.remove(2);
                    input.insert(0, item);
                }
            }
            let accepted = input.iter().all(|event| push(&mut stream, event));
            assert!(
                !accepted || stream.finish(true).is_err(),
                "completed carrier cannot repeat or receive late content"
            );
        }
    }

    #[test]
    fn text_fallback_review_transient_kind_copy_is_reserved_before_parse() {
        use super::super::payload_buffer::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};
        let (_store, mut stream, budget) = fixture();
        let raw = format!("data: {}\n\n", json!({"type":"x".repeat(16 * 1024)}));
        let baseline = budget.used();
        let seen = Rc::new(Cell::new(false));
        let observed = seen.clone();
        let reserve = raw.len();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation && !observed.replace(true) {
                assert!(
                    event.used >= baseline + reserve + event.requested,
                    "inherited temporary kind copy requires a prior charge"
                );
            }
            Action::Continue
        });
        assert!(stream.push_frame(&raw).is_err());
        drop(hook);
        assert!(seen.get());
        drop(stream);
        assert_eq!(budget.used(), baseline);
    }
}

#[cfg(test)]
mod token_format_diagnostic_sink_tests {
    use super::super::{
        payload_budget::PayloadBudget,
        response_restoration::{ArgumentNormalizationMode, RestoreError, SseRestorationState},
        restore_diagnostics::{RestoreReason, RestoreStage},
    };
    use serde_json::{json, Value};
    use std::sync::Mutex;

    static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.target().ends_with("token_format_diagnostics") {
                RECORDS.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;

    fn state() -> (SseRestorationState, String, PayloadBudget) {
        let (lease, token) = super::chat_carrier_fixture();
        let binding = lease
            .bind_actual_attempt(
                "SYNTHETIC_PRIVATE_PROVIDER",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                None,
            )
            .unwrap();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let state = SseRestorationState::new(
            lease,
            binding,
            budget.clone(),
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        (state, token, budget)
    }
    fn event(kind: &str, mut data: Value) -> String {
        data["type"] = json!(kind);
        format!("event: {kind}\ndata: {data}\n\n")
    }
    fn message(text: &str) -> Value {
        json!({"id":"SYNTHETIC_PRIVATE_MESSAGE", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"refusal", "refusal":text}]})
    }
    fn completed(items: Value) -> String {
        event(
            "response.completed",
            json!({"response":{"id":"SYNTHETIC_PRIVATE_RESPONSE", "status":"completed", "output":items}}),
        )
    }
    fn count() -> usize {
        RECORDS.lock().unwrap().len()
    }
    fn format_error(result: Result<Vec<bytes::Bytes>, RestoreError>) {
        assert!(
            matches!(
                result,
                Err(RestoreError::FailedBecause(RestoreReason::TokenFormat))
            ),
            "strict carrier must reject malformed token syntax"
        );
    }

    #[test]
    fn token_format_diagnostic_sink_is_bounded_once_and_observational(
    ) -> Result<(), Box<dyn std::error::Error>> {
        const CHILD: &str = "VARYAI_SYNTHETIC_TOKEN_FORMAT_DIAGNOSTIC_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args(["--exact", "proxy::security::reversible_completion_tests::token_format_diagnostic_sink_tests::token_format_diagnostic_sink_is_bounded_once_and_observational", "--nocapture"])
                .env(CHILD, "1").output()?;
            assert!(output.status.success(), "isolated token-format diagnostic assertions failed; synthetic fixture output suppressed");
            let report = String::from_utf8_lossy(&output.stdout);
            assert!(
                report.contains("running 1 test") && report.contains("1 passed; 0 failed"),
                "isolated diagnostic test must execute exactly one test"
            );
            return Ok(());
        }
        log::set_logger(&LOGGER).map_err(|_| "isolated diagnostic logger unavailable")?;
        log::set_max_level(log::LevelFilter::Warn);
        let malformed = [
            "rredact-custom-` SYNTHETIC_PRIVATE_BODY",
            "rredact-custom-0000000000000000000G-ai SYNTHETIC_PRIVATE_BODY",
            "rredact-custom-00000000000000000000-ax SYNTHETIC_PRIVATE_BODY",
        ];
        for bad in malformed {
            for kind in ["response.refusal.delta"] {
                let (mut state, _, budget) = state();
                let before = count();
                format_error(state.process_sse_block(&event(kind, json!({"delta":bad}))));
                assert_eq!(state.failure_stage(), RestoreStage::TokenRestore);
                assert_eq!(
                    count(),
                    before + 1,
                    "first format failure must emit one structural log"
                );
                format_error(
                    state
                        .process_sse_block(&event(kind, json!({"delta":"SYNTHETIC_PRIVATE_LATE"}))),
                );
                format_error(state.finalize_and_flush(true));
                assert_eq!(
                    count(),
                    before + 1,
                    "sticky errors must not duplicate the diagnostic"
                );
                drop(state);
                assert_eq!(budget.used(), 0);
            }
        }
        // Ordinary output-text nonmatches are preserved without a format diagnostic.
        for bad in malformed {
            let (mut state, _, budget) = state();
            let before = count();
            state
                .process_sse_block(&event("response.output_text.delta", json!({"delta":bad})))
                .map_err(|_| "ordinary nonmatch delta rejected")?;
            state
                .process_sse_block(&event("response.output_text.done", json!({"text":bad})))
                .map_err(|_| "ordinary nonmatch snapshot rejected")?;
            assert_eq!(
                count(),
                before,
                "ordinary nonmatches must not emit format diagnostics"
            );
            drop(state);
            assert_eq!(budget.used(), 0);
        }
        // Supported reasoning is not a token-format diagnostic source.
        for bad in malformed {
            let (mut state, _, budget) = state();
            let before = count();
            assert!(
                state
                    .process_sse_block(&event(
                        "response.reasoning_summary_text.delta",
                        json!({"delta":bad})
                    ))
                    .is_ok(),
                "reasoning delta must be unprocessed"
            );
            assert!(
                state
                    .process_sse_block(&event(
                        "response.reasoning_summary_text.done",
                        json!({"text":bad})
                    ))
                    .is_ok(),
                "reasoning done must be unprocessed"
            );
            assert!(
                count() == before,
                "reasoning must emit no format diagnostic"
            );
            drop(state);
            assert!(budget.used() == 0, "reasoning buffers must release budget");
        }
        // A valid boundary-spanning reference still restores; no diagnostic is emitted.
        for split in 0..=(32 + "custom".len()) {
            let (mut state, token, budget) = state();
            let before = count();
            for delta in [&token[..split], &token[split..]] {
                state
                    .process_sse_block(&event("response.output_text.delta", json!({"delta":delta})))
                    .unwrap();
            }
            assert_eq!(state.restored_occurrences(), 1);
            assert_eq!(count(), before);
            drop(state);
            assert_eq!(budget.used(), 0);
        }
        let (mut crossing, _, _) = state();
        crossing
            .process_sse_block(&event(
                "response.refusal.delta",
                json!({"delta":"rredact-custom-"}),
            ))
            .unwrap();
        let before = count();
        format_error(
            crossing.process_sse_block(&event("response.refusal.delta", json!({"delta":"`"}))),
        );
        assert_eq!(count(), before + 1);
        // Done and final snapshots observe the actual failing restore branch.
        for kind in ["response.refusal.done"] {
            let (mut state, _, _) = state();
            let before = count();
            let field = if kind == "response.refusal.done" {
                "refusal"
            } else {
                "text"
            };
            let mut data = json!({});
            data[field] = json!(malformed[0]);
            format_error(state.process_sse_block(&event(kind, data)));
            assert_eq!(count(), before + 1);
        }
        for barrier in [false, true] {
            let (mut state, _, budget) = state();
            if barrier {
                assert!(state
                    .process_sse_block(&event(
                        "response.function_call_arguments.delta",
                        json!({"output_index":0,"delta":"{}"})
                    ))
                    .unwrap()
                    .is_empty());
            }
            let mut items = vec![];
            if barrier {
                items.push(json!({"id":"SYNTHETIC_PRIVATE_TOOL", "type":"function_call", "call_id":"SYNTHETIC_PRIVATE_CALL", "name":"non_executing_fixture", "arguments":"{}", "status":"completed"}));
            }
            items.push(message(malformed[0]));
            let before = count();
            assert!(state
                .process_sse_block(&completed(json!(items)))
                .unwrap()
                .is_empty());
            assert_eq!(
                count(),
                before,
                "buffered events must not emit speculative failures"
            );
            format_error(state.finalize_and_flush(true));
            assert_eq!(count(), before + 1);
            format_error(state.finalize_and_flush(true));
            assert_eq!(count(), before + 1);
            drop(state);
            assert_eq!(budget.used(), 0);
        }
        // Snapshot precedence, unfinished tails, capacity and cancellation retain their original errors.
        let (mut mismatch, _, _) = state();
        mismatch
            .process_sse_block(&event(
                "response.output_text.delta",
                json!({"delta":"safe"}),
            ))
            .unwrap();
        let before = count();
        assert_eq!(
            mismatch
                .process_sse_block(&event(
                    "response.output_text.done",
                    json!({"text":malformed[0]})
                ))
                .unwrap_err(),
            RestoreError::RestoreFailed
        );
        assert_eq!(mismatch.failure_stage(), RestoreStage::SseSnapshot);
        let (mut unfinished, _, _) = state();
        unfinished
            .process_sse_block(&event(
                "response.refusal.delta",
                json!({"delta":"rredact-custom-"}),
            ))
            .unwrap();
        assert_eq!(
            unfinished
                .process_sse_block(&event(
                    "response.refusal.done",
                    json!({"refusal":"rredact-custom-"})
                ))
                .unwrap_err(),
            RestoreError::RestoreFailed
        );
        let (mut cancelled, _, _) = state();
        assert_eq!(
            cancelled.finalize_and_flush(false).unwrap_err(),
            RestoreError::RestoreFailed
        );
        let (mut capacity, _, _) = state();
        assert_eq!(
            capacity
                .process_sse_block(&" ".repeat(1024 * 1024 + 1))
                .unwrap_err(),
            RestoreError::RestoreFailed
        );
        assert_eq!(
            count(),
            before,
            "non-format failures must not emit a structural format log"
        );
        // Event whitelisting and each observable carrier/site are checked at the real sink.
        let (mut unknown, _, _) = state();
        unknown
            .process_sse_block(&event("SYNTHETIC_PRIVATE_EVENT", json!({})))
            .unwrap();
        format_error(unknown.process_sse_block(&event(
            "response.refusal.delta",
            json!({"delta":malformed[0]}),
        )));
        assert!(RECORDS
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("event=other"));
        for (kind, data, carrier) in [
            (
                "response.output_item.added",
                json!({"output_index":0,"item":message(malformed[0])}),
                "refusal",
            ),
            (
                "response.content_part.added",
                json!({"part":{"type":"refusal","refusal":malformed[0]}}),
                "refusal",
            ),
            (
                "response.content_part.added",
                json!({"part":{"type":"summary_text","text":malformed[0]}}),
                "reasoning",
            ),
            (
                "response.content_part.done",
                json!({"part":{"type":"refusal","refusal":malformed[0]}}),
                "refusal",
            ),
        ] {
            let (mut state, _, _) = state();
            let before = count();
            format_error(state.process_sse_block(&event(kind, data)));
            assert_eq!(count(), before + 1);
            let records = RECORDS.lock().unwrap();
            let record = records.last().unwrap();
            assert!(record.contains("site=snapshot_restore"));
            assert!(
                record.contains(&format!("carrier={carrier}")),
                "snapshot carrier must reflect its supported field"
            );
        }
        for custom in [false, true] {
            let (mut state, _, _) = state();
            let item = if custom {
                json!({"id":"SYNTHETIC_PRIVATE_TOOL", "type":"custom_tool_call", "call_id":"SYNTHETIC_PRIVATE_CALL", "name":"non_executing_fixture", "input":malformed[0], "status":"completed"})
            } else {
                json!({"id":"SYNTHETIC_PRIVATE_TOOL", "type":"function_call", "call_id":"SYNTHETIC_PRIVATE_CALL", "name":"non_executing_fixture", "arguments":json!({"value":malformed[0]}).to_string(), "status":"completed"})
            };
            state.process_sse_block(&completed(json!([item]))).unwrap();
            let before = count();
            format_error(state.finalize_and_flush(true));
            assert_eq!(count(), before + 1);
            assert!(RECORDS
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .contains("site=final_tool_restore"));
        }
        let records = RECORDS.lock().unwrap();
        assert!(!records.is_empty());
        for record in records.iter() {
            assert!(
                record.len() <= 2048,
                "diagnostic rendering must have a fixed upper bound"
            );
            assert!(
                ![
                    "SYNTHETIC_PRIVATE",
                    "vary_",
                    "rredact-",
                    "synthetic-restoration-value",
                    "synthetic-carrier-session",
                    "non_executing_fixture",
                    "http://",
                    "00000000000000000000"
                ]
                .iter()
                .any(|sentinel| record.contains(sentinel)),
                "diagnostic must contain no input-derived text"
            );
        }
        Ok(())
    }
}

mod output_text_exact_stream_tests {
    use super::super::{
        payload_budget::PayloadBudget,
        response_restoration::{ArgumentNormalizationMode, SseRestorationState},
        tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    };
    use bytes::Bytes;
    use serde_json::{json, Value};

    fn frame(value: &Value) -> String {
        format!(
            "event: {}\ndata: {value}\n\n",
            value["type"].as_str().expect("synthetic event")
        )
    }

    fn run(text: &str, splits: &[usize], terminal: u8) -> Result<Vec<Bytes>, ()> {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let mut lease = store
            .admit("codex", Some("synthetic-exact-stream"), 1)
            .map_err(|_| ())?;
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-original",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .map_err(|_| ())?;
        let binding = lease
            .bind_actual_attempt(
                "synthetic",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                None,
            )
            .map_err(|_| ())?;
        let mut state =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                .map_err(|_| ())?;
        let mut frames = Vec::new();
        let mut start = 0;
        for end in splits.iter().copied().chain(std::iter::once(text.len())) {
            frames.extend(state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"synthetic-message","delta":&text[start..end]}))).map_err(|_| ())?);
            start = end;
        }
        let part = json!({"type":"output_text","text":text,"annotations":[]});
        let item = json!({"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[part]});
        if terminal == 0 {
            frames.extend(state.process_sse_block(&frame(&json!({"type":"response.output_text.done","output_index":0,"content_index":0,"item_id":"synthetic-message","text":text}))).map_err(|_| ())?);
        }
        if terminal <= 1 {
            frames.extend(state.process_sse_block(&frame(&json!({"type":"response.content_part.done","output_index":0,"content_index":0,"item_id":"synthetic-message","part":part}))).map_err(|_| ())?);
        }
        if terminal <= 2 {
            frames.extend(
                state
                    .process_sse_block(&frame(
                        &json!({"type":"response.output_item.done","output_index":0,"item":item}),
                    ))
                    .map_err(|_| ())?,
            );
        }
        frames.extend(state.process_sse_block(&frame(&json!({"type":"response.completed","response":{"id":"synthetic-response","object":"response","status":"completed","output":[item]}}))).map_err(|_| ())?);
        frames.extend(state.finalize_and_flush(true).map_err(|_| ())?);
        Ok(frames)
    }

    fn text_deltas(frames: &[Bytes]) -> String {
        let mut text = String::new();
        for frame in frames {
            let raw = std::str::from_utf8(frame).expect("synthetic UTF-8");
            let data = raw
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .expect("synthetic data");
            let value: Value = serde_json::from_str(data).expect("synthetic JSON");
            if value["type"] == "response.output_text.delta" {
                text.push_str(value["delta"].as_str().expect("synthetic delta"));
            }
        }
        text
    }

    #[test]
    fn output_text_exact_sse_all_utf8_splits_preserve_nonmatches_and_snapshots() {
        let header = ["rredact", "-custom-"].concat();
        for text in [
            format!("前🔒`{header}` **explanation** 尾"),
            format!("{header}oops {header}** invalid"),
            format!("{header}{}", "0".repeat(20)),
        ] {
            let boundaries = (0..=text.len())
                .filter(|offset| text.is_char_boundary(*offset))
                .collect::<Vec<_>>();
            let frames = run(&text, &boundaries, 0).expect("synthetic multi-delta stream");
            assert!(
                text_deltas(&frames) == text,
                "many UTF-8 deltas must preserve all nonmatching text"
            );
            for split in (0..=text.len()).filter(|offset| text.is_char_boundary(*offset)) {
                let frames = run(&text, &[split], 0);
                assert!(
                    frames.is_ok(),
                    "nonmatching output text must survive every delta split and snapshot"
                );
                assert!(
                    text_deltas(&frames.expect("synthetic frames")) == text,
                    "delta text must be delivered exactly once"
                );
            }
        }
    }

    #[test]
    fn output_text_exact_sse_incomplete_tail_released_at_each_valid_terminal() {
        let header = ["rredact", "-custom-"].concat();
        for terminal in 0..4 {
            for length in 1..(32 + "custom".len()) {
                let reference = format!("{header}{}-ai", "0".repeat(20));
                let text = format!("prefix {}", &reference[..length]);
                let frames = run(&text, &[text.len()], terminal);
                assert!(
                    frames.is_ok(),
                    "incomplete field-end candidates remain ordinary text"
                );
                assert!(
                    text_deltas(&frames.expect("synthetic frames")) == text,
                    "terminal flush must not lose or duplicate text"
                );
            }
        }
    }
    #[test]
    fn output_text_exact_sse_complete_reference_across_tool_barrier_all_splits() {
        for split in 1..(32 + "custom".len()) {
            for terminal_done in [false, true] {
                for authorized in [false, true] {
                    let (lease, reference) = super::chat_carrier_fixture();
                    let binding = lease
                        .bind_actual_attempt(
                            "synthetic",
                            "openai_responses",
                            "http://127.0.0.1:1/responses",
                            None,
                        )
                        .expect("synthetic binding");
                    let mut observer = lease.fork_attempt().expect("synthetic observer");
                    let mut candidate = reference.clone();
                    if !authorized {
                        let replacement = if &candidate[15..16] == "0" { "1" } else { "0" };
                        candidate.replace_range(15..16, replacement);
                    }
                    let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
                    let mut state = SseRestorationState::new(
                        lease,
                        binding.clone(),
                        budget.clone(),
                        ArgumentNormalizationMode::Exact,
                    )
                    .expect("synthetic state");
                    let released = state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":format!("前🔒{reference} ")}))).expect("synthetic released prefix");
                    let mut delivered = text_deltas(&released);
                    assert!(
                        delivered == "前🔒synthetic-restoration-value ",
                        "the already released prefix must be restored normally"
                    );
                    assert!(state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[..split]}))).expect("synthetic prefix").is_empty());
                    let tool = json!({"id":"synthetic-tool","type":"function_call","call_id":"synthetic-call","name":"non_executing_fixture","arguments":"{}","status":"completed"});
                    assert!(state.process_sse_block(&frame(&json!({"type":"response.output_item.added","output_index":1,"item":tool}))).expect("synthetic tool").is_empty());
                    assert!(state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[split..]}))).expect("synthetic suffix").is_empty());
                    let text = format!("前🔒{reference} {candidate}");
                    if terminal_done {
                        assert!(state.process_sse_block(&frame(&json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":text}))).expect("synthetic done").is_empty());
                    }
                    let item = json!({"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text}]});
                    assert!(state.process_sse_block(&frame(&json!({"type":"response.completed","response":{"id":"synthetic-response","status":"completed","output":[item,tool]}}))).expect("synthetic completion").is_empty());
                    let frames = state.finalize_and_flush(true);
                    if authorized {
                        assert!(
                            frames.is_ok(),
                            "authorized references spanning the tool barrier must restore"
                        );
                        let frames = frames.expect("synthetic released frames");
                        delivered.push_str(&text_deltas(&frames));
                        assert!(delivered == "前🔒synthetic-restoration-value synthetic-restoration-value", "barrier replay must restore once without duplicating an already released prefix");
                        assert!(
                            frames
                                .iter()
                                .any(|frame| std::str::from_utf8(frame)
                                    .is_ok_and(|text| text
                                        .starts_with("event: response.output_item.added"))),
                            "validated inert tool is released only at final commit"
                        );
                    } else {
                        assert!(matches!(frames, Err(super::super::response_restoration::RestoreError::FailedBecause(super::super::restore_diagnostics::RestoreReason::TokenContext))), "foreign references spanning the barrier retain context refusal");
                        assert!(
                            observer
                                .resume_completed_context(
                                    &binding,
                                    Some("synthetic-response"),
                                    &[],
                                    &[]
                                )
                                .is_err(),
                            "failed barrier commit must not publish cursor authority"
                        );
                    }
                    drop(released);
                    drop(state);
                    assert_eq!(budget.used(), 0);
                }
            }
        }
    }
}

#[tokio::test]
async fn reversible_http_upstream_error_status_contract_survives_isolation_and_protocols() {
    use super::http_restoration::DlpForwardError;
    use crate::{
        app_config::AppType,
        proxy::{
            error::ProxyError,
            error_mapper::{get_error_message, map_proxy_error_to_status},
            response_processor::isolate_codex_dlp_response_error,
        },
    };
    use axum::body::to_bytes;
    for status in [400, 401, 403, 404, 413, 422, 429, 500, 502, 503, 504] {
        let error = DlpForwardError::UpstreamStatus(http::StatusCode::from_u16(status).unwrap());
        for leased in [false, true] {
            let isolated = isolate_codex_dlp_response_error(ProxyError::DlpForward(error), leased);
            assert!(
                matches!(isolated, ProxyError::DlpForward(actual) if actual == error),
                "safe status-only upstream errors must survive leased error isolation"
            );
            assert_eq!(map_proxy_error_to_status(&isolated), status);
            assert_eq!(
                get_error_message(&isolated),
                "Upstream request failed: upstream_error"
            );
        }
        let unsafe_error = isolate_codex_dlp_response_error(
            ProxyError::UpstreamError {
                status,
                body: Some("synthetic-private-upstream-body".into()),
            },
            true,
        );
        assert!(
            matches!(
                unsafe_error,
                ProxyError::DlpForward(DlpForwardError::RestoreFailedAt(
                    super::restore_diagnostics::RestoreStage::ResponseProcessing
                ))
            ),
            "legacy body-bearing upstream errors must not enter the safe status-only allowlist"
        );
        assert!(!get_error_message(&unsafe_error).contains("synthetic-private-upstream-body"));
        for app_type in [
            AppType::Codex,
            AppType::Claude,
            AppType::ClaudeDesktop,
            AppType::Gemini,
        ] {
            let response = error.into_app_response(&app_type);
            assert_eq!(response.status().as_u16(), status);
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(!std::str::from_utf8(&bytes)
                .unwrap()
                .contains("dlp_restore_failed"));
            match app_type {
                AppType::Claude | AppType::ClaudeDesktop => {
                    assert_eq!(body["type"], "error");
                    assert_eq!(body["error"]["code"], "upstream_error");
                }
                AppType::Gemini => {
                    assert_eq!(body["error"]["code"], status);
                    assert_eq!(body["error"]["message"], "upstream_error");
                }
                _ => {
                    assert_eq!(body["error"]["code"], "upstream_error");
                    assert_eq!(body["error"]["type"], "upstream_error");
                }
            }
        }
    }
}

#[tokio::test]
async fn reasoning_pair_chat_rejects_ignored_reasoning_containers_before_conversion() {
    use crate::proxy::providers::transform_codex_chat::chat_completion_to_response;
    use serde_json::json;
    let marker = "rredact-custom-00000000000000000000-ai";
    let cases = [
        json!({"reasoning_content":{"text":marker}}),
        json!({"reasoning":{"text":"synthetic-safe","future_text":marker}}),
        json!({"reasoning":{"text":"synthetic-safe","metadata":{"text":marker}}}),
        json!({"reasoning":{"text":"synthetic-safe","type":marker}}),
        json!({"reasoning":{"content":"synthetic-safe","text":marker}}),
        json!({"reasoning":[{"text":marker}]}),
        json!({"reasoning":{"text":7}}),
        json!({"reasoning_details":[{"text":"synthetic-safe","signature":marker}]}),
        json!({"reasoning_details":{"parts":[{"text":"synthetic-safe","future_text":marker}]}}),
        json!({"reasoning_details":{"text":"synthetic-safe","parts":[{"text":marker}]}}),
        json!({"reasoning_details":[marker]}),
        json!({"reasoning_details":{"parts":{ "text":marker }}}),
        json!({"reasoning_content":"synthetic-safe","reasoning":{"text":marker}}),
        json!({"reasoning":"synthetic-safe","reasoning_details":{"text":marker}}),
    ];
    for mut message in cases {
        message["content"] = json!("synthetic answer");
        let body = chat_carrier_json(message.clone());
        let converted = chat_completion_to_response(body.clone()).expect("synthetic conversion");
        assert!(
            !converted.to_string().contains(marker),
            "synthetic fixture must exercise ignored reasoning data"
        );
        assert!(
            !chat_carrier_json_permitted(&body),
            "ignored reasoning containers must not authorize JSON completion"
        );
        assert!(
            !strict_sse_candidate(false, chat_carrier_sse(message, false)).await,
            "ignored reasoning containers must not authorize SSE completion"
        );
    }
}

#[test]
fn reasoning_pair_native_json_validates_content_part_types() {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    use serde_json::json;
    let budget = PayloadBudget::with_limit(1024 * 1024);
    for (content, expected) in [
        (
            json!([{"type":"reasoning_text","text":"rredact-custom-"}]),
            true,
        ),
        (
            json!([{"type":"summary_text","text":"synthetic-safe"}]),
            false,
        ),
        (
            json!([{"type":"future_text","text":"synthetic-safe"}]),
            false,
        ),
        (json!([{"type":"reasoning_text","text":7}]), false),
        (json!([{"type":"reasoning_text","text":null}]), false),
        (json!([{"type":"reasoning_text"}]), false),
        (
            json!({"type":"reasoning_text","text":"synthetic-safe"}),
            false,
        ),
    ] {
        let response = json!({"id":"synthetic-pair-response","status":"completed","output":[{
            "id":"synthetic-pair-reasoning","type":"reasoning","content":content
        }]});
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::NativeResponses);
        completion.observe_raw_json(&response.to_string(), &budget);
        assert!(
            completion.permits_completion() == expected,
            "native JSON completion must validate the exact reasoning content shape"
        );
    }
}
