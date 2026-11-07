//! Synthetic characterization harness; no production route or completion authority.
use axum::{body::Body, http::HeaderMap, response::Response};
use http_body_util::BodyExt;
use serde_json::{json, Value};

use super::{
    http_restoration::{
        admit_http_request, DlpHttpRequest, DlpPreparedAttempt, RestorationAdmission,
        RestorationGateway,
    },
    payload_budget::PayloadBudget,
    response_restoration::ArgumentNormalizationMode,
    runtime::{self, SecurityRuntimeConfig},
    tokenization::DlpTokenStore,
};
use crate::app_config::AppType;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub(super) const CONTACT: &str = "13900000000";
pub(super) const PROVIDER: &str = "synthetic-unified-provider";
pub(super) const TARGET: &str = "https://synthetic.invalid/v1/responses";
pub(super) const PRINCIPAL: &str = "synthetic-unified-principal";
pub(super) const SESSION: &str = "synthetic-unified-session";
pub(super) const BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Callers hold the repository's serial-test lock while using the global runtime.
pub(super) struct ResponsesFixture {
    previous: SecurityRuntimeConfig,
    pub(super) gateway: RestorationGateway,
    pub(super) store: DlpTokenStore,
}

impl ResponsesFixture {
    pub(super) fn new() -> Self {
        Self::with_runtime(SecurityRuntimeConfig::default())
    }

    pub(super) fn with_runtime(config: SecurityRuntimeConfig) -> Self {
        let previous = (*runtime::get_security_runtime()).clone();
        runtime::set_security_runtime_for_test(config);
        let store = DlpTokenStore::new();
        let gateway = RestorationGateway::with_test_store_and_rules(
            store.clone(),
            &["chinese_phone_number"],
            PayloadBudget::with_limit(BUDGET_BYTES),
        );
        Self {
            previous,
            gateway,
            store,
        }
    }

    pub(super) fn admit(
        &self,
        body: &mut Value,
        compact: bool,
        session: Option<&str>,
    ) -> TestResult<Box<DlpHttpRequest>> {
        let mut headers = HeaderMap::new();
        if let Some(session) = session {
            headers.insert("session-id", session.parse()?);
        }
        match admit_http_request(
            &self.gateway,
            &AppType::Codex,
            true,
            compact,
            body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => Ok(request),
            _ => Err("synthetic request was not admitted".into()),
        }
    }

    pub(super) fn prepare(
        &self,
        body: &mut Value,
        compact: bool,
        session: Option<&str>,
    ) -> TestResult<DlpPreparedAttempt> {
        let request = self.admit(body, compact, session)?;
        prepare_attempt(
            &request,
            body,
            PROVIDER,
            if compact {
                "https://synthetic.invalid/v1/responses/compact"
            } else {
                TARGET
            },
        )
    }
}

impl Drop for ResponsesFixture {
    fn drop(&mut self) {
        runtime::set_security_runtime_for_test(self.previous.clone());
    }
}

pub(super) fn prepare_attempt(
    request: &DlpHttpRequest,
    body: &mut Value,
    provider: &str,
    target: &str,
) -> TestResult<DlpPreparedAttempt> {
    request
        .prepare_attempt(
            provider,
            "openai_responses",
            target,
            Some(PRINCIPAL),
            body,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "synthetic attempt preparation failed".into())
}

pub(super) fn completed_response(text: &str) -> Value {
    json!({
        "id": "synthetic-unified-response", "object": "response", "status": "completed",
        "output": [{"id": "synthetic-unified-message", "type": "message", "role": "assistant",
            "status": "completed", "content": [{"type": "output_text", "text": text}]}]
    })
}

pub(super) async fn restore_json(
    attempt: DlpPreparedAttempt,
    value: &Value,
) -> TestResult<(http::StatusCode, Value)> {
    let response = Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(value)?))?;
    decode_json(
        attempt
            .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
            .await,
    )
    .await
}

pub(super) async fn decode_json(response: Response) -> TestResult<(http::StatusCode, Value)> {
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes)?))
}

/// Uses production state construction and observation; no completion authority is fabricated.
pub(super) fn c3_completion_state(
    attempt: DlpPreparedAttempt,
    provenance: super::restoration_core::AttemptProvenance,
    protocol: Option<super::completion_json::JsonCompletionProtocol>,
    blocks: &[String],
) -> TestResult<super::response_restoration::SseRestorationState> {
    let mut state = super::response_restoration::SseRestorationState::new(
        attempt.lease,
        attempt.binding,
        attempt.budget,
        ArgumentNormalizationMode::Exact,
    )
    .map_err(|_| "synthetic_state_failed")?;
    state.require_original_evidence(super::restoration_core::SseEvidenceRequirement::new(
        provenance,
        protocol,
        ArgumentNormalizationMode::Exact,
    ));
    for block in blocks {
        state
            .process_sse_block(block)
            .map_err(|_| "synthetic_block_failed")?;
    }
    Ok(state)
}

pub(super) fn c3_completion_blocks(text: &str) -> Vec<String> {
    vec![format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type":"response.completed", "response":completed_response(text)
        })
    )]
}
