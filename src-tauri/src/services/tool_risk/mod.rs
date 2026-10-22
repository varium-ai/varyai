//! Tool-call risk detection services.
//!
//! Semantic adapters, built-in detectors, declarative rules and versioned snapshots.
//! Never execute observed input or read sensitive targets to validate a finding.

pub mod adapters;
pub mod builtin;
pub mod classifier;
pub(crate) mod command_semantics;
pub mod detectors;
pub(crate) mod download_semantics;
pub mod engine;
pub(crate) mod patterns;
pub mod response_parser;
pub mod runtime;
pub mod sanitize;
pub mod service;
pub(crate) mod shell;
pub(crate) mod transfer_semantics;
pub mod types;
pub mod worker;

#[allow(unused_imports)]
pub use types::{
    ConditionOp, LocalReview, OperationType, RiskCategory, RuleCondition, RuleOrigin, RuleSelector,
    RuleTarget, RuleValidationError, Severity, ToolRiskRule,
};
