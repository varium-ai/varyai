//! Direct native HTTP orchestration; adapters never acquire transport authority.
use sha2::{Digest, Sha256};
use std::{
    io,
    sync::{Arc, Mutex},
};

use super::super::{
    protocol_adapters::chat,
    restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol},
};
use super::*;
use crate::proxy::{hyper_client::ProxyResponse, protocol_bridge::Protocol};

const ENTITY_LIMIT: usize = 1024 * 1024;

/// Boxed delivery stream returned by the native finalizer.
type NativeDelivery = std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

#[cfg(test)]
pub(in crate::proxy::security) mod capture_test_support {
    use std::{
        cell::RefCell,
        marker::PhantomData,
        rc::Rc,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::sync::Notify;

    #[derive(Default)]
    pub(in crate::proxy::security) struct PendingCapture {
        bytes: AtomicUsize,
        changed: Notify,
    }
    thread_local! {
        static OBSERVER: RefCell<Option<Arc<PendingCapture>>> = const { RefCell::new(None) };
    }
    pub(in crate::proxy::security) struct Guard {
        previous: Option<Arc<PendingCapture>>,
        _thread: PhantomData<Rc<()>>,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            OBSERVER.with(|slot| {
                slot.replace(self.previous.take());
            });
        }
    }
    pub(in crate::proxy::security) fn install() -> (Guard, Arc<PendingCapture>) {
        let observer = Arc::new(PendingCapture::default());
        let previous = OBSERVER.with(|slot| slot.replace(Some(observer.clone())));
        (
            Guard {
                previous,
                _thread: PhantomData,
            },
            observer,
        )
    }
    pub(super) fn pending(bytes: usize) {
        OBSERVER.with(|slot| {
            if let Some(observer) = slot.borrow().as_ref() {
                observer.bytes.store(bytes, Ordering::SeqCst);
                observer.changed.notify_one();
            }
        });
    }
    impl PendingCapture {
        pub(in crate::proxy::security) async fn after_bytes(&self, expected: &AtomicUsize) {
            loop {
                let notice = self.changed.notified();
                let expected = expected.load(Ordering::SeqCst);
                if expected > 0 && self.bytes.load(Ordering::SeqCst) == expected {
                    return;
                }
                notice.await;
            }
        }
    }
}

fn contract_error(error: super::super::restoration_contract::ContractError) -> DlpForwardError {
    use super::super::restoration_contract::ContractError;
    match error {
        ContractError::Resource(error) => DlpForwardError::from_token_error(error),
        // Signed or opaque content that a mandatory patch would edit is a stable,
        // user-actionable category, not an anonymous preparation failure.
        ContractError::ImmutableSite => {
            DlpForwardError::RestoreUnsupportedFor(RestoreUnsupportedCause::ImmutableSignedUnit)
        }
        _ => DlpForwardError::RedactionFailed,
    }
}

pub(super) fn admit(
    gateway: &RestorationGateway,
    app: &AppType,
    body: &mut Value,
    headers: &HeaderMap,
    protocol: Protocol,
) -> Result<RestorationAdmission, crate::proxy::error::ProxyError> {
    let Some(state) = gateway.state().map_err(|error| {
        crate::proxy::error::ProxyError::DlpForward(DlpForwardError::from_token_error(error))
    })?
    else {
        return admit_http_request(gateway, app, false, false, true, body, headers);
    };
    let snapshot = admit_security_runtime()
        .map_err(|_| crate::proxy::error::ProxyError::DlpForward(DlpForwardError::Unavailable))?;
    if !(snapshot.runtime.restoration_enabled()
        || (snapshot.runtime.enabled && !state.rules.is_empty()))
    {
        return admit_http_request(gateway, app, false, false, true, body, headers);
    }
    let build = || -> Result<DlpHttpRequest, DlpForwardError> {
        let session = match protocol {
            Protocol::Chat => {
                crate::proxy::session::extract_chat_restoration_session_id(headers, body)
            }
            Protocol::Anthropic => {
                crate::proxy::session::extract_anthropic_restoration_session_id(headers, body)
            }
            Protocol::Gemini => {
                crate::proxy::session::extract_gemini_restoration_session_id(headers)
            }
            _ => return Err(DlpForwardError::RestoreUnsupported),
        };
        let authority = match protocol {
            Protocol::Chat => RequestAuthority::admit_native_chat(
                &state.store,
                app.as_str(),
                session,
                snapshot.restoration_policy_revision,
                state.budget.clone(),
            ),
            Protocol::Anthropic => RequestAuthority::admit_native_anthropic(
                &state.store,
                app.as_str(),
                session,
                snapshot.restoration_policy_revision,
                state.budget.clone(),
            ),
            Protocol::Gemini => RequestAuthority::admit_native_gemini(
                &state.store,
                app.as_str(),
                session,
                snapshot.restoration_policy_revision,
                state.budget.clone(),
            ),
            _ => return Err(DlpForwardError::RestoreUnsupported),
        }
        .map_err(DlpForwardError::from_token_error)?;
        let ledger = state
            .store
            .allocation_ledger()
            .map_err(DlpForwardError::from_token_error)?;
        let mut rules = StoreVec::with_capacity(state.rules.len(), ledger.clone(), 4096)
            .map_err(DlpForwardError::from_token_error)?;
        let mut refs =
            PayloadVec::new(state.budget.clone()).map_err(DlpForwardError::from_token_error)?;
        for rule in state.rules.as_slice() {
            rules
                .push_without_growth(
                    StoreText::try_from_str(rule.as_str(), ledger.clone(), 256)
                        .map_err(DlpForwardError::from_token_error)?,
                )
                .map_err(DlpForwardError::from_token_error)?;
            refs.push(rule.as_str())
                .map_err(DlpForwardError::from_token_error)?;
        }
        let source_body =
            PayloadJson::try_from_value(body, state.budget.clone(), state.budget.limit())
                .map_err(DlpForwardError::from_token_error)?;
        let audit_state = Arc::new(
            RequestAuditState::new(app, session, state.budget.clone())
                .map_err(DlpForwardError::from_token_error)?,
        );
        let initial_audit = Arc::new(
            PendingRequestAudit::capture(
                body,
                &snapshot.runtime,
                refs.as_slice(),
                authority.lease(),
                state.budget.clone(),
            )
            .map_err(DlpForwardError::from_plan_error_native)?,
        );
        Ok(DlpHttpRequest {
            authority,
            runtime: (*snapshot.runtime).clone(),
            restore_rules: rules,
            source_body,
            pretokenized: false,
            native_protocol: Some(protocol),
            endpoint_is_compact: false,
            audit_state,
            initial_audit,
        })
    };
    Ok(match build() {
        Ok(request) => RestorationAdmission::Admitted(Box::new(request)),
        Err(error) => RestorationAdmission::Intercepted(error.into_app_response(app)),
    })
}

