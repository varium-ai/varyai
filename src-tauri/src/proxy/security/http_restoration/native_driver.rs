//! Private HTTP-owned, bounded native completion transport driver.
//!
//! This collects completion evidence, not a native adapter or a route-enablement
//! grant. Each entity must reach actual asynchronous `None` before the real
//! synchronous Read/framing verifier can inspect it and issue an opaque proof.
//! A protocol terminal, an empty Bytes item, and Pending are never entity EOF.
//! Original and pre-restoration client evidence must use the same native Chat
//! protocol, exact primitive digest, actual attempt, and actual-attempt binding.
//!
//! PayloadBuffer charges before allocating or copying and caps each entity at
//! one MiB. Retained owners are dropped on error or cancellation. Collection is
//! driven only by polling this future: Pending suspends it, the original SSE
//! entity is consumed and verified before the client source starts, and neither
//! source is polled after error, budget denial, or EOF. There is no background
//! drainer, unbounded channel, or independently retained transport chunk.
//! This does NOT prove per-yield delivery backpressure, tool release,
//! finalization, publication, live-tee liveness, or remote receipt.
//!
//! Only opaque completion proofs leave this module. There is no finish/EOF flag,
//! frames/pending pair, boolean witness, raw publisher, or proof constructor here.
//! Primitive constructors/issuers must ultimately be HTTP-private; old sibling
//! fixtures need migration to an HTTP-owned cfg(test) transport selection, not a
//! production visibility exception. This file intentionally does not edit them.

use std::io;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;

use crate::proxy::protocol_bridge::Protocol;

use super::{
    super::{
        completed_context::ActualAttemptBinding, payload_budget::PayloadBudget,
        payload_buffer::PayloadBuffer, protocol_adapters::responses::RestoreError,
        restoration_contract::ProtocolPair, restoration_core::AttemptProvenance,
    },
    completion_verifier::{
        NativeJsonVerifier, NativeSseVerifier, VerifiedNativeJson, VerifiedNativeSse,
    },
};

const NATIVE_DRIVER_ENTITY_LIMIT: usize = 1024 * 1024;

/// Consumed by one HTTP completion attempt; not an adapter-facing capability.
/// Deliberately no Clone, Debug, serialization, finish method, or proof issuer.
pub(super) struct NativeCompletionDriver {
    pair: ProtocolPair,
    attempt: AttemptProvenance,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
}

impl NativeCompletionDriver {
    pub(super) fn new(
        pair: ProtocolPair,
        attempt: AttemptProvenance,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
    ) -> Self {
        Self {
            pair,
            attempt,
            binding,
            budget,
        }
    }

    fn check_attempts(
        &self,
        original_actual: &AttemptProvenance,
        client_actual: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        if self.pair.client().protocol() != Protocol::Chat
            || self.pair.upstream().protocol() != Protocol::Chat
        {
            return Err(RestoreError::RestoreUnsupported);
        }
        if !self.attempt.matches(original_actual) || !self.attempt.matches(client_actual) {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    }

    /// Consume the original async JSON entity and verify the pre-restoration
    /// client Value through the real primitive, with exact native digest equality.
    /// Both provenance inputs must match before the first source poll, even when
    /// there are zero mappings.
    pub(super) async fn consume_json<S>(
        self,
        original: S,
        original_actual: &AttemptProvenance,
        pre_restoration_client: &Value,
        client_actual: &AttemptProvenance,
    ) -> Result<VerifiedNativeJson, RestoreError>
    where
        S: Stream<Item = Result<Bytes, io::Error>>,
    {
        self.check_attempts(original_actual, client_actual)?;
        let mut verifier =
            NativeJsonVerifier::new(self.pair, self.attempt, self.binding, self.budget.clone());
        let original = collect_native_entity(original, &self.budget).await?;
        verifier.consume_upstream_json(original.as_slice(), original_actual)?;
        drop(original);
        verifier.observe_client_json(pre_restoration_client, client_actual)?;
        verifier.finish()
    }

    /// Consume original SSE to actual EOF before polling the pre-restoration
    /// client entity, then consume/parse its framing independently to actual EOF.
    /// Return only the opaque proof, never raw frames or a publication owner.
    /// These are independent evidence streams, not a bounded tee implementation;
    /// live-source wiring/tee deadlock and per-yield release need separate tests.
    pub(super) async fn consume_sse<O, C>(
        self,
        original: O,
        original_actual: &AttemptProvenance,
        pre_restoration_client: C,
        client_actual: &AttemptProvenance,
    ) -> Result<VerifiedNativeSse, RestoreError>
    where
        O: Stream<Item = Result<Bytes, io::Error>>,
        C: Stream<Item = Result<Bytes, io::Error>>,
    {
        self.check_attempts(original_actual, client_actual)?;
        let mut verifier =
            NativeSseVerifier::new(self.pair, self.attempt, self.binding, self.budget.clone());
        let original = collect_native_entity(original, &self.budget).await?;
        verifier.consume_upstream_sse(original.as_slice(), original_actual)?;
        drop(original);
        let client = collect_native_entity(pre_restoration_client, &self.budget).await?;
        verifier.consume_client_sse(client.as_slice(), client_actual)?;
        drop(client);
        verifier.finish()
    }
}

/// The returned slice can serve as a synchronous Read source only after real EOF.
async fn collect_native_entity<S>(
    source: S,
    budget: &PayloadBudget,
) -> Result<PayloadBuffer, RestoreError>
where
    S: Stream<Item = Result<Bytes, io::Error>>,
{
    let mut bytes = PayloadBuffer::new(budget.clone(), NATIVE_DRIVER_ENTITY_LIMIT)
        .map_err(|_| RestoreError::RestoreFailed)?;
    futures::pin_mut!(source);
    while let Some(chunk) = source.next().await {
        // No retries or further polls on any error, including Interrupted.
        let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
        // Append checks the entity ceiling and charges before allocation/copy.
        // The transport item is dropped before the next asynchronous poll.
        bytes
            .append(&chunk)
            .map_err(|_| RestoreError::RestoreFailed)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll, Waker},
    };

