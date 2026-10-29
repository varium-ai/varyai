//! Private pre-conversion response admission and payload ownership.
//!
//! Call only for an active restoration lease, before validation/history/conversion.
//! Raw and decoded buffers reserve before copying; emitted Bytes share their owners.
//! The separate processing owner must survive conversion, final delivery, and caches.
//! Its finite allowance is NOT a proven bound for all legacy converter amplification
//! or decoder-internal allocations. Those callers still require budget-aware copies.
use super::{
    http_restoration::DlpForwardError,
    payload_budget::{PayloadBudget, Reservation},
    payload_buffer::PayloadBuffer,
    payload_json::PayloadJson,
};
use crate::proxy::hyper_client::{ProxyResponse, MAX_RESPONSE_BODY_BYTES};
use bytes::Bytes;
use futures::StreamExt;
use http::{header, HeaderMap};
use std::{
    io::{self, Read},
    sync::{Arc, Mutex},
};

const MAX_RAW_CHUNK_BYTES: usize = 1024 * 1024;
const MAX_CUMULATIVE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENCODING_LAYERS: usize = 8;
const WORKSPACE_BASE_BYTES: usize = 64 * 1024;
// Four conservative <=1024n JSON-tree/buffer allowances. This does not prove
// arbitrary legacy conversion/cache amplification fits in four simultaneous copies.
const WORKSPACE_BYTES_PER_INPUT: usize = 4096;

struct ProcessingWorkspace {
    input_bytes: usize,
    reservation: Reservation,
    budget: PayloadBudget,
}

/// Share this owner with every downstream body/stream/cache retaining derived data.
/// Cloning shares one charge, not permission for an unlimited number of copies.
#[derive(Clone)]
pub(crate) struct UpstreamPayloadOwner {
    copy_charge: Option<Arc<Reservation>>,
    workspace: Arc<Mutex<ProcessingWorkspace>>,
}

impl UpstreamPayloadOwner {
    #[cfg(test)]
    pub(crate) fn test_fixture() -> Result<(Self, impl Fn() -> usize), DlpForwardError> {
        Self::test_fixture_with_limit(4 * 1024 * 1024)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_limit(
        limit: usize,
    ) -> Result<(Self, impl Fn() -> usize), DlpForwardError> {
        let budget = PayloadBudget::with_limit(limit);
        let owner = Self::new(&budget)?;
        Ok((owner, move || budget.used()))
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.workspace, &other.workspace)
            && match (&self.copy_charge, &other.copy_charge) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }

    pub(crate) fn chat_enrichment_budget(
        &self,
    ) -> Result<super::chat_enrichment::ChatEnrichmentBudget, DlpForwardError> {
        let budget = self
            .workspace
            .lock()
            .map_err(|_| DlpForwardError::Unavailable)?
            .budget
            .clone();
        Ok(super::chat_enrichment::ChatEnrichmentBudget::new(budget))
    }

    /// Reserve and rebuild one independent JSON allocation in the upstream budget.
    /// Keep the returned owner until the copied tree (or fields moved from it) drops.
    /// Cloning that owner shares this one charge; it does not authorize another copy.
    pub(crate) fn try_clone_json(
        &self,
        value: &serde_json::Value,
    ) -> Result<(serde_json::Value, Self), DlpForwardError> {
        let budget = self
            .workspace
            .lock()
            .map_err(|_| DlpForwardError::Unavailable)?
            .budget
            .clone();
        let limit = budget.limit();
        // The lock is released before sizing, reserving or building the copy.
        let copy = PayloadJson::try_from_value(value, budget, limit)
            .map_err(DlpForwardError::from_token_error)?;
        let (value, reservation) = copy.into_parts();
        Ok((
            value,
            Self {
                copy_charge: Some(Arc::new(reservation)),
                workspace: Arc::clone(&self.workspace),
            },
        ))
    }