/// Optional native replay lookup only: exact ordered assistant snapshots, never tool results.
/// Both JSON and composed SSE evidence retain their frozen, distinct snapshot shapes.
fn import_native_chat_history(
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    body: &Value,
    budget: &PayloadBudget,
) -> Result<(), DlpForwardError> {
    use super::super::payload_json::{JsonProjection, ProjectedItem};
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    if messages.len() > 4096 {
        return Err(DlpForwardError::CapacityExceeded);
    }
    let mut snapshots =
        PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
    for message in messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
    {
        if snapshots.len() >= 128 {
            return Err(DlpForwardError::CapacityExceeded);
        }
        snapshots
            .push(ProjectedItem {
                value: message,
                cached: None,
                missing: 0,
            })
            .map_err(DlpForwardError::from_token_error)?;
    }
    if snapshots.as_slice().is_empty() {
        return Ok(());
    }
    // Stage all imports on a fork: an edited, reordered or incomplete batch must
    // not retain privileges from a previously matched prefix.
    let client = ClientProtocol::new(Protocol::Chat);
    let mut staged = lease
        .fork_attempt_extracted()
        .map_err(DlpForwardError::from_token_error)?;
    let mut start = 0usize;
    let mut previous_serial = None;
    let mut work = 32 * 1024 * 1024usize;
    while start < snapshots.len() {
        let mut matched = None;
        for end in start + 1..=snapshots.len() {
            for composed_shape in [false, true] {
                let mut items =
                    PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
                for snapshot in &snapshots.as_slice()[start..end] {
                    items
                        .push(*snapshot)
                        .map_err(DlpForwardError::from_token_error)?;
                    if composed_shape {
                        if let Some(calls) =
                            snapshot.value.get("tool_calls").and_then(Value::as_array)
                        {
                            for call in calls {
                                if items.len() >= 128 {
                                    return Err(DlpForwardError::CapacityExceeded);
                                }
                                items
                                    .push(ProjectedItem {
                                        value: call,
                                        cached: None,
                                        missing: 0,
                                    })
                                    .map_err(DlpForwardError::from_token_error)?;
                            }
                        }
                    }
                }
                let delivered = PayloadJson::try_from_projection(
                    JsonProjection::Input(items.as_slice()),
                    budget.clone(),
                    ENTITY_LIMIT,
                )
                .map_err(DlpForwardError::from_token_error)?;
                // Bound repeated exact hashing, including rejected partitions.
                work = work
                    .checked_sub(
                        encode_outbound_with_budget(delivered.as_value(), budget.clone())?.len(),
                    )
                    .ok_or(DlpForwardError::CapacityExceeded)?;
                let handle = match staged.verify_native_delivered_extracted(
                    binding,
                    client,
                    delivered.as_value(),
                ) {
                    Ok(handle) => handle,
                    Err(TokenError::ContextUnavailable) => continue,
                    Err(error) => return Err(DlpForwardError::from_token_error(error)),
                };
                let serial = staged
                    .native_delivered_serial(binding, client, &handle)
                    .map_err(DlpForwardError::from_token_error)?;
                if previous_serial.is_some_and(|previous| serial <= previous) {
                    continue;
                }
                if let Some((prior_end, prior_serial, _)) = &matched {
                    if *prior_end != end || *prior_serial != serial {
                        return Ok(());
                    }
                } else {
                    matched = Some((end, serial, handle));
                }
            }
        }
        let Some((end, serial, handle)) = matched else {
            return Ok(());
        };
        staged
            .import_native_completed_context_extracted(binding, client, &handle)
            .map_err(DlpForwardError::from_token_error)?;
        start = end;
        previous_serial = Some(serial);
    }
    *lease = staged;
    Ok(())
}

