//! Test-only native execution composition; no route grant or completion issuer.
use std::{
    cell::Cell,
    collections::VecDeque,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

use super::{
    completed_context::ActualAttemptBinding,
    http_restoration::{test_transport, VerifiedNativeJson, VerifiedNativeSse},
    payload_budget::PayloadBudget,
    request_preparation::prepare_observed_request,
    response_restoration::{
        finalize_native_json, finalize_native_sse, FinalizedNativeJson, FinalizedNativeSse,
    },
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, ProtocolPair,
        RequestObservation, SourceClass, TextSites, UpstreamProtocol,
    },
    restoration_core::{
        stage_native_json_with_lifecycle, stage_native_sse_with_lifecycle, AttemptProvenance,
        NativeJsonObservation, NativeSseFrameObservation, NativeSseObservation,
        NativeSseSnapshotSelection, NativeSseTextStream,
    },
    runtime::SecurityRuntimeConfig,
    tokenization::{publication_test_support, DlpRequestLease, DlpTokenStore, TokenError},
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub(super) const LIMIT: usize = 16 * 1024 * 1024;
pub(super) const TEXT: &str = "13900000000";
pub(super) const MAX_ENTITY: usize = 1024 * 1024;

pub(super) fn chat() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}

pub(super) struct NativeFixture {
    pub store: DlpTokenStore,
    pub lease: DlpRequestLease,
    pub budget: PayloadBudget,
    pub attempt: AttemptProvenance,
    pub binding: ActualAttemptBinding,
    pub reference: String,
}

impl NativeFixture {
    /// Obtain mappings only through the real observed-request transaction.
    pub fn new(mapped: bool) -> TestResult<Self> {
        Self::with_store(DlpTokenStore::new(), mapped)
    }

    fn with_store(store: DlpTokenStore, mapped: bool) -> TestResult<Self> {
        let lease = admit_execution(&store)?;
        Self::from_lease(store, lease, PayloadBudget::with_limit(LIMIT), mapped)
    }

    fn from_lease(
        store: DlpTokenStore,
        mut lease: DlpRequestLease,
        budget: PayloadBudget,
        mapped: bool,
    ) -> TestResult<Self> {
        let text = if mapped {
            TEXT
        } else {
            "Synthetic public text🔒"
        };
        let request = json!({"model":"synthetic-model","messages":[{"role":"user","content":text}],
            "extension":{"same_value":text,"types":[true,null,7]}});
        let mut observation = RequestObservation::new(&request, chat(), budget.clone())?;
        observation.observe_text(
            &request["messages"][0]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let prepared = prepare_observed_request(
            &mut lease,
            &observation,
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            |candidate, encoded, proof| {
                let parsed: Value =
                    serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
                if &parsed != candidate
                    || proof.len() != usize::from(mapped)
                    || store.stats()?.entries != 0
                {
                    return Err(TokenError::RedactionFailed);
                }
                Ok(())
            },
        )
        .map_err(|_| "native_execution_request_rejected")?;
        assert!(
            prepared.body.as_value()["extension"] == request["extension"],
            "native_execution_sibling_changed"
        );
        assert!(
            prepared.reversible_occurrences == usize::from(mapped),
            "native_execution_request_mapping_missing"
        );
        let reference = prepared.body.as_value()["messages"][0]["content"]
            .as_str()
            .ok_or("native_execution_reference_missing")?
            .to_owned();
        if mapped {
            assert!(
                reference != text && lease.resolve(&reference)? == text,
                "native_execution_request_not_committed"
            );
        } else {
            assert!(
                reference == text && lease.is_empty(),
                "native_execution_zero_mapping_changed"
            );
        }
        drop(prepared);
        drop(observation);
        assert!(
            budget.used() == 0,
            "native_execution_request_charge_retained"
        );
        let binding = lease.bind_actual_attempt(
            "synthetic-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-principal"),
        )?;
        let attempt = AttemptProvenance::new(&budget)?;
        Ok(Self {
            store,
            lease,
            budget,
            attempt,
            binding,
            reference,
        })
    }

    pub fn counts(&self) -> TestResult<(usize, usize, usize)> {
        let (published, pending, pins, ..) = publication_test_support::counts(&self.store)?;
        Ok((published, pending, pins))
    }

    pub fn json_source(&self) -> Value {
        json!({"id":"synthetic-response","object":"chat.completion","model":"synthetic-model",
            "choices":[{"index":0,"message":{"role":"assistant","content":self.reference},"finish_reason":"stop"}],
            "extension":{"types":[true,null,7,1.0]}})
    }

    pub async fn json_proof<S>(
        &self,
        source: &Value,
        transport: S,
    ) -> Result<VerifiedNativeJson, super::response_restoration::RestoreError>
    where
        S: Stream<Item = Result<Bytes, io::Error>>,
    {
        test_transport::native_json_from_stream(
            &self.attempt,
            &self.binding,
            &self.budget,
            transport,
            source,
        )
        .await
    }

