//! Descriptive registration only. This module grants no admission capability.
pub(super) mod anthropic;
pub(super) mod chat;
pub(super) mod gemini;
pub(super) mod responses;

use super::restoration_contract::{Framing, Operation, ProtocolPair};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Registration {
    DeferToResponsesFacade,
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
        (Protocol::Chat, Operation::ChatCompletions, _)
        | (Protocol::Anthropic, Operation::Messages, _)
        | (Protocol::Gemini, Operation::GenerateContent, _) => Registration::Disabled,
        _ => Registration::Unsupported,
    }
}