/// Import complete, ordered assistant block batches atomically; tool results are never proof.
fn import_native_anthropic_history(
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    body: &Value,
    budget: &PayloadBudget,
) -> Result<(), DlpForwardError> {
    use super::super::payload_json::{JsonProjection, ProjectedItem};
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    if messages.len() > 4096 {
        return Err(DlpForwardError::CapacityExceeded);
    }
    let mut assistants =
        PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
    for message in messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
    {
        if assistants.len() >= 128 {
            return Err(DlpForwardError::CapacityExceeded);
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            // Unknown assistant shapes invalidate the entire optional history claim.
            return Ok(());
        };
        if blocks.is_empty() {
            return Ok(());
        }
        assistants
            .push(blocks)
            .map_err(DlpForwardError::from_token_error)?;
    }
    if assistants.as_slice().is_empty() {
        return Ok(());
    }
    let client = ClientProtocol::new(Protocol::Anthropic);
    let mut staged = lease
        .fork_attempt_extracted()
        .map_err(DlpForwardError::from_token_error)?;
    let mut start = 0usize;
    let mut previous_serial = None;
    let mut work = 32 * 1024 * 1024usize;
    while start < assistants.len() {
        let mut matched = None;
        for end in start + 1..=assistants.len() {
            let mut items =
                PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for blocks in &assistants.as_slice()[start..end] {
                for block in *blocks {
                    if items.len() >= 128 {
                        return Err(DlpForwardError::CapacityExceeded);
                    }
                    items
                        .push(ProjectedItem {
                            value: block,
                            cached: None,
                            missing: 0,
                        })
                        .map_err(DlpForwardError::from_token_error)?;
                }
            }
            let delivered = PayloadJson::try_from_projection(
                JsonProjection::Input(items.as_slice()),
                budget.clone(),
                ENTITY_LIMIT,
            )
            .map_err(DlpForwardError::from_token_error)?;
            work = work
                .checked_sub(
                    encode_outbound_with_budget(delivered.as_value(), budget.clone())?.len(),
                )
                .ok_or(DlpForwardError::CapacityExceeded)?;
            let handle = match staged.verify_native_delivered_extracted(
                binding,
                client,
                delivered.as_value(),
            ) {
                Ok(handle) => handle,
                Err(TokenError::ContextUnavailable) => continue,
                Err(error) => return Err(DlpForwardError::from_token_error(error)),
            };
            let serial = staged
                .native_delivered_serial(binding, client, &handle)
                .map_err(DlpForwardError::from_token_error)?;
            if previous_serial.is_some_and(|previous| serial <= previous) {
                continue;
            }
            if let Some((prior_end, prior_serial, _)) = &matched {
                if *prior_end != end || *prior_serial != serial {
                    return Ok(());
                }
            } else {
                matched = Some((end, serial, handle));
            }
        }
        let Some((end, serial, handle)) = matched else {
            return Ok(());
        };
        staged
            .import_native_completed_context_extracted(binding, client, &handle)
            .map_err(DlpForwardError::from_token_error)?;
        start = end;
        previous_serial = Some(serial);
    }
    *lease = staged;
    Ok(())
}

/// Check ordered protected messages after request redaction, not as history proof.
/// Only the digest is kept on the attempt; no plaintext or authority is retained here.
pub(super) fn anthropic_send_history_digest(body: &Value) -> Result<[u8; 32], DlpForwardError> {
    let mut digest = Sha256::new();
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        if messages.len() > 4096 {
            return Err(DlpForwardError::CapacityExceeded);
        }
        for (index, message) in messages.iter().enumerate() {
            let assistant = message.get("role").and_then(Value::as_str) == Some("assistant");
            let immutable = message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        matches!(
                            block.get("type").and_then(Value::as_str),
                            Some("thinking" | "redacted_thinking")
                        )
                    })
                });
            if assistant || immutable {
                digest.update(index.to_be_bytes());
                digest.update(
                    completion_verifier::json_digest(message)
                        .map_err(|_| DlpForwardError::RedactionFailed)?,
                );
            }
        }
    }
    Ok(digest.finalize().into())
}

pub(super) fn validate_native_history_digest(
    expected: Option<[u8; 32]>,
    body: &Value,
) -> Result<(), DlpForwardError> {
    if let Some(expected) = expected {
        if anthropic_send_history_digest(body)? != expected {
            return Err(DlpForwardError::ContextUnavailable);
        }
    }
    Ok(())
}

impl DlpHttpRequest {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_native_chat_attempt<F>(
        &self,
        provider: &str,
        protocol: &str,
        target: &str,
        principal: Option<&str>,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
        enriched: &mut Value,
        transform: F,
    ) -> Result<(DlpPreparedAttempt, DlpTransformInputOwner), DlpForwardError>
    where
        F: FnOnce(
            Value,
            &DlpHistoryTransform<'_>,
        ) -> Result<(Value, Option<DlpHistoryMapping>), DlpForwardError>,
    {
        let native = self
            .native_protocol
            .ok_or(DlpForwardError::RestoreUnsupported)?;
        let expected = match native {
            Protocol::Chat => "openai_chat",
            Protocol::Anthropic => "anthropic",
            Protocol::Gemini => "gemini_native",
            _ => return Err(DlpForwardError::RestoreUnsupported),
        };
        if protocol != expected || operation.is_some() {
            return Err(DlpForwardError::RestoreUnsupported);
        }
        let (mut lease, binding) = self
            .authority
            .fork_bound_attempt(provider, protocol, target, principal, None)
            .map_err(|error| DlpForwardError::from_token_error(error.error))?;
        let budget = self.authority.budget();
        lease
            .require_native_attempt_binding(ClientProtocol::new(native), &binding)
            .map_err(DlpForwardError::from_token_error)?;
        let observation = match native {
            Protocol::Chat => {
                import_native_chat_history(&mut lease, &binding, enriched, &budget)?;
                chat::history::observe_request(enriched, budget.clone()).map_err(contract_error)
            }
            Protocol::Anthropic => {
                import_native_anthropic_history(&mut lease, &binding, enriched, &budget)?;
                super::super::protocol_adapters::anthropic::history::observe_request(
                    enriched,
                    budget.clone(),
                )
                .map_err(contract_error)
            }
            Protocol::Gemini => {
                super::super::protocol_adapters::gemini::history::observe_admitted_request(
                    enriched,
                    budget.clone(),
                )
                .map_err(DlpForwardError::from_plan_error_native)
            }
            _ => return Err(DlpForwardError::RestoreUnsupported),
        }?;
        let mut rules =
            PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
        for rule in self.restore_rules.as_slice() {
            rules
                .push(rule.as_str())
                .map_err(DlpForwardError::from_token_error)?;
        }
        let pending_audit = PendingRequestAudit::capture(
            enriched,
            &self.runtime,
            rules.as_slice(),
            &lease,
            budget.clone(),
        )
        .map_err(DlpForwardError::from_plan_error_native)?;
        let prepared = super::super::request_preparation::prepare_observed_request(
            &mut lease,
            &observation,
            &self.runtime,
            rules.as_slice(),
            &[],
            |_, _, _| Ok(()),
        )
        .map_err(DlpForwardError::from_plan_error_native)?;
        drop(observation);
        let (body, reservation) = prepared.body.into_parts();
        let input_owner = DlpTransformInputOwner {
            _reservation: reservation,
        };
        let context = DlpHistoryTransform {
            lease: &lease,
            binding: &binding,
            origin: self.source_body.as_value(),
        };
        let native_history_digest = (native == Protocol::Anthropic)
            .then(|| anthropic_send_history_digest(&body))
            .transpose()?;
        let (final_body, _) = transform(body, &context)?;
        validate_native_history_digest(native_history_digest, &final_body)?;
        validate_final_outbound_body(
            &final_body,
            protocol,
            &lease,
            FinalOutboundContext {
                runtime: &self.runtime,
                budget: &budget,
                audit_state: &self.audit_state,
                provider_id: provider,
                binding: Some(&binding),
                history: None,
            },
        )?;
        *enriched = final_body;
        Ok((
            DlpPreparedAttempt {
                lease,
                binding,
                budget: budget.clone(),
                endpoint_is_compact: false,
                // Gemini has no converted-JSON label; a missing label fails closed
                // instead of pairing Client=Gemini with an unrelated upstream protocol.
                upstream_json_protocol: match native {
                    Protocol::Anthropic => Some(JsonCompletionProtocol::AnthropicMessages),
                    Protocol::Gemini => None,
                    _ => Some(JsonCompletionProtocol::ChatCompletions),
                },
                provenance: super::super::restoration_core::AttemptProvenance::new(&budget)
                    .map_err(DlpForwardError::from_token_error)?,
                json_completion: completion_verifier::JsonCompletionVerifier::new(),
                audit_state: self.audit_state.clone(),
                initial_audit: self.initial_audit.clone(),
                pending_audit,
                upstream_payload_owner: None,
                history_mapping: None,
                native_history_digest,
                native_protocol: Some(native),
                native_original: None,
                native_error_safety: None,
                sse_evidence: Mutex::new(None),
            },
            input_owner,
        ))
    }
}

