//! Private HTTP-owned, bounded native completion transport driver.
//!
//! This collects completion evidence, not a native adapter or a route-enablement
//! grant. Each entity must reach actual asynchronous `None` before the real
//! synchronous Read/framing verifier can inspect it and issue an opaque proof.
//! A protocol terminal, an empty Bytes item, and Pending are never entity EOF.
//! Original and pre-restoration client evidence must use the same approved native
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
        let client = self.pair.client().protocol();
        let upstream = self.pair.upstream().protocol();
        if client != upstream
            || !matches!(
                client,
                Protocol::Chat | Protocol::Anthropic | Protocol::Gemini
            )
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
        .map_err(RestoreError::scan_error)?;
    futures::pin_mut!(source);
    while let Some(chunk) = source.next().await {
        // No retries or further polls on any error, including Interrupted.
        let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
        // Append checks the entity ceiling and charges before allocation/copy.
        // The transport item is dropped before the next asynchronous poll.
        bytes.append(&chunk).map_err(RestoreError::scan_error)?;
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
    fn c5_native_driver_contract_does_not_expand_integrated_native_lanes() {
        for (protocol, operation) in [
            (Protocol::Chat, Operation::ChatCompletions),
            (Protocol::Anthropic, Operation::Messages),
            (Protocol::Gemini, Operation::GenerateContent),
        ] {
            for framing in [Framing::Json, Framing::Sse] {
                let expected = match protocol {
                    Protocol::Chat => Registration::NativeChat,
                    Protocol::Anthropic => Registration::NativeAnthropic,
                    Protocol::Gemini => Registration::NativeGemini,
                    _ => Registration::Disabled,
                };
                assert!(
                    registration(pair(protocol, protocol), operation, framing) == expected,
                    "C5_NATIVE_DRIVER_MUST_NOT_EXPAND_LANES"
                );
            }
        }
    }

    // Native Anthropic SSE fixtures keep protocol terminals separate from transport EOF.
    fn native_shared_an_events() -> Vec<Value> {
        vec![
            json!({"type":"message_start","message":{"id":"msg_synthetic_sse","type":"message","role":"assistant","model":"synthetic-model","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"synthetic public thought"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic-late-signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"synthetic 雪 🦀 text"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call_synthetic_sse","name":"synthetic-tool","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"value\":\"synthetic "}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"leaf\",\"types\":[true,null,7]}"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ]
    }

    fn native_shared_an_encode_events(events: &[Value]) -> TestResult<Vec<u8>> {
        let mut wire = Vec::new();
        for event in events {
            let name = event
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| io::Error::other("NATIVE_SHARED_AN_SSE_FIXTURE_EVENT_TYPE"))?;
            wire.extend_from_slice(
                format!(
                    "event: {name}\r\ndata: {}\r\n\r\n",
                    serde_json::to_string(event)?
                )
                .as_bytes(),
            );
        }
        Ok(wire)
    }

    async fn native_shared_an_sse_proof(
        fixture: &Fixture,
        raw: &[u8],
    ) -> TestResult<VerifiedNativeSse> {
        let (original, original_probe) = ScriptedStream::split(raw, raw.len() / 2);
        let (client, client_probe) = ScriptedStream::split(raw, raw.len() / 3);
        let proof = required(
            native_shared_an_driver(fixture)
                .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_SSE_DONOR_PROOF_REQUIRED",
        )?;
        original_probe.assert_eof();
        client_probe.assert_eof();
        Ok(proof)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_real_eof_split_and_final_wire() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let raw = native_shared_an_encode_events(&native_shared_an_events())?;
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (original, original_probe) = ScriptedStream::split(&raw, at);
            let (client, client_probe) = ScriptedStream::split(&raw, raw.len() - at);
            let proof = required(
                native_shared_an_driver(&fixture)
                    .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_AN_SSE_REAL_EOF_PROOF_REQUIRED",
            )?;
            original_probe.assert_eof();
            client_probe.assert_eof();
            required(
                proof.validate_restored(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                    io::Cursor::new(&raw),
                    &fixture.budget,
                ),
                "NATIVE_SHARED_AN_SSE_FINAL_WIRE_REQUIRED",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        let (original, original_probe) = ScriptedStream::one_byte(&raw);
        let (client, client_probe) = ScriptedStream::one_byte(&raw);
        let proof = required(
            native_shared_an_driver(&fixture)
                .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_SSE_BYTEWISE_PROOF_REQUIRED",
        )?;
        original_probe.assert_eof();
        client_probe.assert_eof();
        drop(proof);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_pending_and_cancel_do_not_issue_proof() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let raw = native_shared_an_encode_events(&native_shared_an_events())?;
        let baseline = fixture.budget.used();
        for wait_client in [false, true] {
            for cancel in [false, true] {
                let (original, original_probe, original_gate) = ScriptedStream::before_eof(&raw);
                let (client, client_probe, client_gate) = ScriptedStream::before_eof(&raw);
                if wait_client {
                    original_gate.release();
                }
                let mut future = Box::pin(native_shared_an_driver(&fixture).consume_sse(
                    original,
                    &fixture.attempt,
                    client,
                    &fixture.attempt,
                ));
                assert!(
                    poll_once(future.as_mut()).is_pending(),
                    "NATIVE_SHARED_AN_SSE_PENDING_NOT_EOF"
                );
                assert_eq!(original_probe.snapshot().eofs, usize::from(wait_client));
                assert_eq!(client_probe.snapshot().eofs, 0);
                if !wait_client {
                    assert_eq!(client_probe.snapshot().polls, 0);
                }
                let original_before = original_probe.snapshot();
                let client_before = client_probe.snapshot();
                tokio::task::yield_now().await;
                assert_eq!(original_probe.snapshot(), original_before);
                assert_eq!(client_probe.snapshot(), client_before);
                if cancel {
                    drop(future);
                    assert_eq!(original_probe.snapshot().drops, 1);
                    assert_eq!(client_probe.snapshot().drops, 1);
                } else {
                    original_gate.release();
                    client_gate.release();
                    drop(required(
                        future.await,
                        "NATIVE_SHARED_AN_SSE_RELEASED_EOF_REQUIRED",
                    )?);
                    original_probe.assert_eof();
                    client_probe.assert_eof();
                }
                assert_eq!(fixture.budget.used(), baseline);
                let counts =
                    crate::proxy::security::tokenization::publication_test_support::counts(
                        &fixture.store,
                    )?;
                assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
            }
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_transport_error_is_not_eof() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let raw = native_shared_an_encode_events(&native_shared_an_events())?;
        let baseline = fixture.budget.used();
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::ConnectionReset] {
            for fail_client in [false, true] {
                let (original, original_probe) = if fail_client {
                    ScriptedStream::entity(&raw)
                } else {
                    ScriptedStream::terminal_error(&raw, kind)
                };
                let (client, client_probe) = if fail_client {
                    ScriptedStream::terminal_error(&raw, kind)
                } else {
                    ScriptedStream::entity(&raw)
                };
                rejected(
                    native_shared_an_driver(&fixture)
                        .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                        .await,
                    "NATIVE_SHARED_AN_SSE_ERROR_ACCEPTED",
                );
                if fail_client {
                    original_probe.assert_eof();
                    client_probe.assert_error();
                } else {
                    original_probe.assert_error();
                    assert_eq!(client_probe.snapshot().polls, 0);
                    assert_eq!(client_probe.snapshot().drops, 1);
                }
                assert_eq!(fixture.budget.used(), baseline);
            }
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_strict_original_client_and_final_wire() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let events = native_shared_an_events();
        let raw = native_shared_an_encode_events(&events)?;
        drop(native_shared_an_sse_proof(&fixture, &raw).await?);
        let baseline = fixture.budget.used();
        let mut invalid = Vec::new();
        for mode in 0..10 {
            let mut bad = events.clone();
            match mode {
                0 => {
                    bad.remove(13);
                }
                1 => {
                    bad.insert(1, events[0].clone());
                }
                2 => bad[6]["index"] = json!(0),
                3 => bad[12]["delta"]["stop_reason"] = json!("max_tokens"),
                4 => bad[10]["delta"]["partial_json"] = json!("not-json"),
                5 => bad[8]["content_block"]["id"] = json!("invalid id"),
                6 => bad.push(json!({"type":"error","error":{"type":"synthetic_error"}})),
                7 => bad.push(events[6].clone()),
                8 => {
                    bad.remove(11);
                }
                _ => bad[6]["delta"]["text"] = json!(7),
            }
            invalid.push(native_shared_an_encode_events(&bad)?);
        }
        invalid.push(raw[..raw.len() - 1].to_vec());
        let mut invalid_utf8 = raw.clone();
        invalid_utf8.extend_from_slice(&[0xff, b'\n', b'\n']);
        invalid.push(invalid_utf8);
        invalid.push(
            String::from_utf8(raw.clone())?
                .replacen("event: content_block_delta", "event: message_delta", 1)
                .into_bytes(),
        );
        let mut done = raw.clone();
        done.extend_from_slice(b"data: [DONE]\r\n\r\n");
        invalid.push(done);
        for bad in invalid {
            for fail_client in [false, true] {
                let (original, original_probe) =
                    ScriptedStream::entity(if fail_client { &raw } else { &bad });
                let (client, client_probe) =
                    ScriptedStream::entity(if fail_client { &bad } else { &raw });
                rejected(
                    native_shared_an_driver(&fixture)
                        .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                        .await,
                    "NATIVE_SHARED_AN_SSE_INVALID_INPUT_ACCEPTED",
                );
                original_probe.assert_eof();
                if fail_client {
                    client_probe.assert_eof();
                } else {
                    assert_eq!(client_probe.snapshot().polls, 0);
                }
            }
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            rejected(
                proof.validate_restored(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                    io::Cursor::new(&bad),
                    &fixture.budget,
                ),
                "NATIVE_SHARED_AN_SSE_INVALID_FINAL_WIRE_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_identity_and_budget_controls() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let raw = native_shared_an_encode_events(&native_shared_an_events())?;
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let baseline = fixture.budget.used();
        for wrong_original in [false, true] {
            let (original, original_probe) = ScriptedStream::entity(&raw);
            let (client, client_probe) = ScriptedStream::entity(&raw);
            rejected(
                native_shared_an_driver(&fixture)
                    .consume_sse(
                        original,
                        if wrong_original {
                            &wrong_attempt
                        } else {
                            &fixture.attempt
                        },
                        client,
                        if wrong_original {
                            &fixture.attempt
                        } else {
                            &wrong_attempt
                        },
                    )
                    .await,
                "NATIVE_SHARED_AN_SSE_FOREIGN_ATTEMPT_ACCEPTED",
            );
            assert_eq!(original_probe.snapshot().polls, 0);
            assert_eq!(client_probe.snapshot().polls, 0);
        }
        for protocol in [Protocol::Chat, Protocol::Responses, Protocol::Gemini] {
            for reverse in [false, true] {
                let pair = if reverse {
                    pair(protocol, Protocol::Anthropic)
                } else {
                    pair(Protocol::Anthropic, protocol)
                };
                let driver = NativeCompletionDriver::new(
                    pair,
                    fixture.attempt.clone(),
                    fixture.binding.clone(),
                    fixture.budget.clone(),
                );
                let (original, original_probe) = ScriptedStream::entity(&raw);
                let (client, client_probe) = ScriptedStream::entity(&raw);
                assert!(matches!(
                    driver
                        .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                        .await,
                    Err(RestoreError::RestoreUnsupported)
                ));
                assert_eq!(original_probe.snapshot().polls, 0);
                assert_eq!(client_probe.snapshot().polls, 0);
            }
        }
        let foreign = native_shared_an_fixture()?;
        let mut other_wire = b": synthetic framing difference\r\n\r\n".to_vec();
        other_wire.extend_from_slice(&raw);
        for mode in 0..4 {
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            rejected(
                proof.validate(
                    if mode == 0 { &other_wire } else { &raw },
                    if mode == 1 {
                        &foreign.binding
                    } else {
                        &fixture.binding
                    },
                    if mode == 2 {
                        &wrong_attempt
                    } else {
                        &fixture.attempt
                    },
                    ClientProtocol::new(if mode == 3 {
                        Protocol::Chat
                    } else {
                        Protocol::Anthropic
                    }),
                ),
                "NATIVE_SHARED_AN_SSE_FOREIGN_PROOF_IDENTITY_ACCEPTED",
            );
        }
        let (original, original_probe) = ScriptedStream::entity(&raw);
        let (client, client_probe) = ScriptedStream::entity(&other_wire);
        rejected(
            native_shared_an_driver(&fixture)
                .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_SSE_FRAMING_SUBSTITUTION_ACCEPTED",
        );
        original_probe.assert_eof();
        client_probe.assert_eof();
        assert_eq!(fixture.budget.used(), baseline);
        let saturation = fixture.budget.reserve(fixture.budget.limit() - baseline)?;
        let (original, original_probe) = ScriptedStream::entity(&raw);
        let (client, client_probe) = ScriptedStream::entity(&raw);
        rejected(
            native_shared_an_driver(&fixture)
                .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_SSE_UNCHARGED_COLLECTION_ACCEPTED",
        );
        assert_eq!(original_probe.snapshot().polls, 1);
        assert_eq!(original_probe.snapshot().eofs, 0);
        assert_eq!(client_probe.snapshot().polls, 0);
        drop(saturation);
        assert_eq!(fixture.budget.used(), baseline);
        let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
        let saturation = fixture.budget.reserve(fixture.budget.limit() - baseline)?;
        rejected(
            proof.validate_restored(
                &raw,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Anthropic),
                io::Cursor::new(&raw),
                &fixture.budget,
            ),
            "NATIVE_SHARED_AN_SSE_UNCHARGED_FINAL_CHECK_ACCEPTED",
        );
        drop(saturation);
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    fn native_shared_an_sse_stage_case(
        fixture: &Fixture,
        events: &[Value],
        mode: usize,
    ) -> TestResult<Result<(usize, usize, Value), RestoreError>> {
        native_shared_an_sse_stage_apply(fixture, events, mode, |stage| {
            Ok((
                stage.restored_occurrences(),
                stage.output_tokens().len(),
                stage.delivered().clone(),
            ))
        })
    }

    fn native_shared_an_sse_stage_apply<T>(
        fixture: &Fixture,
        events: &[Value],
        mode: usize,
        apply: impl FnOnce(
            crate::proxy::security::restoration_core::StagedNativeSse<'_>,
        ) -> Result<T, RestoreError>,
    ) -> TestResult<Result<T, RestoreError>> {
        use crate::proxy::security::{
            restoration_contract::{EditConstraint, SourceClass, TextPolicy, TextSites},
            restoration_core::{
                stage_native_sse, stage_native_sse_with_lifecycle, NativeSseFrameObservation,
                NativeSseObservation, NativeSseSnapshotSelection, NativeSseTextStream,
            },
        };
        let raw = native_shared_an_encode_events(events)?;
        let raw_text = std::str::from_utf8(&raw)?;
        let mut ranges = Vec::new();
        let mut offset = 0;
        for frame in raw_text.split_inclusive("\r\n\r\n") {
            ranges.push(offset..offset + frame.len());
            offset += frame.len();
        }
        fn leaves<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
            match node {
                Value::String(_) => out.push(node),
                Value::Array(values) => {
                    for value in values {
                        leaves(value, out);
                    }
                }
                Value::Object(values) => {
                    for value in values.values() {
                        leaves(value, out);
                    }
                }
                _ => {}
            }
        }
        let mut groups = vec![Vec::new(), Vec::new(), Vec::new()];
        let mut frames = Vec::new();
        for (event, range) in events.iter().zip(ranges) {
            let mut sites = TextSites::new(fixture.budget.clone())?;
            let node = match event["type"].as_str() {
                Some("content_block_start") => match event["content_block"]["type"].as_str() {
                    Some("text") => Some((&event["content_block"]["text"], false, false)),
                    Some("thinking") => Some((&event["content_block"]["thinking"], true, false)),
                    _ => None,
                },
                Some("content_block_delta") => match event["delta"]["type"].as_str() {
                    Some("text_delta") => Some((&event["delta"]["text"], false, false)),
                    Some("thinking_delta") => Some((&event["delta"]["thinking"], true, false)),
                    Some("input_json_delta") => {
                        Some((&event["delta"]["partial_json"], false, true))
                    }
                    _ => None,
                },
                _ => None,
            };
            if event["type"] == "content_block_start"
                && event["content_block"]["type"] == "tool_use"
            {
                let mut nodes = Vec::new();
                leaves(&event["content_block"]["input"], &mut nodes);
                for leaf in nodes {
                    sites.observe(leaf, SourceClass::ToolArguments, EditConstraint::Editable)?;
                    groups.push(vec![leaf]);
                }
            }
            if let Some((node, immutable, arguments)) = node {
                sites.observe(
                    node,
                    if arguments {
                        SourceClass::ToolArguments
                    } else {
                        SourceClass::HistoricalText
                    },
                    if immutable && mode != 9 {
                        EditConstraint::Immutable
                    } else {
                        EditConstraint::Editable
                    },
                )?;
                let index = event["index"]
                    .as_u64()
                    .ok_or("NATIVE_SHARED_AN_SSE_FIXTURE_INDEX")?
                    as usize;
                groups
                    .get_mut(index)
                    .ok_or("NATIVE_SHARED_AN_SSE_FIXTURE_GROUP")?
                    .push(node);
            }
            frames.push(required(
                NativeSseFrameObservation::new(
                    &raw_text[range],
                    Some(event),
                    sites,
                    fixture.budget.clone(),
                ),
                "NATIVE_SHARED_AN_SSE_FRAME_OBSERVATION_REQUIRED",
            )?);
        }
        let frame_refs: Vec<_> = frames.iter().collect();
        let observed = required(
            NativeSseObservation::new(
                &raw,
                &frame_refs,
                ClientProtocol::new(Protocol::Anthropic),
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
            ),
            "NATIVE_SHARED_AN_SSE_ENTITY_OBSERVATION_REQUIRED",
        )?;
        let mut snapshots: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(index, event)| NativeSseSnapshotSelection::new(index, event))
            .collect();
        match mode {
            5 => {
                snapshots.remove(2);
            }
            6 => snapshots.insert(2, NativeSseSnapshotSelection::new(1, &events[1])),
            7 => snapshots.swap(1, 2),
            8 => snapshots[2] = NativeSseSnapshotSelection::new(2, &events[1]),
            _ => {}
        }
        if mode == 1 {
            let other = groups[0].clone();
            groups[1].extend(other);
            groups[0].clear();
        } else if mode == 2 {
            groups[1].pop();
        } else if mode == 3 {
            if let Some(node) = groups[1].last().copied() {
                groups[1].push(node);
            }
        }
        let streams: Vec<_> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| !group.is_empty())
            .map(|(index, group)| {
                NativeSseTextStream::with_policy(
                    group,
                    if index == 0 {
                        TextPolicy::Reasoning
                    } else if index >= 2 {
                        TextPolicy::ToolArguments
                    } else {
                        TextPolicy::Output
                    },
                )
            })
            .collect();
        let stage = if matches!(mode, 4 | 9) {
            stage_native_sse(
                &observed,
                &snapshots,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
            )
        } else {
            stage_native_sse_with_lifecycle(
                &observed,
                &snapshots,
                &streams,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt,
            )
        };
        Ok(stage.and_then(apply))
    }

    #[test]
    fn native_shared_an_sse_block_streams_share_one_lifecycle_ledger() -> TestResult {
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let baseline = fixture.budget.used();
        let mut events = native_shared_an_events();
        events[6]["delta"]["text"] = json!(&reference[..reference.len() / 2]);
        events.insert(
            7,
            json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":&reference[reference.len() / 2..]}}),
        );
        events[10]["delta"]["partial_json"] = json!(format!(
            "{{\"value\":\"{}",
            &reference[..reference.len() / 2]
        ));
        events[11]["delta"]["partial_json"] = json!(format!(
            "{}\",\"types\":[true,null,7]}}",
            &reference[reference.len() / 2..]
        ));
        let (occurrences, dependencies, delivered) = required(
            native_shared_an_sse_stage_case(&fixture, &events, 0)?,
            "NATIVE_SHARED_AN_SSE_BLOCK_LIFECYCLE_REQUIRED",
        )?;
        assert_eq!((occurrences, dependencies), (2, 1));
        assert!(
            delivered.as_array().is_some_and(|blocks| blocks.len() == 3),
            "NATIVE_SHARED_AN_SSE_COMPLETE_BLOCKS_COMPOSER_REQUIRED"
        );
        assert!(delivered[0]["thinking"].as_str() == Some("synthetic public thought"));
        assert!(delivered[0]["signature"].as_str() == Some("synthetic-late-signature"));
        assert!(
            delivered[1]["text"].as_str() == Some(NATIVE_SHARED_AN_TEXT),
            "NATIVE_SHARED_AN_SSE_SPLIT_TEXT_NOT_RESTORED"
        );
        assert!(delivered[2]["input"]["value"].as_str() == Some(NATIVE_SHARED_AN_TEXT));
        assert!(delivered[2]["input"]["types"] == json!([true, null, 7]));
        assert!(delivered[2]["id"].as_str() == Some("call_synthetic_sse"));
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_start_input_leaves_share_ledger_and_preserve_types() -> TestResult
    {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let baseline = fixture.budget.used();
        let mut events = native_shared_an_events();
        events[6]["delta"]["text"] = json!(reference);
        events[8]["content_block"]["input"] = json!({"value":reference,
            "nested":[true,null,7,{"value":reference}],"json_looking":"{not an object string}"});
        events.drain(9..11);
        let raw = native_shared_an_encode_events(&events)?;
        let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
        let (occurrences, dependencies, delivered) = required(
            native_shared_an_sse_stage_case(&fixture, &events, 0)?,
            "NATIVE_SHARED_AN_SSE_START_INPUT_NATIVE_LEAVES_REQUIRED",
        )?;
        assert_eq!((occurrences, dependencies), (3, 1));
        assert!(delivered[2]["input"]["value"].as_str() == Some(NATIVE_SHARED_AN_TEXT));
        assert!(
            delivered[2]["input"]["nested"] == json!([true,null,7,{"value":NATIVE_SHARED_AN_TEXT}])
        );
        assert!(delivered[2]["input"]["json_looking"] == "{not an object string}");
        let finalized = required(
            native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                finalize_native_sse(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    NATIVE_DRIVER_ENTITY_LIMIT,
                )
            })?,
            "NATIVE_SHARED_AN_SSE_START_INPUT_SEALED_REQUIRED",
        )?;
        let mut stream = finalized.into_delivery_stream();
        while let Some(chunk) = stream.next().await {
            drop(chunk?);
        }
        drop(stream);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (1, 0, 0));
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[test]
    fn native_shared_an_sse_complete_ordered_snapshot_selections() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let baseline = fixture.budget.used();
        let events = native_shared_an_events();
        drop(required(
            native_shared_an_sse_stage_case(&fixture, &events, 0)?,
            "NATIVE_SHARED_AN_SSE_SELECTION_POSITIVE_CONTROL_REQUIRED",
        )?);
        for mode in 5..=8 {
            rejected(
                native_shared_an_sse_stage_case(&fixture, &events, mode)?,
                "NATIVE_SHARED_AN_SSE_INEXACT_SELECTION_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn native_shared_an_sse_immutable_thinking_and_grouping_controls() -> TestResult {
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let baseline = fixture.budget.used();
        let events = native_shared_an_events();
        // Positive control prevents unsupported dispatch from masking invalid-group assertions.
        drop(required(
            native_shared_an_sse_stage_case(&fixture, &events, 0)?,
            "NATIVE_SHARED_AN_SSE_IMMUTABLE_CONTROL_LIFECYCLE_REQUIRED",
        )?);
        for mode in 1..4 {
            rejected(
                native_shared_an_sse_stage_case(&fixture, &events, mode)?,
                "NATIVE_SHARED_AN_SSE_INEXACT_STREAM_GROUP_ACCEPTED",
            );
        }
        let mut thinking = events.clone();
        thinking[2]["delta"]["thinking"] = json!(reference);
        rejected(
            native_shared_an_sse_stage_case(&fixture, &thinking, 0)?,
            "NATIVE_SHARED_AN_SSE_IMMUTABLE_REASONING_REFERENCE_ACCEPTED",
        );
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_final_wire_derives_exact_complete_blocks() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let events = native_shared_an_events();
        let raw = native_shared_an_encode_events(&events)?;
        let expected = json!([
            {"type":"thinking","thinking":"synthetic public thought","signature":"synthetic-late-signature"},
            {"type":"text","text":"synthetic 雪 🦀 text"},
            {"type":"tool_use","id":"call_synthetic_sse","name":"synthetic-tool",
                "input":{"value":"synthetic leaf","types":[true,null,7]}}
        ]);
        let baseline = fixture.budget.used();
        for mode in 0..4 {
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            let mut snapshot = expected.clone();
            match mode {
                1 => {
                    snapshot
                        .as_array_mut()
                        .ok_or("synthetic blocks missing")?
                        .swap(0, 1);
                }
                2 => snapshot[2]["input"]["value"] = json!("synthetic changed"),
                3 => snapshot[0]["signature"] = json!("synthetic wrong signature"),
                _ => {}
            }
            let result = proof.validate_restored_chat_snapshots(
                &raw,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Anthropic),
                io::Cursor::new(&raw),
                &fixture.budget,
                &snapshot,
            );
            if mode == 0 {
                required(
                    result,
                    "NATIVE_SHARED_AN_SSE_SEALED_SNAPSHOT_DISPATCH_REQUIRED",
                )?;
            } else {
                rejected(
                    result,
                    "NATIVE_SHARED_AN_SSE_INEXACT_FINAL_SNAPSHOT_ACCEPTED",
                );
            }
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_finalizer_rejects_uncomposed_event_history() -> TestResult {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        let fixture = native_shared_an_fixture()?;
        let events = native_shared_an_events();
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
        rejected(
            native_shared_an_sse_stage_apply(&fixture, &events, 4, |stage| {
                finalize_native_sse(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    NATIVE_DRIVER_ENTITY_LIMIT,
                )
            })?,
            "NATIVE_SHARED_AN_SSE_UNCOMPOSED_PUBLICATION_ACCEPTED",
        );
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_publication_requires_poll_after_last_yield() -> TestResult {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        fn counts(store: &DlpTokenStore) -> TestResult<(usize, usize, usize)> {
            let (published, pending, pins, _) =
                crate::proxy::security::tokenization::publication_test_support::counts(store)?;
            Ok((published, pending, pins))
        }
        let template = native_shared_an_events();
        for yields in 0..=template.len() + 1 {
            let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
            let mut events = template.clone();
            events[6]["delta"]["text"] = json!(reference);
            let raw = native_shared_an_encode_events(&events)?;
            let baseline = fixture.budget.used();
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            let finalized = required(
                native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                    finalize_native_sse(
                        stage,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        NATIVE_DRIVER_ENTITY_LIMIT,
                    )
                })?,
                "NATIVE_SHARED_AN_SSE_FINALIZED_DELIVERY_REQUIRED",
            )?;
            assert!(counts(&fixture.store)? == (0, 1, 1));
            let mut delivery = finalized.into_delivery_stream();
            let mut delivered = Vec::new();
            for _ in 0..yields.min(events.len()) {
                let chunk = delivery
                    .next()
                    .await
                    .ok_or("synthetic delivery missing")??;
                delivered.extend_from_slice(&chunk);
                assert!(
                    counts(&fixture.store)? == (0, 1, 1),
                    "NATIVE_SHARED_AN_SSE_PUBLICATION_ACTIVE_DURING_YIELD"
                );
            }
            if yields > events.len() {
                assert!(
                    delivery.next().await.is_none(),
                    "NATIVE_SHARED_AN_SSE_CONTINUATION_NOT_COMPLETE"
                );
                assert!(counts(&fixture.store)? == (1, 0, 0));
                let (expected_occurrences, expected_dependencies, _) = required(
                    native_shared_an_sse_stage_case(&fixture, &events, 0)?,
                    "NATIVE_SHARED_AN_SSE_POST_DELIVERY_CONTROL_REQUIRED",
                )?;
                assert_eq!((expected_occurrences, expected_dependencies), (1, 1));
                assert!(std::str::from_utf8(&delivered)?.contains(NATIVE_SHARED_AN_TEXT));
                assert!(!std::str::from_utf8(&delivered)?.contains(&reference));
            }
            drop(delivery);
            if yields <= events.len() {
                assert!(counts(&fixture.store)? == (0, 0, 0));
            }
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_finalizer_identity_and_final_wire_failures_refund() -> TestResult
    {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let foreign = native_shared_an_fixture()?;
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let mut events = native_shared_an_events();
        events[6]["delta"]["text"] = json!(reference);
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        for mode in 0..4 {
            let mut donor_events = events.clone();
            if mode == 2 {
                donor_events[6]["delta"]["text"] = json!("synthetic donor text");
            }
            let donor_raw = native_shared_an_encode_events(&donor_events)?;
            let (original, _) = ScriptedStream::entity(&donor_raw);
            let (client, _) = ScriptedStream::entity(&donor_raw);
            let driver = NativeCompletionDriver::new(
                pair(Protocol::Anthropic, Protocol::Anthropic),
                if mode == 0 {
                    wrong_attempt.clone()
                } else {
                    fixture.attempt.clone()
                },
                if mode == 1 {
                    foreign.binding.clone()
                } else {
                    fixture.binding.clone()
                },
                fixture.budget.clone(),
            );
            let attempt = if mode == 0 {
                &wrong_attempt
            } else {
                &fixture.attempt
            };
            let proof = required(
                driver.consume_sse(original, attempt, client, attempt).await,
                "NATIVE_SHARED_AN_SSE_IDENTITY_DONOR_REQUIRED",
            )?;
            rejected(
                native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                    finalize_native_sse(
                        stage,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        if mode == 3 {
                            1
                        } else {
                            NATIVE_DRIVER_ENTITY_LIMIT
                        },
                    )
                })?,
                "NATIVE_SHARED_AN_SSE_FOREIGN_FINALIZATION_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        // Nonempty object start plus argument deltas cannot pass completion.
        let mut conflict = events.clone();
        conflict[8]["content_block"]["input"] = json!({"value":reference});
        let conflict_raw = native_shared_an_encode_events(&conflict)?;
        assert!(
            native_shared_an_sse_proof(&fixture, &conflict_raw)
                .await
                .is_err(),
            "NATIVE_SHARED_AN_SSE_CONFLICTING_START_INPUT_ACCEPTED"
        );
        // A signature on another block or after block stop has no authority.
        for index in [1, 2] {
            let mut conflict = events.clone();
            conflict[3]["index"] = json!(index);
            assert!(
                native_shared_an_sse_proof(&fixture, &native_shared_an_encode_events(&conflict)?)
                    .await
                    .is_err(),
                "NATIVE_SHARED_AN_SSE_FOREIGN_SIGNATURE_ACCEPTED"
            );
        }
        let mut conflict = events.clone();
        conflict.swap(3, 4);
        assert!(
            native_shared_an_sse_proof(&fixture, &native_shared_an_encode_events(&conflict)?)
                .await
                .is_err(),
            "NATIVE_SHARED_AN_SSE_SIGNATURE_AFTER_STOP_ACCEPTED"
        );
        assert_eq!(fixture.budget.used(), baseline);
        drop(raw);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_finalizer_faults_are_atomic_and_precharged() -> TestResult {
        use crate::proxy::security::{
            payload_buffer::test_support as buffers,
            payload_metadata::test_support as metadata,
            response_restoration::finalize_native_sse,
            restoration_core::{LifecycleOperation, ResponseLifecycle},
        };
        use std::{cell::Cell, rc::Rc};
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let mut events = native_shared_an_events();
        events[6]["delta"]["text"] = json!(reference);
        events[9]["delta"]["partial_json"] = json!(format!("{{\"value\":\"{reference}"));
        events[10]["delta"]["partial_json"] = json!("\"}");
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        for operation in [
            LifecycleOperation::Begin,
            LifecycleOperation::OpenText,
            LifecycleOperation::Delta,
            LifecycleOperation::Done,
            LifecycleOperation::Hold,
            LifecycleOperation::Finish,
        ] {
            let guard = ResponseLifecycle::reject_operation_for_test(operation);
            rejected(
                native_shared_an_sse_stage_case(&fixture, &events, 0)?,
                "NATIVE_SHARED_AN_SSE_LIFECYCLE_FAULT_ACCEPTED",
            );
            drop(guard);
            assert_eq!(fixture.budget.used(), baseline);
        }
        for mode in 0..4 {
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            let observed = Rc::new(Cell::new(0));
            let seen = Rc::clone(&observed);
            let store = fixture.store.clone();
            let result = native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                let saturation = if mode == 0 {
                    Some(
                        fixture
                            .budget
                            .reserve(fixture.budget.limit() - fixture.budget.used())
                            .map_err(RestoreError::capacity)?,
                    )
                } else {
                    None
                };
                let buffer_hook = if matches!(mode, 1 | 2) {
                    let seen = Rc::clone(&seen);
                    Some(buffers::install(move |event| {
                        if event.phase
                            == if mode == 1 {
                                buffers::Phase::BeforeAllocation
                            } else {
                                buffers::Phase::BeforeCopy
                            }
                        {
                            assert!(
                                event.used >= event.requested,
                                "NATIVE_SHARED_AN_SSE_COPY_NOT_PRECHARGED"
                            );
                            seen.set(seen.get() + 1);
                            buffers::Action::FailAllocation
                        } else {
                            buffers::Action::Continue
                        }
                    }))
                } else {
                    None
                };
                let metadata_hook = if mode == 3 {
                    Some(metadata::install(move |event| {
                        let counts =
                            crate::proxy::security::tokenization::publication_test_support::counts(
                                &store,
                            );
                        if event.phase == metadata::Phase::BeforeAllocation
                            && counts.is_ok_and(|counts| counts.1 == 1)
                        {
                            assert!(
                                event.used >= event.bytes,
                                "NATIVE_SHARED_AN_SSE_METADATA_NOT_PRECHARGED"
                            );
                            seen.set(seen.get() + 1);
                            metadata::Action::Fail
                        } else {
                            metadata::Action::Continue
                        }
                    }))
                } else {
                    None
                };
                let result = finalize_native_sse(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    NATIVE_DRIVER_ENTITY_LIMIT,
                );
                drop((saturation, buffer_hook, metadata_hook));
                result
            })?;
            rejected(result, "NATIVE_SHARED_AN_SSE_FINALIZER_FAULT_RELEASED");
            if mode != 0 {
                assert_eq!(
                    observed.get(),
                    1,
                    "NATIVE_SHARED_AN_SSE_FAULT_NOT_EXERCISED"
                );
            }
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_continuation_failure_does_not_publish() -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_sse, tokenization::publication_test_support,
        };
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let mut events = native_shared_an_events();
        events[6]["delta"]["text"] = json!(reference);
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
        let finalized = required(
            native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                finalize_native_sse(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    NATIVE_DRIVER_ENTITY_LIMIT,
                )
            })?,
            "NATIVE_SHARED_AN_SSE_CONTINUATION_CONTROL_REQUIRED",
        )?;
        let mut stream = finalized.into_delivery_stream();
        for _ in &events {
            drop(stream.next().await.ok_or("synthetic missing frame")??);
        }
        let counts = publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 1, 1));
        publication_test_support::unavailable(&fixture.store)?;
        assert!(stream.next().await.is_some_and(
            |result| result.is_err_and(|error| error.to_string() == "Response stream failed.")
        ));
        assert!(stream.next().await.is_none());
        drop(stream);
        let counts = publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_complete_call_delivery_and_retained_bytes() -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_sse, tokenization::publication_test_support,
        };
        for mapped in [false, true] {
            let (fixture, reference) = native_shared_an_mapped_fixture(mapped)?;
            let mut events = native_shared_an_events();
            events[6]["delta"]["text"] = json!(reference);
            events[9]["delta"]["partial_json"] = json!(format!("{{\"value\":\"{reference}"));
            events[10]["delta"]["partial_json"] = json!("\",\"types\":[true,null,7]}");
            let raw = native_shared_an_encode_events(&events)?;
            let baseline = fixture.budget.used();
            let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
            let finalized = required(
                native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                    assert_eq!(
                        (stage.restored_occurrences(), stage.output_tokens().len()),
                        if mapped { (2, 1) } else { (0, 0) }
                    );
                    assert!(stage.delivered()[2]["id"] == "call_synthetic_sse");
                    assert!(stage.delivered()[2]["name"] == "synthetic-tool");
                    finalize_native_sse(
                        stage,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        NATIVE_DRIVER_ENTITY_LIMIT,
                    )
                })?,
                "NATIVE_SHARED_AN_SSE_COMPLETE_CALL_FINALIZATION_REQUIRED",
            )?;
            let mut stream = finalized.into_delivery_stream();
            let mut retained = Vec::new();
            for _ in &events {
                retained.push(
                    stream
                        .next()
                        .await
                        .ok_or("synthetic missing complete-call frame")??,
                );
                let counts = publication_test_support::counts(&fixture.store)?;
                assert!((counts.0, counts.1, counts.2) == (0, 1, 1));
            }
            assert!(stream.next().await.is_none());
            drop(stream);
            let counts = publication_test_support::counts(&fixture.store)?;
            assert!((counts.0, counts.1, counts.2) == (1, 0, 0));
            assert!(
                fixture.budget.used() > baseline,
                "NATIVE_SHARED_AN_SSE_YIELDED_OWNER_UNCHARGED"
            );
            let wire: Vec<u8> = retained
                .iter()
                .flat_map(|chunk| chunk.iter().copied())
                .collect();
            let expected = if mapped {
                NATIVE_SHARED_AN_TEXT
            } else {
                &reference
            };
            let text = std::str::from_utf8(&wire)?;
            let mut arguments = String::new();
            for frame in text.split("\r\n\r\n").filter(|frame| !frame.is_empty()) {
                let data = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data:"))
                    .ok_or("synthetic data missing")?;
                let event: Value = serde_json::from_str(data)?;
                if event["delta"]["type"] == "text_delta" {
                    assert!(event["delta"]["text"].as_str() == Some(expected));
                }
                if let Some(fragment) = event["delta"]["partial_json"].as_str() {
                    arguments.push_str(fragment);
                }
            }
            let input: Value = serde_json::from_str(&arguments)?;
            assert!(input == json!({"value":expected,"types":[true,null,7]}));
            if !mapped {
                assert!(wire == raw, "NATIVE_SHARED_AN_SSE_UNCHANGED_WIRE_LOST");
            }
            let last = retained.pop().ok_or("synthetic retained last missing")?;
            drop(retained);
            assert!(
                fixture.budget.used() > baseline,
                "NATIVE_SHARED_AN_SSE_LAST_OWNER_UNCHARGED"
            );
            drop(last);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_review_legacy_editable_thinking_bypass_rejected() -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_sse,
            restoration_contract::{EditConstraint, SourceClass, TextSites},
            restoration_core::{
                stage_native_sse, NativeSseFrameObservation, NativeSseObservation,
                NativeSseSnapshotSelection,
            },
        };
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let mut events = native_shared_an_events();
        events[1]["content_block"]["thinking"] = json!(reference);
        events[1]["content_block"]["signature"] = json!("synthetic-start-signature");
        events = vec![
            events[0].clone(),
            events[1].clone(),
            events[4].clone(),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
            events[13].clone(),
        ];
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        // This block selection matches the complete final snapshot exactly; the
        // legacy path must not edit thinking based on an adapter's Editable claim.
        let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
        let result = {
            let text = std::str::from_utf8(&raw)?;
            let mut offset = 0;
            let mut frames = Vec::new();
            for (event, wire) in events.iter().zip(text.split_inclusive("\r\n\r\n")) {
                let mut sites = TextSites::new(fixture.budget.clone())?;
                if event["content_block"]["type"] == "thinking" {
                    sites.observe(
                        &event["content_block"]["thinking"],
                        SourceClass::HistoricalText,
                        EditConstraint::Editable,
                    )?;
                }
                frames.push(required(
                    NativeSseFrameObservation::new(
                        &text[offset..offset + wire.len()],
                        Some(event),
                        sites,
                        fixture.budget.clone(),
                    ),
                    "NATIVE_SHARED_AN_SSE_LEGACY_FRAME_REQUIRED",
                )?);
                offset += wire.len();
            }
            let frame_refs: Vec<_> = frames.iter().collect();
            let observed = required(
                NativeSseObservation::new(
                    &raw,
                    &frame_refs,
                    ClientProtocol::new(Protocol::Anthropic),
                    &fixture.lease,
                    &fixture.binding,
                    fixture.budget.clone(),
                ),
                "NATIVE_SHARED_AN_SSE_LEGACY_OBSERVATION_REQUIRED",
            )?;
            let blocks: Vec<_> = events
                .iter()
                .enumerate()
                .filter(|(_, event)| event["type"] == "content_block_start")
                .map(|(index, event)| {
                    NativeSseSnapshotSelection::new(index, &event["content_block"])
                })
                .collect();
            stage_native_sse(
                &observed,
                &blocks,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
            )
            .and_then(|stage| {
                finalize_native_sse(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    NATIVE_DRIVER_ENTITY_LIMIT,
                )
            })
        };
        rejected(result, "NATIVE_SHARED_AN_SSE_REVIEW_LEGACY_THINKING_BYPASS");
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_sse_review_malformed_message_delta_rejected_on_all_wires(
    ) -> TestResult {
        use crate::proxy::security::http_restoration::completion_verifier::NativeSseVerifier;
        let fixture = native_shared_an_fixture()?;
        let events = native_shared_an_events();
        let raw = native_shared_an_encode_events(&events)?;
        let baseline = fixture.budget.used();
        for malformed in [
            json!({"type":"message_delta"}),
            json!({"type":"message_delta","delta":7}),
            json!({"type":"message_delta","delta":null}),
            json!({"type":"message_delta","delta":[]}),
        ] {
            let mut invalid = events.clone();
            invalid.insert(invalid.len() - 1, malformed);
            let invalid_raw = native_shared_an_encode_events(&invalid)?;
            for client in [false, true] {
                let mut verifier = NativeSseVerifier::new(
                    pair(Protocol::Anthropic, Protocol::Anthropic),
                    fixture.attempt.clone(),
                    fixture.binding.clone(),
                    fixture.budget.clone(),
                );
                let result = if client {
                    verifier.consume_client_sse(io::Cursor::new(&invalid_raw), &fixture.attempt)
                } else {
                    verifier.consume_upstream_sse(io::Cursor::new(&invalid_raw), &fixture.attempt)
                };
                rejected(
                    result,
                    "NATIVE_SHARED_AN_SSE_REVIEW_MALFORMED_DELTA_ORIGINAL_CLIENT",
                );
                rejected(
                    verifier.consume_upstream_sse(io::Cursor::new(&raw), &fixture.attempt),
                    "NATIVE_SHARED_AN_SSE_REVIEW_INVALID_STATE_REUSED",
                );
                rejected(
                    verifier.finish(),
                    "NATIVE_SHARED_AN_SSE_REVIEW_INVALID_PROOF_ISSUED",
                );
                let (original, original_probe) =
                    ScriptedStream::entity(if client { &raw } else { &invalid_raw });
                let (client_wire, client_probe) =
                    ScriptedStream::entity(if client { &invalid_raw } else { &raw });
                rejected(
                    native_shared_an_driver(&fixture)
                        .consume_sse(original, &fixture.attempt, client_wire, &fixture.attempt)
                        .await,
                    "NATIVE_SHARED_AN_SSE_REVIEW_ASYNC_INVALID_PROOF",
                );
                original_probe.assert_eof();
                if client {
                    client_probe.assert_eof();
                }
                let donor = native_shared_an_sse_proof(&fixture, &raw).await?;
                rejected(
                    native_shared_an_sse_stage_apply(&fixture, &invalid, 0, |stage| {
                        crate::proxy::security::response_restoration::finalize_native_sse(
                            stage,
                            &fixture.lease,
                            donor,
                            &fixture.attempt,
                            NATIVE_DRIVER_ENTITY_LIMIT,
                        )
                    })?,
                    "NATIVE_SHARED_AN_SSE_REVIEW_MALFORMED_PUBLICATION_ACCEPTED",
                );
                let proof = native_shared_an_sse_proof(&fixture, &raw).await?;
                rejected(
                    proof.validate_restored(
                        &raw,
                        &fixture.binding,
                        &fixture.attempt,
                        ClientProtocol::new(Protocol::Anthropic),
                        io::Cursor::new(&invalid_raw),
                        &fixture.budget,
                    ),
                    "NATIVE_SHARED_AN_SSE_REVIEW_MALFORMED_DELTA_FINAL",
                );
                assert_eq!(fixture.budget.used(), baseline);
            }
        }
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    // Native Anthropic JSON: exact complete message, structural tool-input object,
    // allowed stop reason and actual entity None; signing never grants authority.
    fn native_shared_an_fixture() -> TestResult<Fixture> {
        let store = DlpTokenStore::new();
        let lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Anthropic),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-an-shared-app",
            Some("synthetic-an-shared-session"),
            1,
        )?;
        let binding = lease.bind_actual_attempt(
            "synthetic-an-shared-provider",
            "anthropic",
            "https://synthetic.invalid/v1/messages",
            Some("synthetic-an-shared-principal"),
        )?;
        let budget = PayloadBudget::with_limit(4 * NATIVE_DRIVER_ENTITY_LIMIT);
        let attempt = AttemptProvenance::new(&budget)?;
        Ok(Fixture {
            store,
            lease,
            binding,
            attempt,
            budget,
        })
    }

    fn native_shared_an_driver(fixture: &Fixture) -> NativeCompletionDriver {
        NativeCompletionDriver::new(
            pair(Protocol::Anthropic, Protocol::Anthropic),
            fixture.attempt.clone(),
            fixture.binding.clone(),
            fixture.budget.clone(),
        )
    }

    fn native_shared_an_message() -> Value {
        json!({"id":"msg_synthetic_shared", "type":"message", "role":"assistant",
            "model":"synthetic-model", "content":[
                {"type":"thinking", "thinking":"synthetic public thought", "signature":"synthetic-opaque-signature"},
                {"type":"text", "text":"synthetic public 雪 text"},
                {"type":"tool_use", "id":"call_synthetic_shared", "name":"synthetic-tool",
                    "input":{"value":"ordinary string leaf", "types":[true,null,7]}}
            ], "stop_reason":"tool_use", "stop_sequence":null,
            "usage":{"input_tokens":1,"output_tokens":2}, "extension":{"preserve":true}})
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_real_eof_split_and_final_wire() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let client = native_shared_an_message();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (source, probe) = ScriptedStream::split(&raw, at);
            let proof = required(
                native_shared_an_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_AN_JSON_REAL_EOF_PROOF_REQUIRED",
            )?;
            probe.assert_eof();
            required(
                proof.validate_restored(
                    &client,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                    &raw,
                    &fixture.budget,
                ),
                "NATIVE_SHARED_AN_JSON_STRICT_FINAL_WIRE_REQUIRED",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        assert_eq!(fixture.store.stats()?.entries, 0);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_pending_and_cancel_do_not_issue_proof() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let client = native_shared_an_message();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        let (source, probe, gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(native_shared_an_driver(&fixture).consume_json(
            source,
            &fixture.attempt,
            &client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "NATIVE_SHARED_AN_JSON_PENDING_NOT_EOF"
        );
        assert_eq!(probe.snapshot().eofs, 0);
        drop(future);
        gate.release();
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(probe.snapshot().drops, 1);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_final_wire_rechecks_native_object_shape() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let source_value = native_shared_an_message();
        let raw = serde_json::to_vec(&source_value)?;
        let (source, probe) = ScriptedStream::entity(&raw);
        let proof = required(
            native_shared_an_driver(&fixture)
                .consume_json(source, &fixture.attempt, &source_value, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_JSON_DONOR_PROOF_REQUIRED",
        )?;
        probe.assert_eof();
        let mut invalid = source_value.clone();
        invalid["content"][2]["input"] = json!("not an object");
        let wire = serde_json::to_vec(&invalid)?;
        assert!(
            proof
                .validate_restored(
                    &source_value,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                    &wire,
                    &fixture.budget
                )
                .is_err(),
            "NATIVE_SHARED_AN_JSON_FINAL_NATIVE_OBJECT_NOT_RECHECKED"
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_original_strictness_and_transport_errors() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let client = native_shared_an_message();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        for kind in [io::ErrorKind::UnexpectedEof, io::ErrorKind::Interrupted] {
            let (source, probe) = ScriptedStream::terminal_error(&raw, kind);
            assert!(native_shared_an_driver(&fixture)
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await
                .is_err());
            probe.assert_error();
            assert_eq!(fixture.budget.used(), baseline);
        }
        for wire in [
            format!("{} trailing", String::from_utf8(raw.clone())?),
            "{\"id\":\"first\",\"id\":\"second\"}".to_owned(),
        ] {
            let (source, probe) = ScriptedStream::entity(wire.as_bytes());
            assert!(native_shared_an_driver(&fixture)
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await
                .is_err());
            probe.assert_eof();
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    const NATIVE_SHARED_AN_TEXT: &str = "Synthetic native object restored text";

    fn native_shared_an_mapped_fixture(mapped: bool) -> TestResult<(Fixture, String)> {
        use crate::proxy::security::tokenization::{TokenCategory, TokenFragment};
        let mut fixture = native_shared_an_fixture()?;
        let reference = if mapped {
            fixture.lease.prepare(
                &[TokenFragment {
                    raw: NATIVE_SHARED_AN_TEXT,
                    whole_span_categories: &[TokenCategory::Phone],
                }],
                &[],
                |tokens| Ok(tokens[0].clone()),
            )?
        } else {
            "ordinary native string leaf 雪".to_owned()
        };
        fixture.binding = fixture.lease.bind_actual_attempt(
            "synthetic-an-shared-provider",
            "anthropic",
            "https://synthetic.invalid/v1/messages",
            Some("synthetic-an-shared-principal"),
        )?;
        Ok((fixture, reference))
    }

    fn native_shared_an_leaf_message(reference: &str) -> Value {
        let mut source = native_shared_an_message();
        source["content"][1]["text"] = json!(reference);
        source["content"][2]["input"] = json!({
            "label":reference, "nested":[7,true,null,{"value":reference}],
            "opaque":{"preserve":false}, "json_looking":"{not a JSON object string}"
        });
        source["extension"]["opaque_reference"] = json!(reference);
        source
    }

    fn native_shared_an_leaf_observation<'a>(
        fixture: &Fixture,
        source: &'a Value,
        immutable_leaf: bool,
    ) -> TestResult<crate::proxy::security::restoration_core::NativeJsonObservation<'a>> {
        use crate::proxy::security::{
            restoration_contract::{EditConstraint, SourceClass, TextPolicy, TextSites},
            restoration_core::NativeJsonObservation,
        };
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            &source["content"][0]["thinking"],
            SourceClass::HistoricalText,
            EditConstraint::Immutable,
        )?;
        sites.observe(
            &source["content"][1]["text"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        for node in [
            &source["content"][2]["input"]["label"],
            &source["content"][2]["input"]["nested"][3]["value"],
            &source["content"][2]["input"]["json_looking"],
        ] {
            sites.observe(
                node,
                SourceClass::ToolArguments,
                if immutable_leaf {
                    EditConstraint::Immutable
                } else {
                    EditConstraint::Editable
                },
            )?;
        }
        required(
            NativeJsonObservation::new_with_policies(
                source,
                ClientProtocol::new(Protocol::Anthropic),
                sites,
                &[
                    &source["content"][0],
                    &source["content"][1],
                    &source["content"][2],
                ],
                &[
                    TextPolicy::Reasoning,
                    TextPolicy::Output,
                    TextPolicy::ToolArguments,
                    TextPolicy::ToolArguments,
                    TextPolicy::ToolArguments,
                ],
                fixture.budget.clone(),
            ),
            "NATIVE_SHARED_AN_JSON_LEAF_OBSERVATION_REQUIRED",
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_object_leaves_single_lifecycle_and_sealed_delivery() -> TestResult
    {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::{stage_native_json_with_lifecycle, ResponseLifecycle},
        };
        for mapped in [false, true] {
            let (fixture, reference) = native_shared_an_mapped_fixture(mapped)?;
            let source = native_shared_an_leaf_message(&reference);
            let original = source.clone();
            let baseline = fixture.budget.used();
            let wire = serde_json::to_vec_pretty(&source)?;
            let (transport, probe) = ScriptedStream::split(&wire, wire.len() / 2);
            let proof = required(
                native_shared_an_driver(&fixture)
                    .consume_json(transport, &fixture.attempt, &source, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_AN_JSON_LEAF_HTTP_PROOF_REQUIRED",
            )?;
            probe.assert_eof();
            let observed = native_shared_an_leaf_observation(&fixture, &source, false)?;
            let begins = ResponseLifecycle::begin_count_for_test();
            let stage = required(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Anthropic, Protocol::Anthropic),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_AN_JSON_NATIVE_OBJECT_LIFECYCLE_REQUIRED",
            )?;
            assert_eq!(
                ResponseLifecycle::begin_count_for_test(),
                begins + 1,
                "NATIVE_SHARED_AN_JSON_REQUIRES_ONE_LIFECYCLE"
            );
            assert_eq!(
                stage.output_tokens().len(),
                usize::from(mapped),
                "NATIVE_SHARED_AN_JSON_REQUIRES_SINGLE_LEDGER"
            );
            assert_eq!(stage.restored_occurrences(), 3 * usize::from(mapped));
            assert!(stage.changed() == mapped);
            let expected_text = if mapped {
                NATIVE_SHARED_AN_TEXT
            } else {
                &reference
            };
            let mut expected = source.clone();
            expected["content"][1]["text"] = json!(expected_text);
            expected["content"][2]["input"]["label"] = json!(expected_text);
            expected["content"][2]["input"]["nested"][3]["value"] = json!(expected_text);
            assert!(
                stage.as_value() == &expected,
                "NATIVE_SHARED_AN_JSON_TYPES_OR_OPAQUE_CHANGED"
            );
            assert!(
                stage.delivered() == &expected["content"],
                "NATIVE_SHARED_AN_JSON_FULL_BLOCKS_REQUIRED"
            );
            assert!(source == original, "NATIVE_SHARED_AN_JSON_MUTATED_SOURCE");
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
            let finalized = required(
                finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    Bytes::from(wire.clone()),
                    NATIVE_DRIVER_ENTITY_LIMIT,
                ),
                "NATIVE_SHARED_AN_JSON_SEALED_DELIVERY_REQUIRED",
            )?;
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(
                        &fixture.binding,
                        ClientProtocol::new(Protocol::Anthropic),
                        &expected["content"]
                    )
                    .is_ok(),
                "NATIVE_SHARED_AN_JSON_NOT_ACTIVE_BEFORE_BODY_RETURN"
            );
            let mut stream = finalized.into_delivery_stream();
            let body = stream
                .next()
                .await
                .ok_or("NATIVE_SHARED_AN_JSON_MISSING_BODY")??;
            assert!(serde_json::from_slice::<Value>(&body)? == expected);
            if !mapped {
                assert!(body.as_ref() == wire.as_slice());
            }
            assert!(stream.next().await.is_none());
            drop(body);
            drop(stream);
            drop(observed);
            assert_eq!(
                fixture.budget.used(),
                baseline,
                "NATIVE_SHARED_AN_JSON_LEAF_REFUND_REQUIRED"
            );
        }
        Ok(())
    }

    #[test]
    fn native_shared_an_json_immutable_native_leaf_is_not_stringified_or_edited() -> TestResult {
        use crate::proxy::security::restoration_core::stage_native_json_with_lifecycle;
        for mapped in [false, true] {
            let (fixture, reference) = native_shared_an_mapped_fixture(mapped)?;
            let source = native_shared_an_leaf_message(&reference);
            let baseline = fixture.budget.used();
            let observed = native_shared_an_leaf_observation(&fixture, &source, true)?;
            let staged = stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt,
            );
            if mapped {
                assert!(
                    staged.is_err(),
                    "NATIVE_SHARED_AN_JSON_IMMUTABLE_MANDATORY_EDIT_ACCEPTED"
                );
            } else {
                let stage = required(staged, "NATIVE_SHARED_AN_JSON_IMMUTABLE_LITERAL_REQUIRED")?;
                assert!(!stage.changed());
                assert!(stage.output_tokens().is_empty());
            }
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn native_shared_an_json_lifecycle_failure_is_atomic_and_refunded() -> TestResult {
        use crate::proxy::security::restoration_core::{
            stage_native_json_with_lifecycle, LifecycleOperation, ResponseLifecycle,
        };
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let source = native_shared_an_leaf_message(&reference);
        let observed = native_shared_an_leaf_observation(&fixture, &source, false)?;
        let baseline = fixture.budget.used();
        for operation in [
            LifecycleOperation::Begin,
            LifecycleOperation::OpenText,
            LifecycleOperation::Done,
            LifecycleOperation::Hold,
            LifecycleOperation::Finish,
        ] {
            let guard = ResponseLifecycle::reject_operation_for_test(operation);
            assert!(stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt
            )
            .is_err());
            drop(guard);
            assert_eq!(fixture.budget.used(), baseline);
        }
        let stage = required(
            stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt,
            ),
            "NATIVE_SHARED_AN_JSON_ATOMIC_RECOVERY_REQUIRED",
        )?;
        assert_eq!(stage.restored_occurrences(), 3);
        assert_eq!(stage.output_tokens().len(), 1);
        drop(stage);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[test]
    fn native_shared_an_json_immutable_thinking_authorized_reference_cannot_hide_as_reasoning(
    ) -> TestResult {
        use crate::proxy::security::restoration_core::stage_native_json_with_lifecycle;
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let mut source = native_shared_an_leaf_message(&reference);
        source["content"][0]["thinking"] = json!(reference);
        let baseline = fixture.budget.used();
        let observed = native_shared_an_leaf_observation(&fixture, &source, false)?;
        assert!(
            stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt
            )
            .is_err(),
            "NATIVE_SHARED_AN_JSON_IMMUTABLE_REASONING_AUTHORIZED_REFERENCE_ACCEPTED"
        );
        drop(observed);
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[test]
    fn native_shared_an_json_delivered_nodes_must_be_complete_ordered_blocks() -> TestResult {
        use crate::proxy::security::{
            restoration_contract::TextSites,
            restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
        };
        let fixture = native_shared_an_fixture()?;
        let source = native_shared_an_message();
        let baseline = fixture.budget.used();
        for nodes in [
            vec![&source["content"][1]],
            vec![
                &source["content"][1],
                &source["content"][0],
                &source["content"][2],
            ],
            vec![
                &source["content"][0],
                &source["content"][1],
                &source["content"][1],
            ],
            vec![
                &source["content"][0],
                &source["content"][1],
                &source["content"][2]["input"],
            ],
        ] {
            let observed = required(
                NativeJsonObservation::new(
                    &source,
                    ClientProtocol::new(Protocol::Anthropic),
                    TextSites::new(fixture.budget.clone())?,
                    &nodes,
                    fixture.budget.clone(),
                ),
                "NATIVE_SHARED_AN_JSON_SNAPSHOT_OBSERVATION_REQUIRED",
            )?;
            assert!(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Anthropic, Protocol::Anthropic),
                    &fixture.attempt
                )
                .is_err(),
                "NATIVE_SHARED_AN_JSON_INEXACT_DELIVERED_BLOCKS_ACCEPTED"
            );
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_identity_controls_reject_before_poll_or_release() -> TestResult {
        let fixture = native_shared_an_fixture()?;
        let other = native_shared_an_fixture()?;
        let client = native_shared_an_message();
        let raw = serde_json::to_vec(&client)?;
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let baseline = fixture.budget.used();
        for (original, converted) in [
            (&wrong_attempt, &fixture.attempt),
            (&fixture.attempt, &wrong_attempt),
        ] {
            let (source, probe) = ScriptedStream::entity(&raw);
            assert!(native_shared_an_driver(&fixture)
                .consume_json(source, original, &client, converted)
                .await
                .is_err());
            assert_eq!(probe.snapshot().polls, 0);
            assert_eq!(probe.snapshot().drops, 1);
            assert_eq!(fixture.budget.used(), baseline);
        }
        for protocols in [
            pair(Protocol::Anthropic, Protocol::Chat),
            pair(Protocol::Chat, Protocol::Anthropic),
        ] {
            let (source, probe) = ScriptedStream::entity(&raw);
            let driver = NativeCompletionDriver::new(
                protocols,
                fixture.attempt.clone(),
                fixture.binding.clone(),
                fixture.budget.clone(),
            );
            assert!(matches!(
                driver
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                Err(RestoreError::RestoreUnsupported)
            ));
            assert_eq!(probe.snapshot().polls, 0);
        }
        let (source, probe) = ScriptedStream::entity(&raw);
        let gemini_driver = NativeCompletionDriver::new(
            pair(Protocol::Gemini, Protocol::Gemini),
            fixture.attempt.clone(),
            fixture.binding.clone(),
            fixture.budget.clone(),
        );
        assert!(matches!(
            gemini_driver
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await,
            Err(RestoreError::RestoreFailed)
        ));
        probe.assert_eof();
        for mode in 0..4 {
            let (source, probe) = ScriptedStream::entity(&raw);
            let proof = required(
                native_shared_an_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_AN_JSON_IDENTITY_DONOR_REQUIRED",
            )?;
            probe.assert_eof();
            let mut changed = client.clone();
            changed["extension"]["preserve"] = json!(false);
            let result = match mode {
                0 => proof.validate(
                    &client,
                    &other.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                ),
                1 => proof.validate(
                    &client,
                    &fixture.binding,
                    &wrong_attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                ),
                2 => proof.validate(&client, &fixture.binding, &fixture.attempt, chat()),
                _ => proof.validate(
                    &changed,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                ),
            };
            assert!(
                result.is_err(),
                "NATIVE_SHARED_AN_JSON_FOREIGN_PROOF_ACCEPTED"
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        let mut changed = client.clone();
        changed["extension"]["preserve"] = json!(false);
        let (source, probe) = ScriptedStream::entity(&raw);
        assert!(native_shared_an_driver(&fixture)
            .consume_json(source, &fixture.attempt, &changed, &fixture.attempt)
            .await
            .is_err());
        probe.assert_eof();
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_strict_completion_controls_recheck_all_three_views() -> TestResult
    {
        let fixture = native_shared_an_fixture()?;
        let valid = native_shared_an_message();
        let raw = serde_json::to_vec(&valid)?;
        let baseline = fixture.budget.used();
        for mode in 0..15 {
            let mut invalid = valid.clone();
            match mode {
                0 => invalid["stop_reason"] = Value::Null,
                1 => invalid["stop_reason"] = json!("max_tokens"),
                2 => invalid["stop_reason"] = json!("synthetic_unknown_stop"),
                3 => invalid["stop_reason"] = json!("end_turn"),
                4 => invalid["content"][2]["input"] = json!([]),
                5 => invalid["content"][2]["input"] = json!("{}"),
                6 => invalid["content"][2]["name"] = json!(""),
                7 => invalid["content"][2]["id"] = json!(""),
                8 => {
                    let duplicate = invalid["content"][2].clone();
                    invalid["content"]
                        .as_array_mut()
                        .ok_or("NATIVE_SHARED_AN_CONTENT_MISSING")?
                        .push(duplicate);
                }
                9 => invalid["content"][1]["text"] = json!(17),
                10 => invalid["role"] = json!("user"),
                11 => invalid["error"] = json!({"type":"synthetic_error"}),
                12 => invalid["id"] = json!(""),
                13 => invalid["content"][2]["type"] = json!("synthetic_unsupported_carrier"),
                _ => {
                    invalid["content"][2]["name"] = json!("Read");
                    invalid["content"][2]["input"]["pages"] = json!("");
                }
            }
            let encoded = serde_json::to_vec(&invalid)?;
            for original_invalid in [true, false] {
                let (source, probe) =
                    ScriptedStream::entity(if original_invalid { &encoded } else { &raw });
                let result = native_shared_an_driver(&fixture)
                    .consume_json(
                        source,
                        &fixture.attempt,
                        if original_invalid { &valid } else { &invalid },
                        &fixture.attempt,
                    )
                    .await;
                assert!(
                    result.is_err(),
                    "NATIVE_SHARED_AN_JSON_INCOMPLETE_OR_INVALID_PROVED"
                );
                probe.assert_eof();
                assert_eq!(fixture.budget.used(), baseline);
            }
            let (source, probe) = ScriptedStream::entity(&raw);
            let proof = required(
                native_shared_an_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &valid, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_AN_JSON_FINAL_CONTROL_DONOR_REQUIRED",
            )?;
            probe.assert_eof();
            assert!(proof
                .validate_restored(
                    &valid,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Anthropic),
                    &encoded,
                    &fixture.budget
                )
                .is_err());
            assert_eq!(fixture.budget.used(), baseline);
        }
        let duplicate = String::from_utf8(raw.clone())?.replacen(
            "\"role\":\"assistant\"",
            "\"role\":\"assistant\",\"role\":\"assistant\"",
            1,
        );
        assert!(
            duplicate.len() > raw.len(),
            "NATIVE_SHARED_AN_JSON_DUPLICATE_FIXTURE_MISSING"
        );
        for bytes in [
            Vec::new(),
            raw[..raw.len() - 1].to_vec(),
            duplicate.into_bytes(),
            vec![0xf0, 0x9f, 0xa6],
        ] {
            let (source, probe) = ScriptedStream::entity(&bytes);
            assert!(native_shared_an_driver(&fixture)
                .consume_json(source, &fixture.attempt, &valid, &fixture.attempt)
                .await
                .is_err());
            probe.assert_eof();
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_budget_controls_refund_each_retained_owner() -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::stage_native_json_with_lifecycle,
        };
        let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
        let source_value = native_shared_an_leaf_message(&reference);
        let raw = serde_json::to_vec(&source_value)?;
        let baseline = fixture.budget.used();
        let saturation = fixture.budget.reserve(fixture.budget.limit() - baseline)?;
        let (source, probe) = ScriptedStream::entity(&raw);
        assert!(native_shared_an_driver(&fixture)
            .consume_json(source, &fixture.attempt, &source_value, &fixture.attempt)
            .await
            .is_err());
        // An empty collector reserves zero; the first chunk triggers the denied charge.
        assert_eq!(probe.snapshot().polls, 1);
        assert_eq!(probe.snapshot().chunks, 1);
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(probe.snapshot().drops, 1);
        drop(saturation);
        assert_eq!(fixture.budget.used(), baseline);
        let observed = native_shared_an_leaf_observation(&fixture, &source_value, false)?;
        let observed_charge = fixture.budget.used();
        let saturation = fixture
            .budget
            .reserve(fixture.budget.limit() - observed_charge)?;
        assert!(stage_native_json_with_lifecycle(
            &observed,
            &fixture.lease,
            &fixture.binding,
            pair(Protocol::Anthropic, Protocol::Anthropic),
            &fixture.attempt
        )
        .is_err());
        drop(saturation);
        assert_eq!(fixture.budget.used(), observed_charge);
        let (source, probe) = ScriptedStream::entity(&raw);
        let proof = required(
            native_shared_an_driver(&fixture)
                .consume_json(source, &fixture.attempt, &source_value, &fixture.attempt)
                .await,
            "NATIVE_SHARED_AN_JSON_BUDGET_DONOR_REQUIRED",
        )?;
        probe.assert_eof();
        let stage = required(
            stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Anthropic, Protocol::Anthropic),
                &fixture.attempt,
            ),
            "NATIVE_SHARED_AN_JSON_BUDGET_STAGE_REQUIRED",
        )?;
        let saturation = fixture
            .budget
            .reserve(fixture.budget.limit() - fixture.budget.used())?;
        assert!(finalize_native_json(
            stage,
            &fixture.lease,
            proof,
            &fixture.attempt,
            Bytes::from(raw),
            NATIVE_DRIVER_ENTITY_LIMIT
        )
        .is_err());
        drop(saturation);
        assert_eq!(fixture.budget.used(), observed_charge);
        drop(observed);
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_an_json_finalizer_controls_fail_atomically_without_publication(
    ) -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::stage_native_json_with_lifecycle,
        };
        for mode in 0..5 {
            let (fixture, reference) = native_shared_an_mapped_fixture(true)?;
            let other = native_shared_an_fixture()?;
            let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
            let source_value = native_shared_an_leaf_message(&reference);
            let mut foreign = source_value.clone();
            foreign["extension"]["preserve"] = json!(false);
            let proof_value = if mode == 4 { &foreign } else { &source_value };
            let proof_fixture = if mode == 3 { &other } else { &fixture };
            let (source, probe) = ScriptedStream::entity(&serde_json::to_vec(proof_value)?);
            let proof = required(
                native_shared_an_driver(proof_fixture)
                    .consume_json(
                        source,
                        &proof_fixture.attempt,
                        proof_value,
                        &proof_fixture.attempt,
                    )
                    .await,
                "NATIVE_SHARED_AN_JSON_FINALIZER_CONTROL_DONOR_REQUIRED",
            )?;
            probe.assert_eof();
            let baseline = fixture.budget.used();
            let observed = native_shared_an_leaf_observation(&fixture, &source_value, false)?;
            let stage = required(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Anthropic, Protocol::Anthropic),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_AN_JSON_FINALIZER_CONTROL_STAGE_REQUIRED",
            )?;
            let wire = serde_json::to_vec(if mode == 1 { &foreign } else { &source_value })?;
            assert!(
                finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    if mode == 0 {
                        &wrong_attempt
                    } else {
                        &fixture.attempt
                    },
                    Bytes::from(wire),
                    if mode == 2 {
                        1
                    } else {
                        NATIVE_DRIVER_ENTITY_LIMIT
                    }
                )
                .is_err(),
                "NATIVE_SHARED_AN_JSON_INVALID_FINALIZATION_ACCEPTED"
            );
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    // Native Gemini JSON: all observed candidates require a structural STOP and
    // actual entity EOF. Native Content/Part values remain typed and opaque.
    fn native_shared_ge_fixture() -> TestResult<Fixture> {
        let store = DlpTokenStore::new();
        let lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Gemini),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-ge-shared-app",
            Some("synthetic-ge-shared-session"),
            1,
        )?;
        let binding = lease.bind_actual_attempt(
            "synthetic-ge-shared-provider",
            "gemini",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge-shared-principal"),
        )?;
        let budget = PayloadBudget::with_limit(4 * NATIVE_DRIVER_ENTITY_LIMIT);
        let attempt = AttemptProvenance::new(&budget)?;
        Ok(Fixture {
            store,
            lease,
            binding,
            attempt,
            budget,
        })
    }

    fn native_shared_ge_driver(fixture: &Fixture) -> NativeCompletionDriver {
        NativeCompletionDriver::new(
            pair(Protocol::Gemini, Protocol::Gemini),
            fixture.attempt.clone(),
            fixture.binding.clone(),
            fixture.budget.clone(),
        )
    }

    fn native_shared_ge_response() -> Value {
        json!({
            "candidates":[
                {"index":0,"content":{"role":"model","parts":[
                    {"text":"synthetic public 雪 text"},
                    {"functionCall":{"id":"call_ge_shared","name":"synthetic_tool","args":{
                        "path":"ordinary string leaf","nested":[7,true,null]
                    }}},
                    {"text":"synthetic signed thought","thought":true,
                        "thoughtSignature":"synthetic-opaque-signature"}
                ]},"finishReason":"STOP"},
                {"index":1,"content":{"role":"model","parts":[
                    {"text":"synthetic second candidate"}
                ]},"finishReason":"STOP"}
            ],
            "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":2,"totalTokenCount":3},
            "responseId":"synthetic-ge-response",
            "extension":{"preserve":true}
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_real_eof_split_and_final_wire() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let client = native_shared_ge_response();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (source, probe) = ScriptedStream::split(&raw, at);
            let proof = required(
                native_shared_ge_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_JSON_REAL_EOF_PROOF_REQUIRED",
            )?;
            probe.assert_eof();
            required(
                proof.validate_restored(
                    &client,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Gemini),
                    &raw,
                    &fixture.budget,
                ),
                "NATIVE_SHARED_GE_JSON_STRICT_FINAL_WIRE_REQUIRED",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        assert_eq!(fixture.store.stats()?.entries, 0);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_strict_candidate_completion_and_final_wire() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let valid = native_shared_ge_response();
        let raw = serde_json::to_vec(&valid)?;
        let baseline = fixture.budget.used();
        for mode in 0..14 {
            let mut invalid = valid.clone();
            match mode {
                0 => {
                    invalid["candidates"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("finishReason");
                }
                1 => invalid["candidates"][0]["finishReason"] = json!("MAX_TOKENS"),
                2 => invalid["candidates"][0]["finishReason"] = json!("SAFETY"),
                3 => invalid["candidates"][0]["finishReason"] = json!("UNKNOWN_REASON"),
                4 => {
                    invalid["candidates"][1]
                        .as_object_mut()
                        .unwrap()
                        .remove("finishReason");
                }
                5 => invalid["promptFeedback"] = json!({"blockReason":"SAFETY"}),
                6 => invalid["error"] = json!({"code":500,"message":"synthetic"}),
                7 => invalid["candidates"] = json!([]),
                8 => invalid["candidates"][0]["content"]["parts"] = json!({}),
                9 => {
                    invalid["candidates"][0]["content"]["parts"][1]["functionCall"]["args"] =
                        json!("not an object")
                }
                10 => invalid["candidates"][0]["content"]["parts"][0]["text"] = json!(7),
                11 => {
                    invalid["candidates"][0]["content"]["parts"][2]["thoughtSignature"] = json!(7)
                }
                12 => invalid["candidates"][0]["finishReason"] = json!(7),
                _ => {
                    invalid["candidates"][0]["content"]["parts"][1]["functionCall"]
                        .as_object_mut()
                        .unwrap()
                        .remove("name");
                }
            };
            let (source, probe) = ScriptedStream::entity(&serde_json::to_vec(&invalid)?);
            assert!(
                native_shared_ge_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &invalid, &fixture.attempt)
                    .await
                    .is_err(),
                "NATIVE_SHARED_GE_JSON_INVALID_COMPLETION_ACCEPTED mode={mode}"
            );
            probe.assert_eof();
            let (valid_original, valid_probe) = ScriptedStream::entity(&raw);
            assert!(
                native_shared_ge_driver(&fixture)
                    .consume_json(valid_original, &fixture.attempt, &invalid, &fixture.attempt,)
                    .await
                    .is_err(),
                "NATIVE_SHARED_GE_JSON_INVALID_CLIENT_ACCEPTED mode={mode}"
            );
            valid_probe.assert_eof();
            assert_eq!(fixture.budget.used(), baseline);
        }
        let mut invalid_args = valid.clone();
        invalid_args["candidates"][0]["content"]["parts"][1]["functionCall"]["args"] =
            json!("not an object");
        let mut truncated = valid.clone();
        truncated["candidates"][0]["finishReason"] = json!("MAX_TOKENS");
        let mut blocked = valid.clone();
        blocked["promptFeedback"] = json!({"blockReason":"SAFETY"});
        let invalid_wires = vec![
            serde_json::to_vec(&invalid_args)?,
            serde_json::to_vec(&truncated)?,
            serde_json::to_vec(&blocked)?,
            b"{".to_vec(),
            vec![0xff],
        ];
        for invalid_wire in invalid_wires {
            let (source, probe) = ScriptedStream::entity(&raw);
            let proof = required(
                native_shared_ge_driver(&fixture)
                    .consume_json(source, &fixture.attempt, &valid, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_JSON_COMPLETION_DONOR_REQUIRED",
            )?;
            probe.assert_eof();
            assert!(
                proof
                    .validate_restored(
                        &valid,
                        &fixture.binding,
                        &fixture.attempt,
                        ClientProtocol::new(Protocol::Gemini),
                        &invalid_wire,
                        &fixture.budget,
                    )
                    .is_err(),
                "NATIVE_SHARED_GE_JSON_FINAL_NATIVE_OBJECT_NOT_RECHECKED"
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_pending_cancel_transport_and_identity() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let other = native_shared_ge_fixture()?;
        let client = native_shared_ge_response();
        let raw = serde_json::to_vec(&client)?;
        let baseline = fixture.budget.used();
        let (source, probe, gate) = ScriptedStream::before_eof(&raw);
        let mut future = Box::pin(native_shared_ge_driver(&fixture).consume_json(
            source,
            &fixture.attempt,
            &client,
            &fixture.attempt,
        ));
        assert!(
            poll_once(future.as_mut()).is_pending(),
            "NATIVE_SHARED_GE_JSON_PENDING_NOT_EOF"
        );
        drop(future);
        gate.release();
        assert_eq!(probe.snapshot().eofs, 0);
        assert_eq!(probe.snapshot().drops, 1);
        assert_eq!(fixture.budget.used(), baseline);
        for kind in [io::ErrorKind::UnexpectedEof, io::ErrorKind::Interrupted] {
            let (source, probe) = ScriptedStream::terminal_error(&raw, kind);
            assert!(native_shared_ge_driver(&fixture)
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await
                .is_err());
            probe.assert_error();
            assert_eq!(fixture.budget.used(), baseline);
        }
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let (source, original) = ScriptedStream::entity(&raw);
        let wrong_driver = NativeCompletionDriver::new(
            pair(Protocol::Gemini, Protocol::Gemini),
            wrong_attempt,
            fixture.binding.clone(),
            fixture.budget.clone(),
        );
        assert!(wrong_driver
            .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
            .await
            .is_err());
        assert_eq!(original.snapshot().polls, 0);
        assert_eq!(fixture.budget.used(), baseline);

        let (source, original) = ScriptedStream::entity(&raw);
        let foreign_driver = NativeCompletionDriver::new(
            pair(Protocol::Gemini, Protocol::Gemini),
            fixture.attempt.clone(),
            other.binding.clone(),
            fixture.budget.clone(),
        );
        let proof = required(
            foreign_driver
                .consume_json(source, &fixture.attempt, &client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_GE_JSON_BINDING_DONOR_REQUIRED",
        )?;
        original.assert_eof();
        assert!(proof
            .validate_restored(
                &client,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Gemini),
                &raw,
                &fixture.budget,
            )
            .is_err());
        assert_eq!(fixture.budget.used(), baseline);
        // S4 approves native Gemini SSE. Non-SSE JSON reaches actual original EOF,
        // then fails native framing without polling the independent client stream.
        let (original, original_probe) = ScriptedStream::entity(&raw);
        let (client_stream, client_probe) = ScriptedStream::entity(&raw);
        assert!(matches!(
            native_shared_ge_driver(&fixture)
                .consume_sse(original, &fixture.attempt, client_stream, &fixture.attempt,)
                .await,
            Err(RestoreError::RestoreFailed)
        ));
        original_probe.assert_eof();
        assert_eq!(client_probe.snapshot().polls, 0);
        Ok(())
    }

    const NATIVE_SHARED_GE_TEXT: &str = "Synthetic Gemini native restored text";

    fn native_shared_ge_mapped_fixture(mapped: bool) -> TestResult<(Fixture, String)> {
        use crate::proxy::security::tokenization::{TokenCategory, TokenFragment};
        let mut fixture = native_shared_ge_fixture()?;
        let reference = if mapped {
            fixture.lease.prepare(
                &[TokenFragment {
                    raw: NATIVE_SHARED_GE_TEXT,
                    whole_span_categories: &[TokenCategory::Phone],
                }],
                &[],
                |tokens| Ok(tokens[0].clone()),
            )?
        } else {
            "ordinary Gemini native string 雪".to_owned()
        };
        fixture.binding = fixture.lease.bind_actual_attempt(
            "synthetic-ge-shared-provider",
            "gemini",
            "https://synthetic.invalid/v1beta/models/synthetic:generateContent",
            Some("synthetic-ge-shared-principal"),
        )?;
        Ok((fixture, reference))
    }

    fn native_shared_ge_leaf_response(reference: &str) -> Value {
        let mut source = native_shared_ge_response();
        source["candidates"][0]["content"]["parts"][0]["text"] = json!(reference);
        source["candidates"][0]["content"]["parts"][1]["functionCall"]["args"] = json!({
            "path":reference,
            "nested":[7,true,null,{"value":reference}],
            "json_looking":"{not a JSON object string}"
        });
        source["extension"]["opaque_reference"] = json!(reference);
        source
    }

    fn native_shared_ge_observation_with_nodes<'a>(
        fixture: &Fixture,
        source: &'a Value,
        delivered: &[&'a Value],
        signed_editable: bool,
    ) -> TestResult<crate::proxy::security::restoration_core::NativeJsonObservation<'a>> {
        use crate::proxy::security::{
            restoration_contract::{EditConstraint, SourceClass, TextPolicy, TextSites},
            restoration_core::NativeJsonObservation,
        };
        let first = &source["candidates"][0]["content"]["parts"];
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            &first[0]["text"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        for node in [
            &first[1]["functionCall"]["args"]["path"],
            &first[1]["functionCall"]["args"]["nested"][3]["value"],
            &first[1]["functionCall"]["args"]["json_looking"],
        ] {
            sites.observe(node, SourceClass::ToolArguments, EditConstraint::Editable)?;
        }
        sites.observe(
            &first[2]["text"],
            SourceClass::HistoricalText,
            if signed_editable {
                EditConstraint::Editable
            } else {
                EditConstraint::Immutable
            },
        )?;
        sites.observe(
            &source["candidates"][1]["content"]["parts"][0]["text"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        required(
            NativeJsonObservation::new_with_policies(
                source,
                ClientProtocol::new(Protocol::Gemini),
                sites,
                delivered,
                &[
                    TextPolicy::Output,
                    TextPolicy::ToolArguments,
                    TextPolicy::ToolArguments,
                    TextPolicy::ToolArguments,
                    TextPolicy::Reasoning,
                    TextPolicy::Output,
                ],
                fixture.budget.clone(),
            ),
            "NATIVE_SHARED_GE_JSON_OBSERVATION_REQUIRED",
        )
    }

    fn native_shared_ge_observation<'a>(
        fixture: &Fixture,
        source: &'a Value,
        signed_editable: bool,
    ) -> TestResult<crate::proxy::security::restoration_core::NativeJsonObservation<'a>> {
        native_shared_ge_observation_with_nodes(
            fixture,
            source,
            &[
                &source["candidates"][0]["content"],
                &source["candidates"][1]["content"],
            ],
            signed_editable,
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_object_leaves_single_lifecycle_and_sealed_delivery() -> TestResult
    {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::{stage_native_json_with_lifecycle, ResponseLifecycle},
        };
        for mapped in [false, true] {
            let (fixture, reference) = native_shared_ge_mapped_fixture(mapped)?;
            let source = native_shared_ge_leaf_response(&reference);
            let original = source.clone();
            let baseline = fixture.budget.used();
            let wire = serde_json::to_vec_pretty(&source)?;
            let (transport, probe) = ScriptedStream::split(&wire, wire.len() / 2);
            let proof = required(
                native_shared_ge_driver(&fixture)
                    .consume_json(transport, &fixture.attempt, &source, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_JSON_LEAF_HTTP_PROOF_REQUIRED",
            )?;
            probe.assert_eof();
            let observed = native_shared_ge_observation(&fixture, &source, false)?;
            let begins = ResponseLifecycle::begin_count_for_test();
            let stage = required(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Gemini, Protocol::Gemini),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_GE_JSON_NATIVE_OBJECT_LIFECYCLE_REQUIRED",
            )?;
            assert_eq!(ResponseLifecycle::begin_count_for_test(), begins + 1);
            assert_eq!(stage.output_tokens().len(), usize::from(mapped));
            assert_eq!(stage.restored_occurrences(), 3 * usize::from(mapped));
            assert_eq!(stage.changed(), mapped);
            let expected_text = if mapped {
                NATIVE_SHARED_GE_TEXT
            } else {
                &reference
            };
            let mut expected = source.clone();
            expected["candidates"][0]["content"]["parts"][0]["text"] = json!(expected_text);
            expected["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]["path"] =
                json!(expected_text);
            expected["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]["nested"][3]
                ["value"] = json!(expected_text);
            let delivered = json!([
                expected["candidates"][0]["content"],
                expected["candidates"][1]["content"]
            ]);
            assert!(
                stage.as_value() == &expected,
                "NATIVE_SHARED_GE_JSON_TYPES_CHANGED"
            );
            assert!(
                stage.delivered() == &delivered,
                "NATIVE_SHARED_GE_JSON_FULL_CONTENT_REQUIRED"
            );
            assert!(source == original, "NATIVE_SHARED_GE_JSON_MUTATED_SOURCE");
            let finalized = required(
                finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    Bytes::from(wire.clone()),
                    NATIVE_DRIVER_ENTITY_LIMIT,
                ),
                "NATIVE_SHARED_GE_JSON_SEALED_DELIVERY_REQUIRED",
            )?;
            assert!(fixture
                .lease
                .verify_native_delivered(
                    &fixture.binding,
                    ClientProtocol::new(Protocol::Gemini),
                    &delivered,
                )
                .is_ok());
            let mut stream = finalized.into_delivery_stream();
            let body = stream
                .next()
                .await
                .ok_or("NATIVE_SHARED_GE_JSON_MISSING_BODY")??;
            assert!(serde_json::from_slice::<Value>(&body)? == expected);
            if !mapped {
                assert!(body.as_ref() == wire.as_slice());
            }
            assert!(stream.next().await.is_none());
            drop(body);
            drop(stream);
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_json_signed_parts_and_complete_contents_are_mandatory() -> TestResult {
        use crate::proxy::security::{
            restoration_contract::{EditConstraint, SourceClass, TextSites},
            restoration_core::{
                stage_native_json, stage_native_json_with_lifecycle, NativeJsonObservation,
            },
        };
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let mut source = native_shared_ge_leaf_response(&reference);
        source["candidates"][0]["content"]["parts"][2]["text"] = json!(reference);
        let baseline = fixture.budget.used();
        for editable in [false, true] {
            let observed = native_shared_ge_observation(&fixture, &source, editable)?;
            rejected(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Gemini, Protocol::Gemini),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_GE_JSON_SIGNED_PART_RESTORE_ACCEPTED",
            );
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
        }
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            &source["candidates"][0]["content"]["parts"][2]["text"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        let legacy = required(
            NativeJsonObservation::new(
                &source,
                ClientProtocol::new(Protocol::Gemini),
                sites,
                &[
                    &source["candidates"][0]["content"],
                    &source["candidates"][1]["content"],
                ],
                fixture.budget.clone(),
            ),
            "NATIVE_SHARED_GE_JSON_LEGACY_OBSERVATION_REQUIRED",
        )?;
        rejected(
            stage_native_json(&legacy, &fixture.lease, &fixture.binding),
            "NATIVE_SHARED_GE_JSON_LEGACY_SIGNED_BYPASS",
        );
        drop(legacy);
        assert_eq!(fixture.budget.used(), baseline);

        let mut signed_call = native_shared_ge_leaf_response(&reference);
        signed_call["candidates"][0]["content"]["parts"][1]["thoughtSignature"] =
            json!("synthetic-call-signature");
        let observed = native_shared_ge_observation(&fixture, &signed_call, false)?;
        rejected(
            stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Gemini, Protocol::Gemini),
                &fixture.attempt,
            ),
            "NATIVE_SHARED_GE_JSON_SIGNED_ARGUMENT_RESTORE_ACCEPTED",
        );
        drop(observed);
        assert_eq!(fixture.budget.used(), baseline);

        let fixture = native_shared_ge_fixture()?;
        let source = native_shared_ge_leaf_response("ordinary synthetic text");
        let baseline = fixture.budget.used();
        let foreign = source.clone();
        for nodes in [
            vec![],
            vec![&source["candidates"][0]["content"]],
            vec![
                &source["candidates"][1]["content"],
                &source["candidates"][0]["content"],
            ],
            vec![
                &source["candidates"][0]["content"],
                &source["candidates"][0]["content"],
            ],
            vec![&source["candidates"][0], &source["candidates"][1]],
            vec![
                &source["candidates"][0]["content"],
                &foreign["candidates"][1]["content"],
            ],
        ] {
            let observed =
                native_shared_ge_observation_with_nodes(&fixture, &source, &nodes, false);
            let accepted = match observed {
                Ok(observed) => stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Gemini, Protocol::Gemini),
                    &fixture.attempt,
                )
                .is_ok(),
                Err(_) => false,
            };
            assert!(
                !accepted,
                "NATIVE_SHARED_GE_JSON_INCOMPLETE_CONTENT_ACCEPTED"
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_json_lifecycle_faults_are_atomic_and_refunded() -> TestResult {
        use crate::proxy::security::restoration_core::{
            stage_native_json_with_lifecycle, LifecycleOperation, ResponseLifecycle,
        };
        for operation in [
            LifecycleOperation::Begin,
            LifecycleOperation::OpenText,
            LifecycleOperation::Done,
            LifecycleOperation::Hold,
            LifecycleOperation::Finish,
        ] {
            let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
            let source = native_shared_ge_leaf_response(&reference);
            let baseline = fixture.budget.used();
            let observed = native_shared_ge_observation(&fixture, &source, false)?;
            let guard = ResponseLifecycle::reject_operation_for_test(operation);
            rejected(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Gemini, Protocol::Gemini),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_GE_JSON_LIFECYCLE_FAULT_ACCEPTED",
            );
            drop(guard);
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_finalizer_identity_and_wire_faults_refund() -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::stage_native_json_with_lifecycle,
        };
        for mode in 0..5 {
            let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
            let other = native_shared_ge_fixture()?;
            let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
            let source = native_shared_ge_leaf_response(&reference);
            let mut foreign = source.clone();
            foreign["extension"]["preserve"] = json!(false);
            let proof_value = if mode == 2 { &foreign } else { &source };
            let proof_fixture = if mode == 1 { &other } else { &fixture };
            let (transport, probe) = ScriptedStream::entity(&serde_json::to_vec(proof_value)?);
            let proof = required(
                native_shared_ge_driver(proof_fixture)
                    .consume_json(
                        transport,
                        &proof_fixture.attempt,
                        proof_value,
                        &proof_fixture.attempt,
                    )
                    .await,
                "NATIVE_SHARED_GE_JSON_FINALIZER_DONOR_REQUIRED",
            )?;
            probe.assert_eof();
            let baseline = fixture.budget.used();
            let observed = native_shared_ge_observation(&fixture, &source, false)?;
            let stage = required(
                stage_native_json_with_lifecycle(
                    &observed,
                    &fixture.lease,
                    &fixture.binding,
                    pair(Protocol::Gemini, Protocol::Gemini),
                    &fixture.attempt,
                ),
                "NATIVE_SHARED_GE_JSON_FINALIZER_STAGE_REQUIRED",
            )?;
            let original = if mode == 4 { &foreign } else { &source };
            assert!(
                finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    if mode == 0 {
                        &wrong_attempt
                    } else {
                        &fixture.attempt
                    },
                    Bytes::from(serde_json::to_vec(original)?),
                    if mode == 3 {
                        1
                    } else {
                        NATIVE_DRIVER_ENTITY_LIMIT
                    },
                )
                .is_err(),
                "NATIVE_SHARED_GE_JSON_INVALID_FINALIZATION_ACCEPTED mode={mode}"
            );
            drop(observed);
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_json_successful_stage_drop_refunds_without_publication() -> TestResult
    {
        use crate::proxy::security::restoration_core::stage_native_json_with_lifecycle;
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let source = native_shared_ge_leaf_response(&reference);
        let wire = serde_json::to_vec(&source)?;
        let (transport, probe) = ScriptedStream::entity(&wire);
        let proof = required(
            native_shared_ge_driver(&fixture)
                .consume_json(transport, &fixture.attempt, &source, &fixture.attempt)
                .await,
            "NATIVE_SHARED_GE_JSON_DROP_DONOR_REQUIRED",
        )?;
        probe.assert_eof();
        let baseline = fixture.budget.used();
        let observed = native_shared_ge_observation(&fixture, &source, false)?;
        let stage = required(
            stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair(Protocol::Gemini, Protocol::Gemini),
                &fixture.attempt,
            ),
            "NATIVE_SHARED_GE_JSON_DROP_STAGE_REQUIRED",
        )?;
        drop(stage);
        drop(proof);
        drop(observed);
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    // S4 source-linked state table (protocol-evidence-01 + Vesper scoped PASS):
    // G0 start: no candidate/Content/completion is inferred.
    // G1 candidate: explicit unique Candidate.index only; absent/duplicate identity rejects.
    // G2 later Content: append separate whole Content observations in arrival order;
    //   never infer cross-frame Part identity or merge Parts by repeated array position.
    // G3 usage-only: no candidates/Content means no Content projection and no terminal.
    //   Present-but-empty or non-array candidates are malformed, not usage-only
    //   (protocol-evidence-01 G3 accepts only frames without a candidates field).
    // G4 signed Part: thoughtSignature makes its own Part immutable; never merge
    //   signed/unsigned Parts or two signed Parts.
    // G5 late standalone signature: an empty-text signed Part in later Content remains
    //   a distinct immutable Part; retroactive attachment to an earlier Part is unsupported.
    // G6 terminal: every observed candidate requires one consistent STOP; later Content
    //   or contradictory reasons reject. Post-terminal usage-only metadata is allowed.
    // G7 invalid: MAX_TOKENS/blocked/error/unknown/malformed grants no completed proof.
    // G8 transport: bounded data-only SSE plus actual async EOF; Pending/error/cancel
    //   are not EOF and no synthetic [DONE] exists. Ambiguous Part semantics stay held.
    fn native_shared_ge_sse_events() -> Vec<Value> {
        vec![
            json!({"candidates":[{"index":0,"content":{"role":"model","parts":[
                {"text":"synthetic first 雪 text"},
                {"functionCall":{"id":"call_ge_sse","name":"synthetic_tool","args":{
                    "path":"ordinary argument","nested":[7,true,null]
                }}}
            ]}}]}),
            json!({"candidates":[{"index":1,"content":{"role":"model","parts":[
                {"text":"synthetic second candidate"}
            ]}}]}),
            json!({"usageMetadata":{"promptTokenCount":1,"totalTokenCount":1}}),
            json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            json!({"candidates":[{"index":1,"content":{"role":"model","parts":[
                {"text":"","thoughtSignature":"synthetic-late-signature"}
            ]},"finishReason":"STOP"}]}),
            json!({"usageMetadata":{"candidatesTokenCount":2,"totalTokenCount":3},
                "responseId":"synthetic-ge-sse-response","extension":{"preserve":true}}),
        ]
    }

    fn native_shared_ge_sse_encode(events: &[Value]) -> TestResult<Vec<u8>> {
        let mut raw = Vec::new();
        for event in events {
            raw.extend_from_slice(b"data: ");
            raw.extend_from_slice(&serde_json::to_vec(event)?);
            raw.extend_from_slice(b"\r\n\r\n");
        }
        Ok(raw)
    }

    async fn native_shared_ge_sse_proof(
        fixture: &Fixture,
        raw: &[u8],
    ) -> TestResult<VerifiedNativeSse> {
        let (original, original_probe) = ScriptedStream::split(raw, raw.len() / 2);
        let (client, client_probe) = ScriptedStream::split(raw, raw.len() / 3);
        let proof = required(
            native_shared_ge_driver(fixture)
                .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                .await,
            "NATIVE_SHARED_GE_SSE_PROOF_REQUIRED",
        )?;
        original_probe.assert_eof();
        client_probe.assert_eof();
        Ok(proof)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_real_eof_and_candidate_terminals() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let raw = native_shared_ge_sse_encode(&native_shared_ge_sse_events())?;
        let baseline = fixture.budget.used();
        for at in 0..=raw.len() {
            let (original, original_probe) = ScriptedStream::split(&raw, at);
            let (client, client_probe) = ScriptedStream::split(&raw, raw.len() - at);
            let proof = required(
                native_shared_ge_driver(&fixture)
                    .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_SSE_REAL_EOF_PROOF_REQUIRED",
            )?;
            original_probe.assert_eof();
            client_probe.assert_eof();
            required(
                proof.validate_restored(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Gemini),
                    io::Cursor::new(&raw),
                    &fixture.budget,
                ),
                "NATIVE_SHARED_GE_SSE_FINAL_WIRE_REQUIRED",
            )?;
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_strict_original_client_and_final_wire() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let events = native_shared_ge_sse_events();
        let raw = native_shared_ge_sse_encode(&events)?;
        drop(native_shared_ge_sse_proof(&fixture, &raw).await?);
        let baseline = fixture.budget.used();
        for mode in 0..18 {
            let mut bad = events.clone();
            match mode {
                0 => {
                    bad.remove(4);
                }
                1 => bad[3]["candidates"][0]["finishReason"] = json!("MAX_TOKENS"),
                2 => bad.push(json!({"candidates":[{"index":0,"content":{
                    "role":"model","parts":[{"text":"post-terminal contradiction"}]
                }}]})),
                3 => {
                    bad[0]["candidates"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("index");
                }
                4 => bad[0]["candidates"].as_array_mut().unwrap().push(
                    json!({"index":0,"content":{"role":"model","parts":[{"text":"duplicate"}]}}),
                ),
                5 => bad.push(json!({"candidates":[{"index":0,"finishReason":"MAX_TOKENS"}]})),
                6 => bad[0]["candidates"][0]["content"]["parts"][0] = json!({"text":7}),
                7 => bad.push(json!({"error":{"code":500,"message":"synthetic"}})),
                8 => bad.push(json!({"promptFeedback":{"blockReason":"SAFETY"}})),
                9 => bad[4]["candidates"][0]["finishReason"] = json!("UNKNOWN_REASON"),
                10 => bad.push(json!({"candidates":[{"index":1,"finishReason":"STOP"}]})),
                11 => bad[1]["candidates"][0]["content"]["parts"] = json!([{}]),
                12 => bad[3]["candidates"][0]["index"] = json!("0"),
                // Malformed present-but-non-array candidates with valid usage
                // metadata must reject as malformed, not pass as usage-only.
                13 => bad.push(json!({"candidates":null,"usageMetadata":{"totalTokenCount":1}})),
                14 => bad.push(json!({"candidates":{},"usageMetadata":{"totalTokenCount":1}})),
                15 => bad
                    .push(json!({"candidates":"synthetic","usageMetadata":{"totalTokenCount":1}})),
                16 => bad.push(json!({"candidates":7,"usageMetadata":{"totalTokenCount":1}})),
                17 => bad.push(json!({"candidates":[],"usageMetadata":{"totalTokenCount":1}})),
                _ => unreachable!("invalid Gemini SSE fixture mode"),
            }
            let mut invalid = native_shared_ge_sse_encode(&bad)?;
            if mode == 10 {
                invalid.extend_from_slice(b"data: [DONE]\r\n\r\n");
            }
            let (original, original_probe) = ScriptedStream::entity(&invalid);
            let (client, client_probe) = ScriptedStream::entity(&raw);
            rejected(
                native_shared_ge_driver(&fixture)
                    .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_SSE_INVALID_ORIGINAL_ACCEPTED",
            );
            original_probe.assert_eof();
            assert_eq!(client_probe.snapshot().polls, 0);
            let (original, original_probe) = ScriptedStream::entity(&raw);
            let (client, client_probe) = ScriptedStream::entity(&invalid);
            rejected(
                native_shared_ge_driver(&fixture)
                    .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_SSE_INVALID_CLIENT_ACCEPTED",
            );
            original_probe.assert_eof();
            client_probe.assert_eof();
            let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
            rejected(
                proof.validate_restored(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Gemini),
                    io::Cursor::new(&invalid),
                    &fixture.budget,
                ),
                "NATIVE_SHARED_GE_SSE_INVALID_FINAL_WIRE_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        for invalid in [
            Vec::new(),
            native_shared_ge_sse_encode(&[json!({"usageMetadata":{"totalTokenCount":1}})])?,
            b"data: {}\r\n".to_vec(),
        ] {
            let (original, _) = ScriptedStream::entity(&invalid);
            let (client, _) = ScriptedStream::entity(&invalid);
            rejected(
                native_shared_ge_driver(&fixture)
                    .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                    .await,
                "NATIVE_SHARED_GE_SSE_EMPTY_OR_USAGE_ONLY_ACCEPTED",
            );
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_pending_cancel_and_transport_errors() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let raw = native_shared_ge_sse_encode(&native_shared_ge_sse_events())?;
        let baseline = fixture.budget.used();
        for wait_client in [false, true] {
            for cancel in [false, true] {
                let (original, original_probe, original_gate) = ScriptedStream::before_eof(&raw);
                let (client, client_probe, client_gate) = ScriptedStream::before_eof(&raw);
                if wait_client {
                    original_gate.release();
                }
                let mut future = Box::pin(native_shared_ge_driver(&fixture).consume_sse(
                    original,
                    &fixture.attempt,
                    client,
                    &fixture.attempt,
                ));
                assert!(
                    poll_once(future.as_mut()).is_pending(),
                    "NATIVE_SHARED_GE_SSE_PENDING_NOT_EOF"
                );
                assert_eq!(original_probe.snapshot().eofs, usize::from(wait_client));
                assert_eq!(client_probe.snapshot().eofs, 0);
                if cancel {
                    drop(future);
                    assert_eq!(original_probe.snapshot().drops, 1);
                    assert_eq!(client_probe.snapshot().drops, 1);
                } else {
                    original_gate.release();
                    client_gate.release();
                    drop(required(
                        future.await,
                        "NATIVE_SHARED_GE_SSE_RELEASED_EOF_REQUIRED",
                    )?);
                    original_probe.assert_eof();
                    client_probe.assert_eof();
                }
                assert_eq!(fixture.budget.used(), baseline);
            }
        }
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::ConnectionReset] {
            for fail_client in [false, true] {
                let (original, original_probe) = if fail_client {
                    ScriptedStream::entity(&raw)
                } else {
                    ScriptedStream::terminal_error(&raw, kind)
                };
                let (client, client_probe) = if fail_client {
                    ScriptedStream::terminal_error(&raw, kind)
                } else {
                    ScriptedStream::entity(&raw)
                };
                rejected(
                    native_shared_ge_driver(&fixture)
                        .consume_sse(original, &fixture.attempt, client, &fixture.attempt)
                        .await,
                    "NATIVE_SHARED_GE_SSE_TRANSPORT_ERROR_ACCEPTED",
                );
                if fail_client {
                    original_probe.assert_eof();
                    client_probe.assert_error();
                } else {
                    original_probe.assert_error();
                    assert_eq!(client_probe.snapshot().polls, 0);
                }
                assert_eq!(fixture.budget.used(), baseline);
            }
        }
        Ok(())
    }
    fn native_shared_ge_sse_stage_apply<T>(
        fixture: &Fixture,
        events: &[Value],
        mode: usize,
        apply: impl FnOnce(
            crate::proxy::security::restoration_core::StagedNativeSse<'_>,
        ) -> Result<T, RestoreError>,
    ) -> TestResult<Result<T, RestoreError>> {
        use crate::proxy::security::{
            restoration_contract::{EditConstraint, SourceClass, TextPolicy, TextSites},
            restoration_core::{
                stage_native_sse, stage_native_sse_with_lifecycle, NativeSseFrameObservation,
                NativeSseObservation, NativeSseSnapshotSelection, NativeSseTextStream,
            },
        };
        fn leaves<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
            match node {
                Value::String(_) => out.push(node),
                Value::Array(values) => {
                    for value in values {
                        leaves(value, out);
                    }
                }
                Value::Object(values) => {
                    for value in values.values() {
                        leaves(value, out);
                    }
                }
                _ => {}
            }
        }

        let raw = native_shared_ge_sse_encode(events)?;
        let raw_text = std::str::from_utf8(&raw)?;
        let ranges = raw_text
            .split_inclusive("\r\n\r\n")
            .scan(0usize, |offset, frame| {
                let range = *offset..*offset + frame.len();
                *offset += frame.len();
                Some(range)
            })
            .collect::<Vec<_>>();
        let mut frames = Vec::new();
        let mut snapshots = Vec::new();
        let mut groups = Vec::new();
        for (frame_index, (event, range)) in events.iter().zip(ranges).enumerate() {
            let mut sites = TextSites::new(fixture.budget.clone())?;
            if let Some(candidates) = event.get("candidates").and_then(Value::as_array) {
                for candidate in candidates {
                    snapshots.push(NativeSseSnapshotSelection::new(frame_index, candidate));
                    let Some(content) = candidate.get("content") else {
                        continue;
                    };
                    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
                        continue;
                    };
                    for part in parts {
                        let signed = part.get("thoughtSignature").is_some();
                        let constraint = if signed && mode != 8 {
                            EditConstraint::Immutable
                        } else {
                            EditConstraint::Editable
                        };
                        if let Some(text) = part.get("text") {
                            sites.observe(text, SourceClass::HistoricalText, constraint)?;
                            groups.push((
                                vec![text],
                                if signed && mode == 8 {
                                    TextPolicy::Reasoning
                                } else {
                                    TextPolicy::Output
                                },
                            ));
                        }
                        if let Some(args) = part.pointer("/functionCall/args") {
                            let mut nodes = Vec::new();
                            leaves(args, &mut nodes);
                            for node in nodes {
                                sites.observe(node, SourceClass::ToolArguments, constraint)?;
                                groups.push((vec![node], TextPolicy::ToolArguments));
                            }
                        }
                    }
                }
            }
            frames.push(required(
                NativeSseFrameObservation::new(
                    &raw_text[range],
                    Some(event),
                    sites,
                    fixture.budget.clone(),
                ),
                "NATIVE_SHARED_GE_SSE_FRAME_OBSERVATION_REQUIRED",
            )?);
        }
        match mode {
            1 => {
                snapshots.remove(1);
            }
            2 => snapshots.insert(
                1,
                NativeSseSnapshotSelection::new(0, &events[0]["candidates"][0]),
            ),
            3 => snapshots.swap(0, 1),
            4 => {
                snapshots[0] =
                    NativeSseSnapshotSelection::new(0, &events[0]["candidates"][0]["content"]);
            }
            5 => {
                let later_content = groups.remove(3);
                groups[2].0.extend(later_content.0);
            }
            6 => {
                groups.remove(1);
            }
            7 => {
                let node = *groups[0]
                    .0
                    .first()
                    .ok_or("NATIVE_SHARED_GE_SSE_DUPLICATE_FIXTURE")?;
                groups[0].0.push(node);
            }
            10 => {
                let other_candidate = groups.remove(2);
                groups[0].0.extend(other_candidate.0);
            }
            11 | 12 => {
                let other_part_or_content = groups.remove(if mode == 11 { 1 } else { 3 });
                groups[if mode == 11 { 0 } else { 2 }]
                    .0
                    .extend(other_part_or_content.0);
            }
            _ => {}
        }
        let streams: Vec<_> = groups
            .iter()
            .map(|(group, policy)| NativeSseTextStream::with_policy(group, *policy))
            .collect();
        let frame_refs: Vec<_> = frames.iter().collect();
        let observed = required(
            NativeSseObservation::new(
                &raw,
                &frame_refs,
                ClientProtocol::new(Protocol::Gemini),
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
            ),
            "NATIVE_SHARED_GE_SSE_ENTITY_OBSERVATION_REQUIRED",
        )?;
        let stage = if mode == 9 {
            stage_native_sse(
                &observed,
                &snapshots,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
            )
        } else {
            stage_native_sse_with_lifecycle(
                &observed,
                &snapshots,
                &streams,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                pair(Protocol::Gemini, Protocol::Gemini),
                &fixture.attempt,
            )
        };
        Ok(stage.and_then(apply))
    }

    #[test]
    fn native_shared_ge_sse_object_leaves_share_lifecycle_and_preserve_contents() -> TestResult {
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let baseline = fixture.budget.used();
        let mut events = native_shared_ge_sse_events();
        events[0]["candidates"][0]["content"]["parts"][0]["text"] = json!(reference);
        events[0]["candidates"][0]["content"]["parts"][1]["functionCall"]["args"] = json!({
            "path":reference,
            "nested":[7,true,null,{"value":reference}],
            "json_looking":"{not a JSON object string}"
        });
        let (occurrences, dependencies, delivered) = required(
            native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                Ok((
                    stage.restored_occurrences(),
                    stage.output_tokens().len(),
                    stage.delivered().clone(),
                ))
            })?,
            "NATIVE_SHARED_GE_SSE_NATIVE_OBJECT_LIFECYCLE_REQUIRED",
        )?;
        assert_eq!((occurrences, dependencies), (3, 1));
        assert_eq!(delivered[0]["index"], json!(0));
        assert_eq!(delivered[1]["index"], json!(1));
        assert_eq!(
            delivered[0]["contents"][0]["parts"][0]["text"],
            json!(NATIVE_SHARED_GE_TEXT)
        );
        let args = &delivered[0]["contents"][0]["parts"][1]["functionCall"]["args"];
        assert_eq!(args["path"], json!(NATIVE_SHARED_GE_TEXT));
        assert_eq!(
            args["nested"],
            json!([7,true,null,{"value":NATIVE_SHARED_GE_TEXT}])
        );
        assert_eq!(args["json_looking"], json!("{not a JSON object string}"));
        assert_eq!(delivered[1]["contents"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            delivered[1]["contents"][1]["parts"][0]["thoughtSignature"],
            json!("synthetic-late-signature")
        );
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }

    #[test]
    fn native_shared_ge_sse_exact_selections_and_carrier_isolation() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let baseline = fixture.budget.used();
        let events = native_shared_ge_sse_events();
        drop(required(
            native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                Ok(stage.delivered().clone())
            })?,
            "NATIVE_SHARED_GE_SSE_SELECTION_POSITIVE_CONTROL_REQUIRED",
        )?);
        for mode in [1, 2, 3, 4, 5, 6, 7, 9, 10] {
            rejected(
                native_shared_ge_sse_stage_apply(&fixture, &events, mode, |stage| {
                    Ok(stage.delivered().clone())
                })?,
                "NATIVE_SHARED_GE_SSE_INEXACT_SELECTION_OR_CARRIER_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_sse_signed_parts_override_forged_policies() -> TestResult {
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let baseline = fixture.budget.used();
        for signed_call in [false, true] {
            let mut events = native_shared_ge_sse_events();
            if signed_call {
                let part = &mut events[0]["candidates"][0]["content"]["parts"][1];
                part["thoughtSignature"] = json!("synthetic-call-signature");
                part["functionCall"]["args"]["path"] = json!(&reference);
            } else {
                events[4]["candidates"][0]["content"]["parts"][0]["text"] = json!(&reference);
            }
            rejected(
                native_shared_ge_sse_stage_apply(&fixture, &events, 8, |stage| {
                    Ok(stage.delivered().clone())
                })?,
                "NATIVE_SHARED_GE_SSE_SIGNED_PART_RESTORE_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_sse_part_and_unsigned_content_observations_cannot_merge() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let baseline = fixture.budget.used();

        let mut same_content = native_shared_ge_sse_events();
        same_content[0]["candidates"][0]["content"]["parts"] = json!([
            {"text":"synthetic first unsigned Part"},
            {"text":"synthetic second unsigned Part"}
        ]);
        let delivered = required(
            native_shared_ge_sse_stage_apply(&fixture, &same_content, 0, |stage| {
                Ok(stage.delivered().clone())
            })?,
            "NATIVE_SHARED_GE_SSE_SEPARATE_PARTS_CONTROL_REQUIRED",
        )?;
        assert_eq!(
            delivered[0]["contents"][0]["parts"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        rejected(
            native_shared_ge_sse_stage_apply(&fixture, &same_content, 11, |stage| {
                Ok(stage.delivered().clone())
            })?,
            "NATIVE_SHARED_GE_SSE_PART_MERGE_ACCEPTED",
        );
        assert_eq!(fixture.budget.used(), baseline);

        let mut separate_contents = native_shared_ge_sse_events();
        separate_contents[2] = json!({"candidates":[{"index":1,"content":{
            "role":"model","parts":[{"text":"synthetic later unsigned Content"}]
        }}]});
        let delivered = required(
            native_shared_ge_sse_stage_apply(&fixture, &separate_contents, 0, |stage| {
                Ok(stage.delivered().clone())
            })?,
            "NATIVE_SHARED_GE_SSE_SEPARATE_CONTENTS_CONTROL_REQUIRED",
        )?;
        assert_eq!(delivered[1]["contents"].as_array().map(Vec::len), Some(3));
        rejected(
            native_shared_ge_sse_stage_apply(&fixture, &separate_contents, 12, |stage| {
                Ok(stage.delivered().clone())
            })?,
            "NATIVE_SHARED_GE_SSE_CROSS_CONTENT_MERGE_ACCEPTED",
        );
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_publication_requires_poll_after_last_yield() -> TestResult {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        fn counts(store: &DlpTokenStore) -> TestResult<(usize, usize, usize)> {
            let (published, pending, pins, _) =
                crate::proxy::security::tokenization::publication_test_support::counts(store)?;
            Ok((published, pending, pins))
        }
        let template = native_shared_ge_sse_events();
        for yields in 0..=template.len() + 1 {
            let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
            let mut events = template.clone();
            events[0]["candidates"][0]["content"]["parts"][0]["text"] = json!(&reference);
            events[0]["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]["path"] =
                json!(&reference);
            let raw = native_shared_ge_sse_encode(&events)?;
            let baseline = fixture.budget.used();
            let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
            let mut delivered = None;
            let finalized = required(
                native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                    delivered = Some(stage.delivered().clone());
                    finalize_native_sse(
                        stage,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        NATIVE_DRIVER_ENTITY_LIMIT,
                    )
                })?,
                "NATIVE_SHARED_GE_SSE_FINALIZED_DELIVERY_REQUIRED",
            )?;
            let delivered = delivered.ok_or("NATIVE_SHARED_GE_SSE_DELIVERED_SNAPSHOT_REQUIRED")?;
            assert!(counts(&fixture.store)? == (0, 1, 1));
            let mut delivery = finalized.into_delivery_stream();
            let mut restored_wire = Vec::new();
            for _ in 0..yields.min(events.len()) {
                let chunk = delivery
                    .next()
                    .await
                    .ok_or("NATIVE_SHARED_GE_SSE_DELIVERY_MISSING")??;
                restored_wire.extend_from_slice(&chunk);
                assert!(
                    counts(&fixture.store)? == (0, 1, 1),
                    "NATIVE_SHARED_GE_SSE_PUBLICATION_ACTIVE_DURING_YIELD"
                );
            }
            if yields > events.len() {
                assert!(
                    delivery.next().await.is_none(),
                    "NATIVE_SHARED_GE_SSE_CONTINUATION_NOT_COMPLETE"
                );
                assert!(counts(&fixture.store)? == (1, 0, 0));
                fixture.lease.verify_native_delivered(
                    &fixture.binding,
                    ClientProtocol::new(Protocol::Gemini),
                    &delivered,
                )?;
                let restored_wire = std::str::from_utf8(&restored_wire)?;
                assert!(restored_wire.contains(NATIVE_SHARED_GE_TEXT));
                assert!(!restored_wire.contains(&reference));
            }
            drop(delivery);
            if yields <= events.len() {
                assert!(counts(&fixture.store)? == (0, 0, 0));
            }
            assert_eq!(fixture.budget.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_sse_lifecycle_faults_are_atomic_and_refunded() -> TestResult {
        use crate::proxy::security::restoration_core::{LifecycleOperation, ResponseLifecycle};
        for operation in [
            LifecycleOperation::Begin,
            LifecycleOperation::OpenText,
            LifecycleOperation::Done,
            LifecycleOperation::Hold,
            LifecycleOperation::Finish,
        ] {
            let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
            let mut events = native_shared_ge_sse_events();
            events[0]["candidates"][0]["content"]["parts"][0]["text"] = json!(&reference);
            events[0]["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]["path"] =
                json!(&reference);
            let baseline = fixture.budget.used();
            let guard = ResponseLifecycle::reject_operation_for_test(operation);
            rejected(
                native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                    Ok(stage.delivered().clone())
                })?,
                "NATIVE_SHARED_GE_SSE_LIFECYCLE_FAULT_ACCEPTED",
            );
            drop(guard);
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    #[test]
    fn native_shared_ge_sse_successful_stage_drop_refunds_without_publication() -> TestResult {
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let mut events = native_shared_ge_sse_events();
        events[0]["candidates"][0]["content"]["parts"][0]["text"] = json!(&reference);
        events[0]["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]["path"] =
            json!(&reference);
        let baseline = fixture.budget.used();
        required(
            native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                drop(stage);
                Ok(())
            })?,
            "NATIVE_SHARED_GE_SSE_DROP_STAGE_REQUIRED",
        )?;
        assert_eq!(fixture.budget.used(), baseline);
        let counts =
            crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_finalizer_identity_and_wire_failures_refund() -> TestResult {
        use crate::proxy::security::response_restoration::finalize_native_sse;
        let (fixture, reference) = native_shared_ge_mapped_fixture(true)?;
        let foreign = native_shared_ge_fixture()?;
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let mut events = native_shared_ge_sse_events();
        events[0]["candidates"][0]["content"]["parts"][0]["text"] = json!(&reference);
        let baseline = fixture.budget.used();
        for mode in 0..4 {
            let mut donor_events = events.clone();
            if mode == 2 {
                donor_events[0]["candidates"][0]["content"]["parts"][0]["text"] =
                    json!("synthetic donor text");
            }
            let donor_raw = native_shared_ge_sse_encode(&donor_events)?;
            let (original, _) = ScriptedStream::entity(&donor_raw);
            let (client, _) = ScriptedStream::entity(&donor_raw);
            let attempt = if mode == 0 {
                &wrong_attempt
            } else {
                &fixture.attempt
            };
            let driver = NativeCompletionDriver::new(
                pair(Protocol::Gemini, Protocol::Gemini),
                attempt.clone(),
                if mode == 1 {
                    foreign.binding.clone()
                } else {
                    fixture.binding.clone()
                },
                fixture.budget.clone(),
            );
            let proof = required(
                driver.consume_sse(original, attempt, client, attempt).await,
                "NATIVE_SHARED_GE_SSE_FINALIZER_DONOR_REQUIRED",
            )?;
            rejected(
                native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                    finalize_native_sse(
                        stage,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        if mode == 3 {
                            1
                        } else {
                            NATIVE_DRIVER_ENTITY_LIMIT
                        },
                    )
                })?,
                "NATIVE_SHARED_GE_SSE_INVALID_FINALIZATION_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
            let counts = crate::proxy::security::tokenization::publication_test_support::counts(
                &fixture.store,
            )?;
            assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
        }
        Ok(())
    }

    fn native_shared_ge_sse_expected(events: &[Value]) -> Value {
        json!([
            {"index":0,"contents":[events[0]["candidates"][0]["content"]]},
            {"index":1,"contents":[
                events[1]["candidates"][0]["content"],
                events[4]["candidates"][0]["content"]
            ]}
        ])
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_ge_sse_final_wire_derives_exact_append_only_contents() -> TestResult {
        let fixture = native_shared_ge_fixture()?;
        let events = native_shared_ge_sse_events();
        let raw = native_shared_ge_sse_encode(&events)?;
        let expected = native_shared_ge_sse_expected(&events);
        let baseline = fixture.budget.used();
        let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
        required(
            proof.validate_restored_chat_snapshots(
                &raw,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Gemini),
                io::Cursor::new(&raw),
                &fixture.budget,
                &expected,
            ),
            "NATIVE_SHARED_GE_SSE_EXACT_SNAPSHOT_REQUIRED",
        )?;
        assert_eq!(fixture.budget.used(), baseline);
        for mode in 0..7 {
            let mut wrong = expected.clone();
            match mode {
                0 => {
                    let first = events[1]["candidates"][0]["content"].clone();
                    let late = events[4]["candidates"][0]["content"].clone();
                    let mut merged = first;
                    merged["parts"]
                        .as_array_mut()
                        .unwrap()
                        .extend(late["parts"].as_array().unwrap().iter().cloned());
                    wrong[1]["contents"] = json!([merged]);
                }
                1 => wrong[1]["contents"][1]["parts"] = json!([]),
                2 => {
                    wrong[1]["contents"] = json!([
                        events[4]["candidates"][0]["content"],
                        events[1]["candidates"][0]["content"]
                    ])
                }
                3 => wrong[1]["contents"][1] = events[4]["candidates"][0].clone(),
                4 => wrong[1]["index"] = json!(7),
                5 => wrong = json!([wrong[1].clone()]),
                6 => wrong = json!([wrong[1].clone(), wrong[0].clone()]),
                _ => unreachable!("invalid Gemini SSE snapshot fixture"),
            }
            let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
            rejected(
                proof.validate_restored_chat_snapshots(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Gemini),
                    io::Cursor::new(&raw),
                    &fixture.budget,
                    &wrong,
                ),
                "NATIVE_SHARED_GE_SSE_INEXACT_EXPECTED_SNAPSHOT_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        for mode in 0..5 {
            let mut changed = events.clone();
            match mode {
                0 => {
                    let late = changed[4]["candidates"][0]["content"].clone();
                    changed[1]["candidates"][0]["content"]["parts"]
                        .as_array_mut()
                        .unwrap()
                        .extend(late["parts"].as_array().unwrap().iter().cloned());
                    changed[4]["candidates"][0]["content"]["parts"] = json!([]);
                }
                1 => changed[4]["candidates"][0]["content"]["parts"] = json!([]),
                2 => {
                    changed[1]["candidates"][0]["content"] =
                        events[4]["candidates"][0]["content"].clone();
                    changed[4]["candidates"][0]["content"] =
                        events[1]["candidates"][0]["content"].clone();
                }
                3 => changed[1]["candidates"][0]["content"]["foreign"] = json!({"preserve":true}),
                4 => {}
                _ => unreachable!("invalid Gemini SSE wire fixture"),
            }
            let mut invalid = native_shared_ge_sse_encode(&changed)?;
            if mode == 4 {
                invalid.extend_from_slice(b"data: [DONE]\r\n\r\n");
            }
            let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
            rejected(
                proof.validate_restored_chat_snapshots(
                    &raw,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Gemini),
                    io::Cursor::new(&invalid),
                    &fixture.budget,
                    &expected,
                ),
                "NATIVE_SHARED_GE_SSE_INEXACT_FINAL_SNAPSHOT_ACCEPTED",
            );
            assert_eq!(fixture.budget.used(), baseline);
        }
        let mut named = Vec::new();
        named.extend_from_slice(b"event: message\r\n");
        named.extend_from_slice(&raw);
        let proof = native_shared_ge_sse_proof(&fixture, &raw).await?;
        rejected(
            proof.validate_restored_chat_snapshots(
                &raw,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Gemini),
                io::Cursor::new(&named),
                &fixture.budget,
                &expected,
            ),
            "NATIVE_SHARED_GE_SSE_NAMED_EVENT_ACCEPTED",
        );
        assert_eq!(fixture.budget.used(), baseline);
        Ok(())
    }
    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_finalization_json_activation_drop_and_charged_owner_refunds(
    ) -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_json,
            restoration_core::stage_native_json_with_lifecycle,
            tokenization::publication_test_support,
        };
        for protocol in [Protocol::Anthropic, Protocol::Gemini] {
            for mapped in [false, true] {
                for (poll_body, deny_activation) in [(false, false), (true, false), (false, true)] {
                    let (fixture, reference) = match protocol {
                        Protocol::Anthropic => native_shared_an_mapped_fixture(mapped)?,
                        Protocol::Gemini => native_shared_ge_mapped_fixture(mapped)?,
                        _ => unreachable!("unsupported shared finalization protocol"),
                    };
                    let source = match protocol {
                        Protocol::Anthropic => native_shared_an_leaf_message(&reference),
                        Protocol::Gemini => native_shared_ge_leaf_response(&reference),
                        _ => unreachable!("unsupported shared finalization protocol"),
                    };
                    let wire = serde_json::to_vec(&source)?;
                    let baseline = fixture.budget.used();
                    let (transport, probe) = ScriptedStream::entity(&wire);
                    let driver = match protocol {
                        Protocol::Anthropic => native_shared_an_driver(&fixture),
                        Protocol::Gemini => native_shared_ge_driver(&fixture),
                        _ => unreachable!("unsupported shared finalization protocol"),
                    };
                    let proof = required(
                        driver
                            .consume_json(transport, &fixture.attempt, &source, &fixture.attempt)
                            .await,
                        "NATIVE_SHARED_FINALIZATION_JSON_PROOF_REQUIRED",
                    )?;
                    probe.assert_eof();
                    let observed = match protocol {
                        Protocol::Anthropic => {
                            native_shared_an_leaf_observation(&fixture, &source, false)?
                        }
                        Protocol::Gemini => native_shared_ge_observation(&fixture, &source, false)?,
                        _ => unreachable!("unsupported shared finalization protocol"),
                    };
                    let stage = required(
                        stage_native_json_with_lifecycle(
                            &observed,
                            &fixture.lease,
                            &fixture.binding,
                            pair(protocol, protocol),
                            &fixture.attempt,
                        ),
                        "NATIVE_SHARED_FINALIZATION_JSON_STAGE_REQUIRED",
                    )?;
                    let delivered = stage.delivered().clone();
                    if deny_activation {
                        publication_test_support::unavailable(&fixture.store)?;
                        rejected(
                            finalize_native_json(
                                stage,
                                &fixture.lease,
                                proof,
                                &fixture.attempt,
                                Bytes::from(wire),
                                NATIVE_DRIVER_ENTITY_LIMIT,
                            ),
                            "NATIVE_SHARED_FINALIZATION_JSON_UNAVAILABLE_ACTIVATED",
                        );
                        assert!(
                            fixture
                                .lease
                                .verify_native_delivered(
                                    &fixture.binding,
                                    ClientProtocol::new(protocol),
                                    &delivered,
                                )
                                .is_err(),
                            "NATIVE_SHARED_FINALIZATION_JSON_UNAVAILABLE_DELIVERED"
                        );
                        let counts = publication_test_support::counts(&fixture.store)?;
                        assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
                        drop(observed);
                        assert_eq!(fixture.budget.used(), baseline);
                        continue;
                    }
                    let finalized = required(
                        finalize_native_json(
                            stage,
                            &fixture.lease,
                            proof,
                            &fixture.attempt,
                            Bytes::from(wire),
                            NATIVE_DRIVER_ENTITY_LIMIT,
                        ),
                        "NATIVE_SHARED_FINALIZATION_JSON_DELIVERY_REQUIRED",
                    )?;
                    assert!(
                        fixture
                            .lease
                            .verify_native_delivered(
                                &fixture.binding,
                                ClientProtocol::new(protocol),
                                &delivered,
                            )
                            .is_ok(),
                        "NATIVE_SHARED_FINALIZATION_JSON_NOT_ACTIVE_BEFORE_BODY"
                    );
                    let counts = publication_test_support::counts(&fixture.store)?;
                    assert!((counts.0, counts.1, counts.2) == (1, 0, 0));
                    let mut stream = finalized.into_delivery_stream();
                    let retained = if poll_body {
                        Some(
                            stream
                                .next()
                                .await
                                .ok_or("NATIVE_SHARED_FINALIZATION_JSON_BODY_MISSING")??,
                        )
                    } else {
                        None
                    };
                    drop(stream);
                    assert!(
                        fixture
                            .lease
                            .verify_native_delivered(
                                &fixture.binding,
                                ClientProtocol::new(protocol),
                                &delivered,
                            )
                            .is_ok(),
                        "NATIVE_SHARED_FINALIZATION_JSON_DROP_UNPUBLISHED"
                    );
                    let counts = publication_test_support::counts(&fixture.store)?;
                    assert!((counts.0, counts.1, counts.2) == (1, 0, 0));
                    drop(observed);
                    if mapped && poll_body {
                        assert!(
                            fixture.budget.used() > baseline,
                            "NATIVE_SHARED_FINALIZATION_JSON_RETAINED_OWNER_UNCHARGED"
                        );
                    } else {
                        assert_eq!(fixture.budget.used(), baseline);
                    }
                    drop(retained);
                    assert_eq!(
                        fixture.budget.used(),
                        baseline,
                        "NATIVE_SHARED_FINALIZATION_JSON_OWNER_REFUND_REQUIRED"
                    );
                }
            }
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_finalization_sse_pending_cancel_resume_and_charged_owner_refunds(
    ) -> TestResult {
        use crate::proxy::security::{
            response_restoration::finalize_native_sse, tokenization::publication_test_support,
        };
        #[derive(Clone, Copy)]
        enum Outcome {
            CancelBeforeYield,
            CancelAfterLastYield,
            FailActivation,
            Resume,
        }
        for protocol in [Protocol::Anthropic, Protocol::Gemini] {
            for outcome in [
                Outcome::CancelBeforeYield,
                Outcome::CancelAfterLastYield,
                Outcome::FailActivation,
                Outcome::Resume,
            ] {
                let (fixture, reference) = match protocol {
                    Protocol::Anthropic => native_shared_an_mapped_fixture(true)?,
                    Protocol::Gemini => native_shared_ge_mapped_fixture(true)?,
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                let mut events = match protocol {
                    Protocol::Anthropic => native_shared_an_events(),
                    Protocol::Gemini => native_shared_ge_sse_events(),
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                match protocol {
                    Protocol::Anthropic => {
                        events[6]["delta"]["text"] = json!(reference);
                    }
                    Protocol::Gemini => {
                        events[0]["candidates"][0]["content"]["parts"][0]["text"] =
                            json!(&reference);
                        events[0]["candidates"][0]["content"]["parts"][1]["functionCall"]["args"]
                            ["path"] = json!(&reference);
                    }
                    _ => unreachable!("unsupported shared finalization protocol"),
                }
                let raw = match protocol {
                    Protocol::Anthropic => native_shared_an_encode_events(&events)?,
                    Protocol::Gemini => native_shared_ge_sse_encode(&events)?,
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                let baseline = fixture.budget.used();
                let proof = match protocol {
                    Protocol::Anthropic => native_shared_an_sse_proof(&fixture, &raw).await?,
                    Protocol::Gemini => native_shared_ge_sse_proof(&fixture, &raw).await?,
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                let mut delivered = None;
                let finalized = match protocol {
                    Protocol::Anthropic => required(
                        native_shared_an_sse_stage_apply(&fixture, &events, 0, |stage| {
                            delivered = Some(stage.delivered().clone());
                            finalize_native_sse(
                                stage,
                                &fixture.lease,
                                proof,
                                &fixture.attempt,
                                NATIVE_DRIVER_ENTITY_LIMIT,
                            )
                        })?,
                        "NATIVE_SHARED_FINALIZATION_AN_SSE_DELIVERY_REQUIRED",
                    )?,
                    Protocol::Gemini => required(
                        native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                            delivered = Some(stage.delivered().clone());
                            finalize_native_sse(
                                stage,
                                &fixture.lease,
                                proof,
                                &fixture.attempt,
                                NATIVE_DRIVER_ENTITY_LIMIT,
                            )
                        })?,
                        "NATIVE_SHARED_FINALIZATION_GE_SSE_DELIVERY_REQUIRED",
                    )?,
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                let delivered =
                    delivered.ok_or("NATIVE_SHARED_FINALIZATION_SSE_SNAPSHOT_REQUIRED")?;
                let counts = publication_test_support::counts(&fixture.store)?;
                assert!((counts.0, counts.1, counts.2) == (0, 1, 1));
                let mut stream = finalized.into_delivery_stream();
                let mut retained = None;
                let yields = if matches!(outcome, Outcome::CancelBeforeYield) {
                    0
                } else {
                    events.len()
                };
                for _ in 0..yields {
                    let frame = stream
                        .next()
                        .await
                        .ok_or("NATIVE_SHARED_FINALIZATION_SSE_FRAME_MISSING")??;
                    if retained.is_none() {
                        retained = Some(frame);
                    }
                    let counts = publication_test_support::counts(&fixture.store)?;
                    assert!(
                        (counts.0, counts.1, counts.2) == (0, 1, 1),
                        "NATIVE_SHARED_FINALIZATION_SSE_PUBLISHED_DURING_YIELD"
                    );
                }
                match outcome {
                    Outcome::CancelBeforeYield | Outcome::CancelAfterLastYield => {}
                    Outcome::FailActivation => {
                        publication_test_support::unavailable(&fixture.store)?;
                        assert!(
                            stream.next().await.is_some_and(|result| {
                                result.is_err_and(|error| {
                                    error.to_string() == "Response stream failed."
                                })
                            }),
                            "NATIVE_SHARED_FINALIZATION_SSE_ACTIVATION_ERROR_REQUIRED"
                        );
                        assert!(stream.next().await.is_none());
                    }
                    Outcome::Resume => {
                        assert!(
                            stream.next().await.is_none(),
                            "NATIVE_SHARED_FINALIZATION_SSE_EXTRA_DELIVERY"
                        );
                        let counts = publication_test_support::counts(&fixture.store)?;
                        assert!(
                            (counts.0, counts.1, counts.2) == (1, 0, 0),
                            "NATIVE_SHARED_FINALIZATION_SSE_RESUME_NOT_PUBLISHED"
                        );
                    }
                }
                drop(stream);
                let counts = publication_test_support::counts(&fixture.store)?;
                let verification = fixture.lease.verify_native_delivered(
                    &fixture.binding,
                    ClientProtocol::new(protocol),
                    &delivered,
                );
                if matches!(outcome, Outcome::Resume) {
                    assert!((counts.0, counts.1, counts.2) == (1, 0, 0));
                    assert!(
                        verification.is_ok(),
                        "NATIVE_SHARED_FINALIZATION_SSE_RESUME_DELIVERY_MISSING"
                    );
                } else {
                    assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
                    assert!(
                        verification.is_err(),
                        "NATIVE_SHARED_FINALIZATION_SSE_CANCEL_PUBLISHED"
                    );
                }
                if retained.is_some() {
                    assert!(
                        fixture.budget.used() > baseline,
                        "NATIVE_SHARED_FINALIZATION_SSE_RETAINED_OWNER_UNCHARGED"
                    );
                } else {
                    assert_eq!(fixture.budget.used(), baseline);
                }
                drop(retained);
                assert_eq!(
                    fixture.budget.used(),
                    baseline,
                    "NATIVE_SHARED_FINALIZATION_SSE_OWNER_REFUND_REQUIRED"
                );
            }
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shared_finalization_sse_snapshot_source_and_final_mismatch_refunds(
    ) -> TestResult {
        use crate::proxy::security::tokenization::publication_test_support;
        for protocol in [Protocol::Anthropic, Protocol::Gemini] {
            let fixture = match protocol {
                Protocol::Anthropic => native_shared_an_fixture()?,
                Protocol::Gemini => native_shared_ge_fixture()?,
                _ => unreachable!("unsupported shared finalization protocol"),
            };
            let events = match protocol {
                Protocol::Anthropic => native_shared_an_events(),
                Protocol::Gemini => native_shared_ge_sse_events(),
                _ => unreachable!("unsupported shared finalization protocol"),
            };
            let raw = match protocol {
                Protocol::Anthropic => native_shared_an_encode_events(&events)?,
                Protocol::Gemini => native_shared_ge_sse_encode(&events)?,
                _ => unreachable!("unsupported shared finalization protocol"),
            };
            let baseline = fixture.budget.used();
            let expected = match protocol {
                Protocol::Anthropic => {
                    let (_, _, delivered) = required(
                        native_shared_an_sse_stage_case(&fixture, &events, 0)?,
                        "NATIVE_SHARED_FINALIZATION_AN_SNAPSHOT_CONTROL_REQUIRED",
                    )?;
                    delivered
                }
                Protocol::Gemini => required(
                    native_shared_ge_sse_stage_apply(&fixture, &events, 0, |stage| {
                        Ok(stage.delivered().clone())
                    })?,
                    "NATIVE_SHARED_FINALIZATION_GE_SNAPSHOT_CONTROL_REQUIRED",
                )?,
                _ => unreachable!("unsupported shared finalization protocol"),
            };
            assert_eq!(fixture.budget.used(), baseline);
            let mut donor_events = events.clone();
            match protocol {
                Protocol::Anthropic => {
                    donor_events[6]["delta"]["text"] = json!("synthetic donor text");
                }
                Protocol::Gemini => {
                    donor_events[0]["candidates"][0]["content"]["parts"][0]["text"] =
                        json!("synthetic donor text");
                }
                _ => unreachable!("unsupported shared finalization protocol"),
            }
            let donor_raw = match protocol {
                Protocol::Anthropic => native_shared_an_encode_events(&donor_events)?,
                Protocol::Gemini => native_shared_ge_sse_encode(&donor_events)?,
                _ => unreachable!("unsupported shared finalization protocol"),
            };
            for mode in 0..3 {
                let proof = match protocol {
                    Protocol::Anthropic => native_shared_an_sse_proof(&fixture, &raw).await?,
                    Protocol::Gemini => native_shared_ge_sse_proof(&fixture, &raw).await?,
                    _ => unreachable!("unsupported shared finalization protocol"),
                };
                let mut wrong = expected.clone();
                match (protocol, mode) {
                    (Protocol::Anthropic, 0) => wrong
                        .as_array_mut()
                        .ok_or("NATIVE_SHARED_FINALIZATION_AN_BLOCKS_MISSING")?
                        .swap(0, 1),
                    (Protocol::Gemini, 0) => wrong[1]["contents"]
                        .as_array_mut()
                        .ok_or("NATIVE_SHARED_FINALIZATION_GE_CONTENTS_MISSING")?
                        .swap(0, 1),
                    _ => {}
                }
                let (source, final_wire, snapshot) = match mode {
                    0 => (raw.as_slice(), raw.as_slice(), &wrong),
                    1 => (donor_raw.as_slice(), raw.as_slice(), &expected),
                    2 => (raw.as_slice(), donor_raw.as_slice(), &expected),
                    _ => unreachable!("invalid shared finalization mismatch mode"),
                };
                rejected(
                    proof.validate_restored_chat_snapshots(
                        source,
                        &fixture.binding,
                        &fixture.attempt,
                        ClientProtocol::new(protocol),
                        io::Cursor::new(final_wire),
                        &fixture.budget,
                        snapshot,
                    ),
                    "NATIVE_SHARED_FINALIZATION_SSE_MISMATCH_ACCEPTED",
                );
                let counts = publication_test_support::counts(&fixture.store)?;
                assert!((counts.0, counts.1, counts.2) == (0, 0, 0));
                assert_eq!(fixture.budget.used(), baseline);
            }
        }
        Ok(())
    }
}
