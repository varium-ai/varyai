//! Read-only observation of model-proposed tool calls.
//!
//! Assemble ToolObservation values on the proxy response path.
//! Send observations to a risk worker through a bounded asynchronous queue.
//!
//! Preserve wire bytes, event order and model parameters; observation failures only degrade coverage.

pub mod channel;
pub mod mount;
pub mod parsers;
pub mod stream;
pub mod types;

#[allow(unused_imports)]
pub use types::{
    ContextOrigin, ContextValue, ContractError, Correlation, ExecutionScope, IdentityQuality,
    InputEncoding, InputFidelity, InputStatus, ObservationContext, ObservationQuality,
    ObservationSource, ObservationStage, QualityOrigin, SourceTermination, ToolIdentity,
    ToolIdentityQuality, ToolInput, ToolObservation,
};