// Only the transport wrapper below can create this state, after actual async EOF.
// Terminal markers and client-side re-encoding cannot manufacture original evidence.
// Keep the verifier inline: CaptureState is charged in full, while boxing adds an uncharged allocation.
#[allow(clippy::large_enum_variant)]
enum OriginalReady {
    /// Token-free Gemini answer: there is nothing to restore or publish, so strict
    /// completion shape checks would only reject a legitimate upstream response.
    Verbatim(PayloadBuffer),
    Json(completion_verifier::NativeJsonVerifier, PayloadBuffer),
    Sse(completion_verifier::NativeSseVerifier),
    SseError(PayloadBuffer),
}
struct CaptureState {
    ready: Option<OriginalReady>,
    _reservation: super::super::payload_budget::Reservation,
}
pub(super) struct OriginalCapture(Arc<Mutex<CaptureState>>);

fn record_original(state: &Mutex<CaptureState>, ready: OriginalReady) -> Result<(), io::Error> {
    state
        .lock()
        .map_err(|_| io::Error::other("Response stream failed."))?
        .ready = Some(ready);
    Ok(())
}

pub(super) fn capture_original(
    response: ProxyResponse,
    budget: &PayloadBudget,
    actual: &super::super::restoration_core::AttemptProvenance,
    binding: &ActualAttemptBinding,
    protocol: Protocol,
    error_safety: Option<super::super::upstream_error::NativeErrorSafety>,
) -> Result<(ProxyResponse, OriginalCapture), DlpForwardError> {
    let reservation = budget
        .reserve(std::mem::size_of::<CaptureState>() + 64)
        .map_err(DlpForwardError::from_token_error)?;
    let capture = Arc::new(Mutex::new(CaptureState {
        ready: None,
        _reservation: reservation,
    }));
    let mut bytes = PayloadBuffer::new(budget.clone(), ENTITY_LIMIT)
        .map_err(DlpForwardError::from_token_error)?;
    let budget = budget.clone();
    let actual = actual.clone();
    let binding = binding.clone();
    let status = response.status();
    let headers = response.headers().clone();
    let sse = response.is_sse();
    let decoder = super::super::upstream_guard::NativeJsonDecoder::new(&headers)?;
    let observer = capture.clone();
    let stream = async_stream::try_stream! {
        let source = response.bytes_stream();
        futures::pin_mut!(source);
        while let Some(chunk) = {
            #[cfg(test)]
            {
                futures::future::poll_fn(|cx| {
                    let next = source.as_mut().poll_next(cx);
                    if next.is_pending() { capture_test_support::pending(bytes.len()); }
                    next
                }).await
            }
            #[cfg(not(test))]
            { source.next().await }
        } {
            let chunk = chunk.map_err(|_| io::Error::other("Response stream failed."))?;
            bytes.append(&chunk).map_err(|_| io::Error::other("Response stream failed."))?;
            yield chunk;
        }
        let pair = ProtocolPair::new(ClientProtocol::new(protocol), UpstreamProtocol::new(protocol));
        let token_free_gemini =
            protocol == Protocol::Gemini && !entity_contains_token_marker(bytes.as_slice(), sse);
        let ready = if token_free_gemini {
            OriginalReady::Verbatim(bytes)
        } else if sse {
            if let Some(error) = native_sse_error(bytes.as_slice(), &budget, error_safety, protocol).map_err(|_| io::Error::other("Response stream failed."))? {
                OriginalReady::SseError(error)
            } else {
                let mut verifier = completion_verifier::NativeSseVerifier::new(pair, actual.clone(), binding, budget);
                verifier.consume_upstream_sse(bytes.as_slice(), &actual).map_err(|_| io::Error::other("Response stream failed."))?;
                OriginalReady::Sse(verifier)
            }
        } else {
            // The actual upstream yielded None above; decoding cannot manufacture EOF.
            let bytes = decoder.decode(bytes, &budget, ENTITY_LIMIT).map_err(|_| io::Error::other("Response stream failed."))?;
            let mut verifier = completion_verifier::NativeJsonVerifier::new(pair, actual.clone(), binding, budget);
            verifier.consume_upstream_json(bytes.as_slice(), &actual).map_err(|_| io::Error::other("Response stream failed."))?;
            OriginalReady::Json(verifier, bytes)
        };
        record_original(&observer, ready)?;
    };
    Ok((
        ProxyResponse::streamed(status, headers, stream),
        OriginalCapture(capture),
    ))
}

