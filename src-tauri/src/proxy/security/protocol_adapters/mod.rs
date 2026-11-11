//! Descriptive registration only. This module grants no admission capability.
pub(super) mod anthropic;
pub(super) mod chat;
pub(super) mod gemini;
pub(super) mod responses;

use super::restoration_contract::{Framing, Operation, ProtocolPair};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Registration {
    DeferToResponsesFacade,
    NativeChat,
    NativeAnthropic,
    NativeGemini,
    Disabled,
    Unsupported,
}

pub(super) fn registration(
    pair: ProtocolPair,
    operation: Operation,
    framing: Framing,
) -> Registration {
    use crate::proxy::protocol_bridge::Protocol;
    // Upstream identity is deliberately not a grant: only the old facade can
    // decide whether an existing Responses conversion cell is actually usable.
    match (pair.client().protocol(), operation, framing) {
        (Protocol::Responses, Operation::Responses, _)
        | (Protocol::Responses, Operation::ResponsesCompact, Framing::Json) => {
            Registration::DeferToResponsesFacade
        }
        (Protocol::Chat, Operation::ChatCompletions, Framing::Json | Framing::Sse)
            if pair.upstream().protocol() == Protocol::Chat =>
        {
            Registration::NativeChat
        }
        (Protocol::Anthropic, Operation::Messages, Framing::Json | Framing::Sse)
            if pair.upstream().protocol() == Protocol::Anthropic =>
        {
            Registration::NativeAnthropic
        }
        (Protocol::Gemini, Operation::GenerateContent, Framing::Json | Framing::Sse)
            if pair.upstream().protocol() == Protocol::Gemini =>
        {
            Registration::NativeGemini
        }
        (Protocol::Chat, Operation::ChatCompletions, _)
        | (Protocol::Anthropic, Operation::Messages, _)
        | (Protocol::Gemini, Operation::GenerateContent, _) => Registration::Disabled,
        _ => Registration::Unsupported,
    }
}

#[cfg(test)]
mod integration_registration_tests {
    use super::*;
    use crate::proxy::protocol_bridge::Protocol;
    use crate::proxy::security::restoration_contract::{ClientProtocol, UpstreamProtocol};

    #[test]
    fn m2_registry_describes_chat_and_later_integrated_cells() {
        for framing in [Framing::Json, Framing::Sse] {
            for upstream in [
                Protocol::Chat,
                Protocol::Responses,
                Protocol::Anthropic,
                Protocol::Gemini,
            ] {
                let pair = ProtocolPair::new(
                    ClientProtocol::new(Protocol::Chat),
                    UpstreamProtocol::new(upstream),
                );
                let registered = registration(pair, Operation::ChatCompletions, framing);
                if upstream == Protocol::Chat {
                    assert!(
                        !matches!(
                            registered,
                            Registration::Disabled
                                | Registration::Unsupported
                                | Registration::DeferToResponsesFacade
                        ),
                        "m2_direct_chat_registration_not_enabled"
                    );
                } else {
                    assert!(
                        matches!(
                            registered,
                            Registration::Disabled | Registration::Unsupported
                        ),
                        "m2_cross_protocol_chat_registration_enabled"
                    );
                }
            }
            // M4 integrates the previously deferred same-protocol Gemini cell.
            for (client, operation) in [(Protocol::Gemini, Operation::GenerateContent)] {
                let pair =
                    ProtocolPair::new(ClientProtocol::new(client), UpstreamProtocol::new(client));
                assert!(
                    registration(pair, operation, framing) == Registration::NativeGemini,
                    "m4_same_protocol_gemini_registration_not_enabled"
                );
            }
        }
    }

    #[test]
    fn m3_registry_describes_only_same_protocol_anthropic_messages_json_and_sse() {
        for framing in [Framing::Json, Framing::Sse] {
            for upstream in [
                Protocol::Chat,
                Protocol::Responses,
                Protocol::Anthropic,
                Protocol::Gemini,
            ] {
                for operation in [
                    Operation::Responses,
                    Operation::ResponsesCompact,
                    Operation::ChatCompletions,
                    Operation::Messages,
                    Operation::GenerateContent,
                ] {
                    let pair = ProtocolPair::new(
                        ClientProtocol::new(Protocol::Anthropic),
                        UpstreamProtocol::new(upstream),
                    );
                    let registered = registration(pair, operation, framing);
                    if upstream == Protocol::Anthropic && operation == Operation::Messages {
                        assert!(
                            registered == Registration::NativeAnthropic,
                            "m3_same_protocol_anthropic_registration_not_enabled"
                        );
                    } else {
                        assert!(
                            matches!(
                                registered,
                                Registration::Disabled | Registration::Unsupported
                            ),
                            "m3_anthropic_registration_expanded_protocol_or_operation"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn m4_registry_describes_only_same_protocol_gemini_generate_content_json_and_sse() {
        for framing in [Framing::Json, Framing::Sse] {
            for upstream in [
                Protocol::Chat,
                Protocol::Responses,
                Protocol::Anthropic,
                Protocol::Gemini,
            ] {
                for operation in [
                    Operation::Responses,
                    Operation::ResponsesCompact,
                    Operation::ChatCompletions,
                    Operation::Messages,
                    Operation::GenerateContent,
                ] {
                    let pair = ProtocolPair::new(
                        ClientProtocol::new(Protocol::Gemini),
                        UpstreamProtocol::new(upstream),
                    );
                    let registered = registration(pair, operation, framing);
                    if upstream == Protocol::Gemini && operation == Operation::GenerateContent {
                        assert!(
                            registered == Registration::NativeGemini,
                            "m4_same_protocol_gemini_registration_not_enabled"
                        );
                    } else {
                        assert!(
                            matches!(
                                registered,
                                Registration::Disabled | Registration::Unsupported
                            ),
                            "m4_gemini_registration_expanded_protocol_or_operation"
                        );
                    }
                }
            }
        }
    }
}