    /// Encode a separate wire allocation in the same budget as guarded history.
    pub(crate) fn encode_outbound_body(
        &self,
        value: &serde_json::Value,
    ) -> Result<Bytes, DlpForwardError> {
        let budget = self
            .workspace
            .lock()
            .map_err(|_| DlpForwardError::Unavailable)?
            .budget
            .clone();
        super::http_restoration::encode_outbound_with_budget(value, budget)
    }

    fn new(budget: &PayloadBudget) -> Result<Self, DlpForwardError> {
        let reservation = budget
            .reserve(WORKSPACE_BASE_BYTES)
            .map_err(DlpForwardError::from_token_error)?;
        Ok(Self {
            copy_charge: None,
            workspace: Arc::new(Mutex::new(ProcessingWorkspace {
                input_bytes: 0,
                reservation,
                budget: budget.clone(),
            })),
        })
    }

    // No lock crosses allocation, decoding, a yield, or an I/O await.
    fn cover_input(&self, bytes: usize) -> Result<(), DlpForwardError> {
        if bytes > MAX_CUMULATIVE_BYTES || bytes > MAX_RESPONSE_BODY_BYTES {
            return Err(DlpForwardError::CapacityExceeded);
        }
        let mut state = self
            .workspace
            .lock()
            .map_err(|_| DlpForwardError::Unavailable)?;
        if bytes > state.input_bytes {
            let additional = (bytes - state.input_bytes)
                .checked_mul(WORKSPACE_BYTES_PER_INPUT)
                .ok_or(DlpForwardError::CapacityExceeded)?;
            state
                .reservation
                .grow(additional)
                .map_err(DlpForwardError::from_token_error)?;
            state.input_bytes = bytes;
        }
        Ok(())
    }
}

pub(super) struct GuardedUpstream {
    pub(super) response: ProxyResponse,
    pub(super) owner: UpstreamPayloadOwner,
}

struct OwnedPayload {
    // Drop the allocation/charge before the shared processing reservation.
    buffer: PayloadBuffer,
    _owner: UpstreamPayloadOwner,
}

impl AsRef<[u8]> for OwnedPayload {
    fn as_ref(&self) -> &[u8] {
        self.buffer.as_slice()
    }
}

#[derive(Clone, Copy)]
enum Coding {
    Identity,
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

fn content_codings(
    headers: &HeaderMap,
) -> Result<([Coding; MAX_ENCODING_LAYERS], usize), DlpForwardError> {
    let mut codings = [Coding::Identity; MAX_ENCODING_LAYERS];
    let mut count = 0;
    let mut total = 0;
    for header in headers.get_all(header::CONTENT_ENCODING) {
        let value = header
            .to_str()
            .map_err(|_| DlpForwardError::RestoreFailed)?;
        for coding in value.split(',').map(str::trim) {
            total += 1;
            if total > MAX_ENCODING_LAYERS || coding.is_empty() {
                return Err(DlpForwardError::RestoreFailed);
            }
            let coding = if coding.eq_ignore_ascii_case("identity") {
                continue;
            } else if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
                Coding::Gzip
            } else if coding.eq_ignore_ascii_case("deflate") {
                Coding::Deflate
            } else if coding.eq_ignore_ascii_case("br") {
                Coding::Brotli
            } else if coding.eq_ignore_ascii_case("zstd") || coding.eq_ignore_ascii_case("zst") {
                Coding::Zstd
            } else {
                return Err(DlpForwardError::RestoreFailed);
            };
            codings[count] = coding;
            count += 1;
        }
    }
    Ok((codings, count))
}