/// Recognize only a fully framed genuine terminal error from the actual upstream.
/// Successful completion verification is deliberately separate; no tools escape here.
fn native_sse_error(
    source: &[u8],
    budget: &PayloadBudget,
    safety: Option<super::super::upstream_error::NativeErrorSafety>,
    protocol: Protocol,
) -> Result<Option<PayloadBuffer>, RestoreError> {
    let raw = std::str::from_utf8(source).map_err(|_| RestoreError::RestoreFailed)?;
    let mut frame_start = 0usize;
    let mut offset = 0usize;
    let mut error = None;
    let mut terminal = false;
    let mut frames = 0usize;
    for line in raw.split_inclusive('\n') {
        offset += line.len();
        let content = line.strip_suffix('\n').ok_or(RestoreError::RestoreFailed)?;
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content.contains('\r') {
            return Err(RestoreError::RestoreFailed);
        }
        if !content.is_empty() {
            continue;
        }
        frames += 1;
        if frames > 4096 {
            return Err(RestoreError::RestoreFailed);
        }
        let frame = &raw[frame_start..offset];
        frame_start = offset;
        let mut event = None;
        let mut data =
            PayloadBuffer::new(budget.clone(), ENTITY_LIMIT).map_err(RestoreError::capacity)?;
        let mut data_lines = 0usize;
        for line in frame.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match name {
                "event" if event.is_none() => event = Some(value),
                "data" => {
                    if data_lines != 0 {
                        data.append(b"\n").map_err(RestoreError::capacity)?;
                    }
                    data.append(value.as_bytes())
                        .map_err(RestoreError::capacity)?;
                    data_lines += 1;
                }
                "id" | "retry" => {}
                _ => return Err(RestoreError::RestoreFailed),
            }
        }
        if data_lines == 0 {
            continue;
        }
        if error.is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        if protocol == Protocol::Chat && data.as_slice() == b"[DONE]" {
            terminal = true;
            continue;
        }
        let text = std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        let parsed = PayloadJson::try_from_str(text, budget.clone(), ENTITY_LIMIT)
            .map_err(RestoreError::capacity)?;
        let value = parsed.as_value();
        if protocol == Protocol::Anthropic {
            let kind = value
                .get("type")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            if event != Some(kind) || terminal {
                return Err(RestoreError::RestoreFailed);
            }
            match kind {
                "error"
                    if value.get("error").is_some_and(Value::is_object)
                        && value["error"].get("type").is_some_and(Value::is_string)
                        && value["error"].get("message").is_some_and(Value::is_string)
                        && value.get("content").is_none()
                        && value.get("message").is_none() =>
                {
                    error = Some((parsed, true));
                }
                "message_stop" => terminal = true,
                "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "ping"
                    if value.get("error").is_none() => {}
                _ => return Err(RestoreError::RestoreFailed),
            }
            continue;
        }
        if protocol == Protocol::Gemini {
            // Gemini has no [DONE] marker. A native error envelope is never forwarded
            // here because M4 does not claim error-frame sanitation; anything else is
            // left to strict completion verification.
            if value.get("error").is_some_and(|error| !error.is_null()) {
                return Err(RestoreError::RestoreFailed);
            }
            continue;
        }
        if protocol != Protocol::Chat {
            return Err(RestoreError::RestoreUnsupported);
        }
        if value.get("error").is_some_and(|error| !error.is_null()) {
            if terminal
                || !matches!(event, None | Some("" | "message" | "error"))
                || !value["error"].is_object()
                || value.get("choices").is_some()
            {
                return Err(RestoreError::RestoreFailed);
            }
            error = Some((parsed, event == Some("error")));
        } else {
            if !matches!(event, None | Some("" | "message")) {
                return Err(RestoreError::RestoreFailed);
            }
            // A choice finish_reason does not terminate the stream: other choices
            // may still be active. Only the whole-stream [DONE] marker forbids errors.
        }
    }
    if frame_start != raw.len() {
        return Err(RestoreError::RestoreFailed);
    }
    let Some((error, named)) = error else {
        return Ok(None);
    };
    let safe = safety
        .ok_or(RestoreError::RestoreFailed)?
        .sanitize(error.as_value())
        .map_err(RestoreError::capacity)?;
    let mut wire =
        PayloadBuffer::new(budget.clone(), ENTITY_LIMIT).map_err(RestoreError::capacity)?;
    if named {
        wire.append(b"event: error\n")
            .map_err(RestoreError::capacity)?;
    }
    wire.append(b"data: ").map_err(RestoreError::capacity)?;
    wire.append(safe.as_slice())
        .map_err(RestoreError::capacity)?;
    wire.append(b"\n\n").map_err(RestoreError::capacity)?;
    Ok(Some(wire))
}

