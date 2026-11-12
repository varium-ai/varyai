//! HTTP-owned transport selection for synthetic native JSON and SSE proof fixtures.
//!
//! The helper returns only an opaque proof from the real async driver. It grants
//! no native registration, publication, release identity, or completion shortcut.

use std::{cell::Cell, io, task::Poll};

use bytes::Bytes;
use futures::stream;
use serde_json::Value;

use crate::proxy::protocol_bridge::Protocol;

use super::{
    super::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        protocol_adapters::responses::RestoreError,
        restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol},
        restoration_core::AttemptProvenance,
    },
    native_driver::NativeCompletionDriver,
    VerifiedNativeJson, VerifiedNativeSse,
};

/// Preserve the original Chat/Chat fixture API for existing shared tests.
pub(in crate::proxy::security) async fn native_json_proof(
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: Bytes,
    pre_restoration_client: &Value,
) -> Result<VerifiedNativeJson, RestoreError> {
    native_json_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Chat),
            UpstreamProtocol::new(Protocol::Chat),
        ),
        attempt,
        binding,
        budget,
        original,
        pre_restoration_client,
    )
    .await
}

/// Consume synthetic original bytes through an async Stream, including actual EOF.
/// Keep the actual attempt independent of the selected binding for rejection tests.
pub(in crate::proxy::security) async fn native_json_proof_for_pair(
    pair: ProtocolPair,
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: Bytes,
    pre_restoration_client: &Value,
) -> Result<VerifiedNativeJson, RestoreError> {
    let split = original.len() / 2;
    let mut chunks = [original.slice(..split), original.slice(split..)].into_iter();
    let mut suspend_once = true;
    let eof_polls = Cell::new(0usize);
    let source = stream::poll_fn(|context| {
        if suspend_once {
            suspend_once = false;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        let chunk = chunks.next();
        if chunk.is_none() {
            eof_polls.set(eof_polls.get() + 1);
        }
        Poll::Ready(chunk.map(Ok::<Bytes, io::Error>))
    });
    let proof = NativeCompletionDriver::new(pair, attempt.clone(), binding.clone(), budget.clone())
        .consume_json(source, attempt, pre_restoration_client, attempt)
        .await?;
    assert_eq!(
        eof_polls.get(),
        1,
        "c5_native_http_fixture_requires_actual_stream_eof"
    );
    Ok(proof)
}

/// Preserve the original Chat/Chat fixture API for existing shared tests.
pub(in crate::proxy::security) async fn native_sse_proof(
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: Bytes,
    pre_restoration_client: Bytes,
) -> Result<VerifiedNativeSse, RestoreError> {
    native_sse_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Chat),
            UpstreamProtocol::new(Protocol::Chat),
        ),
        attempt,
        binding,
        budget,
        original,
        pre_restoration_client,
    )
    .await
}

/// Consume independent synthetic SSE entities through the real async driver.
/// Each source yields chunks and Pending/wake before reaching actual None EOF.
/// This does not prove live-tee liveness, per-yield delivery, or publication.
/// Keep the actual attempt independent of the selected binding for rejection tests.
pub(in crate::proxy::security) async fn native_sse_proof_for_pair(
    pair: ProtocolPair,
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: Bytes,
    pre_restoration_client: Bytes,
) -> Result<VerifiedNativeSse, RestoreError> {
    let original_split = original.len() / 2;
    let mut original_chunks = [
        original.slice(..original_split),
        original.slice(original_split..),
    ]
    .into_iter();
    let mut original_suspend = true;
    let original_eof_polls = Cell::new(0usize);
    let original_source = stream::poll_fn(|context| {
        if original_suspend {
            original_suspend = false;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        original_suspend = true;
        let chunk = original_chunks.next();
        if chunk.is_none() {
            original_eof_polls.set(original_eof_polls.get() + 1);
        }
        Poll::Ready(chunk.map(Ok::<Bytes, io::Error>))
    });

    let client_split = pre_restoration_client.len() / 3;
    let mut client_chunks = [
        pre_restoration_client.slice(..client_split),
        pre_restoration_client.slice(client_split..),
    ]
    .into_iter();
    let mut client_suspend = true;
    let client_eof_polls = Cell::new(0usize);
    let client_source = stream::poll_fn(|context| {
        if client_suspend {
            client_suspend = false;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        client_suspend = true;
        let chunk = client_chunks.next();
        if chunk.is_none() {
            client_eof_polls.set(client_eof_polls.get() + 1);
        }
        Poll::Ready(chunk.map(Ok::<Bytes, io::Error>))
    });

    let proof = NativeCompletionDriver::new(pair, attempt.clone(), binding.clone(), budget.clone())
        .consume_sse(original_source, attempt, client_source, attempt)
        .await?;
    assert_eq!(
        original_eof_polls.get(),
        1,
        "c5_native_sse_http_fixture_requires_actual_original_stream_eof"
    );
    assert_eq!(
        client_eof_polls.get(),
        1,
        "c5_native_sse_http_fixture_requires_actual_client_stream_eof"
    );
    Ok(proof)
}
/// Test-only configurable transport entry point; never constructs a proof itself.
pub(in crate::proxy::security) async fn native_json_from_stream<S>(
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: S,
    client: &Value,
) -> Result<VerifiedNativeJson, RestoreError>
where
    S: futures::Stream<Item = Result<Bytes, io::Error>>,
{
    NativeCompletionDriver::new(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Chat),
            UpstreamProtocol::new(Protocol::Chat),
        ),
        attempt.clone(),
        binding.clone(),
        budget.clone(),
    )
    .consume_json(original, attempt, client, attempt)
    .await
}

/// Test-only independent source selection, without any EOF or completion shortcut.
pub(in crate::proxy::security) async fn native_sse_from_streams<O, C>(
    attempt: &AttemptProvenance,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    original: O,
    client: C,
) -> Result<VerifiedNativeSse, RestoreError>
where
    O: futures::Stream<Item = Result<Bytes, io::Error>>,
    C: futures::Stream<Item = Result<Bytes, io::Error>>,
{
    NativeCompletionDriver::new(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Chat),
            UpstreamProtocol::new(Protocol::Chat),
        ),
        attempt.clone(),
        binding.clone(),
        budget.clone(),
    )
    .consume_sse(original, attempt, client, attempt)
    .await
}

/// Test-only native orchestration of optional exact-delivery lookup/import.
/// The core stays strict; mandatory request/response work is outside this decision.
pub(in crate::proxy::security) fn optional_native_delivered_history(
    lease: &mut super::super::tokenization::DlpRequestLease,
    binding: &ActualAttemptBinding,
    client: super::super::restoration_contract::ClientProtocol,
    delivered: &Value,
) -> Result<bool, super::DlpForwardError> {
    // Reject a bad current binding independently, before optional lookup.
    lease
        .require_native_attempt_binding(client, binding)
        .map_err(super::DlpForwardError::from_token_error)?;
    let result = lease
        .verify_native_delivered(binding, client, delivered)
        .and_then(|handle| lease.import_native_completed_context(binding, client, &handle));
    match result {
        Ok(()) => Ok(true),
        Err(error) => {
            super::optional_history_miss(super::DlpForwardError::from_token_error(error))?;
            Ok(false)
        }
    }
}