    pub fn finalize_json(
        &self,
        source: &Value,
        wire: Bytes,
        proof: VerifiedNativeJson,
    ) -> TestResult<FinalizedNativeJson> {
        let mut sites = TextSites::new(self.budget.clone())?;
        sites.observe(
            &source["choices"][0]["message"]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let observed = NativeJsonObservation::new(
            source,
            chat(),
            sites,
            &[&source["choices"][0]["message"]],
            self.budget.clone(),
        )
        .map_err(|_| "native_execution_response_observation_rejected")?;
        let staged = stage_native_json_with_lifecycle(
            &observed,
            &self.lease,
            &self.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &self.attempt,
        )
        .map_err(|_| "native_execution_stage_rejected")?;
        assert!(
            staged.output_tokens().len() == usize::from(!self.lease.is_empty()),
            "native_execution_dependency_missing"
        );
        finalize_native_json(staged, &self.lease, proof, &self.attempt, wire, MAX_ENTITY)
            .map_err(|_| "native_execution_finalization_rejected".into())
    }

    pub fn finalize_sse(
        &self,
        entity: &NativeSseEntity,
        proof: VerifiedNativeSse,
    ) -> TestResult<FinalizedNativeSse> {
        let mut sites = TextSites::new(self.budget.clone())?;
        sites.observe(
            &entity.sources[0]["choices"][0]["delta"]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let frames = [
            NativeSseFrameObservation::new(
                &entity.wire[..entity.first_end],
                Some(&entity.sources[0]),
                sites,
                self.budget.clone(),
            ),
            NativeSseFrameObservation::new(
                &entity.wire[entity.first_end..entity.second_end],
                Some(&entity.sources[1]),
                TextSites::new(self.budget.clone())?,
                self.budget.clone(),
            ),
            NativeSseFrameObservation::new(
                &entity.wire[entity.second_end..],
                None,
                TextSites::new(self.budget.clone())?,
                self.budget.clone(),
            ),
        ];
        let [first, second, done] = frames;
        let first = first.map_err(|_| "native_execution_first_frame_rejected")?;
        let second = second.map_err(|_| "native_execution_second_frame_rejected")?;
        let done = done.map_err(|_| "native_execution_done_frame_rejected")?;
        let observed = NativeSseObservation::new(
            entity.wire.as_bytes(),
            &[&first, &second, &done],
            chat(),
            &self.lease,
            &self.binding,
            self.budget.clone(),
        )
        .map_err(|_| "native_execution_sse_observation_rejected")?;
        let nodes = [&entity.sources[0]["choices"][0]["delta"]["content"]];
        let streams = [NativeSseTextStream::new(&nodes)];
        let staged = stage_native_sse_with_lifecycle(
            &observed,
            &[NativeSseSnapshotSelection::new(
                0,
                &entity.sources[0]["choices"][0]["delta"],
            )],
            &streams,
            &self.lease,
            &self.binding,
            self.budget.clone(),
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &self.attempt,
        )
        .map_err(|_| "native_execution_sse_stage_rejected")?;
        finalize_native_sse(staged, &self.lease, proof, &self.attempt, MAX_ENTITY)
            .map_err(|_| "native_execution_sse_finalization_rejected".into())
    }
}

pub(super) struct NativeSseEntity {
    pub sources: [Value; 2],
    pub wire: String,
    first_end: usize,
    second_end: usize,
}
impl NativeSseEntity {
    pub fn new(fixture: &NativeFixture) -> Self {
        let sources = [
            json!({"id":"synthetic-response","object":"chat.completion.chunk","choices":[{"index":0,
                "delta":{"role":"assistant","content":fixture.reference},"finish_reason":null}],
                "extension":{"types":[true,null,7,1.0]}}),
            json!({"id":"synthetic-response","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        ];
        let first = format!(
            ": synthetic comment\r\nid: synthetic-event\r\ndata: {}\r\n\r\n",
            sources[0]
        );
        let second = format!("data:{}\n\n", sources[1]);
        let first_end = first.len();
        let second_end = first_end + second.len();
        Self {
            sources,
            wire: format!("{first}{second}data: [DONE]\n\n"),
            first_end,
            second_end,
        }
    }
}

/// Scripted source observations are not EOF or completion authority.
pub(super) enum Step {
    Bytes(Bytes),
    Pending,
    Error(io::ErrorKind),
    Park,
}
#[derive(Clone, Default)]
pub(super) struct TransportStats {
    pub polls: Rc<Cell<usize>>,
    pub eof: Rc<Cell<usize>>,
    pub pending: Rc<Cell<usize>>,
    pub dropped: Rc<Cell<usize>>,
}
pub(super) struct ScriptedSource {
    steps: VecDeque<Step>,
    stats: TransportStats,
}
impl ScriptedSource {
    pub fn new(steps: impl IntoIterator<Item = Step>) -> (Self, TransportStats) {
        let stats = TransportStats::default();
        (
            Self {
                steps: steps.into_iter().collect(),
                stats: stats.clone(),
            },
            stats,
        )
    }
    /// Offsets are byte boundaries, deliberately not UTF-8 character boundaries.
    pub fn split(wire: Bytes, offsets: &[usize]) -> TestResult<(Self, TransportStats)> {
        let mut steps = Vec::new();
        let mut start = 0;
        for end in offsets.iter().copied().chain(std::iter::once(wire.len())) {
            if end < start || end > wire.len() {
                return Err("native_execution_split_invalid".into());
            }
            steps.push(Step::Pending);
            steps.push(Step::Bytes(wire.slice(start..end)));
            start = end;
        }
        Ok(Self::new(steps))
    }
}
impl Stream for ScriptedSource {
    type Item = Result<Bytes, io::Error>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stats.polls.set(self.stats.polls.get() + 1);
        match self.steps.pop_front() {
            Some(Step::Bytes(bytes)) => Poll::Ready(Some(Ok(bytes))),
            Some(Step::Error(kind)) => Poll::Ready(Some(Err(io::Error::new(
                kind,
                "Synthetic transport failure.",
            )))),
            Some(Step::Pending) => {
                self.stats.pending.set(self.stats.pending.get() + 1);
                context.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Step::Park) => {
                self.steps.push_front(Step::Park);
                Poll::Pending
            }
            None => {
                self.stats.eof.set(self.stats.eof.get() + 1);
                Poll::Ready(None)
            }
        }
    }
}
impl Drop for ScriptedSource {
    fn drop(&mut self) {
        self.stats.dropped.set(self.stats.dropped.get() + 1);
    }
}

pub(super) async fn record_delivery<S>(stream: S) -> TestResult<Vec<Bytes>>
where
    S: Stream<Item = Result<Bytes, io::Error>>,
{
    futures::pin_mut!(stream);
    let mut delivered = Vec::new();
    while let Some(chunk) = stream.next().await {
        delivered.push(chunk?);
    }
    Ok(delivered)
}

fn admit_execution(store: &DlpTokenStore) -> Result<DlpRequestLease, TokenError> {
    admit_execution_revision(store, 1)
}

fn admit_execution_revision(
    store: &DlpTokenStore,
    revision: u64,
) -> Result<DlpRequestLease, TokenError> {
    store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: chat(),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-execution-app",
        Some("synthetic-execution-session"),
        revision,
    )
}

/// Real delivery followed by eviction, never a fabricated handle or mapping grant.
pub(super) struct ExpiredNativeHistory {
    pub store: DlpTokenStore,
    pub delivered: Value,
    pub reference: String,
    revision: u64,
}

impl ExpiredNativeHistory {
    pub fn new() -> TestResult<Self> {
        use super::tokenization::TokenStoreLimits;
        use std::time::{Duration, Instant};

        let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
            idle_ttl: Duration::from_secs(1),
            ..Default::default()
        });
        super::runtime::set_security_runtime_for_test(SecurityRuntimeConfig::default());
        let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
        let lease = admit_execution_revision(&store, revision)?;
        let fixture = NativeFixture::from_lease(
            store.clone(),
            lease,
            PayloadBudget::with_limit(LIMIT),
            true,
        )?;
        let budget = fixture.budget.clone();
        let source = fixture.json_source();
        let wire = Bytes::from(serde_json::to_vec(&source)?);
        let (transport, _) = ScriptedSource::split(wire.clone(), &[1])?;
        let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
            .map_err(|_| "native_history_original_proof_rejected")?;
        let finalized = fixture.finalize_json(&source, wire, proof)?;
        let chunks =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        let body: Value = serde_json::from_slice(&chunks[0])?;
        let delivered = json!([body["choices"][0]["message"]]);
        let mut next = admit_execution_revision(&store, revision)?;
        let binding = next.bind_actual_attempt(
            "synthetic-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-principal"),
        )?;
        let handle = next.verify_native_delivered(&binding, chat(), &delivered)?;
        next.import_native_completed_context(&binding, chat(), &handle)?;
        assert!(
            next.resolve(&fixture.reference)? == TEXT,
            "native_history_live_control_missing"
        );
        let reference = fixture.reference.clone();
        drop(next);
        drop(chunks);
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_history_delivery_charge_retained"
        );
        store.expire_idle_at(Instant::now() + Duration::from_secs(60))?;
        assert!(store.stats()?.namespaces == 0, "native_history_not_expired");
        Ok(Self {
            store,
            delivered,
            reference,
            revision,
        })
    }

    pub fn current(&self) -> TestResult<(DlpRequestLease, ActualAttemptBinding)> {
        let lease = admit_execution_revision(&self.store, self.revision)?;
        let binding = lease.bind_actual_attempt(
            "synthetic-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-principal"),
        )?;
        assert!(
            lease.history_binding_live(&binding),
            "native_history_current_binding_invalid"
        );
        Ok((lease, binding))
    }

    pub fn prepare_current(&self, lease: DlpRequestLease) -> TestResult<NativeFixture> {
        NativeFixture::from_lease(
            self.store.clone(),
            lease,
            PayloadBudget::with_limit(LIMIT),
            true,
        )
    }
}