/// Conservative scan for the local placeholder grammar. Raw bytes are checked first;
/// text leaves are then decoded so a marker split across Parts or SSE frames is still
/// found. Any parse, depth or work failure returns `true`, which forces the strict path.
fn entity_contains_token_marker(entity: &[u8], sse: bool) -> bool {
    if raw_contains_token_marker(entity) {
        return true;
    }
    let Ok(text) = std::str::from_utf8(entity) else {
        return true;
    };
    let mut scan = MarkerScan::new();
    if sse {
        sse_payloads_contain_marker(text, &mut scan)
    } else {
        json_payload_contains_marker(text, &mut scan)
    }
    .unwrap_or(true)
}

fn raw_contains_token_marker(bytes: &[u8]) -> bool {
    const MARKERS: [&[u8]; 2] = [
        MARKER_STEM.as_bytes(),
        super::super::token_format::LEGACY_PREFIX.as_bytes(),
    ];
    MARKERS
        .iter()
        .any(|marker| bytes.windows(marker.len()).any(|window| window == *marker))
}

/// Prefix up to but excluding the separator, so a doubly escaped dash
/// (`rredact\u002d...`) still registers as a placeholder.
const MARKER_STEM: &str = "rredact";
const MARKER_CARRY_BYTES: usize = 16;
const MAX_MARKER_SCAN_WORK: usize = 4 * 1024 * 1024;
const MAX_MARKER_SCAN_DEPTH: usize = 128;

/// Feeds text leaves in order, remembering the previous tail so a marker spanning two
/// leaves is detected without joining (and therefore without a large allocation).
struct MarkerScan {
    carry: Vec<u8>,
    remaining_work: usize,
}

impl MarkerScan {
    fn new() -> Self {
        Self {
            carry: Vec::new(),
            remaining_work: MAX_MARKER_SCAN_WORK,
        }
    }

    fn feed(&mut self, text: &[u8]) -> Result<bool, ()> {
        self.remaining_work = self.remaining_work.checked_sub(text.len() + 1).ok_or(())?;
        const MARKERS: [&[u8]; 2] = [
            MARKER_STEM.as_bytes(),
            super::super::token_format::LEGACY_PREFIX.as_bytes(),
        ];
        if MARKERS
            .iter()
            .any(|marker| text.windows(marker.len()).any(|window| window == *marker))
        {
            return Ok(true);
        }
        for marker in MARKERS {
            for split in 1..marker.len() {
                if self.carry.ends_with(&marker[..split]) && text.starts_with(&marker[split..]) {
                    return Ok(true);
                }
            }
        }
        let keep = text.len().min(MARKER_CARRY_BYTES);
        self.carry.clear();
        self.carry.extend_from_slice(&text[text.len() - keep..]);
        Ok(false)
    }
}

#[cfg(test)]
mod marker_scan_tests {
    use super::*;

    fn json(text: &str) -> bool {
        entity_contains_token_marker(text.as_bytes(), false)
    }

    fn sse(text: &str) -> bool {
        entity_contains_token_marker(text.as_bytes(), true)
    }

    #[test]
    fn marker_in_one_leaf_is_found() {
        assert!(json(
            r#"{"candidates":[{"content":{"parts":[{"text":"rredact-cn_phone-abc"}]}}]}"#
        ));
        assert!(json(
            r#"{"candidates":[{"content":{"parts":[{"text":"vary_cn_phone-abc"}]}}]}"#
        ));
    }

    #[test]
    fn marker_split_across_two_parts_is_found() {
        assert!(json(
            r#"{"candidates":[{"content":{"parts":[{"text":"rreda"},{"text":"ct-cn_phone-abc"}]}}]}"#
        ));
    }

