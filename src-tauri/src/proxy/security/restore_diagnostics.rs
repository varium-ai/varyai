//! Fixed restoration checkpoints only; never attach payloads or upstream diagnostics.

macro_rules! restore_stages {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum RestoreStage { $($variant),+ }

        impl RestoreStage {
            pub(crate) fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }

            pub(crate) fn message(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("Reversible DLP response restoration failed. [stage=", $label, "]")),+
                }
            }
        }
    };
}

restore_stages! {
    Unknown => "unknown",
    UpstreamGuard => "upstream_guard",
    UpstreamTimeout => "upstream_timeout",
    SuccessValidation => "success_validation",
    ResponseProcessing => "response_processing",
    ResponseEncoding => "response_encoding",
    SseInit => "sse_init",
    SseRead => "sse_read",
    SseFrame => "sse_frame",
    SseEof => "sse_eof",
    SseEvent => "sse_event",
    TokenRestore => "token_restore",
    SseSnapshot => "sse_snapshot",
    CompletionEvidence => "completion_evidence",
    ToolCommit => "tool_commit",
    ContextPublish => "context_publish",
    JsonRestore => "json_restore",
}

pub(super) fn log_failure(code: &'static str, stage: RestoreStage) {
    log::warn!(
        "[DLP] restoration failure code={} stage={}",
        code,
        stage.label()
    );
}

/// Fixed checkpoints only; these labels never include parser or upstream details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestoreReason {
    Unknown,
    TokenFormat,
    TokenContext,
    RestoreCapacity,
    ArgumentsEmpty,
    ArgumentsSize,
    ArgumentsJson,
    ArgumentsShape,
    ArgumentsRebuild,
    ArgumentsSerialize,
    ArgumentsCopy,
}

impl RestoreReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::TokenFormat => "token_format",
            Self::TokenContext => "token_context",
            Self::RestoreCapacity => "restore_capacity",
            Self::ArgumentsEmpty => "arguments_empty",
            Self::ArgumentsSize => "arguments_size",
            Self::ArgumentsJson => "arguments_json",
            Self::ArgumentsShape => "arguments_shape",
            Self::ArgumentsRebuild => "arguments_rebuild",
            Self::ArgumentsSerialize => "arguments_serialize",
            Self::ArgumentsCopy => "arguments_copy",
        }
    }
}

pub(super) fn log_failure_with_reason(
    code: &'static str,
    stage: RestoreStage,
    reason: Option<RestoreReason>,
) {
    if let Some(reason) = reason {
        log::warn!(
            "[DLP] restoration failure code={} stage={} reason={}",
            code,
            stage.label(),
            reason.label()
        );
    } else {
        log_failure(code, stage);
    }
}

/// Request-side refusal boundaries, not proof of a more specific root cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextCheckpoint {
    ReferenceScan,
    UnleasedReference,
    GatewayState,
    GatewayMissing,
    RuntimeDisabled,
    NamespaceAdmission,
    HistoryExtract,
    HistoryClassify,
    AdmissionAudit,
    AdmissionPrepare,
    AttemptFork,
    AttemptBinding,
    AttemptHistory,
    ContextResume,
    OutputCallIds,
    TokenResume,
    AttemptAudit,
    RequestPrepare,
    RequestTransform,
    FinalValidation,
}

impl ContextCheckpoint {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ReferenceScan => "reference_scan",
            Self::UnleasedReference => "unleased_reference",
            Self::GatewayState => "gateway_state",
            Self::GatewayMissing => "gateway_missing",
            Self::RuntimeDisabled => "runtime_disabled",
            Self::NamespaceAdmission => "namespace_admission",
            Self::HistoryExtract => "history_extract",
            Self::HistoryClassify => "history_classify",
            Self::AdmissionAudit => "admission_audit",
            Self::AdmissionPrepare => "admission_prepare",
            Self::AttemptFork => "attempt_fork",
            Self::AttemptBinding => "attempt_binding",
            Self::AttemptHistory => "attempt_history",
            Self::ContextResume => "context_resume",
            Self::OutputCallIds => "output_call_ids",
            Self::TokenResume => "token_resume",
            Self::AttemptAudit => "attempt_audit",
            Self::RequestPrepare => "request_prepare",
            Self::RequestTransform => "request_transform",
            Self::FinalValidation => "final_validation",
        }
    }
}

pub(super) fn log_context_failure(checkpoint: ContextCheckpoint) {
    log::warn!(
        "[DLP] context failure code=dlp_context_unavailable checkpoint={}",
        checkpoint.label()
    );
}