fn is_sse(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

fn clear_entity_headers(headers: &mut HeaderMap) {
    for name in [
        "content-encoding",
        "content-length",
        "transfer-encoding",
        "etag",
        "digest",
        "content-md5",
        "content-digest",
        "repr-digest",
    ] {
        headers.remove(name);
    }
}

fn charged_chunk(
    chunk: Bytes,
    budget: &PayloadBudget,
    owner: &UpstreamPayloadOwner,
) -> Result<Bytes, DlpForwardError> {
    // A transport has already produced this bounded chunk. Charge its logical bytes
    // while copying into an exact-capacity owner; never retain a possibly huge slice
    // backing allocation supplied by an untrusted transport.
    let incoming_charge = budget
        .reserve(chunk.len())
        .map_err(DlpForwardError::from_token_error)?;
    let buffer = PayloadBuffer::try_from_slice(&chunk, budget.clone(), MAX_RAW_CHUNK_BYTES)
        .map_err(DlpForwardError::from_token_error)?;
    drop(chunk);
    drop(incoming_charge);
    Ok(Bytes::from_owner(OwnedPayload {
        buffer,
        _owner: owner.clone(),
    }))
}

fn next_input_size(current: usize, chunk: usize) -> Result<usize, DlpForwardError> {
    if chunk > MAX_RAW_CHUNK_BYTES {
        return Err(DlpForwardError::CapacityExceeded);
    }
    current
        .checked_add(chunk)
        .filter(|bytes| *bytes <= MAX_CUMULATIVE_BYTES.min(MAX_RESPONSE_BODY_BYTES))
        .ok_or(DlpForwardError::CapacityExceeded)
}

fn read_decoded(
    mut reader: impl Read,
    budget: &PayloadBudget,
    owner: &UpstreamPayloadOwner,
) -> Result<PayloadBuffer, DlpForwardError> {
    let limit = MAX_CUMULATIVE_BYTES.min(MAX_RESPONSE_BODY_BYTES);
    let mut output =
        PayloadBuffer::new(budget.clone(), limit).map_err(DlpForwardError::from_token_error)?;
    let mut scratch = [0u8; 8192];
    loop {
        // One stack byte past the cap distinguishes exact-cap EOF from overrun.
        let capacity = scratch.len().min(limit - output.len() + 1);
        let count = match reader.read(&mut scratch[..capacity]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result.map_err(|_| DlpForwardError::RestoreFailed)?,
        };
        if count == 0 {
            return Ok(output);
        }
        let next = output
            .len()
            .checked_add(count)
            .filter(|bytes| *bytes <= limit)
            .ok_or(DlpForwardError::CapacityExceeded)?;
        owner.cover_input(next)?;
        output
            .append(&scratch[..count])
            .map_err(DlpForwardError::from_token_error)?;
    }
}

fn decode_one(
    coding: Coding,
    input: &[u8],
    budget: &PayloadBudget,
    owner: &UpstreamPayloadOwner,
) -> Result<PayloadBuffer, DlpForwardError> {
    match coding {
        Coding::Gzip => read_decoded(flate2::read::MultiGzDecoder::new(input), budget, owner),
        Coding::Deflate => {
            match read_decoded(flate2::read::ZlibDecoder::new(input), budget, owner) {
                // Capacity failures must not become a second unbudgeted decoding attempt.
                Err(DlpForwardError::RestoreFailed) => {
                    read_decoded(flate2::read::DeflateDecoder::new(input), budget, owner)
                }
                result => result,
            }
        }
        Coding::Brotli => read_decoded(brotli::Decompressor::new(input, 4096), budget, owner),
        Coding::Zstd => {
            let mut decoder = zstd::stream::read::Decoder::new(input)
                .map_err(|_| DlpForwardError::RestoreFailed)?;
            // Bound a remote frame's advertised window; internal decoder allocations
            // are not PayloadBuffer allocations and remain a separate accounting gap.
            decoder
                .window_log_max(23)
                .map_err(|_| DlpForwardError::RestoreFailed)?;
            read_decoded(decoder, budget, owner)
        }
        Coding::Identity => Err(DlpForwardError::RestoreFailed),
    }
}

pub(super) async fn guard_upstream_response(
    response: ProxyResponse,
    budget: &PayloadBudget,
) -> Result<GuardedUpstream, DlpForwardError> {
    let (codings, coding_count) = content_codings(response.headers())?;
    let streamed = is_sse(response.headers());
    if streamed && coding_count != 0 {
        return Err(DlpForwardError::RestoreFailed);
    }
    let status = response.status();
    let mut headers = response.headers().clone();
    let owner = UpstreamPayloadOwner::new(budget)?;
    if streamed {
        let stream_owner = owner.clone();
        let stream_budget = budget.clone();
        let guarded_stream = async_stream::stream! {
            let mut upstream = Box::pin(response.bytes_stream());
            let mut total = 0usize;
            while let Some(chunk) = upstream.next().await {
                let checked = (|| {
                    let chunk = chunk.map_err(|_| DlpForwardError::RestoreFailed)?;
                    let next = next_input_size(total, chunk.len())?;
                    stream_owner.cover_input(next)?;
                    let bytes = charged_chunk(chunk, &stream_budget, &stream_owner)?;
                    total = next;
                    Ok::<_, DlpForwardError>(bytes)
                })();
                match checked {
                    Ok(bytes) => yield Ok(bytes),
                    Err(error) => {
                        yield Err(io::Error::other(error.code()));
                        break;
                    }
                }
            }
        };
        return Ok(GuardedUpstream {
            response: ProxyResponse::streamed(status, headers, guarded_stream),
            owner,
        });
    }

    let mut body = PayloadBuffer::new(
        budget.clone(),
        MAX_CUMULATIVE_BYTES.min(MAX_RESPONSE_BODY_BYTES),
    )
    .map_err(DlpForwardError::from_token_error)?;
    let mut stream = Box::pin(response.bytes_stream());
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| DlpForwardError::RestoreFailed)?;
        let next = next_input_size(body.len(), chunk.len())?;
        owner.cover_input(next)?;
        let incoming_charge = budget
            .reserve(chunk.len())
            .map_err(DlpForwardError::from_token_error)?;
        body.append(&chunk)
            .map_err(DlpForwardError::from_token_error)?;
        drop(chunk);
        drop(incoming_charge);
    }
    drop(stream);
    for coding in codings[..coding_count].iter().rev() {
        // Old and new buffers are separately charged until replacement completes.
        body = decode_one(*coding, body.as_slice(), budget, &owner)?;
    }
    if coding_count != 0 {
        clear_entity_headers(&mut headers);
    }
    let bytes = Bytes::from_owner(OwnedPayload {
        buffer: body,
        _owner: owner.clone(),
    });
    Ok(GuardedUpstream {
        response: ProxyResponse::buffered(status, headers, bytes),
        owner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use http::{header, HeaderMap, HeaderValue, StatusCode};
    use std::{
        io::{self, Write},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn headers(content_type: &'static str, encoding: Option<&'static str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        if let Some(encoding) = encoding {
            headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
        }
        headers
    }

    fn failure_code(result: Result<GuardedUpstream, DlpForwardError>) -> &'static str {
        match result {
            Ok(_) => "unexpected_success",
            Err(error) => error.code(),
        }
    }

    async fn admitted(
        response: ProxyResponse,
        budget: &PayloadBudget,
    ) -> Result<GuardedUpstream, io::Error> {
        guard_upstream_response(response, budget)
            .await
            .map_err(|error| io::Error::other(error.code()))
    }

    fn gzip(payload: &[u8]) -> Result<Vec<u8>, io::Error> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(payload)?;
        encoder.finish()
    }

    #[test]
    fn upstream_json_copies_charge_before_build_and_keep_distinct_owner_identity() -> TestResult {
        use super::super::payload_json::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};

        let (owner, used) =
            UpstreamPayloadOwner::test_fixture().map_err(|error| io::Error::other(error.code()))?;
        let workspace_charge = used();
        let allocations = Rc::new(Cell::new(0));
        let observed = Rc::clone(&allocations);
        let _hook = test_support::install(move |event| {
            if matches!(event.phase, Phase::BeforeBuild | Phase::BeforeAllocation) {
                assert!(event.charged > 0);
                assert!(event.used >= workspace_charge + event.charged);
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let source = serde_json::json!({"synthetic": ["vary_cu_00000000000000000000_ai"]});
        let (first, first_owner) = owner
            .try_clone_json(&source)
            .map_err(|error| io::Error::other(error.code()))?;
        let one_copy_charge = used() - workspace_charge;
        let (second, second_owner) = first_owner
            .try_clone_json(&source)
            .map_err(|error| io::Error::other(error.code()))?;
        assert_eq!(used(), workspace_charge + 2 * one_copy_charge);
        assert!(!first_owner.ptr_eq(&owner));
        assert!(!first_owner.ptr_eq(&second_owner));
        let shared = first_owner.clone();
        assert!(shared.ptr_eq(&first_owner));
        assert_eq!(used(), workspace_charge + 2 * one_copy_charge);
        assert!(allocations.get() > 0);
        assert_eq!(first, source);
        assert_eq!(second, source);
        drop(owner);
        drop(first);
        drop(first_owner);
        assert_eq!(used(), workspace_charge + 2 * one_copy_charge);
        drop(shared);
        assert_eq!(used(), workspace_charge + one_copy_charge);
        drop(second);
        drop(second_owner);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[test]
    fn upstream_json_copy_capacity_rejects_before_build_and_build_failure_rolls_back() -> TestResult
    {
        use super::super::payload_json::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};

        let (owner, used) = UpstreamPayloadOwner::test_fixture_with_limit(WORKSPACE_BASE_BYTES)
            .map_err(|error| io::Error::other(error.code()))?;
        let builds = Rc::new(Cell::new(0));
        let observed = Rc::clone(&builds);
        let hook = test_support::install(move |event| {
            if matches!(event.phase, Phase::BeforeBuild | Phase::BeforeAllocation) {
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let source = serde_json::json!({"synthetic": "vary_cu_00000000000000000000_ai"});
        assert!(matches!(
            owner.try_clone_json(&source),
            Err(DlpForwardError::CapacityExceeded)
        ));
        assert_eq!(
            builds.get(),
            0,
            "capacity admission must precede JSON build"
        );
        assert_eq!(used(), WORKSPACE_BASE_BYTES);
        drop(hook);
        drop(owner);
        assert_eq!(used(), 0);

        let (owner, used) =
            UpstreamPayloadOwner::test_fixture().map_err(|error| io::Error::other(error.code()))?;
        let hook = test_support::install(|event| {
            if event.phase == Phase::AfterBuild {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        assert!(matches!(
            owner.try_clone_json(&source),
            Err(DlpForwardError::Unavailable)
        ));
        assert_eq!(used(), WORKSPACE_BASE_BYTES);
        drop(hook);
        drop(owner);
        assert_eq!(used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_rejects_compressed_sse_before_poll_for_all_statuses() {
        for status in [StatusCode::OK, StatusCode::BAD_REQUEST] {
            for encoding in ["gzip", "br", "identity, gzip", "unknown", "IDENTITY, ZSTD"] {
                let polls = Arc::new(AtomicUsize::new(0));
                let observed = Arc::clone(&polls);
                let response = ProxyResponse::streamed(
                    status,
                    headers("text/event-stream", Some(encoding)),
                    stream::poll_fn(move |_| {
                        observed.fetch_add(1, Ordering::Relaxed);
                        std::task::Poll::Ready(Some(Ok(Bytes::from_static(b"not-read"))))
                    }),
                );
                let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                assert_eq!(
                    failure_code(guard_upstream_response(response, &budget).await),
                    "dlp_restore_failed"
                );
                assert_eq!(polls.load(Ordering::Relaxed), 0);
                assert_eq!(budget.used(), 0);
            }
        }
    }

    #[tokio::test]
    async fn upstream_guard_json_owner_survives_replay_and_independent_clones() -> TestResult {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let input = Bytes::from_static(br#"{"synthetic":"value"}"#);
        let guarded = admitted(
            ProxyResponse::buffered(
                StatusCode::CREATED,
                headers("application/json", None),
                input.clone(),
            ),
            &budget,
        )
        .await?;
        assert!(budget.used() > input.len());
        assert_eq!(guarded.response.status(), StatusCode::CREATED);
        let retained = guarded.owner.clone();
        let body = guarded.response.bytes_with_limit(1024).await?;
        assert_eq!(body, input);
        drop(guarded.owner);
        drop(body);
        assert!(
            budget.used() > 0,
            "processing/cache owner must outlive raw replay"
        );
        drop(retained);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_replay_bytes_alone_keep_charges_alive() -> TestResult {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let guarded = admitted(
            ProxyResponse::buffered(
                StatusCode::OK,
                headers("application/json", None),
                Bytes::from_static(b"{}"),
            ),
            &budget,
        )
        .await?;
        let mut stream = Box::pin(guarded.response.bytes_stream());
        let first = stream
            .next()
            .await
            .ok_or_else(|| io::Error::other("missing replay"))??;
        let last_clone = first.clone();
        drop(first);
        drop(stream);
        drop(guarded.owner);
        assert!(budget.used() > 0);
        drop(last_clone);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_sse_is_lazy_preserves_raw_chunks_and_retains_cumulative_allowance(
    ) -> TestResult {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&polls);
        let chunks = [
            Bytes::from_static(b"data: {\"x\":\""),
            Bytes::from_static(b"synthetic\"}\r\n\r\n"),
        ];
        let mut source = chunks.clone().into_iter();
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            headers("text/event-stream; charset=utf-8", Some("IDENTITY")),
            stream::poll_fn(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                std::task::Poll::Ready(source.next().map(Ok))
            }),
        );
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let guarded = admitted(response, &budget).await?;
        assert_eq!(polls.load(Ordering::Relaxed), 0);
        let initial = budget.used();
        let mut source = Box::pin(guarded.response.bytes_stream());
        for expected in chunks {
            let bytes = source
                .next()
                .await
                .ok_or_else(|| io::Error::other("missing chunk"))??;
            assert_eq!(bytes, expected);
            drop(bytes);
        }
        assert_eq!(polls.load(Ordering::Relaxed), 2);
        drop(source);
        assert!(
            budget.used() > initial,
            "cumulative workspace must not disappear with yielded chunks"
        );
        drop(guarded.owner);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_rejects_oversized_transport_chunks_before_retaining() {
        for content_type in ["application/json", "text/event-stream"] {
            let budget = PayloadBudget::with_limit(usize::MAX);
            let response = ProxyResponse::buffered(
                StatusCode::OK,
                headers(content_type, None),
                Bytes::from(vec![b'x'; 1024 * 1024 + 1]),
            );
            let code = match guard_upstream_response(response, &budget).await {
                Err(error) => error.code(),
                Ok(guarded) => {
                    let mut stream = Box::pin(guarded.response.bytes_stream());
                    match stream.next().await {
                        Some(Err(error)) => {
                            assert_eq!(error.to_string(), "dlp_capacity_exceeded");
                            "dlp_capacity_exceeded"
                        }
                        _ => "unexpected_success",
                    }
                }
            };
            assert_eq!(code, "dlp_capacity_exceeded");
            assert_eq!(budget.used(), 0);
        }
    }

    #[tokio::test]
    async fn upstream_guard_capacity_failure_and_cancellation_release_every_charge() -> TestResult {
        let budget = PayloadBudget::with_limit(128);
        let response = ProxyResponse::buffered(
            StatusCode::OK,
            headers("application/json", None),
            Bytes::from_static(b"{}"),
        );
        assert_eq!(
            failure_code(guard_upstream_response(response, &budget).await),
            "dlp_capacity_exceeded"
        );
        assert_eq!(budget.used(), 0);

        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            headers("application/json", None),
            stream::once(async { Ok(Bytes::from_static(b"{\"synthetic\":")) })
                .chain(stream::pending()),
        );
        let timed_out = tokio::time::timeout(
            std::time::Duration::from_millis(10),
            guard_upstream_response(response, &budget),
        )
        .await;
        assert!(timed_out.is_err());
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_decodes_gzip_and_removes_stale_entity_headers() -> TestResult {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let payload = br#"{"synthetic":"compressed"}"#;
        let mut response_headers = headers("application/json", Some("gzip"));
        for name in [
            "content-length",
            "transfer-encoding",
            "etag",
            "digest",
            "content-md5",
        ] {
            response_headers.insert(
                http::HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_static("synthetic"),
            );
        }
        response_headers.insert("x-synthetic", HeaderValue::from_static("preserved"));
        let response = ProxyResponse::buffered(
            StatusCode::OK,
            response_headers,
            Bytes::from(gzip(payload)?),
        );
        let guarded = admitted(response, &budget).await?;
        for name in [
            "content-encoding",
            "content-length",
            "transfer-encoding",
            "etag",
            "digest",
            "content-md5",
        ] {
            assert!(
                !guarded.response.headers().contains_key(name),
                "stale entity header"
            );
        }
        assert_eq!(
            guarded.response.headers().get("x-synthetic"),
            Some(&HeaderValue::from_static("preserved"))
        );
        let mut stream = Box::pin(guarded.response.bytes_stream());
        assert_eq!(
            stream
                .next()
                .await
                .ok_or_else(|| io::Error::other("missing decoded body"))??
                .as_ref(),
            payload
        );
        drop(stream);
        assert!(budget.used() > 0);
        drop(guarded.owner);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_unknown_or_corrupt_encoding_fails_closed() {
        for encoding in ["unknown", "gzip", "gzip, unknown", "gzip,,zstd", ""] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let response = ProxyResponse::buffered(
                StatusCode::BAD_GATEWAY,
                headers("application/json", Some(encoding)),
                Bytes::from_static(b"synthetic invalid compressed body"),
            );
            assert_eq!(
                failure_code(guard_upstream_response(response, &budget).await),
                "dlp_restore_failed"
            );
            assert_eq!(budget.used(), 0);
        }
    }

    #[tokio::test]
    async fn upstream_guard_decompression_bomb_fails_under_small_payload_budget() -> TestResult {
        let budget = PayloadBudget::with_limit(512 * 1024);
        let response = ProxyResponse::buffered(
            StatusCode::OK,
            headers("application/json", Some("gzip")),
            Bytes::from(gzip(&vec![b'x'; 1024 * 1024])?),
        );
        assert_eq!(
            failure_code(guard_upstream_response(response, &budget).await),
            "dlp_capacity_exceeded"
        );
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn upstream_guard_sanitizes_io_errors_before_and_after_stream_headers() -> TestResult {
        for content_type in ["application/json", "text/event-stream"] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let response = ProxyResponse::streamed(
                StatusCode::OK,
                headers(content_type, None),
                stream::once(async { Err(io::Error::other("synthetic private transport detail")) }),
            );
            if content_type == "application/json" {
                assert_eq!(
                    failure_code(guard_upstream_response(response, &budget).await),
                    "dlp_restore_failed"
                );
            } else {
                let guarded = admitted(response, &budget).await?;
                let mut stream = Box::pin(guarded.response.bytes_stream());
                let error = stream
                    .next()
                    .await
                    .ok_or_else(|| io::Error::other("missing stream failure"))?
                    .err()
                    .ok_or_else(|| io::Error::other("unexpected stream success"))?;
                assert_eq!(error.to_string(), "dlp_restore_failed");
                assert!(stream.next().await.is_none());
                drop(stream);
                drop(guarded.owner);
            }
            assert_eq!(budget.used(), 0);
        }
        Ok(())
    }
}