    #[test]
    fn marker_split_across_sibling_values_is_found() {
        // Keys between the two values must not break the carried tail.
        assert!(json(r#"{"x":"rreda","y":"ct-cn_phone-abc"}"#));
    }

    #[test]
    fn marker_split_across_two_sse_frames_is_found() {
        let wire = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"rreda\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ct-cn_phone-abc\"}]}}]}\n\n"
        );
        assert!(sse(wire));
    }

    #[test]
    fn escaped_or_legacy_markers_are_found() {
        // A doubly escaped dash would parse to a literal backslash-u sequence.
        assert!(json(r#"{"a":"rredact\u002dcn_phone-abc"}"#));
        assert!(json(r#"{"a":"vary_cn_phone-abc"}"#));
    }

    #[test]
    fn top_level_array_is_scanned() {
        assert!(json(r#"[{"text":"rredact-cn_phone-abc"}]"#));
    }

    #[test]
    fn clean_bodies_are_not_flagged() {
        assert!(!json(
            r#"{"candidates":[{"content":{"parts":[{"text":"Synthetic public answer."}]}}]}"#
        ));
        assert!(!sse(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]}}]}\n\n"
        ));
    }

    #[test]
    fn unparsable_input_fails_closed() {
        assert!(json("not json"));
        assert!(sse("data: {\"a\":1}\n"));
        assert!(entity_contains_token_marker(&[0xff, 0xfe], false));
    }
}

fn json_payload_contains_marker(text: &str, scan: &mut MarkerScan) -> Result<bool, ()> {
    let value: Value = serde_json::from_str(text).map_err(|_| ())?;
    scan_json_leaves(&value, scan, 0)
}

fn scan_json_leaves(value: &Value, scan: &mut MarkerScan, depth: usize) -> Result<bool, ()> {
    if depth > MAX_MARKER_SCAN_DEPTH {
        return Err(());
    }
    match value {
        Value::String(text) => scan.feed(text.as_bytes()),
        Value::Array(items) => {
            for item in items {
                if scan_json_leaves(item, scan, depth + 1)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Object(entries) => {
            // Keys are not restored carriers. Feeding them would also break the carried
            // tail between sibling values and across Parts.
            for (_key, item) in entries {
                if scan_json_leaves(item, scan, depth + 1)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn sse_payloads_contain_marker(text: &str, scan: &mut MarkerScan) -> Result<bool, ()> {
    let mut frame_start = 0usize;
    let mut offset = 0usize;
    let mut frames = 0usize;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if !content.is_empty() {
            continue;
        }
        frames += 1;
        if frames > 4096 {
            return Err(());
        }
        if scan_sse_block(&text[frame_start..offset], scan)? {
            return Ok(true);
        }
        frame_start = offset;
    }
    // An unfinished trailing frame, or a body with no complete frame, fails closed.
    if frame_start != text.len() || frames == 0 {
        return Err(());
    }
    Ok(false)
}

fn scan_sse_block(block: &str, scan: &mut MarkerScan) -> Result<bool, ()> {
    let mut data = String::new();
    let mut lines = 0usize;
    for line in block.lines() {
        let Some(value) = line
            .strip_prefix("data:")
            .or_else(|| (line == "data").then_some(""))
        else {
            continue;
        };
        if lines != 0 {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
        lines += 1;
    }
    if lines == 0 {
        return Ok(false);
    }
    let value: Value = serde_json::from_str(&data).map_err(|_| ())?;
    scan_json_leaves(&value, scan, 0)
}

async fn collect_client(body: Body, budget: &PayloadBudget) -> Result<PayloadBuffer, RestoreError> {
    let mut output =
        PayloadBuffer::new(budget.clone(), ENTITY_LIMIT).map_err(RestoreError::capacity)?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
        output.append(&chunk).map_err(RestoreError::capacity)?;
    }
    Ok(output)
}

/// Protocols whose SSE clients are known to accept one fixed terminal error frame.
fn is_framed_error_protocol(protocol: Option<Protocol>) -> bool {
    matches!(protocol, Some(Protocol::Anthropic) | Some(Protocol::Gemini))
}

/// Fixed terminal error frame for a real failure after SSE headers were sent. It carries
/// the generic processing code and no cause: the caller must not receive a fabricated
/// DLP reason, and the failing stage stays in sanitized diagnostics.
fn fixed_sse_error_frame(protocol: Option<Protocol>) -> &'static [u8] {
    match protocol {
        Some(Protocol::Gemini) => {
            b"data: {\"error\":{\"code\":502,\"message\":\"The local proxy could not process the request. Please retry.\",\"status\":\"UNAVAILABLE\"}}\n\n"
        }
        _ => {
            b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"code\":\"proxy_processing_failed\",\"message\":\"The local proxy could not process the request. Please retry.\"}}\n\n"
        }
    }
}

impl DlpPreparedAttempt {
    pub(crate) fn is_native_chat(&self) -> bool {
        self.native_protocol == Some(Protocol::Chat)
    }

    pub(crate) fn is_native_anthropic(&self) -> bool {
        self.native_protocol == Some(Protocol::Anthropic)
    }

    pub(crate) fn is_native_gemini(&self) -> bool {
        self.native_protocol == Some(Protocol::Gemini)
    }

    async fn finalize_native_chat_body(
        &mut self,
        body: Body,
        sse: bool,
    ) -> Result<NativeDelivery, RestoreError> {
        let client = collect_client(body, &self.budget).await?;
        let capture = self
            .native_original
            .take()
            .ok_or(RestoreError::RestoreFailed)?;
        // A token-free Gemini answer has nothing to restore and nothing to publish.
        // Return the upstream bytes verbatim instead of turning an unsupported shape
        // (truncated terminal reason, media Parts, safety block) into a proxy error.
        if self.is_native_gemini() && !entity_contains_token_marker(client.as_slice(), sse) {
            return Ok(Box::pin(futures::stream::once(async {
                Ok(bytes_with_owner(client, capture))
            })));
        }
        // Without a local placeholder the forwarded body already equals the restored
        // one, so a restoration failure there changes nothing for the caller.
        let recoverable = !entity_contains_token_marker(client.as_slice(), sse);
        let ready = capture
            .0
            .lock()
            .map_err(|_| RestoreError::RestoreFailed)?
            .ready
            .take()
            .ok_or(RestoreError::RestoreFailed)?;
        // Verification decides whether the answer may be forwarded and stays strict:
        // an answer this path cannot confirm is a real failure of the answer itself.
        // Only the restoration step that follows it degrades.
        match (sse, ready) {
            // Only reachable if the client copy diverged from the captured upstream
            // body; return the captured answer rather than a fabricated error.
            (_, OriginalReady::Verbatim(original)) => Ok(Box::pin(futures::stream::once(async {
                Ok(bytes_with_owner(original, capture))
            }))),
            (true, OriginalReady::SseError(wire)) => {
                // Failure delivery has no restoration, lifecycle or publication capability.
                let stream = async_stream::try_stream! { yield bytes_with_owner(wire, capture); };
                Ok(Box::pin(stream))
            }
            (true, OriginalReady::Sse(mut verifier)) => {
                verifier.consume_client_sse(client.as_slice(), &self.provenance)?;
                let proof = verifier.finish()?;
                let finalize = match self.native_protocol {
                    Some(Protocol::Chat) => chat::response::finalize_sse,
                    Some(Protocol::Anthropic) => {
                        super::super::protocol_adapters::anthropic::response::finalize_sse
                    }
                    Some(Protocol::Gemini) => {
                        super::super::protocol_adapters::gemini::response::finalize_sse
                    }
                    _ => return Err(RestoreError::RestoreUnsupported),
                };
                let restored = finalize(
                    client.as_slice(),
                    &self.lease,
                    &self.binding,
                    self.budget.clone(),
                    &self.provenance,
                    proof,
                    ENTITY_LIMIT,
                )
                .map(|delivery| delivery.into_delivery_stream());
                Self::degrade_restoration(restored, client, recoverable, self.native_protocol, sse)
            }
            (false, OriginalReady::Json(mut verifier, original)) => {
                let text = std::str::from_utf8(client.as_slice())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                let parsed = PayloadJson::try_from_str(text, self.budget.clone(), ENTITY_LIMIT)
                    .map_err(RestoreError::capacity)?;
                verifier.observe_client_json(parsed.as_value(), &self.provenance)?;
                let proof = verifier.finish()?;
                let stage_fn = match self.native_protocol {
                    Some(Protocol::Chat) => chat::response::stage_json,
                    Some(Protocol::Anthropic) => {
                        super::super::protocol_adapters::anthropic::response::stage_json
                    }
                    Some(Protocol::Gemini) => {
                        super::super::protocol_adapters::gemini::response::stage_json
                    }
                    _ => return Err(RestoreError::RestoreUnsupported),
                };
                let stage = stage_fn(
                    parsed.as_value(),
                    &self.lease,
                    &self.binding,
                    self.budget.clone(),
                    &self.provenance,
                )?;
                let restored = super::super::response_restoration::finalize_native_json(
                    stage,
                    &self.lease,
                    proof,
                    &self.provenance,
                    bytes_with_owner(original, capture),
                    ENTITY_LIMIT,
                )
                .map(|delivery| delivery.into_delivery_stream());
                Self::degrade_restoration(restored, client, recoverable, self.native_protocol, sse)
            }
            _ => Err(RestoreError::RestoreFailed),
        }
    }

    /// Keep an answer the proxy already verified usable. Restoration is an enhancement,
    /// never a gate: when the upstream body carries no local placeholder, forwarding it
    /// unchanged delivers exactly what restoration would have delivered. Nothing is
    /// published and no client capability is granted here.
    ///
    /// A body that does carry placeholders cannot take that path, because forwarding it
    /// would hand the caller unresolved local markers. Such an answer was verified, so a
    /// streaming Anthropic or Gemini client still receives one fixed error frame instead
    /// of a silently cut stream; every other carrier keeps the real failure.
    fn degrade_restoration(
        restored: Result<NativeDelivery, RestoreError>,
        client: PayloadBuffer,
        recoverable: bool,
        protocol: Option<Protocol>,
        sse: bool,
    ) -> Result<NativeDelivery, RestoreError> {
        match restored {
            Ok(delivery) => Ok(delivery),
            Err(error) if error.is_resource_exhaustion() => Err(error),
            Err(_) if recoverable => {
                let stage = RestoreStage::ResponseProcessing;
                super::super::restore_diagnostics::log_failure(
                    DlpForwardError::RestoreFailedAt(stage).code(),
                    stage,
                );
                Ok(Box::pin(futures::stream::once(async move {
                    Ok(bytes_with_owner(client, ()))
                })))
            }
            Err(_) if sse && is_framed_error_protocol(protocol) => {
                let stage = RestoreStage::ResponseProcessing;
                super::super::restore_diagnostics::log_failure(
                    DlpForwardError::RestoreFailedAt(stage).code(),
                    stage,
                );
                Ok(Box::pin(futures::stream::once(async move {
                    Ok(Bytes::from_static(fixed_sse_error_frame(protocol)))
                })))
            }
            Err(error) => Err(error),
        }
    }

    /// Protocol-shaped error for a real failure of the answer itself: a non-success
    /// upstream status, an unverifiable body, or a verified answer that cannot be turned
    /// into a valid client response. The native protocols report server-class status
    /// because the upstream already answered; the client projection stays generic, with
    /// no fabricated DLP cause.
    fn native_restore_error_response(protocol: Option<Protocol>) -> Response {
        let mut response = if protocol == Some(Protocol::Anthropic) {
            DlpForwardError::RestoreFailedAt(RestoreStage::ResponseProcessing)
                .into_app_response(&AppType::Claude)
        } else if protocol == Some(Protocol::Gemini) {
            DlpForwardError::RestoreFailedAt(RestoreStage::ResponseProcessing)
                .into_app_response(&AppType::Gemini)
        } else {
            build_codex_restore_error_response_at(
                RestoreError::RestoreFailed,
                RestoreStage::ResponseProcessing,
            )
        };
        if matches!(protocol, Some(Protocol::Anthropic) | Some(Protocol::Gemini)) {
            *response.status_mut() = StatusCode::BAD_GATEWAY;
        }
        response
    }

    pub(crate) async fn restore_native_chat_http_response(
        mut self,
        response: Response,
    ) -> Response {
        let (mut parts, body) = response.into_parts();
        if !parts.status.is_success()
            || parts
                .headers
                .get(header::CONTENT_ENCODING)
                .is_some_and(|value| value != "identity")
        {
            // A real failure of the answer itself keeps the protocol projection.
            return Self::native_restore_error_response(self.native_protocol);
        }
        let sse = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));
        clear_entity_headers(&mut parts.headers);
        if sse {
            // Restoration problems degrade inside finalize; only a transport failure of
            // the client body itself ends this stream early.
            // Nothing has been yielded yet, so an answer that never verified can still
            // end with one fixed error frame on a native Anthropic or Gemini stream
            // instead of a silently broken one. A verified answer that cannot be
            // restored already produced its own delivery.
            let protocol = self.native_protocol;
            let stream = async_stream::try_stream! {
                match self.finalize_native_chat_body(body, true).await {
                    Ok(mut delivery) => {
                        while let Some(chunk) = delivery.next().await { yield chunk?; }
                    }
                    Err(_) if is_framed_error_protocol(protocol) => {
                        yield Bytes::from_static(fixed_sse_error_frame(protocol));
                    }
                    Err(_) => Err(io::Error::other("Response stream failed."))?,
                }
            };
            let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> =
                Box::pin(stream);
            Response::from_parts(parts, Body::from_stream(stream))
        } else {
            match self.finalize_native_chat_body(body, false).await {
                Ok(delivery) => Response::from_parts(parts, Body::from_stream(delivery)),
                Err(_) => Self::native_restore_error_response(self.native_protocol),
            }
        }
    }
}
