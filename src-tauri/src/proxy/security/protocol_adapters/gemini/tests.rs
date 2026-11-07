//! Compile-only shared boundary probe; no native lane behavior.
use crate::proxy::{protocol_bridge::Protocol, security::restoration_contract::Operation};

const CONTRACT_PROTOCOL: Protocol = Protocol::Gemini;
const CONTRACT_OPERATION: Operation = Operation::GenerateContent;

#[path = "../contract_probe.rs"]
mod contract_probe;
