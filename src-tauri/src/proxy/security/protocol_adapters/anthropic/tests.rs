//! Compile-only shared boundary probe; no native lane behavior.
use crate::proxy::{protocol_bridge::Protocol, security::restoration_contract::Operation};

const CONTRACT_PROTOCOL: Protocol = Protocol::Anthropic;
const CONTRACT_OPERATION: Operation = Operation::Messages;

#[path = "../contract_probe.rs"]
mod contract_probe;
