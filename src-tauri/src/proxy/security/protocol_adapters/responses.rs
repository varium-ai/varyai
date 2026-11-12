//! Responses/Compact protocol facade over the existing strict request and response owners.
//! HTTP admission remains the sole route owner; native scopes acquire no authority here.

pub(in crate::proxy::security) use crate::proxy::security::request_preparation::{
    classify_request_history, is_fresh_readable_history_for_call_selection,
    prepare_codex_request_with_budget as prepare_request,
    prepare_codex_request_with_history as prepare_request_with_history,
    RequestHistoryClassification,
};
pub(in crate::proxy::security) use crate::proxy::security::response_restoration::{
    failed_sse_frame_at, stage_json_response_bound, ArgumentNormalizationMode,
    OriginalXaiArguments, ResponsesJsonInspection, ResponsesSseInspection, RestoreError,
    SseRestorationState,
};
#[cfg(test)]
pub(in crate::proxy::security) use crate::proxy::security::response_restoration::{
    prepare_text_projection, serialize_json_with_budget, stage_json_response, RestoredJsonResponse,
    TextProjectionStream,
};
