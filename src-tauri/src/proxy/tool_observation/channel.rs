//! Non-blocking, byte- and item-bounded response observation ingress.
//! Forwarding only clones immutable Bytes; parsing lives in the worker.
use bytes::Bytes;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc, OnceLock,
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

pub const DEFAULT_QUEUE_CAPACITY: usize = 4096;
pub const MAX_PIECE_BYTES: usize = 1024 * 1024;
pub const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ACTIVE_BYTES: usize = 64 * 1024 * 1024;
// Reserve one transport buffer (1 MiB) + assembler (2 MiB) per response.
pub const RESPONSE_RESERVATION: usize = 3 * 1024 * 1024;
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Json,
    Sse,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutputQuality {
    Native,
    ConvertedUnknown,
}
#[derive(Clone)]
pub struct ToolSchemaRef {
    pub name: String,
    pub namespace: Option<String>,
    pub schema: &'static str,
}
#[derive(Clone)]
pub struct ResponseMetadata {
    pub protocol: &'static str,
    pub client: &'static str,
    pub transport: Transport,
    pub quality: OutputQuality,
    pub definitions: Arc<Vec<ToolSchemaRef>>,
}
#[derive(Default)]
pub struct ObservationCounters {
    pub delivered: AtomicU64,
    pub dropped_queue_full: AtomicU64,
    pub dropped_closed: AtomicU64,
    pub dropped_budget: AtomicU64,
    pub coverage_fault: AtomicU64,
}
pub struct CounterSnapshot {
    pub delivered: u64,
    pub dropped_queue_full: u64,
    pub dropped_closed: u64,
    pub dropped_budget: u64,
}
impl ObservationCounters {
    pub fn snapshot(&self) -> CounterSnapshot {
        CounterSnapshot {
            delivered: self.delivered.load(Ordering::Relaxed),
            dropped_queue_full: self.dropped_queue_full.load(Ordering::Relaxed),
            dropped_closed: self.dropped_closed.load(Ordering::Relaxed),
            dropped_budget: self.dropped_budget.load(Ordering::Relaxed),
        }
    }
}
// No raw payload in this state. End/Drop and ingress failure do not need queue space.
pub struct ResponseState {
    pub id: u64,
    pub policy_version: u64,
    pub metadata: ResponseMetadata,
    pub faulted: AtomicBool,
    fault_reason: AtomicU8,
    diagnostic_logged: AtomicBool,
    pub closed: AtomicBool,
    pub sent: AtomicU64,
    _active: std::sync::Mutex<Option<OwnedSemaphorePermit>>,
}
impl ResponseState {
    pub(crate) fn take_fault_diagnostic(&self) -> bool {
        !self.diagnostic_logged.swap(true, Ordering::Relaxed)
    }
    /// Numeric first-failure marker only. Forwarding never formats/writes logs.
    pub(crate) fn fault_reason(&self) -> &'static str {
        match self.fault_reason.load(Ordering::Acquire) {
            1 => "transport_error",
            2 => "piece_too_large",
            3 => "queue_byte_budget",
            4 => "queue_full",
            5 => "queue_closed",
            6 => "collection_disabled",
            7 => "policy_changed",
            _ => "response_faulted",
        }
    }
    // Background-only: the forwarding path never locks to release this reservation.
    pub fn release_reservation(&self) {
        if let Ok(mut permit) = self._active.lock() {
            permit.take();
        }
    }
}
pub struct RawPiece {
    pub state: Arc<ResponseState>,
    pub sequence: u64,
    pub bytes: Bytes,
    _bytes: OwnedSemaphorePermit,
}
#[derive(Clone)]
pub struct ObservationSender {
    tx: mpsc::Sender<RawPiece>,
    bytes: Arc<Semaphore>,
    active: Arc<Semaphore>,
    counters: Arc<ObservationCounters>,
}
pub struct ObservationReceiver {
    rx: mpsc::Receiver<RawPiece>,
    pub counters: Arc<ObservationCounters>,
}
impl ObservationReceiver {
    pub async fn recv(&mut self) -> Option<RawPiece> {
        self.rx.recv().await
    }
}
pub fn create_channel(capacity: usize) -> (ObservationSender, ObservationReceiver) {
    create_channel_with_budget(capacity, MAX_QUEUE_BYTES, MAX_ACTIVE_BYTES)
}
fn create_channel_with_budget(
    capacity: usize,
    bytes: usize,
    active: usize,
) -> (ObservationSender, ObservationReceiver) {
    let (tx, rx) = mpsc::channel(capacity.max(1));
    let counters = Arc::new(ObservationCounters::default());
    (
        ObservationSender {
            tx,
            bytes: Arc::new(Semaphore::new(bytes)),
            active: Arc::new(Semaphore::new(active)),
            counters: counters.clone(),
        },
        ObservationReceiver { rx, counters },
    )
}
pub struct ResponseTap {
    sender: ObservationSender,
    pub state: Arc<ResponseState>,
    sequence: u64,
    policy_bound: bool,
}
impl ObservationSender {
    pub fn start(&self, metadata: ResponseMetadata, policy_version: u64) -> Option<ResponseTap> {
        if self.tx.is_closed() {
            return None;
        }
        let permit = match self
            .active
            .clone()
            .try_acquire_many_owned(RESPONSE_RESERVATION as u32)
        {
            Ok(p) => p,
            Err(_) => {
                self.counters.dropped_budget.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        Some(ResponseTap {
            sender: self.clone(),
            state: Arc::new(ResponseState {
                id: NEXT_SCOPE.fetch_add(1, Ordering::Relaxed),
                policy_version,
                metadata,
                faulted: AtomicBool::new(false),
                fault_reason: AtomicU8::new(0),
                diagnostic_logged: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                sent: AtomicU64::new(0),
                _active: std::sync::Mutex::new(Some(permit)),
            }),
            sequence: 0,
            policy_bound: false,
        })
    }
    #[cfg(test)]
    pub fn counters(&self) -> Arc<ObservationCounters> {
        self.counters.clone()
    }
}
impl ResponseTap {
    pub fn push(&mut self, bytes: &Bytes) {
        if self.policy_bound {
            use crate::services::tool_risk::runtime::{
                collection_enabled, collection_policy_version,
            };
            self.push_with_policy(bytes, collection_enabled(), collection_policy_version());
        } else {
            self.enqueue(bytes);
        }
    }
    fn enqueue(&mut self, bytes: &Bytes) {
        if self.state.faulted.load(Ordering::Acquire) {
            return;
        }
        if bytes.len() > MAX_PIECE_BYTES {
            self.sender
                .counters
                .dropped_budget
                .fetch_add(1, Ordering::Relaxed);
            self.fail_with_reason(2);
            return;
        }
        let permit = match self
            .sender
            .bytes
            .clone()
            .try_acquire_many_owned(bytes.len() as u32)
        {
            Ok(p) => p,
            Err(_) => {
                self.sender
                    .counters
                    .dropped_budget
                    .fetch_add(1, Ordering::Relaxed);
                self.fail_with_reason(3);
                return;
            }
        };
        let sequence = self.sequence;
        let piece = RawPiece {
            state: self.state.clone(),
            sequence,
            bytes: bytes.clone(),
            _bytes: permit,
        };
        match self.sender.tx.try_send(piece) {
            Ok(()) => {
                self.sequence += 1;
                self.state.sent.store(self.sequence, Ordering::Release);
                self.sender
                    .counters
                    .delivered
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.sender
                    .counters
                    .dropped_queue_full
                    .fetch_add(1, Ordering::Relaxed);
                self.fail_with_reason(4);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.sender
                    .counters
                    .dropped_closed
                    .fetch_add(1, Ordering::Relaxed);
                self.fail_with_reason(5);
            }
        }
    }
    fn push_with_policy(&mut self, bytes: &Bytes, enabled: bool, policy: u64) {
        if !enabled || policy != self.state.policy_version {
            self.fail_with_reason(if !enabled { 6 } else { 7 });
            return;
        }
        self.enqueue(bytes);
    }
    pub fn fail(&self) {
        self.fail_with_reason(1);
    }
    fn fail_with_reason(&self, reason: u8) {
        let _ = self.state.fault_reason.compare_exchange(
            0,
            reason,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.state.faulted.store(true, Ordering::Release);
        // No successful piece means no worker session owns this reservation.
        // Never block forwarding; sessions with queued bytes are reclaimed by the worker.
        if self.sequence == 0 {
            if let Ok(mut permit) = self.state._active.try_lock() {
                permit.take();
            }
        }
    }
}
impl Drop for ResponseTap {
    fn drop(&mut self) {
        self.state.closed.store(true, Ordering::Release);
    }
}
static GLOBAL_SENDER: OnceLock<ObservationSender> = OnceLock::new();
pub fn install_global_sender(sender: ObservationSender) -> bool {
    GLOBAL_SENDER.set(sender).is_ok()
}
pub fn start_response(metadata: ResponseMetadata) -> Option<ResponseTap> {
    use crate::services::tool_risk::runtime::{collection_enabled, collection_policy_version};
    if !collection_enabled() {
        return None;
    }
    let mut tap = GLOBAL_SENDER
        .get()?
        .start(metadata, collection_policy_version())?;
    tap.policy_bound = true;
    Some(tap)
}
#[cfg(test)]
pub async fn test_channel() -> (
    ObservationSender,
    tokio::sync::MutexGuard<'static, ObservationReceiver>,
) {
    static CHANNEL: OnceLock<(ObservationSender, tokio::sync::Mutex<ObservationReceiver>)> =
        OnceLock::new();
    let (sender, receiver) = CHANNEL.get_or_init(|| {
        let (sender, receiver) = create_channel(DEFAULT_QUEUE_CAPACITY);
        assert!(install_global_sender(sender.clone()));
        (sender, tokio::sync::Mutex::new(receiver))
    });
    let mut receiver = receiver.lock().await;
    while receiver.rx.try_recv().is_ok() {}
    (sender.clone(), receiver)
}

#[cfg(test)]
mod budget_regression_tests {
    use super::*;
    fn metadata() -> ResponseMetadata {
        ResponseMetadata {
            protocol: "anthropic_messages",
            client: "claude",
            transport: Transport::Sse,
            quality: OutputQuality::Native,
            definitions: Arc::new(Vec::new()),
        }
    }
    #[test]
    fn diagnostic_fault_reason_preserves_first_failure() {
        let (tx, _rx) = create_channel(1);
        let mut tap = tx.start(metadata(), 1).unwrap();
        tap.push(&Bytes::from_static(b"PRIVATE_FIRST_PAYLOAD"));
        tap.push(&Bytes::from_static(b"PRIVATE_SECOND_PAYLOAD"));
        assert_eq!(tap.state.fault_reason(), "queue_full");
        tap.fail();
        assert_eq!(tap.state.fault_reason(), "queue_full");
        assert_eq!(tap.state.sent.load(Ordering::Acquire), 1);
        assert_eq!(tx.counters().snapshot().delivered, 1);
    }

    #[test]
    fn diagnostic_fault_reasons_distinguish_transport_budget_and_policy() {
        for (kind, expected) in [
            ("oversized", "piece_too_large"),
            ("policy", "policy_changed"),
            ("error", "transport_error"),
        ] {
            let (tx, _rx) = create_channel(2);
            let mut tap = tx.start(metadata(), 1).unwrap();
            match kind {
                "oversized" => tap.push(&Bytes::from(vec![0; MAX_PIECE_BYTES + 1])),
                "policy" => tap.push_with_policy(&Bytes::from_static(b"PRIVATE"), true, 2),
                _ => tap.fail(),
            }
            assert_eq!(tap.state.fault_reason(), expected);
            assert!(tap.state.faulted.load(Ordering::Acquire));
            assert_eq!(tx.counters().snapshot().delivered, 0);
        }
    }

    #[test]
    fn oversized_payload_is_not_queued() {
        let (tx, _rx) = create_channel(2);
        let mut tap = tx.start(metadata(), 1).unwrap();
        tap.push(&Bytes::from(vec![0; MAX_PIECE_BYTES + 1]));
        assert_eq!(tx.counters().snapshot().delivered, 0);
        assert!(tap.state.faulted.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn byte_budget_releases_on_consumption() {
        let (tx, mut rx) = create_channel_with_budget(8, 8, RESPONSE_RESERVATION);
        let mut tap = tx.start(metadata(), 1).unwrap();
        tap.push(&Bytes::from_static(b"12345678"));
        assert_eq!(tx.bytes.available_permits(), 0);
        drop(rx.recv().await);
        assert_eq!(tx.bytes.available_permits(), 8);
        tap.push(&Bytes::from_static(b"12345678"));
        tap.push(&Bytes::from_static(b"x"));
        assert!(tap.state.faulted.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn full_queue_and_drop_signal_cannot_be_lost() {
        let (tx, mut rx) = create_channel(1);
        let mut tap = tx.start(metadata(), 1).unwrap();
        let state = tap.state.clone();
        tap.push(&Bytes::from_static(b"a"));
        tap.push(&Bytes::from_static(b"b"));
        drop(tap);
        assert!(state.closed.load(Ordering::Acquire));
        assert!(state.faulted.load(Ordering::Acquire));
        assert_eq!(tx.counters().snapshot().dropped_queue_full, 1);
        assert_eq!(rx.recv().await.unwrap().sequence, 0);
    }
    #[test]
    fn global_active_budget_refuses_unbounded_responses() {
        let (tx, _rx) = create_channel_with_budget(8, 8, RESPONSE_RESERVATION);
        let tap = tx.start(metadata(), 1).unwrap();
        assert!(tx.start(metadata(), 1).is_none());
        drop(tap);
        assert!(tx.start(metadata(), 1).is_some());
    }
    #[test]
    fn disabled_or_changed_policy_stops_existing_tap_before_cloning() {
        for (enabled, policy) in [(false, 1), (true, 2)] {
            let (tx, mut rx) = create_channel(2);
            let mut tap = tx.start(metadata(), 1).unwrap();
            tap.push_with_policy(&Bytes::from_static(b"private"), enabled, policy);
            assert_eq!(tx.counters().snapshot().delivered, 0);
            assert!(rx.rx.try_recv().is_err());
            assert!(tap.state.faulted.load(Ordering::Acquire));
        }
    }
    #[test]
    fn rejected_first_piece_releases_reservation_without_dropping_tap() {
        for rejection in [
            "oversized",
            "byte_budget",
            "queue_full",
            "closed",
            "disabled",
            "policy",
        ] {
            let (tx, mut rx) = create_channel_with_budget(1, 8, RESPONSE_RESERVATION * 2);
            let mut occupying = tx.start(metadata(), 1).unwrap();
            if rejection == "queue_full" {
                occupying.push(&Bytes::from_static(b"a"));
            }
            let mut tap = tx.start(metadata(), 1).unwrap();
            if rejection == "closed" {
                rx.rx.close();
            }
            match rejection {
                "oversized" => tap.push(&Bytes::from(vec![0; MAX_PIECE_BYTES + 1])),
                "byte_budget" => tap.push(&Bytes::from_static(b"123456789")),
                "disabled" => tap.push_with_policy(&Bytes::from_static(b"a"), false, 1),
                "policy" => tap.push_with_policy(&Bytes::from_static(b"a"), true, 2),
                _ => tap.push(&Bytes::from_static(b"a")),
            }
            assert!(tap.state.faulted.load(Ordering::Acquire), "{rejection}");
            assert_eq!(tap.state.sent.load(Ordering::Acquire), 0, "{rejection}");
            assert_eq!(
                tx.active.available_permits(),
                RESPONSE_RESERVATION,
                "{rejection}"
            );
        }
    }

    #[test]
    fn fault_after_successful_piece_keeps_reservation_until_worker_releases() {
        let (tx, _rx) = create_channel_with_budget(1, 8, RESPONSE_RESERVATION);
        let mut tap = tx.start(metadata(), 1).unwrap();
        tap.push(&Bytes::from_static(b"a"));
        tap.push(&Bytes::from_static(b"b"));
        assert!(tap.state.faulted.load(Ordering::Acquire));
        assert_eq!(tap.state.sent.load(Ordering::Acquire), 1);
        assert_eq!(tx.active.available_permits(), 0);
        tap.state.release_reservation();
        assert_eq!(tx.active.available_permits(), RESPONSE_RESERVATION);
    }

    #[test]
    fn first_piece_rejection_does_not_wait_for_reservation_lock() {
        let (tx, _rx) = create_channel_with_budget(1, 8, RESPONSE_RESERVATION);
        let tap = tx.start(metadata(), 1).unwrap();
        let _held = tap.state._active.lock().unwrap();
        tap.fail();
        assert!(tap.state.faulted.load(Ordering::Acquire));
    }
}