    use futures::{task::noop_waker_ref, StreamExt};
    use serde_json::json;

    use crate::proxy::{
        protocol_bridge::Protocol,
        security::{
            protocol_adapters::{registration, Registration},
            restoration_contract::{
                ClientProtocol, ContinuityScope, Framing, IngressNamespace, Operation,
                UpstreamProtocol,
            },
            tokenization::{DlpRequestLease, DlpTokenStore},
        },
    };

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn required<T>(result: Result<T, RestoreError>, marker: &'static str) -> TestResult<T> {
        result.map_err(|_| io::Error::other(marker).into())
    }

    fn rejected<T>(result: Result<T, RestoreError>, marker: &'static str) {
        assert!(result.is_err(), "{marker}");
    }

    fn pair(client: Protocol, upstream: Protocol) -> ProtocolPair {
        ProtocolPair::new(ClientProtocol::new(client), UpstreamProtocol::new(upstream))
    }

    fn chat() -> ClientProtocol {
        ClientProtocol::new(Protocol::Chat)
    }

    struct Fixture {
        store: DlpTokenStore,
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        attempt: AttemptProvenance,
        budget: PayloadBudget,
    }

    impl Fixture {
        fn new() -> TestResult<Self> {
            let store = DlpTokenStore::new();
            let lease = store.admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: chat(),
                    ingress: IngressNamespace::Direct,
                },
                "synthetic-async-driver-app",
                Some("synthetic-async-driver-session"),
                1,
            )?;
            let binding = lease.bind_actual_attempt(
                "synthetic-async-driver-provider",
                "openai_chat",
                "https://synthetic.invalid/v1/chat/completions",
                Some("synthetic-async-driver-principal"),
            )?;
            let budget = PayloadBudget::with_limit(4 * NATIVE_DRIVER_ENTITY_LIMIT);
            let attempt = AttemptProvenance::new(&budget)?;
            assert_eq!(store.stats()?.entries, 0);
            Ok(Self {
                store,
                lease,
                binding,
                attempt,
                budget,
            })
        }

        fn driver(&self) -> NativeCompletionDriver {
            NativeCompletionDriver::new(
                pair(Protocol::Chat, Protocol::Chat),
                self.attempt.clone(),
                self.binding.clone(),
                self.budget.clone(),
            )
        }

        fn other_binding(&self) -> TestResult<ActualAttemptBinding> {
            Ok(self.lease.bind_actual_attempt(
                "synthetic-other-provider",
                "openai_chat",
                "https://synthetic-other.invalid/v1/chat/completions",
                Some("synthetic-other-principal"),
            )?)
        }

        // These helpers use the async driver, never a cfg(test) proof constructor.
        async fn json_proof(&self, client: &Value) -> TestResult<VerifiedNativeJson> {
            let raw = serde_json::to_vec(client)?;
            let (source, probe) = ScriptedStream::split(&raw, raw.len() / 2);
            let proof = required(
                self.driver()
                    .consume_json(source, &self.attempt, client, &self.attempt)
                    .await,
                "C5_NATIVE_DRIVER_JSON_DONOR_REQUIRES_PROOF",
            )?;
            probe.assert_eof();
            Ok(proof)
        }

        async fn sse_proof(&self, raw: &[u8]) -> TestResult<VerifiedNativeSse> {
            let (source, original) = ScriptedStream::split(raw, raw.len() / 2);
            let (client, delivered) = ScriptedStream::split(raw, raw.len() / 3);
            let proof = required(
                self.driver()
                    .consume_sse(source, &self.attempt, client, &self.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_DONOR_REQUIRES_PROOF",
            )?;
            original.assert_eof();
            delivered.assert_eof();
            Ok(proof)
        }
    }

    fn chat_json() -> Value {
        json!({
            "id":"chatcmpl_synthetic_async_driver",
            "object":"chat.completion",
            "choices":[{
                "index":0,
                "message":{"role":"assistant","content":"synthetic 雪 🦀 text"},
                "finish_reason":"stop"
            }],
            "synthetic_extension":{"preserve":true,"number":1}
        })
    }

    fn chat_sse() -> Vec<u8> {
        let delta = json!({
            "id":"chatcmpl_synthetic_async_driver",
            "object":"chat.completion.chunk",
            "choices":[{
                "index":0,
                "delta":{"role":"assistant","content":"synthetic 雪 🦀 text"},
                "finish_reason":null
            }]
        });
        let terminal = json!({
            "id":"chatcmpl_synthetic_async_driver",
            "object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]
        });
        format!("data: {delta}\r\n\r\ndata: {terminal}\r\n\r\ndata: [DONE]\r\n\r\n").into_bytes()
    }

    // Synthetic fixture storage below is not retained production payload backing.
    // Observations and assertion diagnostics contain counts only, never raw bytes.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Trace {
        polls: usize,
        chunks: usize,
        bytes: usize,
        pending: usize,
        errors: usize,
        eofs: usize,
        polls_after_end: usize,
        drops: usize,
    }

    #[derive(Clone, Default)]
    struct Probe(Rc<RefCell<Trace>>);

    impl Probe {
        fn snapshot(&self) -> Trace {
            *self.0.borrow()
        }

        fn assert_consumed_eof(&self) {
            let trace = self.snapshot();
            assert_eq!(trace.eofs, 1, "C5_NATIVE_DRIVER_ACTUAL_EOF_REQUIRED");
            assert_eq!(trace.errors, 0);
            assert_eq!(
                trace.polls_after_end, 0,
                "C5_NATIVE_DRIVER_POLLED_AFTER_END"
            );
        }

        fn assert_eof(&self) {
            self.assert_consumed_eof();
            assert_eq!(
                self.snapshot().drops,
                1,
                "C5_NATIVE_DRIVER_SOURCE_NOT_DROPPED"
            );
        }

        fn assert_error(&self) {
            let trace = self.snapshot();
            assert_eq!(trace.errors, 1, "C5_NATIVE_DRIVER_ERROR_ITEM_NOT_CONSUMED");
            assert_eq!(trace.eofs, 0, "C5_NATIVE_DRIVER_ERROR_IS_NOT_EOF");
            assert_eq!(
                trace.polls_after_end, 0,
                "C5_NATIVE_DRIVER_POLLED_AFTER_ERROR"
            );
            assert_eq!(trace.drops, 1);
        }
    }

    #[derive(Clone, Default)]
    struct Gate(Rc<GateState>);

    #[derive(Default)]
    struct GateState {
        open: Cell<bool>,
        waker: RefCell<Option<Waker>>,
    }

    impl Gate {
        fn release(&self) {
            self.0.open.set(true);
            if let Some(waker) = self.0.waker.borrow_mut().take() {
                waker.wake();
            }
        }
    }

    enum Step {
        Chunk(Bytes),
        PendingOnce,
        Wait(Gate),
        Error(io::ErrorKind),
        Eof,
    }

    struct ScriptedStream {
        steps: VecDeque<Step>,
        probe: Probe,
        ended: bool,
    }

    impl ScriptedStream {
        fn new(steps: impl IntoIterator<Item = Step>) -> (Self, Probe) {
            let probe = Probe::default();
            (
                Self {
                    steps: steps.into_iter().collect(),
                    probe: probe.clone(),
                    ended: false,
                },
                probe,
            )
        }

        fn entity(raw: &[u8]) -> (Self, Probe) {
            Self::new([Step::Chunk(Bytes::copy_from_slice(raw)), Step::Eof])
        }

        fn split(raw: &[u8], at: usize) -> (Self, Probe) {
            Self::new([
                Step::Chunk(Bytes::copy_from_slice(&raw[..at])),
                Step::PendingOnce,
                Step::Chunk(Bytes::new()),
                Step::PendingOnce,
                Step::Chunk(Bytes::copy_from_slice(&raw[at..])),
                Step::PendingOnce,
                Step::Eof,
            ])
        }

        fn one_byte(raw: &[u8]) -> (Self, Probe) {
            let mut steps = Vec::new();
            for byte in raw {
                steps.push(Step::PendingOnce);
                steps.push(Step::Chunk(Bytes::new()));
                steps.push(Step::Chunk(Bytes::copy_from_slice(&[*byte])));
            }
            steps.push(Step::PendingOnce);
            steps.push(Step::Eof);
            Self::new(steps)
        }

        fn before_eof(raw: &[u8]) -> (Self, Probe, Gate) {
            let gate = Gate::default();
            let (source, probe) = Self::new([
                Step::Chunk(Bytes::copy_from_slice(raw)),
                Step::Wait(gate.clone()),
                Step::Eof,
            ]);
            (source, probe, gate)
        }

        fn terminal_error(raw: &[u8], kind: io::ErrorKind) -> (Self, Probe) {
            Self::new([
                Step::Chunk(Bytes::copy_from_slice(raw)),
                Step::PendingOnce,
                Step::Error(kind),
                // This is a trap: an error must not be retried as a successful EOF.
                Step::Eof,
            ])
        }
    }

    impl Stream for ScriptedStream {
        type Item = Result<Bytes, io::Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.get_mut();
            this.probe.0.borrow_mut().polls += 1;
            if this.ended {
                this.probe.0.borrow_mut().polls_after_end += 1;
                return Poll::Ready(None);
            }
            loop {
                match this.steps.pop_front() {
                    Some(Step::Chunk(bytes)) => {
                        let mut trace = this.probe.0.borrow_mut();
                        trace.chunks += 1;
                        trace.bytes += bytes.len();
                        return Poll::Ready(Some(Ok(bytes)));
                    }
                    Some(Step::PendingOnce) => {
                        this.probe.0.borrow_mut().pending += 1;
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    Some(Step::Wait(gate)) if !gate.0.open.get() => {
                        *gate.0.waker.borrow_mut() = Some(cx.waker().clone());
                        this.steps.push_front(Step::Wait(gate));
                        this.probe.0.borrow_mut().pending += 1;
                        return Poll::Pending;
                    }
                    Some(Step::Wait(_)) => continue,
                    Some(Step::Error(kind)) => {
                        this.ended = true;
                        this.probe.0.borrow_mut().errors += 1;
                        return Poll::Ready(Some(Err(io::Error::new(
                            kind,
                            "synthetic native transport interruption",
                        ))));
                    }
                    Some(Step::Eof) => {
                        this.ended = true;
                        this.probe.0.borrow_mut().eofs += 1;
                        return Poll::Ready(None);
                    }
                    None => panic!("C5_NATIVE_DRIVER_FIXTURE_REQUIRES_EXPLICIT_EOF"),
                }
            }
        }
    }

    impl Drop for ScriptedStream {
        fn drop(&mut self) {
            self.probe.0.borrow_mut().drops += 1;
        }
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        let mut cx = Context::from_waker(noop_waker_ref());
        future.poll(&mut cx)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_arbitrary_utf8_splits_pending_real_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (source, probe) = ScriptedStream::split(&raw, at);
            // An async combinator also prevents accidentally requiring Unpin.
            let source = source.then(|item| async move { item });
            let proof = required(
                fixture
                    .driver()
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_JSON_SPLIT_REQUIRES_PROOF",
            )?;
            probe.assert_eof();
            assert_eq!(probe.snapshot().pending, 3);
            assert_eq!(probe.snapshot().bytes, raw.len());
            required(
                proof.validate(&client, &fixture.binding, &fixture.attempt, chat()),
                "C5_NATIVE_DRIVER_JSON_SPLIT_PROOF_INVALID",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_arbitrary_utf8_crlf_splits_pending_both_eofs() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (source, original) = ScriptedStream::split(&raw, at);
            let (client, delivered) = ScriptedStream::split(&raw, raw.len() - at);
            let proof = required(
                fixture
                    .driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_SPLIT_REQUIRES_PROOF",
            )?;
            original.assert_eof();
            delivered.assert_eof();
            assert_eq!(original.snapshot().pending, 3);
            assert_eq!(delivered.snapshot().pending, 3);
            required(
                proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
                "C5_NATIVE_DRIVER_SSE_SPLIT_PROOF_INVALID",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_single_byte_chunks_empty_items_are_not_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let (source, probe) = ScriptedStream::one_byte(&raw);
        let proof = required(
            fixture
                .driver()
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_JSON_BYTEWISE_REQUIRES_PROOF",
        )?;
        probe.assert_eof();
        assert_eq!(probe.snapshot().chunks, 2 * raw.len());
        assert_eq!(probe.snapshot().bytes, raw.len());
        required(
            proof.validate(&client, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_JSON_BYTEWISE_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_single_byte_chunks_empty_items_are_not_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let (source, original) = ScriptedStream::one_byte(&raw);
        let (client, delivered) = ScriptedStream::one_byte(&raw);
        let proof = required(
            fixture
                .driver()
                .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_SSE_BYTEWISE_REQUIRES_PROOF",
        )?;
        original.assert_eof();
        delivered.assert_eof();
        assert_eq!(original.snapshot().chunks, 2 * raw.len());
        assert_eq!(delivered.snapshot().chunks, 2 * raw.len());
        required(
            proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_SSE_BYTEWISE_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_terminal_pending_is_not_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let (source, probe, gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(fixture.driver().consume_json(
            source,
            &fixture.attempt,
            &client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_JSON_MUST_PENDING_BEFORE_EOF"
        );
        assert_eq!(probe.snapshot().bytes, raw.len());
        assert_eq!(probe.snapshot().eofs, 0);
        gate.release();
        let proof = required(future.await, "C5_NATIVE_DRIVER_JSON_EOF_REQUIRES_PROOF")?;
        probe.assert_eof();
        required(
            proof.validate(&client, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_JSON_EOF_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_original_terminal_pending_is_not_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let (source, original, gate) = ScriptedStream::before_eof(&raw);
        let (client, delivered) = ScriptedStream::entity(&raw);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_ORIGINAL_MUST_PENDING_BEFORE_EOF"
        );
        assert_eq!(original.snapshot().bytes, raw.len());
        assert_eq!(original.snapshot().eofs, 0);
        assert_eq!(delivered.snapshot().polls, 0);
        gate.release();
        let proof = required(future.await, "C5_NATIVE_DRIVER_SSE_EOF_REQUIRES_PROOF")?;
        original.assert_eof();
        delivered.assert_eof();
        required(
            proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_SSE_EOF_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_client_terminal_pending_is_not_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let (source, original) = ScriptedStream::entity(&raw);
        let (client, delivered, gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_CLIENT_MUST_PENDING_BEFORE_EOF"
        );
        original.assert_consumed_eof();
        assert_eq!(delivered.snapshot().bytes, raw.len());
        assert_eq!(delivered.snapshot().eofs, 0);
        gate.release();
        let proof = required(
            future.await,
            "C5_NATIVE_DRIVER_SSE_CLIENT_EOF_REQUIRES_PROOF",
        )?;
        delivered.assert_eof();
        required(
            proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_SSE_CLIENT_EOF_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_pending_backpressure_does_not_poll_ahead() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let (source, probe) = ScriptedStream::new([
            Step::PendingOnce,
            Step::Chunk(Bytes::copy_from_slice(&raw)),
            Step::Eof,
        ]);
        let mut future = Box::pin(fixture.driver().consume_json(
            source,
            &fixture.attempt,
            &client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_JSON_PENDING_MUST_SUSPEND"
        );
        assert_eq!(probe.snapshot().polls, 1);
        assert_eq!(probe.snapshot().bytes, 0);
        let paused = probe.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(
            probe.snapshot(),
            paused,
            "C5_NATIVE_DRIVER_BACKGROUND_DRAIN"
        );
        let proof = required(future.await, "C5_NATIVE_DRIVER_JSON_RESUME_REQUIRES_PROOF")?;
        probe.assert_eof();
        required(
            proof.validate(&client, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_JSON_RESUME_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_pending_backpressure_does_not_start_client() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let gate = Gate::default();
        let (source, original) = ScriptedStream::new([
            Step::Wait(gate.clone()),
            Step::Chunk(Bytes::copy_from_slice(&raw)),
            Step::Eof,
        ]);
        let (client, delivered) = ScriptedStream::entity(&raw);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_PENDING_MUST_SUSPEND"
        );
        assert_eq!(original.snapshot().polls, 1);
        assert_eq!(delivered.snapshot().polls, 0);
        let paused = original.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(
            original.snapshot(),
            paused,
            "C5_NATIVE_DRIVER_BACKGROUND_DRAIN"
        );
        assert_eq!(delivered.snapshot().polls, 0);
        gate.release();
        let proof = required(future.await, "C5_NATIVE_DRIVER_SSE_RESUME_REQUIRES_PROOF")?;
        original.assert_eof();
        delivered.assert_eof();
        required(
            proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_SSE_RESUME_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_client_pending_backpressure_no_background_drain() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let (source, original) = ScriptedStream::entity(&raw);
        let (client, delivered) = ScriptedStream::new([
            Step::PendingOnce,
            Step::Chunk(Bytes::copy_from_slice(&raw)),
            Step::Eof,
        ]);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_CLIENT_PENDING_MUST_SUSPEND"
        );
        original.assert_consumed_eof();
        assert_eq!(delivered.snapshot().polls, 1);
        let paused_original = original.snapshot();
        let paused_client = delivered.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(original.snapshot(), paused_original);
        assert_eq!(delivered.snapshot(), paused_client);
        let proof = required(
            future.await,
            "C5_NATIVE_DRIVER_SSE_CLIENT_RESUME_REQUIRES_PROOF",
        )?;
        delivered.assert_eof();
        required(
            proof.validate(&raw, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_SSE_CLIENT_RESUME_PROOF_INVALID",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_drop_pending_future_refunds_charged_bytes() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        let (source, probe, _gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(fixture.driver().consume_json(
            source,
            &fixture.attempt,
            &client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_JSON_CANCEL_REQUIRES_PENDING"
        );
        assert_eq!(probe.snapshot().bytes, raw.len());
        assert!(
            fixture.budget.used() >= baseline + raw.len(),
            "C5_NATIVE_DRIVER_JSON_RETAINED_BYTES_MUST_BE_CHARGED"
        );
        drop(future);
        assert_eq!(probe.snapshot().drops, 1);
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(fixture.budget.used(), baseline);
        let stopped = probe.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(
            probe.snapshot(),
            stopped,
            "C5_NATIVE_DRIVER_POLL_AFTER_CANCEL"
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_drop_original_pending_cancels_both_sources() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        let (source, original, _gate) = ScriptedStream::before_eof(&raw);
        let (client, delivered) = ScriptedStream::entity(&raw);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_CANCEL_ORIGINAL_REQUIRES_PENDING"
        );
        assert_eq!(original.snapshot().bytes, raw.len());
        assert_eq!(delivered.snapshot().polls, 0);
        assert!(
            fixture.budget.used() >= baseline + raw.len(),
            "C5_NATIVE_DRIVER_SSE_RETAINED_BYTES_MUST_BE_CHARGED"
        );
        drop(future);
        assert_eq!(original.snapshot().drops, 1);
        assert_eq!(delivered.snapshot().drops, 1);
        assert_eq!(original.snapshot().eofs, 0);
        assert_eq!(delivered.snapshot().eofs, 0);
        assert_eq!(fixture.budget.used(), baseline);
        let stopped_original = original.snapshot();
        let stopped_client = delivered.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(original.snapshot(), stopped_original);
        assert_eq!(delivered.snapshot(), stopped_client);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_drop_client_pending_refunds_charged_bytes() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        let (source, original) = ScriptedStream::entity(&raw);
        let (client, delivered, _gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(fixture.driver().consume_sse(
            source,
            &fixture.attempt,
            client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "C5_NATIVE_DRIVER_SSE_CANCEL_CLIENT_REQUIRES_PENDING"
        );
        original.assert_consumed_eof();
        assert_eq!(delivered.snapshot().bytes, raw.len());
        assert!(
            fixture.budget.used() >= baseline + raw.len(),
            "C5_NATIVE_DRIVER_SSE_CLIENT_BYTES_MUST_BE_CHARGED"
        );
        drop(future);
        assert_eq!(delivered.snapshot().drops, 1);
        assert_eq!(delivered.snapshot().eofs, 0);
        assert_eq!(fixture.budget.used(), baseline);
        let stopped = delivered.snapshot();
        tokio::task::yield_now().await;
        assert_eq!(delivered.snapshot(), stopped);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_unpolled_future_drop_never_polls_or_charges() -> TestResult {
        let fixture = Fixture::new()?;
        let client_value = chat_json();
        let raw_json = serde_json::to_vec(&client_value)?;
        let raw_sse = chat_sse();
        let baseline = fixture.budget.used();
        let (json_source, json_probe) = ScriptedStream::entity(&raw_json);
        let future = fixture.driver().consume_json(
            json_source,
            &fixture.attempt,
            &client_value,
            &fixture.attempt,
        );
        assert_eq!(json_probe.snapshot().polls, 0);
        drop(future);
        assert_eq!(json_probe.snapshot().polls, 0);
        assert_eq!(json_probe.snapshot().drops, 1);
        let (source, original) = ScriptedStream::entity(&raw_sse);
        let (client, delivered) = ScriptedStream::entity(&raw_sse);
        let future =
            fixture
                .driver()
                .consume_sse(source, &fixture.attempt, client, &fixture.attempt);
        drop(future);
        assert_eq!(original.snapshot().polls, 0);
        assert_eq!(delivered.snapshot().polls, 0);
        assert_eq!(original.snapshot().drops, 1);
        assert_eq!(delivered.snapshot().drops, 1);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_error_after_terminal_never_becomes_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::Interrupted,
            io::ErrorKind::UnexpectedEof,
        ] {
            let (source, probe) = ScriptedStream::terminal_error(&raw, kind);
            rejected(
                fixture
                    .driver()
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_JSON_ERROR_MUST_REJECT",
            );
            probe.assert_error();
            assert_eq!(probe.snapshot().bytes, raw.len());
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_original_error_after_done_never_polls_client() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        for kind in [io::ErrorKind::ConnectionReset, io::ErrorKind::Interrupted] {
            let (source, original) = ScriptedStream::terminal_error(&raw, kind);
            let (client, delivered) = ScriptedStream::entity(&raw);
            rejected(
                fixture
                    .driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_ORIGINAL_ERROR_MUST_REJECT",
            );
            original.assert_error();
            assert_eq!(delivered.snapshot().polls, 0);
            assert_eq!(delivered.snapshot().drops, 1);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_client_error_after_done_never_becomes_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        for kind in [io::ErrorKind::ConnectionReset, io::ErrorKind::Interrupted] {
            let (source, original) = ScriptedStream::entity(&raw);
            let (client, delivered) = ScriptedStream::terminal_error(&raw, kind);
            rejected(
                fixture
                    .driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_CLIENT_ERROR_MUST_REJECT",
            );
            original.assert_eof();
            delivered.assert_error();
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_real_eof_requires_strict_parse_and_completion() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = serde_json::to_vec(&client)?;
        let text = std::str::from_utf8(&raw)?;
        let duplicate = text.replacen(
            "\"object\":\"chat.completion\"",
            "\"object\":\"chat.completion\",\"object\":\"chat.completion\"",
            1,
        );
        assert_ne!(
            duplicate.len(),
            raw.len(),
            "C5_NATIVE_DRIVER_DUPLICATE_FIXTURE"
        );
        let mut incomplete = client.clone();
        incomplete["choices"][0]["finish_reason"] = Value::Null;
        let invalid = [
            Vec::new(),
            raw[..raw.len() - 1].to_vec(),
            format!("{text}{text}").into_bytes(),
            duplicate.into_bytes(),
            vec![0xf0, 0x9f, 0xa6],
            serde_json::to_vec(&incomplete)?,
        ];
        let baseline = fixture.budget.used();
        for bytes in invalid {
            let (source, probe) = ScriptedStream::split(&bytes, bytes.len() / 2);
            rejected(
                fixture
                    .driver()
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_JSON_INVALID_ENTITY_MUST_REJECT",
            );
            probe.assert_eof();
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    fn invalid_sse_entities(valid: &[u8]) -> Vec<Vec<u8>> {
        let mut after_done = valid.to_vec();
        after_done.extend_from_slice(b"data: {}\r\n\r\n");
        let missing_done = valid
            .strip_suffix(b"data: [DONE]\r\n\r\n")
            .map_or_else(Vec::new, |bytes| bytes.to_vec());
        vec![
            Vec::new(),
            valid[..valid.len() - 2].to_vec(),
            missing_done,
            after_done,
            b"data: {\r\n\r\ndata: [DONE]\r\n\r\n".to_vec(),
            b"data: {\"choices\":[],\"choices\":[]}\r\n\r\ndata: [DONE]\r\n\r\n".to_vec(),
            b"data: [DONE]\r\n\r\n".to_vec(),
            b"data: [DONE]\rX\n\n".to_vec(),
            vec![0xf0, 0x9f, 0xa6],
        ]
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_original_partial_frame_or_invalid_eof_rejected() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        for bytes in invalid_sse_entities(&raw) {
            let (source, original) = ScriptedStream::split(&bytes, bytes.len() / 2);
            let (client, delivered) = ScriptedStream::entity(&raw);
            rejected(
                fixture
                    .driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_INVALID_ORIGINAL_MUST_REJECT",
            );
            original.assert_eof();
            assert_eq!(delivered.snapshot().polls, 0);
            assert_eq!(delivered.snapshot().drops, 1);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_client_partial_frame_or_invalid_eof_rejected() -> TestResult {
        let fixture = Fixture::new()?;
        let raw = chat_sse();
        let baseline = fixture.budget.used();
        for bytes in invalid_sse_entities(&raw) {
            let (source, original) = ScriptedStream::entity(&raw);
            let (client, delivered) = ScriptedStream::split(&bytes, bytes.len() / 2);
            rejected(
                fixture
                    .driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_INVALID_CLIENT_MUST_REJECT",
            );
            original.assert_eof();
            delivered.assert_eof();
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_client_exact_digest_cannot_be_substituted() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        let raw = serde_json::to_vec(&valid)?;
        let mut changed = valid.clone();
        changed["synthetic_extension"]["preserve"] = Value::Bool(false);
        let (source, probe) = ScriptedStream::split(&raw, raw.len() / 2);
        rejected(
            fixture
                .driver()
                .consume_json(source, &fixture.attempt, &changed, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_JSON_CLIENT_DIGEST_MUST_MATCH",
        );
        probe.assert_eof();
        let proof = fixture.json_proof(&valid).await?;
        rejected(
            proof.validate(&changed, &fixture.binding, &fixture.attempt, chat()),
            "C5_NATIVE_DRIVER_JSON_PROOF_DIGEST_MUST_MATCH",
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_sse_exact_framing_digest_cannot_be_substituted() -> TestResult {
        let fixture = Fixture::new()?;
        let original_bytes = chat_sse();
        // The events are equivalent, but the exact native SSE entity is different.
        let changed = std::str::from_utf8(&original_bytes)?.replace("\r\n", "\n");
        let (source, original) = ScriptedStream::entity(&original_bytes);
        let (client, delivered) = ScriptedStream::entity(changed.as_bytes());
        rejected(
            fixture
                .driver()
                .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_SSE_CLIENT_FRAMING_MUST_MATCH",
        );
        original.assert_eof();
        delivered.assert_eof();
        let proof = fixture.sse_proof(&original_bytes).await?;
        rejected(
            proof.validate(
                changed.as_bytes(),
                &fixture.binding,
                &fixture.attempt,
                chat(),
            ),
            "C5_NATIVE_DRIVER_SSE_PROOF_FRAMING_MUST_MATCH",
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_json_and_sse_attempt_mismatch_precedes_any_poll() -> TestResult {
        let fixture = Fixture::new()?;
        let client_value = chat_json();
        let raw_json = serde_json::to_vec(&client_value)?;
        let raw_sse = chat_sse();
        let wrong = AttemptProvenance::new(&fixture.budget)?;
        for (original_attempt, client_attempt) in
            [(&wrong, &fixture.attempt), (&fixture.attempt, &wrong)]
        {
            let (source, probe) = ScriptedStream::entity(&raw_json);
            rejected(
                fixture
                    .driver()
                    .consume_json(source, original_attempt, &client_value, client_attempt)
                    .await,
                "C5_NATIVE_DRIVER_JSON_ATTEMPT_MUST_MATCH",
            );
            assert_eq!(probe.snapshot().polls, 0);
            assert_eq!(probe.snapshot().drops, 1);
            let (source, original) = ScriptedStream::entity(&raw_sse);
            let (client, delivered) = ScriptedStream::entity(&raw_sse);
            rejected(
                fixture
                    .driver()
                    .consume_sse(source, original_attempt, client, client_attempt)
                    .await,
                "C5_NATIVE_DRIVER_SSE_ATTEMPT_MUST_MATCH",
            );
            assert_eq!(original.snapshot().polls, 0);
            assert_eq!(delivered.snapshot().polls, 0);
            assert_eq!(original.snapshot().drops, 1);
            assert_eq!(delivered.snapshot().drops, 1);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_proofs_are_binding_attempt_and_client_specific() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let raw = chat_sse();
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let wrong_binding = fixture.other_binding()?;
        for (binding, attempt, client_protocol) in [
            (&wrong_binding, &fixture.attempt, chat()),
            (&fixture.binding, &wrong_attempt, chat()),
            (
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Anthropic),
            ),
        ] {
            let json_proof = fixture.json_proof(&client).await?;
            rejected(
                json_proof.validate(&client, binding, attempt, client_protocol),
                "C5_NATIVE_DRIVER_JSON_PROOF_AUTHORITY_MUST_MATCH",
            );
            let sse_proof = fixture.sse_proof(&raw).await?;
            rejected(
                sse_proof.validate(&raw, binding, attempt, client_protocol),
                "C5_NATIVE_DRIVER_SSE_PROOF_AUTHORITY_MUST_MATCH",
            );
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_non_chat_pairs_reject_without_polling() -> TestResult {
        let fixture = Fixture::new()?;
        let client_value = chat_json();
        let raw_json = serde_json::to_vec(&client_value)?;
        let raw_sse = chat_sse();
        for protocols in [
            pair(Protocol::Chat, Protocol::Anthropic),
            pair(Protocol::Anthropic, Protocol::Chat),
            pair(Protocol::Responses, Protocol::Chat),
            pair(Protocol::Gemini, Protocol::Gemini),
        ] {
            let driver = || {
                NativeCompletionDriver::new(
                    protocols,
                    fixture.attempt.clone(),
                    fixture.binding.clone(),
                    fixture.budget.clone(),
                )
            };
            let (source, probe) = ScriptedStream::entity(&raw_json);
            rejected(
                driver()
                    .consume_json(source, &fixture.attempt, &client_value, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_NON_CHAT_JSON_MUST_REJECT",
            );
            assert_eq!(probe.snapshot().polls, 0);
            let (source, original) = ScriptedStream::entity(&raw_sse);
            let (client, delivered) = ScriptedStream::entity(&raw_sse);
            rejected(
                driver()
                    .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "C5_NATIVE_DRIVER_NON_CHAT_SSE_MUST_REJECT",
            );
            assert_eq!(original.snapshot().polls, 0);
            assert_eq!(delivered.snapshot().polls, 0);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_budget_denial_stops_polling_and_refunds() -> TestResult {
        let fixture = Fixture::new()?;
        let client_value = chat_json();
        let raw_json = serde_json::to_vec(&client_value)?;
        let raw_sse = chat_sse();
        // Provenance has its own live charge. The driver gets a distinct exhausted
        // payload ceiling so denial cannot be hidden by failing fixture admission.
        let budget = PayloadBudget::with_limit(0);
        let driver = || {
            NativeCompletionDriver::new(
                pair(Protocol::Chat, Protocol::Chat),
                fixture.attempt.clone(),
                fixture.binding.clone(),
                budget.clone(),
            )
        };
        let (source, probe) = ScriptedStream::entity(&raw_json);
        rejected(
            driver()
                .consume_json(source, &fixture.attempt, &client_value, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_JSON_BUDGET_MUST_REJECT",
        );
        assert!(
            probe.snapshot().polls <= 1,
            "C5_NATIVE_DRIVER_POLL_AFTER_BUDGET_DENIAL"
        );
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(probe.snapshot().drops, 1);
        assert_eq!(budget.used(), 0);
        let (source, original) = ScriptedStream::entity(&raw_sse);
        let (client, delivered) = ScriptedStream::entity(&raw_sse);
        rejected(
            driver()
                .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_SSE_BUDGET_MUST_REJECT",
        );
        assert!(original.snapshot().polls <= 1);
        assert_eq!(original.snapshot().eofs, 0);
        assert_eq!(original.snapshot().drops, 1);
        assert_eq!(delivered.snapshot().polls, 0);
        assert_eq!(delivered.snapshot().drops, 1);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn c5_native_driver_entity_limit_rejects_before_polling_tail() -> TestResult {
        let fixture = Fixture::new()?;
        let client_value = chat_json();
        // One oversized transport item; the future must not ask for its EOF/tail.
        let oversized = Bytes::from(vec![b' '; NATIVE_DRIVER_ENTITY_LIMIT + 1]);
        let baseline = fixture.budget.used();
        let (source, probe) = ScriptedStream::new([Step::Chunk(oversized.clone()), Step::Eof]);
        rejected(
            fixture
                .driver()
                .consume_json(source, &fixture.attempt, &client_value, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_JSON_ENTITY_LIMIT_MUST_REJECT",
        );
        assert_eq!(
            probe.snapshot().chunks,
            1,
            "C5_NATIVE_DRIVER_LIMIT_ITEM_NOT_CONSUMED"
        );
        assert_eq!(probe.snapshot().polls, 1);
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(probe.snapshot().drops, 1);
        assert_eq!(fixture.budget.used(), baseline);
        let (source, original) = ScriptedStream::new([Step::Chunk(oversized), Step::Eof]);
        let (client, delivered) = ScriptedStream::entity(&chat_sse());
        rejected(
            fixture
                .driver()
                .consume_sse(source, &fixture.attempt, client, &fixture.attempt)
                .await,
            "C5_NATIVE_DRIVER_SSE_ENTITY_LIMIT_MUST_REJECT",
        );
        assert_eq!(original.snapshot().chunks, 1);
        assert_eq!(original.snapshot().polls, 1);
        assert_eq!(original.snapshot().eofs, 0);
        assert_eq!(delivered.snapshot().polls, 0);
        assert_eq!(delivered.snapshot().drops, 1);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[test]
    fn c5_native_driver_contract_does_not_enable_native_lanes() {
        for (protocol, operation) in [
            (Protocol::Chat, Operation::ChatCompletions),
            (Protocol::Anthropic, Operation::Messages),
            (Protocol::Gemini, Operation::GenerateContent),
        ] {
            for framing in [Framing::Json, Framing::Sse] {
                assert!(
                    registration(pair(protocol, protocol), operation, framing)
                        == Registration::Disabled,
                    "C5_NATIVE_DRIVER_MUST_NOT_ENABLE_LANES"
                );
            }
        }
    }
}
