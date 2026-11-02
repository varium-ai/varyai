//! Private, memory-only primitives for reversible DLP references.
//! Recognition never grants authorization to restore a reference.
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{fmt, ops::Range};

use super::token_format::{self, MAX_REFERENCE_BYTES as TOKEN_BYTES};

pub(super) use super::token_format::TokenCategory;

/// The caller supplies only categories covering the complete merged span.
/// An empty set means no category covers that span, so it is mixed.
pub(super) fn select_category(whole_span_categories: &[TokenCategory]) -> TokenCategory {
    use TokenCategory::*;
    [
        PrivateKey,
        Database,
        ApiKey,
        Password,
        IdCard,
        Phone,
        Infrastructure,
        Personal,
        Custom,
    ]
    .into_iter()
    .find(|category| whole_span_categories.contains(category))
    .unwrap_or(Mixed)
}

/// Errors carry no input, key, token, session identity or parser detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TokenError {
    Unavailable,
    ContextUnavailable,
    CapacityExceeded,
    TokenCollision,
    RedactionFailed,
}

impl TokenError {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "dlp_unavailable",
            Self::ContextUnavailable => "dlp_context_unavailable",
            Self::CapacityExceeded => "dlp_capacity_exceeded",
            Self::TokenCollision => "dlp_token_collision",
            Self::RedactionFailed => "dlp_redaction_failed",
        }
    }
}

impl fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for TokenError {}

pub(super) fn make_token(
    key: &[u8; 32],
    raw: &str,
    category: TokenCategory,
) -> Result<String, TokenError> {
    Ok(encode_token(token_digest(key, raw)?, category))
}

/// Visit exact UTF-8 spans without allocating a result container.
/// Visitors must remain request-local; a later malformed reference may still fail.
pub(super) fn visit_token_references(
    text: &str,
    mut visitor: impl FnMut(Range<usize>) -> Result<(), TokenError>,
) -> Result<(), TokenError> {
    const MAX_REFERENCES: usize = 4096;
    let mut count = 0usize;
    let mut offset = 0;
    while let Some((start, prefix_len)) = token_format::next_candidate(text, offset) {
        offset = start + prefix_len;
        let bytes = &text.as_bytes()[start..];
        let Some(layout) = token_format::header(bytes) else {
            continue;
        };
        if !layout.complete(bytes) {
            return Err(TokenError::ContextUnavailable);
        }
        offset = start + layout.len;
        if count >= MAX_REFERENCES {
            return Err(TokenError::CapacityExceeded);
        }
        count += 1;
        visitor(start..offset)?;
    }
    Ok(())
}

/// Compatibility collector; new payload paths use the allocation-free visitor.
pub(super) fn scan_token_references(text: &str) -> Result<Vec<Range<usize>>, TokenError> {
    let mut ranges = Vec::new();
    visit_token_references(text, |range| {
        ranges
            .try_reserve(1)
            .map_err(|_| TokenError::CapacityExceeded)?;
        ranges.push(range);
        Ok(())
    })?;
    Ok(ranges)
}

/// Validate original response text without granting restoration authority.
/// Format defects are recorded; complete references still require the original lease.
#[cfg(test)]
pub(super) fn validate_response_references(
    text: &str,
    lease: &DlpRequestLease,
) -> Result<bool, TokenError> {
    const MAX_REFERENCES: usize = 4096;
    lease.validate_response_liveness()?;
    let mut malformed = false;
    let mut count = 0usize;
    let mut offset = 0;
    while let Some((start, prefix_len)) = token_format::next_candidate(text, offset) {
        offset = start + prefix_len;
        let bytes = &text.as_bytes()[start..];
        let Some(layout) = token_format::header(bytes) else {
            continue;
        };
        count = count.checked_add(1).ok_or(TokenError::CapacityExceeded)?;
        if count > MAX_REFERENCES {
            return Err(TokenError::CapacityExceeded);
        }
        if !layout.complete(bytes) {
            malformed = true;
            continue;
        }
        offset = start + layout.len;
        lease.resolve(&text[start..offset])?;
    }
    lease.validate_response_liveness()?;
    Ok(malformed)
}

#[cfg(test)]
use std::sync::Arc;
use std::{
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

// Conservative charges include hash-table slack, Arc headers and lease indices.
const NAMESPACE_METADATA: usize = 1024;
const LEASE_METADATA: usize = 256;
const ENTRY_METADATA: usize = 512;
const AUTHORITY_METADATA: usize = 64;
const STAGING_METADATA_PER_FRAGMENT: usize = 1024;
const MAX_COMMIT_ATTEMPTS: usize = 32;

#[derive(Clone)]
pub(super) struct TokenStoreLimits {
    pub namespaces: usize,
    pub entries_per_namespace: usize,
    pub plaintext_per_namespace: usize,
    pub fragment_bytes: usize,
    pub global_entries: usize,
    pub global_plaintext_bytes: usize,
    pub global_metadata_bytes: usize,
    pub idle_ttl: Duration,
}

impl Default for TokenStoreLimits {
    fn default() -> Self {
        Self {
            namespaces: 256,
            entries_per_namespace: 4096,
            plaintext_per_namespace: 4 * 1024 * 1024,
            fragment_bytes: 1024 * 1024,
            global_entries: 65536,
            global_plaintext_bytes: 64 * 1024 * 1024,
            global_metadata_bytes: 16 * 1024 * 1024,
            idle_ttl: Duration::from_secs(30 * 60),
        }
    }
}

/// Only the selected logical string is borrowed; it is never a serialized carrier.
pub(super) struct TokenFragment<'a> {
    pub raw: &'a str,
    pub whole_span_categories: &'a [TokenCategory],
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct TokenStoreStats {
    pub namespaces: usize,
    pub entries: usize,
    pub plaintext_bytes: usize,
    pub metadata_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct Charge {
    entries: usize,
    plaintext_bytes: usize,
    metadata_bytes: usize,
}

impl Charge {
    fn add(self, other: Self) -> Result<Self, TokenError> {
        Ok(Self {
            entries: self
                .entries
                .checked_add(other.entries)
                .ok_or(TokenError::CapacityExceeded)?,
            plaintext_bytes: self
                .plaintext_bytes
                .checked_add(other.plaintext_bytes)
                .ok_or(TokenError::CapacityExceeded)?,
            metadata_bytes: self
                .metadata_bytes
                .checked_add(other.metadata_bytes)
                .ok_or(TokenError::CapacityExceeded)?,
        })
    }

    fn subtract(self, other: Self) -> Result<Self, TokenError> {
        Ok(Self {
            entries: self
                .entries
                .checked_sub(other.entries)
                .ok_or(TokenError::Unavailable)?,
            plaintext_bytes: self
                .plaintext_bytes
                .checked_sub(other.plaintext_bytes)
                .ok_or(TokenError::Unavailable)?,
            metadata_bytes: self
                .metadata_bytes
                .checked_sub(other.metadata_bytes)
                .ok_or(TokenError::Unavailable)?,
        })
    }
}

use super::token_store_owners::{SharedAllocation, StoreAllocationLedger, StoreText, StoreVec};

// Deliberately no Debug/Serialize implementations on secret-owning types.
pub(super) struct Mapping {
    raw: StoreText,
    token: [u8; TOKEN_BYTES],
    token_len: usize,
}

impl Mapping {
    fn token(&self) -> Result<&str, TokenError> {
        std::str::from_utf8(&self.token[..self.token_len]).map_err(|_| TokenError::Unavailable)
    }
}

#[cfg(test)]
impl Drop for Mapping {
    fn drop(&mut self) {
        super::token_store_owners::test_support::observe_drop(
            super::token_store_owners::test_support::Kind::Mapping,
            self.raw.capacity(),
            &self.raw.ledger_for_test(),
        );
    }
}

// Empty indices allocate nothing. Published entries are strictly digest-sorted;
// all replacement backing is allocated and populated before the final swap.
pub(super) type MappingEntry = ([u8; 10], SharedAllocation<Mapping>);
#[derive(Default)]
pub(super) struct MappingIndex {
    entries: Option<StoreVec<MappingEntry>>,
}
impl MappingIndex {
    fn as_slice(&self) -> &[MappingEntry] {
        self.entries.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
    }
    fn len(&self) -> usize {
        self.as_slice().len()
    }
    fn capacity(&self) -> usize {
        self.entries.as_ref().map(StoreVec::capacity).unwrap_or(0)
    }
    fn get(&self, digest: &[u8; 10]) -> Option<&SharedAllocation<Mapping>> {
        self.as_slice()
            .binary_search_by_key(digest, |(key, _)| *key)
            .ok()
            .map(|index| &self.as_slice()[index].1)
    }
    fn contains_key(&self, digest: &[u8; 10]) -> bool {
        self.get(digest).is_some()
    }
    fn values(&self) -> impl Iterator<Item = &SharedAllocation<Mapping>> {
        self.as_slice().iter().map(|(_, mapping)| mapping)
    }
}

type ContextTable = StoreVec<super::completed_context::ContextEntry>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HistoryPolicySource {
    Published,
    #[cfg(test)]
    Fixture,
}

impl HistoryPolicySource {
    fn with_revision<T>(self, revision: u64, operation: impl FnOnce() -> T) -> Option<T> {
        match self {
            Self::Published => super::runtime::with_history_policy_revision(revision, operation),
            #[cfg(test)]
            Self::Fixture => Some(operation()),
        }
    }
}

struct Namespace {
    identity: Option<[u8; 32]>,
    policy_revision: u64,
    history_policy: HistoryPolicySource,
    generation: [u8; 16],
    key: [u8; 32],
    version: u64,
    active_leases: usize,
    last_used: Instant,
    mappings: MappingIndex,
    contexts: Option<ContextTable>,
    context_entries: usize,
    committed: Charge,
    reserved: Charge,
    history: Option<SharedAllocation<super::history_provenance::HistoryNamespace>>,
}

#[cfg(test)]
impl Drop for Namespace {
    fn drop(&mut self) {
        if let Some(entries) = &self.mappings.entries {
            super::token_store_owners::test_support::observe_drop(
                super::token_store_owners::test_support::Kind::Namespace,
                0,
                &entries.ledger_for_test(),
            );
        }
    }
}

// Entries occupy a dense, increasing-ID prefix; spare slots retain their charge.
type NamespaceSlot = Option<(u64, Namespace)>;
#[derive(Default)]
struct NamespaceIndex {
    slots: Option<StoreVec<NamespaceSlot>>,
    len: usize,
}
impl NamespaceIndex {
    fn len(&self) -> usize {
        self.len
    }
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn capacity(&self) -> usize {
        self.slots.as_ref().map(StoreVec::capacity).unwrap_or(0)
    }
    fn iter(&self) -> impl Iterator<Item = (&u64, &Namespace)> {
        self.slots
            .as_ref()
            .map(StoreVec::as_slice)
            .unwrap_or(&[])
            .iter()
            .flatten()
            .map(|(id, namespace)| (id, namespace))
    }
    fn values(&self) -> impl Iterator<Item = &Namespace> {
        self.iter().map(|(_, ns)| ns)
    }
    fn get(&self, id: &u64) -> Option<&Namespace> {
        self.iter().find_map(|(key, ns)| (key == id).then_some(ns))
    }
    fn get_mut(&mut self, id: &u64) -> Option<&mut Namespace> {
        self.slots
            .as_mut()?
            .as_mut_slice()
            .iter_mut()
            .flatten()
            .find_map(|(key, ns)| (key == id).then_some(ns))
    }
}

#[derive(Default)]
struct StoreState {
    namespaces: NamespaceIndex,
    next_id: u64,
    // Retained namespace slots remain charged at their high-water slot count.
    namespace_index_slots: usize,
    context_entries: usize,
    committed: Charge,
    reserved: Charge,
    unavailable: bool,
}

struct StoreInner {
    allocation: StoreAllocationLedger,
    history_allocation: OnceLock<Result<StoreAllocationLedger, TokenError>>,
    state: Mutex<StoreState>,
    // Separate from mapping and audit keys. A failed initialization stays closed.
    identity_key: OnceLock<Result<[u8; 32], TokenError>>,
    limits: TokenStoreLimits,
    #[cfg(test)]
    test_key: Option<[u8; 32]>,
    #[cfg(test)]
    test_digest: Option<[u8; 10]>,
    #[cfg(test)]
    fail_mapping_reserve: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub(crate) struct DlpTokenStore {
    inner: Result<SharedAllocation<StoreInner>, TokenError>,
}

/// Authority is request-local, not equivalent to membership in the namespace.
pub(crate) struct DlpRequestLease {
    store: DlpTokenStore,
    namespace: u64,
    generation: [u8; 16],
    authorized: MappingIndex,
    metadata_bytes: usize,
    #[cfg(test)]
    drop_authority_capacity: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

struct Reservation {
    store: DlpTokenStore,
    namespace: Option<u64>,
    charge: Charge,
    active: bool,
}

impl DlpTokenStore {
    pub(super) fn new() -> Self {
        Self::from_limits(TokenStoreLimits::default())
    }

    #[cfg(test)]
    pub(crate) fn history_fixture_lease_for_test(session: &str) -> Option<(Self, DlpRequestLease)> {
        let store = Self::new();
        // Exact fixture snapshots must not observe other parallel tests' charges.
        let ledger = StoreAllocationLedger::new(0, super::history_provenance::GLOBAL_BYTES).ok()?;
        store
            .inner()
            .ok()?
            .history_allocation
            .set(Ok(ledger))
            .ok()?;
        let lease = store.admit("codex", Some(session), 1).ok()?;
        Some((store, lease))
    }

    fn from_limits(limits: TokenStoreLimits) -> Self {
        Self::from_options(
            limits,
            #[cfg(test)]
            None,
            #[cfg(test)]
            None,
        )
    }

    fn from_options(
        limits: TokenStoreLimits,
        #[cfg(test)] test_key: Option<[u8; 32]>,
        #[cfg(test)] test_digest: Option<[u8; 10]>,
    ) -> Self {
        let inner = (|| {
            let ledger = StoreAllocationLedger::new(
                limits.global_plaintext_bytes,
                limits.global_metadata_bytes,
            )?;
            SharedAllocation::new(
                StoreInner {
                    allocation: ledger.clone(),
                    history_allocation: OnceLock::new(),
                    state: Mutex::new(StoreState::default()),
                    identity_key: OnceLock::new(),
                    limits,
                    #[cfg(test)]
                    test_key,
                    #[cfg(test)]
                    test_digest,
                    #[cfg(test)]
                    fail_mapping_reserve: std::sync::atomic::AtomicBool::new(false),
                },
                ledger,
            )
        })();
        Self { inner }
    }

    fn inner(&self) -> Result<&StoreInner, TokenError> {
        self.inner.as_ref().map_err(|error| *error)?.get()
    }

    pub(super) fn allocation_ledger(&self) -> Result<StoreAllocationLedger, TokenError> {
        Ok(self.inner()?.allocation.clone())
    }

    fn lock(&self) -> Result<MutexGuard<'_, StoreState>, TokenError> {
        let state = self
            .inner()?
            .state
            .lock()
            .map_err(|_| TokenError::Unavailable)?;
        if state.unavailable {
            return Err(TokenError::Unavailable);
        }
        Ok(state)
    }

    pub(super) fn stats(&self) -> Result<TokenStoreStats, TokenError> {
        if matches!(self.inner, Err(TokenError::CapacityExceeded)) {
            return Ok(TokenStoreStats::default());
        }
        let state = self.lock()?;
        Ok(TokenStoreStats {
            namespaces: state.namespaces.len(),
            entries: state.committed.entries,
            plaintext_bytes: state.committed.plaintext_bytes,
            metadata_bytes: state.committed.metadata_bytes,
        })
    }

    fn check_global(&self, state: &StoreState, additional: Charge) -> Result<(), TokenError> {
        let total = state.committed.add(state.reserved)?.add(additional)?;
        let limits = &self.inner()?.limits;
        if total.entries > limits.global_entries
            || total.plaintext_bytes > limits.global_plaintext_bytes
            || total.metadata_bytes > limits.global_metadata_bytes
        {
            return Err(TokenError::CapacityExceeded);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn admit(
        &self,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
    ) -> Result<DlpRequestLease, TokenError> {
        self.admit_with_history_policy(app, session, policy_revision, HistoryPolicySource::Fixture)
    }

    pub(super) fn admit_published_policy(
        &self,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
    ) -> Result<DlpRequestLease, TokenError> {
        self.admit_with_history_policy(
            app,
            session,
            policy_revision,
            HistoryPolicySource::Published,
        )
    }

    fn admit_with_history_policy(
        &self,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
        history_policy: HistoryPolicySource,
    ) -> Result<DlpRequestLease, TokenError> {
        if app.is_empty() || app.len() > 256 {
            return Err(TokenError::ContextUnavailable);
        }
        self.allocation_ledger()?;
        let session = session.filter(|value| {
            !value.is_empty()
                && value.len() <= 256
                && !value
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control())
        });
        let identity = match session {
            Some(session) => Some(self.identity_fingerprint(app, session)?),
            None => {
                // Initialization failure must not silently admit anonymous requests.
                self.identity_key()?;
                None
            }
        };
        self.expire_idle_at(Instant::now())?;
        // Generate entropy outside the store lock, including for anonymous requests.
        let mut key = [0u8; 32];
        let mut generation = [0u8; 16];
        getrandom::getrandom(&mut key).map_err(|_| TokenError::Unavailable)?;
        getrandom::getrandom(&mut generation).map_err(|_| TokenError::Unavailable)?;
        #[cfg(test)]
        if let Some(test_key) = self.inner()?.test_key {
            key = test_key;
        }
        let ledger = self.allocation_ledger()?;
        let mut replacement: Option<StoreVec<NamespaceSlot>> = None;
        for _ in 0..MAX_COMMIT_ATTEMPTS {
            let mut state = self.lock()?;
            let existing = identity.and_then(|identity| {
                state.namespaces.iter().find_map(|(id, namespace)| {
                    (namespace.identity == Some(identity)
                        && namespace.policy_revision == policy_revision
                        && namespace.history_policy == history_policy)
                        .then_some(*id)
                })
            });
            let lease_charge = Charge {
                metadata_bytes: LEASE_METADATA,
                ..Default::default()
            };
            if let Some(id) = existing {
                self.check_global(&state, lease_charge)?;
                let committed = state.committed.add(lease_charge)?;
                let namespace = state
                    .namespaces
                    .get_mut(&id)
                    .ok_or(TokenError::Unavailable)?;
                let active = namespace
                    .active_leases
                    .checked_add(1)
                    .ok_or(TokenError::CapacityExceeded)?;
                namespace.active_leases = active;
                namespace.last_used = Instant::now();
                let generation = namespace.generation;
                state.committed = committed;
                drop(state);
                return Ok(DlpRequestLease {
                    store: self.clone(),
                    namespace: id,
                    generation,
                    authorized: MappingIndex::default(),
                    metadata_bytes: LEASE_METADATA,
                    #[cfg(test)]
                    drop_authority_capacity: None,
                });
            }
            let count = state.namespaces.len();
            if count >= self.inner()?.limits.namespaces {
                return Err(TokenError::CapacityExceeded);
            }
            let index_slots = state.namespace_index_slots.max(count + 1);
            if state.namespaces.capacity() < index_slots
                && replacement.as_ref().map(StoreVec::capacity) != Some(index_slots)
            {
                drop(state);
                drop(replacement.take());
                let mut staged = StoreVec::with_capacity(
                    index_slots,
                    ledger.clone(),
                    self.inner()?.limits.namespaces,
                )?;
                for _ in 0..index_slots {
                    staged.push_without_growth(None)?;
                }
                replacement = Some(staged);
                continue;
            }
            let metadata_bytes = app
                .len()
                .checked_add(session.map(str::len).unwrap_or(0))
                .ok_or(TokenError::CapacityExceeded)?;
            let namespace_charge = Charge {
                metadata_bytes,
                ..Default::default()
            };
            let index_charge = Charge {
                metadata_bytes: (index_slots - state.namespace_index_slots)
                    .checked_mul(NAMESPACE_METADATA)
                    .ok_or(TokenError::CapacityExceeded)?,
                ..Default::default()
            };
            let total_charge = namespace_charge.add(lease_charge)?.add(index_charge)?;
            self.check_global(&state, total_charge)?;
            let id = state
                .next_id
                .checked_add(1)
                .ok_or(TokenError::Unavailable)?;
            let committed = state.committed.add(total_charge)?;
            let namespace = Namespace {
                identity,
                policy_revision,
                history_policy,
                generation,
                key,
                version: 0,
                active_leases: 1,
                last_used: Instant::now(),
                mappings: MappingIndex::default(),
                contexts: None,
                history: None,
                context_entries: 0,
                committed: namespace_charge,
                reserved: Charge::default(),
            };
            if state.namespaces.capacity() < index_slots {
                let staged = replacement.as_mut().ok_or(TokenError::Unavailable)?;
                let vacancy = staged
                    .as_mut_slice()
                    .get_mut(count)
                    .ok_or(TokenError::Unavailable)?;
                if vacancy.is_some() {
                    return Err(TokenError::Unavailable);
                }
                // No fallible work follows the first move. Old backing is displaced
                // into replacement and destroyed only after the state guard drops.
                *vacancy = Some((id, namespace));
                if let Some(old) = state.namespaces.slots.as_mut() {
                    for (old, new) in old.as_mut_slice().iter_mut().zip(staged.as_mut_slice()) {
                        std::mem::swap(old, new);
                    }
                }
                std::mem::swap(&mut state.namespaces.slots, &mut replacement);
            } else {
                let slots = state
                    .namespaces
                    .slots
                    .as_mut()
                    .ok_or(TokenError::Unavailable)?;
                let vacancy = slots
                    .as_mut_slice()
                    .get_mut(count)
                    .ok_or(TokenError::Unavailable)?;
                if vacancy.is_some() {
                    return Err(TokenError::Unavailable);
                }
                *vacancy = Some((id, namespace));
            }
            state.namespaces.len = count + 1;
            state.next_id = id;
            state.namespace_index_slots = index_slots;
            state.committed = committed;
            drop(state);
            drop(replacement);
            return Ok(DlpRequestLease {
                store: self.clone(),
                namespace: id,
                generation,
                authorized: MappingIndex::default(),
                metadata_bytes: LEASE_METADATA,
                #[cfg(test)]
                drop_authority_capacity: None,
            });
        }
        Err(TokenError::Unavailable)
    }

    pub(super) fn expire_idle_at(&self, now: Instant) -> Result<(), TokenError> {
        let idle_ttl = self.inner()?.limits.idle_ttl;
        loop {
            let mut state = self.lock()?;
            let expired =
                state
                    .namespaces
                    .iter()
                    .enumerate()
                    .find_map(|(index, (_, namespace))| {
                        (namespace.active_leases == 0
                            && now.saturating_duration_since(namespace.last_used) >= idle_ttl)
                            .then_some((index, namespace.committed, namespace.context_entries))
                    });
            let Some((index, mut charge, ns_context_entries)) = expired else {
                return Ok(());
            };
            let count = state.namespaces.len();
            if count == 1 {
                charge = charge.add(Charge {
                    metadata_bytes: state
                        .namespace_index_slots
                        .checked_mul(NAMESPACE_METADATA)
                        .ok_or(TokenError::Unavailable)?,
                    ..Default::default()
                })?;
            }
            // Validate subtraction before moving anything; retain the logical charge
            // until both namespace payload and any displaced index have been freed.
            state.committed.subtract(charge)?;
            let next_context_entries = state
                .context_entries
                .checked_sub(ns_context_entries)
                .ok_or(TokenError::Unavailable)?;
            let retired = {
                let slots = state
                    .namespaces
                    .slots
                    .as_mut()
                    .ok_or(TokenError::Unavailable)?;
                let occupied = slots
                    .as_mut_slice()
                    .get_mut(index)
                    .ok_or(TokenError::Unavailable)?;
                if occupied.is_none() {
                    return Err(TokenError::Unavailable);
                }
                let retired = occupied.take();
                slots.as_mut_slice()[index..count].rotate_left(1);
                retired
            };
            state.context_entries = next_context_entries;
            state.namespaces.len = count - 1;
            let retired_index = if count == 1 {
                state.namespace_index_slots = 0;
                state.namespaces.slots.take()
            } else {
                None
            };
            let retired = RetiredNamespace {
                namespace: retired,
                index: retired_index,
                release: RetiredCharge {
                    store: self.clone(),
                    charge,
                },
            };
            drop(state);
            drop(retired);
        }
    }

    fn reserve(&self, namespace: Option<u64>, charge: Charge) -> Result<Reservation, TokenError> {
        let mut state = self.lock()?;
        self.check_global(&state, charge)?;
        let reserved = state.reserved.add(charge)?;
        if let Some(id) = namespace {
            let entry = state
                .namespaces
                .get_mut(&id)
                .ok_or(TokenError::ContextUnavailable)?;
            let total = entry.committed.add(entry.reserved)?.add(charge)?;
            if total.entries > self.inner()?.limits.entries_per_namespace
                || total.plaintext_bytes > self.inner()?.limits.plaintext_per_namespace
            {
                return Err(TokenError::CapacityExceeded);
            }
            entry.reserved = entry.reserved.add(charge)?;
        }
        state.reserved = reserved;
        Ok(Reservation {
            store: self.clone(),
            namespace,
            charge,
            active: true,
        })
    }

    #[cfg(test)]
    pub(super) fn allocation_ledger_for_test(
        &self,
    ) -> Result<super::token_store_owners::StoreAllocationLedger, TokenError> {
        self.allocation_ledger()
    }

    #[cfg(test)]
    pub(super) fn control_backing_bytes_for_test(&self) -> Result<usize, TokenError> {
        super::token_store_owners::arc_backing_bytes::<StoreInner>()
    }

    #[cfg(test)]
    pub(super) fn namespace_slot_bytes_for_test(&self) -> usize {
        std::mem::size_of::<Option<(u64, Namespace)>>()
    }

    #[cfg(test)]
    pub(super) fn mapping_backing_bytes_for_test(&self) -> Result<usize, TokenError> {
        super::token_store_owners::arc_backing_bytes::<Mapping>()
    }

    #[cfg(test)]
    pub(super) fn admission_counts_for_test(&self) -> (usize, usize, usize) {
        // Observe poisoned state without changing production fail-closed access.
        let Ok(inner) = self.inner() else {
            return (0, 0, 0);
        };
        let state = match inner.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        (
            state.namespaces.len(),
            state
                .namespaces
                .values()
                .map(|namespace| namespace.active_leases)
                .sum(),
            state.committed.metadata_bytes,
        )
    }

    #[cfg(test)]
    pub(super) fn identity_storage_for_test(&self) -> Result<(usize, usize), TokenError> {
        let state = self.lock()?;
        // Namespace has no raw identity fields; the paired source guard checks this.
        Ok((
            0,
            state
                .namespaces
                .values()
                .filter_map(|ns| ns.identity.as_ref())
                .map(std::mem::size_of_val)
                .sum(),
        ))
    }

    fn identity_key(&self) -> Result<&[u8; 32], TokenError> {
        self.inner()?
            .identity_key
            .get_or_init(|| {
                #[cfg(test)]
                {
                    let state_unlocked = self.inner()?.state.try_lock().is_ok();
                    identity_test_support::emit(
                        identity_test_support::Phase::Entropy,
                        state_unlocked,
                    )?;
                }
                let mut key = [0u8; 32];
                getrandom::getrandom(&mut key).map_err(|_| TokenError::Unavailable)?;
                Ok(key)
            })
            .as_ref()
            .map_err(|error| *error)
    }

    fn identity_fingerprint(&self, app: &str, session: &str) -> Result<[u8; 32], TokenError> {
        let key = self.identity_key()?;
        #[cfg(test)]
        {
            let state_unlocked = self.inner()?.state.try_lock().is_ok();
            identity_test_support::emit(
                identity_test_support::Phase::IdentityWork,
                state_unlocked,
            )?;
        }
        namespace_identity_digest(key, app, session)
    }

    #[cfg(test)]
    pub(super) fn identity_fingerprint_for_test(
        &self,
        app: &str,
        session: &str,
    ) -> Result<[u8; 32], TokenError> {
        self.identity_fingerprint(app, session)
    }

    #[cfg(test)]
    pub(super) fn fail_next_mapping_reserve_for_test(&self) {
        if let Ok(inner) = self.inner() {
            inner
                .fail_mapping_reserve
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[cfg(test)]
    pub(super) fn namespace_index_bytes_for_test(&self) -> Result<usize, TokenError> {
        Ok(self.lock()?.namespaces.capacity() * std::mem::size_of::<NamespaceSlot>())
    }

    #[cfg(test)]
    pub(super) fn poison_for_test(&self) {
        let _ = std::panic::catch_unwind(|| {
            let Ok(inner) = self.inner() else {
                return;
            };
            if let Ok(_state) = inner.state.lock() {
                panic!("synthetic token store poisoning");
            }
        });
    }

    #[cfg(test)]
    pub(super) fn with_test_limits(limits: TokenStoreLimits) -> Self {
        Self::from_limits(limits)
    }

    #[cfg(test)]
    pub(super) fn with_test_key(key: [u8; 32]) -> Self {
        Self::from_options(TokenStoreLimits::default(), Some(key), None)
    }

    #[cfg(test)]
    pub(super) fn with_test_digest(digest: [u8; 10]) -> Self {
        Self::from_options(TokenStoreLimits::default(), None, Some(digest))
    }
}

impl Default for DlpTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

// Field order keeps all displaced backing charged through payload unwinding.
struct RetiredNamespace {
    namespace: NamespaceSlot,
    index: Option<StoreVec<NamespaceSlot>>,
    release: RetiredCharge,
}

struct RetiredCharge {
    store: DlpTokenStore,
    charge: Charge,
}
impl Drop for RetiredCharge {
    fn drop(&mut self) {
        let Ok(inner) = self.store.inner() else {
            return;
        };
        if let Ok(mut state) = inner.state.lock() {
            match state.committed.subtract(self.charge) {
                Ok(committed) => state.committed = committed,
                Err(_) => state.unavailable = true,
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Ok(inner) = self.store.inner() else {
            return;
        };
        if let Ok(mut state) = inner.state.lock() {
            let result = (|| {
                let reserved = state.reserved.subtract(self.charge)?;
                if let Some(id) = self.namespace {
                    let namespace = state
                        .namespaces
                        .get_mut(&id)
                        .ok_or(TokenError::Unavailable)?;
                    namespace.reserved = namespace.reserved.subtract(self.charge)?;
                }
                state.reserved = reserved;
                Ok::<_, TokenError>(())
            })();
            if result.is_err() {
                state.unavailable = true;
            }
        }
    }
}

struct LeaseRelease<'a> {
    store: &'a DlpTokenStore,
    namespace: u64,
    metadata_bytes: usize,
}

impl Drop for LeaseRelease<'_> {
    fn drop(&mut self) {
        let Ok(inner) = self.store.inner() else {
            return;
        };
        if let Ok(mut state) = inner.state.lock() {
            let result = (|| {
                let committed = state.committed.subtract(Charge {
                    metadata_bytes: self.metadata_bytes,
                    ..Default::default()
                })?;
                let namespace = state
                    .namespaces
                    .get_mut(&self.namespace)
                    .ok_or(TokenError::Unavailable)?;
                namespace.active_leases = namespace
                    .active_leases
                    .checked_sub(1)
                    .ok_or(TokenError::Unavailable)?;
                namespace.last_used = Instant::now();
                state.committed = committed;
                Ok::<_, TokenError>(())
            })();
            if result.is_err() {
                state.unavailable = true;
            }
        }
    }
}

impl Drop for DlpRequestLease {
    fn drop(&mut self) {
        // The guard outlives the detached authority, including its unwind path.
        let _release = LeaseRelease {
            store: &self.store,
            namespace: self.namespace,
            metadata_bytes: self.metadata_bytes,
        };
        // Keep the namespace pinned and authority budget charged until all lease
        // references and their backing allocation have actually been released.
        drop(std::mem::take(&mut self.authorized));
        #[cfg(test)]
        if let Some(observed) = &self.drop_authority_capacity {
            observed.store(
                self.authorized.capacity(),
                std::sync::atomic::Ordering::SeqCst,
            );
        }
    }
}

fn token_digest(key: &[u8; 32], raw: &str) -> Result<[u8; 10], TokenError> {
    let length = u64::try_from(raw.len()).map_err(|_| TokenError::CapacityExceeded)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| TokenError::Unavailable)?;
    mac.update(b"vary-dlp-token-v1\0");
    mac.update(&length.to_be_bytes());
    mac.update(raw.as_bytes());
    let full = mac.finalize().into_bytes();
    let mut digest = [0u8; 10];
    digest.copy_from_slice(&full[..10]);
    Ok(digest)
}

fn encode_token(digest: [u8; 10], category: TokenCategory) -> String {
    // Compatibility fixtures only; production staging writes directly to its owner.
    let (bytes, len) = encode_token_bytes(digest, category);
    String::from_utf8_lossy(&bytes[..len]).into_owned()
}

fn encode_token_bytes(digest: [u8; 10], category: TokenCategory) -> ([u8; TOKEN_BYTES], usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; TOKEN_BYTES];
    let category_end = token_format::PREFIX.len() + category.code().len();
    bytes[..token_format::PREFIX.len()].copy_from_slice(token_format::PREFIX.as_bytes());
    bytes[token_format::PREFIX.len()..category_end].copy_from_slice(category.code().as_bytes());
    bytes[category_end] = b'-';
    let digest_start = category_end + 1;
    for (index, byte) in digest.into_iter().enumerate() {
        bytes[digest_start + index * 2] = HEX[usize::from(byte >> 4)];
        bytes[digest_start + index * 2 + 1] = HEX[usize::from(byte & 15)];
    }
    let len = digest_start + token_format::DIGEST_HEX_BYTES + 3;
    bytes[len - 3..len].copy_from_slice(b"-ai");
    (bytes, len)
}

impl DlpRequestLease {
    /// Check response context without authorizing references or refreshing idle TTL.
    #[cfg(test)]
    pub(super) fn validate_response_liveness(&self) -> Result<(), TokenError> {
        let state = self.store.lock()?;
        let namespace = state
            .namespaces
            .get(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if namespace.generation != self.generation {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn observe_drop_authority_capacity_for_test(
        &mut self,
        observed: Arc<std::sync::atomic::AtomicUsize>,
    ) {
        self.drop_authority_capacity = Some(observed);
    }

    #[cfg(test)]
    pub(super) fn authority_capacity_for_test(&self) -> usize {
        self.authorized.capacity()
    }

    /// Owned compatibility fixtures are never used by production forwarding.
    #[cfg(test)]
    pub(super) fn prepare<T>(
        &mut self,
        fragments: &[TokenFragment<'_>],
        literal_tokens: &[&str],
        mut validate: impl FnMut(&[String]) -> Result<T, TokenError>,
    ) -> Result<T, TokenError> {
        self.prepare_with_budget(
            fragments,
            literal_tokens,
            super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024),
            |tokens| {
                let owned: Vec<_> = tokens
                    .iter()
                    .map(|token| token.as_str().to_owned())
                    .collect();
                validate(&owned)
            },
        )
    }

    /// Validation may repeat after a concurrent commit. Snapshot capacity is
    /// allocated before locking; hashing and callback work stay outside the lock.
    pub(super) fn prepare_with_budget<T>(
        &mut self,
        fragments: &[TokenFragment<'_>],
        literal_tokens: &[&str],
        budget: super::payload_budget::PayloadBudget,
        mut validate: impl FnMut(&[super::payload_text::PayloadText]) -> Result<T, TokenError>,
    ) -> Result<T, TokenError> {
        let limits = &self.store.inner()?.limits;
        if fragments.len() > limits.entries_per_namespace
            || fragments
                .iter()
                .any(|fragment| fragment.raw.len() > limits.fragment_bytes)
        {
            return Err(TokenError::CapacityExceeded);
        }
        let staging_bytes = fragments
            .len()
            .checked_mul(STAGING_METADATA_PER_FRAGMENT)
            .ok_or(TokenError::CapacityExceeded)?;
        let _staging = self.store.reserve(
            None,
            Charge {
                metadata_bytes: staging_bytes,
                ..Default::default()
            },
        )?;
        let key = {
            let state = self.store.lock()?;
            let namespace = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if namespace.generation != self.generation {
                return Err(TokenError::ContextUnavailable);
            }
            namespace.key
        };
        let mut digests = super::payload_metadata::PayloadVec::new(budget.clone())?;
        for fragment in fragments {
            #[cfg(test)]
            let digest = match self.store.inner()?.test_digest {
                Some(digest) => digest,
                None => token_digest(&key, fragment.raw)?,
            };
            #[cfg(not(test))]
            let digest = token_digest(&key, fragment.raw)?;
            digests.push(digest)?;
        }
        for _ in 0..MAX_COMMIT_ATTEMPTS {
            let mut existing = super::payload_metadata::PayloadVec::new(budget.clone())?;
            for _ in fragments {
                existing.push(None)?;
            }
            let (version, mapping_count) = {
                let state = self.store.lock()?;
                let namespace = state
                    .namespaces
                    .get(&self.namespace)
                    .ok_or(TokenError::ContextUnavailable)?;
                if namespace.generation != self.generation {
                    return Err(TokenError::ContextUnavailable);
                }
                for (index, digest) in digests.as_slice().iter().enumerate() {
                    *existing.get_mut(index).ok_or(TokenError::Unavailable)? =
                        namespace.mappings.get(digest).cloned();
                }
                (namespace.version, namespace.mappings.len())
            };
            let mut ordered = super::payload_metadata::PayloadVec::new(budget.clone())?;
            for index in 0..fragments.len() {
                ordered.push(index)?;
            }
            ordered.sort_unstable_by(|left, right| {
                digests.as_slice()[*left]
                    .cmp(&digests.as_slice()[*right])
                    .then_with(|| left.cmp(right))
            });
            let mut new_indices = super::payload_metadata::PayloadVec::new(budget.clone())?;
            let mut tokens =
                super::payload_metadata::PayloadVec::<super::payload_text::PayloadText>::new(
                    budget.clone(),
                )?;
            let mut new_bytes = 0usize;
            for (index, ((fragment, digest), existing)) in fragments
                .iter()
                .zip(digests.as_slice())
                .zip(existing.as_slice())
                .enumerate()
            {
                let token = if let Some(existing) = existing {
                    if existing.get()?.raw.as_str() != fragment.raw {
                        return Err(TokenError::TokenCollision);
                    }
                    super::payload_text::PayloadText::try_from_str(
                        existing.get()?.token()?,
                        budget.clone(),
                        TOKEN_BYTES,
                    )?
                } else {
                    let position = ordered
                        .as_slice()
                        .partition_point(|prior| digests.as_slice()[*prior] < *digest);
                    let prior = *ordered
                        .as_slice()
                        .get(position)
                        .ok_or(TokenError::Unavailable)?;
                    if prior != index {
                        if fragments[prior].raw != fragment.raw {
                            return Err(TokenError::TokenCollision);
                        }
                        super::payload_text::PayloadText::try_from_str(
                            tokens.as_slice()[prior].as_str(),
                            budget.clone(),
                            TOKEN_BYTES,
                        )?
                    } else {
                        new_indices.push(index)?;
                        new_bytes = new_bytes
                            .checked_add(fragment.raw.len())
                            .ok_or(TokenError::CapacityExceeded)?;
                        let (bytes, len) = encode_token_bytes(
                            *digest,
                            select_category(fragment.whole_span_categories),
                        );
                        let text = std::str::from_utf8(&bytes[..len])
                            .map_err(|_| TokenError::Unavailable)?;
                        super::payload_text::PayloadText::try_from_str(
                            text,
                            budget.clone(),
                            TOKEN_BYTES,
                        )?
                    }
                };
                if literal_tokens.contains(&token.as_str()) {
                    return Err(TokenError::TokenCollision);
                }
                tokens.push(token)?;
            }
            let mut new_authority = 0usize;
            let mut previous = None;
            for index in ordered.as_slice() {
                let digest = digests.as_slice()[*index];
                if previous != Some(digest) && !self.authorized.contains_key(&digest) {
                    new_authority += 1;
                }
                previous = Some(digest);
            }
            if self
                .authorized
                .len()
                .checked_add(new_authority)
                .ok_or(TokenError::CapacityExceeded)?
                > limits.entries_per_namespace
            {
                return Err(TokenError::CapacityExceeded);
            }
            let mapping_metadata = new_indices
                .len()
                .checked_mul(ENTRY_METADATA)
                .ok_or(TokenError::CapacityExceeded)?;
            let authority_metadata = new_authority
                .checked_mul(AUTHORITY_METADATA)
                .ok_or(TokenError::CapacityExceeded)?;
            let charge = Charge {
                entries: new_indices.len(),
                plaintext_bytes: new_bytes,
                metadata_bytes: mapping_metadata
                    .checked_add(authority_metadata)
                    .ok_or(TokenError::CapacityExceeded)?,
            };
            let mut reservation = self.store.reserve(Some(self.namespace), charge)?;
            // A replacement authority table must coexist with the old table until
            // commit. Charge its existing entries before allocating the copy.
            let authority_copy_bytes = if new_authority == 0 {
                0
            } else {
                self.authorized
                    .len()
                    .checked_mul(AUTHORITY_METADATA)
                    .ok_or(TokenError::CapacityExceeded)?
            };
            let _authority_copy = self.store.reserve(
                None,
                Charge {
                    metadata_bytes: authority_copy_bytes,
                    ..Default::default()
                },
            )?;
            let candidate = validate(tokens.as_slice())?;
            let ledger = self.store.allocation_ledger()?;
            let mut mappings = StoreVec::with_capacity(
                new_indices.len(),
                ledger.clone(),
                limits.entries_per_namespace,
            )?;
            for index in new_indices.as_slice() {
                let text = tokens.as_slice()[*index].as_str();
                let mut token = [0u8; TOKEN_BYTES];
                token[..text.len()].copy_from_slice(text.as_bytes());
                let mapping = SharedAllocation::new(
                    Mapping {
                        raw: StoreText::try_from_str(
                            fragments[*index].raw,
                            ledger.clone(),
                            limits.fragment_bytes,
                        )?,
                        token,
                        token_len: text.len(),
                    },
                    ledger.clone(),
                )?;
                mappings.push((digests.as_slice()[*index], mapping))?;
            }
            let mut staged_authority = if new_authority == 0 {
                None
            } else {
                let mut staged = StoreVec::with_capacity(
                    self.authorized.len() + new_authority,
                    ledger.clone(),
                    limits.entries_per_namespace,
                )?;
                for (digest, mapping) in self.authorized.as_slice() {
                    staged.push_without_growth((*digest, mapping.clone()))?;
                }
                for (digest, mapping) in mappings.as_slice() {
                    staged.push_without_growth((*digest, mapping.clone()))?;
                }
                let mut previous = None;
                for index in ordered.as_slice() {
                    let digest = digests.as_slice()[*index];
                    if previous != Some(digest) && !self.authorized.contains_key(&digest) {
                        if let Some(mapping) = &existing.as_slice()[*index] {
                            staged.push_without_growth((digest, mapping.clone()))?;
                        }
                    }
                    previous = Some(digest);
                }
                staged.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                Some(MappingIndex {
                    entries: Some(staged),
                })
            };
            let mut staged_mappings = if mappings.as_slice().is_empty() {
                None
            } else {
                let mut staged = StoreVec::with_capacity(
                    mapping_count
                        .checked_add(mappings.as_slice().len())
                        .ok_or(TokenError::CapacityExceeded)?,
                    ledger.clone(),
                    limits.entries_per_namespace,
                )?;
                {
                    let state = self.store.lock()?;
                    let namespace = state
                        .namespaces
                        .get(&self.namespace)
                        .ok_or(TokenError::ContextUnavailable)?;
                    if namespace.version != version {
                        drop(state);
                        continue;
                    }
                    for (digest, mapping) in namespace.mappings.as_slice() {
                        staged.push_without_growth((*digest, mapping.clone()))?;
                    }
                }
                for (digest, mapping) in mappings.as_slice() {
                    staged.push_without_growth((*digest, mapping.clone()))?;
                }
                staged.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                Some(MappingIndex {
                    entries: Some(staged),
                })
            };
            let next_version = version.checked_add(1).ok_or(TokenError::Unavailable)?;
            let lease_metadata = self
                .metadata_bytes
                .checked_add(authority_metadata)
                .ok_or(TokenError::Unavailable)?;
            let mut state = self.store.lock()?;
            let state_reserved = state.reserved.subtract(charge)?;
            let state_committed = state.committed.add(charge)?;
            let namespace = state
                .namespaces
                .get_mut(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if namespace.version != version {
                drop(state);
                // Staged tables, candidate and reservations are all discarded.
                continue;
            }
            let mapping_charge = Charge {
                metadata_bytes: mapping_metadata,
                ..charge
            };
            let namespace_committed = namespace.committed.add(mapping_charge)?;
            let namespace_reserved = namespace.reserved.subtract(charge)?;
            #[cfg(test)]
            if self
                .store
                .inner()?
                .fail_mapping_reserve
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(TokenError::CapacityExceeded);
            }
            // All backing and fallible checks precede publication. Swaps move
            // the displaced indices out; their elements/backing drop unlocked.
            if let Some(staged) = staged_mappings.as_mut() {
                std::mem::swap(&mut namespace.mappings, staged);
            }
            namespace.version = next_version;
            namespace.committed = namespace_committed;
            namespace.reserved = namespace_reserved;
            namespace.last_used = Instant::now();
            state.reserved = state_reserved;
            state.committed = state_committed;
            reservation.active = false;
            if let Some(staged) = staged_authority.as_mut() {
                std::mem::swap(&mut self.authorized, staged);
            }
            self.metadata_bytes = lease_metadata;
            // Drop the displaced authority before releasing its copy reservation.
            drop(state);
            drop(staged_authority);
            drop(staged_mappings);
            return Ok(candidate);
        }
        Err(TokenError::Unavailable)
    }

    /// Visit only this lease's pinned dependencies; never lock the store here.
    pub(super) fn is_empty(&self) -> bool {
        self.authorized.len() == 0
    }

    pub(super) fn visit_authorized_tokens<'a>(
        &'a self,
        mut visitor: impl FnMut(&'a str) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        for mapping in self.authorized.values() {
            visitor(mapping.get()?.token()?)?;
        }
        Ok(())
    }

    /// Collect borrowed dependencies without granting authority to another lease.
    /// Backing is precharged; references cannot outlive this lease. No store lock.
    pub(super) fn authorized_tokens(&self) -> Result<StoreVec<&str>, TokenError> {
        let ledger = self.store.allocation_ledger()?;
        let limit = self.store.inner()?.limits.entries_per_namespace;
        let mut tokens = if self.authorized.len() == 0 {
            StoreVec::new(ledger, limit)
        } else {
            StoreVec::with_capacity(self.authorized.len(), ledger, limit)?
        };
        for mapping in self.authorized.values() {
            tokens.push_without_growth(mapping.get()?.token()?)?;
        }
        Ok(tokens)
    }

    /// A syntactic nonmatch has no authority; live authorized mappings still fail closed.
    pub(super) fn resolve_if_authorized(&self, token: &str) -> Result<Option<&str>, TokenError> {
        let bytes = token.as_bytes();
        let Some(layout) = token_format::header(bytes) else {
            return Ok(None);
        };
        if layout.legacy || token.len() != layout.len || !layout.complete(bytes) {
            return Ok(None);
        }
        let mut digest = [0u8; 10];
        for (index, byte) in digest.iter_mut().enumerate() {
            let offset = layout.digest_start + index * 2;
            let hex = |value: u8| {
                if value <= b'9' {
                    value - b'0'
                } else {
                    value - b'a' + 10
                }
            };
            *byte = (hex(bytes[offset]) << 4) | hex(bytes[offset + 1]);
        }
        let Some(mapping) = self.authorized.get(&digest) else {
            return Ok(None);
        };
        if mapping.get()?.token()? != token {
            return Ok(None);
        }
        self.resolve(token).map(Some)
    }

    pub(super) fn resolve(&self, token: &str) -> Result<&str, TokenError> {
        let bytes = token.as_bytes();
        let layout = token_format::header(bytes).ok_or(TokenError::ContextUnavailable)?;
        if layout.legacy || token.len() != layout.len || !layout.complete(bytes) {
            return Err(TokenError::ContextUnavailable);
        }
        let mut digest = [0u8; 10];
        for (index, byte) in digest.iter_mut().enumerate() {
            let offset = layout.digest_start + index * 2;
            let hex = |value: u8| {
                if value <= b'9' {
                    value - b'0'
                } else {
                    value - b'a' + 10
                }
            };
            *byte = (hex(token.as_bytes()[offset]) << 4) | hex(token.as_bytes()[offset + 1]);
        }
        // Membership and the exact stored category are both required. The borrow
        // prevents restored plaintext escaping the lease's lifetime and budgets.
        let mapping = self
            .authorized
            .get(&digest)
            .ok_or(TokenError::ContextUnavailable)?;
        if mapping.get()?.token()? != token {
            return Err(TokenError::ContextUnavailable);
        }
        let state = self.store.lock()?;
        let namespace = state
            .namespaces
            .get(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if namespace.generation != self.generation {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(mapping.get()?.raw.as_str())
    }

    pub(super) fn fork_attempt_extracted(&self) -> Result<DlpRequestLease, TokenError> {
        let ledger = self.store.allocation_ledger()?;
        let limit = self.store.inner()?.limits.entries_per_namespace;
        let entries = if self.authorized.len() == 0 {
            None
        } else {
            let mut staged = StoreVec::with_capacity(self.authorized.len(), ledger, limit)?;
            for (digest, mapping) in self.authorized.as_slice() {
                staged.push_without_growth((*digest, mapping.clone()))?;
            }
            Some(staged)
        };
        let authority_bytes = self
            .authorized
            .len()
            .checked_mul(AUTHORITY_METADATA)
            .ok_or(TokenError::CapacityExceeded)?;
        let metadata_bytes = LEASE_METADATA
            .checked_add(authority_bytes)
            .ok_or(TokenError::CapacityExceeded)?;
        let charge = Charge {
            metadata_bytes,
            ..Default::default()
        };
        let mut state = self.store.lock()?;
        self.store.check_global(&state, charge)?;
        let committed = state.committed.add(charge)?;
        let namespace = state
            .namespaces
            .get_mut(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if namespace.generation != self.generation {
            return Err(TokenError::ContextUnavailable);
        }
        namespace.active_leases = namespace
            .active_leases
            .checked_add(1)
            .ok_or(TokenError::CapacityExceeded)?;
        namespace.last_used = Instant::now();
        state.committed = committed;
        drop(state);
        Ok(DlpRequestLease {
            store: self.store.clone(),
            namespace: self.namespace,
            generation: self.generation,
            authorized: MappingIndex { entries },
            metadata_bytes,
            #[cfg(test)]
            drop_authority_capacity: None,
        })
    }

    fn store_id(&self) -> Result<usize, TokenError> {
        Ok(self.store.inner()? as *const StoreInner as usize)
    }

    #[cfg(test)]
    pub(crate) fn history_binding_for_test(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        principal: Option<&str>,
    ) -> Option<super::completed_context::ActualAttemptBinding> {
        self.bind_actual_attempt_extracted(provider_id, protocol, target_url, principal)
            .ok()
    }

    pub(super) fn bind_actual_attempt_extracted(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
    ) -> Result<super::completed_context::ActualAttemptBinding, TokenError> {
        self.bind_actual_operation_attempt_extracted(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            None,
        )
    }

    pub(super) fn bind_actual_operation_attempt_extracted(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
    ) -> Result<super::completed_context::ActualAttemptBinding, TokenError> {
        use super::completed_context::valid_external_id;
        if !valid_external_id(provider_id)
            || !valid_external_id(protocol)
            || target_url.is_empty()
            || target_url.len() > 2048
        {
            return Err(TokenError::ContextUnavailable);
        }
        let key = self.store.identity_key()?;
        let valid_principal = auth_principal.filter(|p| valid_external_id(p));
        let provider_fingerprint = super::completed_context::provider_binding_digest(
            key,
            provider_id,
            protocol,
            target_url,
            valid_principal,
        )?;
        let (policy_revision, has_identity) = {
            let state = self.store.lock()?;
            let ns = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if ns.generation != self.generation {
                return Err(TokenError::ContextUnavailable);
            }
            (ns.policy_revision, ns.identity.is_some())
        };
        Ok(super::completed_context::ActualAttemptBinding {
            store_id: self.store_id()?,
            namespace: self.namespace,
            generation: self.generation,
            policy_revision,
            provider_fingerprint,
            reusable_principal: has_identity && valid_principal.is_some(),
            operation_lineage: match (operation, valid_principal) {
                (Some(descriptor), Some(principal)) if has_identity => {
                    Some(super::completed_context::OperationLineage::issue(
                        key,
                        provider_id,
                        protocol,
                        principal,
                        descriptor,
                    )?)
                }
                _ => None,
            },
        })
    }

    pub(super) fn publish_completed_context_extracted(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        response_id: &str,
        output_tokens: &[&str],
        publications: super::completed_context::CompletedCallPublications<'_>,
        opaques: &[&str],
    ) -> Result<super::completed_context::CompletedContextHandle, TokenError> {
        use super::completed_context::{
            call_id_digest, call_snapshot_digest, opaque_content_digest, response_id_digest,
            valid_external_id, CompletedCallEntry, CompletedContextHandle, CompletedContextRecord,
            CALL_ENTRY_METADATA, CONTEXT_ENTRY_METADATA, MAX_CALLS_PER_CONTEXT,
            MAX_CONTEXT_ENTRIES_PER_NAMESPACE, MAX_GLOBAL_CONTEXT_ENTRIES, OPAQUE_ENTRY_METADATA,
        };
        if binding.store_id != self.store_id()?
            || binding.namespace != self.namespace
            || binding.generation != self.generation
            || !binding.reusable_principal
            || !valid_external_id(response_id)
        {
            return Err(TokenError::ContextUnavailable);
        }
        let limits = &self.store.inner()?.limits;
        let calls = publications.originals;
        let delivered = publications.delivered;
        if calls.len() > MAX_CALLS_PER_CONTEXT
            || delivered.len()
                > calls
                    .len()
                    .saturating_mul(super::completed_context::MAX_DELIVERED_CALL_SNAPSHOTS)
            || output_tokens.len() > limits.entries_per_namespace
            || opaques.len() > MAX_CONTEXT_ENTRIES_PER_NAMESPACE
        {
            return Err(TokenError::CapacityExceeded);
        }
        for variant in delivered {
            let original = calls
                .iter()
                .find(|spec| spec.call_id == variant.call_id)
                .ok_or(TokenError::ContextUnavailable)?;
            if original.item_type != variant.item_type
                || original.name != variant.name
                || original.reasoning != variant.reasoning
                || original.encrypted_reasoning != variant.encrypted_reasoning
                || variant.payload.len() > limits.fragment_bytes
            {
                return Err(TokenError::ContextUnavailable);
            }
        }
        for &tok in output_tokens {
            self.resolve(tok)?;
        }
        let key = self.store.identity_key()?;
        let response_digest = response_id_digest(key, response_id)?;
        let ledger = self.store.allocation_ledger()?;

        let mut call_entries = if calls.is_empty() {
            None
        } else {
            let mut vec =
                StoreVec::with_capacity(calls.len(), ledger.clone(), MAX_CALLS_PER_CONTEXT)?;
            for spec in calls {
                if !valid_external_id(spec.call_id)
                    || !valid_external_id(spec.name)
                    || !matches!(spec.item_type, "function_call" | "custom_tool_call")
                    || spec.payload.len() > limits.fragment_bytes
                    || spec
                        .reasoning
                        .is_some_and(|r| r.len() > limits.fragment_bytes)
                    || spec
                        .encrypted_reasoning
                        .is_some_and(|r| r.is_empty() || r.len() > limits.fragment_bytes)
                {
                    return Err(TokenError::ContextUnavailable);
                }
                super::output_text_matching::visit_complete_references(spec.payload, |range| {
                    self.resolve_if_authorized(&spec.payload[range]).map(|_| ())
                })?;
                // Supported upstream reasoning is literal text, not token authority.
                // Its bounded snapshot remains part of mandatory call completion.
                let mut entry = CompletedCallEntry {
                    call_digest: call_id_digest(key, spec.call_id)?,
                    snapshot_digest: call_snapshot_digest(key, spec)?,
                    delivered_digests: [[0; 32];
                        super::completed_context::MAX_DELIVERED_CALL_SNAPSHOTS],
                    delivered_len: 0,
                };
                for variant in delivered
                    .iter()
                    .filter(|variant| variant.call_id == spec.call_id)
                {
                    let digest = call_snapshot_digest(key, variant)?;
                    if !entry.contains_snapshot(&digest) {
                        let slot = entry
                            .delivered_digests
                            .get_mut(entry.delivered_len)
                            .ok_or(TokenError::CapacityExceeded)?;
                        *slot = digest;
                        entry.delivered_len += 1;
                    }
                }
                entry.delivered_digests[..entry.delivered_len].sort_unstable();
                vec.push_without_growth(entry)?;
            }
            vec.sort_unstable_by(|a, b| a.call_digest.cmp(&b.call_digest));
            if vec
                .as_slice()
                .windows(2)
                .any(|w| w[0].call_digest == w[1].call_digest)
            {
                return Err(TokenError::ContextUnavailable);
            }
            Some(vec)
        };

        let mut opaque_entries = if opaques.is_empty() {
            None
        } else {
            let mut vec = StoreVec::with_capacity(
                opaques.len(),
                ledger.clone(),
                MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
            )?;
            for &op in opaques {
                if op.is_empty() || op.len() > limits.fragment_bytes {
                    return Err(TokenError::ContextUnavailable);
                }
                vec.push_without_growth(opaque_content_digest(key, op)?)?;
            }
            vec.sort_unstable_by(|a, b| a.cmp(b));
            if vec.as_slice().windows(2).any(|w| w[0] == w[1]) {
                return Err(TokenError::ContextUnavailable);
            }
            Some(vec)
        };

        let opaque_count = opaque_entries.as_ref().map_or(0, StoreVec::len);
        let entry_delta = 1usize
            .checked_add(opaque_count)
            .ok_or(TokenError::CapacityExceeded)?;
        let metadata_bytes = CONTEXT_ENTRY_METADATA
            .checked_add(
                calls
                    .len()
                    .checked_mul(CALL_ENTRY_METADATA)
                    .ok_or(TokenError::CapacityExceeded)?,
            )
            .and_then(|n| n.checked_add(opaque_count.checked_mul(OPAQUE_ENTRY_METADATA)?))
            .and_then(|n| n.checked_add(self.authorized.len().checked_mul(AUTHORITY_METADATA)?))
            .ok_or(TokenError::CapacityExceeded)?;
        let charge = Charge {
            metadata_bytes,
            ..Default::default()
        };

        let (version, existing_record, context_len) = {
            let state = self.store.lock()?;
            let ns = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if ns.identity.is_none()
                || ns.generation != self.generation
                || ns.policy_revision != binding.policy_revision
            {
                return Err(TokenError::ContextUnavailable);
            }
            let slice = ns.contexts.as_ref().map_or(&[][..], StoreVec::as_slice);
            let found = slice.iter().find_map(|entry| {
                entry
                    .get()
                    .ok()
                    .filter(|r| r.response_digest == response_digest)
                    .map(|_| entry.clone())
            });
            if found.is_none()
                && (ns
                    .context_entries
                    .checked_add(entry_delta)
                    .is_none_or(|n| n > MAX_CONTEXT_ENTRIES_PER_NAMESPACE)
                    || state
                        .context_entries
                        .checked_add(entry_delta)
                        .is_none_or(|n| n > MAX_GLOBAL_CONTEXT_ENTRIES))
            {
                return Err(TokenError::CapacityExceeded);
            }
            (ns.version, found, slice.len())
        };

        if let Some(existing) = existing_record {
            let rec = existing.get()?;
            let calls_slice = call_entries.as_ref().map_or(&[][..], StoreVec::as_slice);
            let opaques_slice = opaque_entries.as_ref().map_or(&[][..], StoreVec::as_slice);
            let same_deps = rec.deps_slice().len() == self.authorized.len()
                && rec
                    .deps_slice()
                    .iter()
                    .zip(self.authorized.as_slice())
                    .all(|((d1, _), (d2, _))| d1 == d2);
            if rec.provider_fingerprint == binding.provider_fingerprint
                && rec.operation_lineage == binding.operation_lineage
                && rec.policy_revision == binding.policy_revision
                && rec.calls_slice() == calls_slice
                && rec.opaques_slice() == opaques_slice
                && same_deps
            {
                return Ok(CompletedContextHandle {
                    store_id: binding.store_id,
                    namespace: self.namespace,
                    generation: self.generation,
                    policy_revision: binding.policy_revision,
                    provider_fingerprint: binding.provider_fingerprint,
                    response_digest,
                });
            }
            return Err(TokenError::ContextUnavailable);
        }

        let mut reservation = self.store.reserve(Some(self.namespace), charge)?;
        let deps_copy = if self.authorized.len() == 0 {
            None
        } else {
            let mut d = StoreVec::with_capacity(
                self.authorized.len(),
                ledger.clone(),
                limits.entries_per_namespace,
            )?;
            for (digest, mapping) in self.authorized.as_slice() {
                d.push_without_growth((*digest, mapping.clone()))?;
            }
            Some(d)
        };
        let record = SharedAllocation::new(
            CompletedContextRecord {
                response_digest,
                provider_fingerprint: binding.provider_fingerprint,
                operation_lineage: binding.operation_lineage,
                policy_revision: binding.policy_revision,
                dependencies: deps_copy,
                calls: call_entries.take(),
                opaques: opaque_entries.take(),
                context_entries: entry_delta,
                metadata_bytes,
            },
            ledger.clone(),
        )?;

        let mut staged = StoreVec::with_capacity(
            context_len + 1,
            ledger.clone(),
            MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
        )?;
        {
            let state = self.store.lock()?;
            let ns = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if ns.version != version {
                drop(state);
                return Err(TokenError::Unavailable);
            }
            for existing in ns.contexts.as_ref().map_or(&[][..], StoreVec::as_slice) {
                staged.push_without_growth(existing.clone())?;
            }
        }
        staged.push_without_growth(record)?;

        let next_version = version.checked_add(1).ok_or(TokenError::Unavailable)?;
        let mut state = self.store.lock()?;
        let state_reserved = state.reserved.subtract(charge)?;
        let state_committed = state.committed.add(charge)?;
        let next_global_entries = state
            .context_entries
            .checked_add(entry_delta)
            .filter(|&n| n <= MAX_GLOBAL_CONTEXT_ENTRIES)
            .ok_or(TokenError::CapacityExceeded)?;
        let ns = state
            .namespaces
            .get_mut(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if ns.version != version {
            drop(state);
            return Err(TokenError::Unavailable);
        }
        let next_ns_entries = ns
            .context_entries
            .checked_add(entry_delta)
            .filter(|&n| n <= MAX_CONTEXT_ENTRIES_PER_NAMESPACE)
            .ok_or(TokenError::CapacityExceeded)?;
        let ns_committed = ns.committed.add(charge)?;
        let ns_reserved = ns.reserved.subtract(charge)?;
        let mut old_contexts = Some(staged);
        std::mem::swap(&mut ns.contexts, &mut old_contexts);
        ns.version = next_version;
        ns.context_entries = next_ns_entries;
        ns.committed = ns_committed;
        ns.reserved = ns_reserved;
        ns.last_used = Instant::now();
        state.context_entries = next_global_entries;
        state.reserved = state_reserved;
        state.committed = state_committed;
        reservation.active = false;
        drop(state);
        drop(old_contexts);
        Ok(CompletedContextHandle {
            store_id: binding.store_id,
            namespace: self.namespace,
            generation: self.generation,
            policy_revision: binding.policy_revision,
            provider_fingerprint: binding.provider_fingerprint,
            response_digest,
        })
    }

    pub(super) fn resume_completed_context_extracted(
        &mut self,
        binding: &super::completed_context::ActualAttemptBinding,
        previous_response_id: Option<&str>,
        required_calls: &[super::completed_context::CompletedCallSpec<'_>],
        required_opaques: &[&str],
    ) -> Result<(), TokenError> {
        use super::completed_context::{
            call_id_digest, call_snapshot_digest, opaque_content_digest, response_id_digest,
            valid_external_id, MAX_CALLS_PER_CONTEXT, MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
        };
        if binding.store_id != self.store_id()?
            || binding.namespace != self.namespace
            || binding.generation != self.generation
        {
            return Err(TokenError::ContextUnavailable);
        }
        if previous_response_id.is_none()
            && required_calls.is_empty()
            && required_opaques.is_empty()
        {
            return Ok(());
        }
        if !binding.reusable_principal {
            return Err(TokenError::ContextUnavailable);
        }
        let limits = &self.store.inner()?.limits;
        if required_calls.len() > MAX_CALLS_PER_CONTEXT
            || required_opaques.len() > MAX_CONTEXT_ENTRIES_PER_NAMESPACE
        {
            return Err(TokenError::CapacityExceeded);
        }
        let key = self.store.identity_key()?;
        let prev_digest = match previous_response_id {
            Some(id) => {
                if !valid_external_id(id) {
                    return Err(TokenError::ContextUnavailable);
                }
                Some(response_id_digest(key, id)?)
            }
            None => None,
        };
        let ledger = self.store.allocation_ledger()?;
        let mut call_reqs = if required_calls.is_empty() {
            None
        } else {
            let mut vec = StoreVec::with_capacity(
                required_calls.len(),
                ledger.clone(),
                MAX_CALLS_PER_CONTEXT,
            )?;
            for spec in required_calls {
                if !valid_external_id(spec.call_id)
                    || !valid_external_id(spec.name)
                    || !matches!(spec.item_type, "function_call" | "custom_tool_call")
                    || spec.payload.len() > limits.fragment_bytes
                {
                    return Err(TokenError::ContextUnavailable);
                }
                vec.push_without_growth((
                    call_id_digest(key, spec.call_id)?,
                    call_snapshot_digest(key, spec)?,
                ))?;
            }
            Some(vec)
        };
        let mut opaque_reqs = if required_opaques.is_empty() {
            None
        } else {
            let mut vec = StoreVec::with_capacity(
                required_opaques.len(),
                ledger.clone(),
                MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
            )?;
            for &op in required_opaques {
                if op.is_empty() || op.len() > limits.fragment_bytes {
                    return Err(TokenError::ContextUnavailable);
                }
                vec.push_without_growth(opaque_content_digest(key, op)?)?;
            }
            Some(vec)
        };

        let snapshot = {
            let state = self.store.lock()?;
            let ns = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if ns.identity.is_none()
                || ns.generation != self.generation
                || ns.policy_revision != binding.policy_revision
            {
                return Err(TokenError::ContextUnavailable);
            }
            let slice = ns.contexts.as_ref().map_or(&[][..], StoreVec::as_slice);
            if slice.is_empty() {
                return Err(TokenError::ContextUnavailable);
            }
            let mut copied = StoreVec::with_capacity(
                slice.len(),
                ledger.clone(),
                MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
            )?;
            for entry in slice {
                copied.push_without_growth(entry.clone())?;
            }
            copied
        };

        let mut matched_indices = StoreVec::with_capacity(
            snapshot.len(),
            ledger.clone(),
            MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
        )?;
        let mut push_match = |idx: usize| -> Result<(), TokenError> {
            if !matched_indices.as_slice().contains(&idx) {
                matched_indices.push_without_growth(idx)?;
            }
            Ok(())
        };

        let prev_idx = if let Some(target) = prev_digest {
            let idx = snapshot
                .as_slice()
                .iter()
                .position(|entry| {
                    entry.get().is_ok_and(|rec| {
                        rec.response_digest == target
                            && rec.authorizes_attempt(binding)
                            && rec.policy_revision == binding.policy_revision
                    })
                })
                .ok_or(TokenError::ContextUnavailable)?;
            push_match(idx)?;
            Some(idx)
        } else {
            None
        };

        if let Some(calls) = &call_reqs {
            for &(call_digest, snapshot_digest) in calls.as_slice() {
                if let Some(pidx) = prev_idx {
                    let rec = snapshot.as_slice()[pidx].get()?;
                    if let Some(found) = rec.find_call(&call_digest) {
                        if !found.contains_snapshot(&snapshot_digest) {
                            return Err(TokenError::ContextUnavailable);
                        }
                        continue;
                    }
                }
                let mut found_idx = None;
                for (idx, entry) in snapshot.as_slice().iter().enumerate() {
                    let rec = entry.get()?;
                    if !rec.authorizes_attempt(binding)
                        || rec.policy_revision != binding.policy_revision
                    {
                        continue;
                    }
                    if let Some(found) = rec.find_call(&call_digest) {
                        if !found.contains_snapshot(&snapshot_digest) || found_idx.is_some() {
                            return Err(TokenError::ContextUnavailable);
                        }
                        found_idx = Some(idx);
                    }
                }
                let idx = found_idx.ok_or(TokenError::ContextUnavailable)?;
                push_match(idx)?;
            }
        }
        drop(call_reqs.take());

        if let Some(opaques) = &opaque_reqs {
            for &op_digest in opaques.as_slice() {
                if let Some(pidx) = prev_idx {
                    let rec = snapshot.as_slice()[pidx].get()?;
                    if rec.contains_opaque(&op_digest) {
                        continue;
                    }
                }
                let mut found_idx = None;
                for (idx, entry) in snapshot.as_slice().iter().enumerate() {
                    let rec = entry.get()?;
                    if !rec.authorizes_attempt(binding)
                        || rec.policy_revision != binding.policy_revision
                    {
                        continue;
                    }
                    if rec.contains_opaque(&op_digest) {
                        found_idx = Some(idx);
                        break;
                    }
                }
                let idx = found_idx.ok_or(TokenError::ContextUnavailable)?;
                push_match(idx)?;
            }
        }
        drop(opaque_reqs.take());

        let mut total_candidates = self.authorized.len();
        for &idx in matched_indices.as_slice() {
            total_candidates = total_candidates
                .checked_add(snapshot.as_slice()[idx].get()?.deps_slice().len())
                .ok_or(TokenError::CapacityExceeded)?;
        }
        if total_candidates == self.authorized.len() {
            return Ok(());
        }
        let mut combined = StoreVec::with_capacity(
            total_candidates.min(limits.entries_per_namespace),
            ledger.clone(),
            limits.entries_per_namespace,
        )?;
        for (digest, mapping) in self.authorized.as_slice() {
            combined.push_without_growth((*digest, mapping.clone()))?;
        }
        let mut added = 0usize;
        for &idx in matched_indices.as_slice() {
            for (digest, mapping) in snapshot.as_slice()[idx].get()?.deps_slice() {
                if !combined.as_slice().iter().any(|(d, _)| d == digest) {
                    if combined.len() >= limits.entries_per_namespace {
                        return Err(TokenError::CapacityExceeded);
                    }
                    combined.push_without_growth((*digest, mapping.clone()))?;
                    added += 1;
                }
            }
        }
        if added == 0 {
            return Ok(());
        }
        combined.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let staged = if combined.len() == combined.capacity() {
            combined
        } else {
            let mut exact =
                StoreVec::with_capacity(combined.len(), ledger, limits.entries_per_namespace)?;
            for (digest, mapping) in combined.as_slice() {
                exact.push_without_growth((*digest, mapping.clone()))?;
            }
            drop(combined);
            exact
        };
        let extra_metadata = added
            .checked_mul(AUTHORITY_METADATA)
            .ok_or(TokenError::CapacityExceeded)?;
        let charge = Charge {
            metadata_bytes: extra_metadata,
            ..Default::default()
        };
        let mut state = self.store.lock()?;
        self.store.check_global(&state, charge)?;
        let committed = state.committed.add(charge)?;
        let ns = state
            .namespaces
            .get_mut(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if ns.generation != self.generation {
            return Err(TokenError::ContextUnavailable);
        }
        ns.last_used = Instant::now();
        state.committed = committed;
        let mut old_authorized = MappingIndex {
            entries: Some(staged),
        };
        std::mem::swap(&mut self.authorized, &mut old_authorized);
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(extra_metadata)
            .ok_or(TokenError::CapacityExceeded)?;
        drop(state);
        drop(old_authorized);
        Ok(())
    }

    pub(super) fn resume_completed_tokens_extracted(
        &mut self,
        binding: &super::completed_context::ActualAttemptBinding,
        referenced_tokens: &[&str],
    ) -> Result<(), TokenError> {
        self.resume_matching_tokens_impl(binding, referenced_tokens, true)
    }

    /// Literal candidates may acquire only existing, exact, binding-authorized completion proof.
    pub(super) fn resume_matching_completed_tokens(
        &mut self,
        binding: &super::completed_context::ActualAttemptBinding,
        referenced_tokens: &[&str],
    ) -> Result<(), TokenError> {
        self.resume_matching_tokens_impl(binding, referenced_tokens, false)
    }

    fn resume_matching_tokens_impl(
        &mut self,
        binding: &super::completed_context::ActualAttemptBinding,
        referenced_tokens: &[&str],
        require_all: bool,
    ) -> Result<(), TokenError> {
        use super::completed_context::MAX_CONTEXT_ENTRIES_PER_NAMESPACE;
        if referenced_tokens.is_empty() {
            return Ok(());
        }
        if referenced_tokens.iter().all(|t| self.resolve(t).is_ok()) {
            return Ok(());
        }
        if binding.store_id != self.store_id()?
            || binding.namespace != self.namespace
            || binding.generation != self.generation
        {
            return Err(TokenError::ContextUnavailable);
        }
        if !binding.reusable_principal {
            return if require_all {
                Err(TokenError::ContextUnavailable)
            } else {
                Ok(())
            };
        }
        let limits = &self.store.inner()?.limits;
        let ledger = self.store.allocation_ledger()?;
        let snapshot = {
            let state = self.store.lock()?;
            let ns = state
                .namespaces
                .get(&self.namespace)
                .ok_or(TokenError::ContextUnavailable)?;
            if ns.generation != self.generation || ns.policy_revision != binding.policy_revision {
                return Err(TokenError::ContextUnavailable);
            }
            if ns.identity.is_none() {
                return if require_all {
                    Err(TokenError::ContextUnavailable)
                } else {
                    Ok(())
                };
            }
            let slice = ns.contexts.as_ref().map_or(&[][..], StoreVec::as_slice);
            if require_all && slice.is_empty() {
                return Err(TokenError::ContextUnavailable);
            }
            let mut copied = StoreVec::with_capacity(
                slice.len(),
                ledger.clone(),
                MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
            )?;
            for entry in slice {
                copied.push_without_growth(entry.clone())?;
            }
            copied
        };
        let mut matched_indices = StoreVec::with_capacity(
            snapshot.len(),
            ledger.clone(),
            MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
        )?;
        for &tok in referenced_tokens {
            if self.resolve(tok).is_ok() {
                continue;
            }
            let mut found_idx = None;
            for (idx, entry) in snapshot.as_slice().iter().enumerate() {
                let rec = entry.get()?;
                if !rec.authorizes_attempt(binding)
                    || rec.policy_revision != binding.policy_revision
                {
                    continue;
                }
                let mut contains = false;
                for (_, mapping) in rec.deps_slice() {
                    if mapping.get()?.token()? == tok {
                        contains = true;
                        break;
                    }
                }
                if contains {
                    found_idx = Some(idx);
                    break;
                }
            }
            let Some(idx) = found_idx else {
                if require_all {
                    return Err(TokenError::ContextUnavailable);
                }
                continue;
            };
            if !matched_indices.as_slice().contains(&idx) {
                matched_indices.push_without_growth(idx)?;
            }
        }
        let mut total_candidates = self.authorized.len();
        for &idx in matched_indices.as_slice() {
            total_candidates = total_candidates
                .checked_add(snapshot.as_slice()[idx].get()?.deps_slice().len())
                .ok_or(TokenError::CapacityExceeded)?;
        }
        let mut combined = StoreVec::with_capacity(
            total_candidates.min(limits.entries_per_namespace),
            ledger.clone(),
            limits.entries_per_namespace,
        )?;
        for (digest, mapping) in self.authorized.as_slice() {
            combined.push_without_growth((*digest, mapping.clone()))?;
        }
        let mut added = 0usize;
        for &idx in matched_indices.as_slice() {
            for (digest, mapping) in snapshot.as_slice()[idx].get()?.deps_slice() {
                if !combined.as_slice().iter().any(|(d, _)| d == digest) {
                    if combined.len() >= limits.entries_per_namespace {
                        return Err(TokenError::CapacityExceeded);
                    }
                    combined.push_without_growth((*digest, mapping.clone()))?;
                    added += 1;
                }
            }
        }
        if added == 0 {
            return Ok(());
        }
        combined.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let staged = if combined.len() == combined.capacity() {
            combined
        } else {
            let mut exact =
                StoreVec::with_capacity(combined.len(), ledger, limits.entries_per_namespace)?;
            for (digest, mapping) in combined.as_slice() {
                exact.push_without_growth((*digest, mapping.clone()))?;
            }
            drop(combined);
            exact
        };
        let extra_metadata = added
            .checked_mul(AUTHORITY_METADATA)
            .ok_or(TokenError::CapacityExceeded)?;
        let charge = Charge {
            metadata_bytes: extra_metadata,
            ..Default::default()
        };
        let mut state = self.store.lock()?;
        self.store.check_global(&state, charge)?;
        let committed = state.committed.add(charge)?;
        let ns = state
            .namespaces
            .get_mut(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if ns.generation != self.generation {
            return Err(TokenError::ContextUnavailable);
        }
        ns.last_used = Instant::now();
        state.committed = committed;
        let mut old_authorized = MappingIndex {
            entries: Some(staged),
        };
        std::mem::swap(&mut self.authorized, &mut old_authorized);
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(extra_metadata)
            .ok_or(TokenError::CapacityExceeded)?;
        drop(state);
        drop(old_authorized);
        Ok(())
    }

    pub(super) fn verify_output_call_ids_extracted(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        previous_response_id: Option<&str>,
        orphan_output_call_ids: &[&str],
    ) -> Result<(), TokenError> {
        use super::completed_context::{call_id_digest, response_id_digest, valid_external_id};
        if orphan_output_call_ids.is_empty() {
            return Ok(());
        }
        let prev_id = previous_response_id
            .filter(|id| valid_external_id(id))
            .ok_or(TokenError::ContextUnavailable)?;
        if binding.store_id != self.store_id()?
            || binding.namespace != self.namespace
            || binding.generation != self.generation
            || !binding.reusable_principal
        {
            return Err(TokenError::ContextUnavailable);
        }
        let key = self.store.identity_key()?;
        let prev_digest = response_id_digest(key, prev_id)?;
        let mut call_digests = StoreVec::with_capacity(
            orphan_output_call_ids.len(),
            self.store.allocation_ledger()?,
            super::completed_context::MAX_CALLS_PER_CONTEXT,
        )?;
        for &cid in orphan_output_call_ids {
            if !valid_external_id(cid) {
                return Err(TokenError::ContextUnavailable);
            }
            call_digests.push_without_growth(call_id_digest(key, cid)?)?;
        }
        let state = self.store.lock()?;
        let ns = state
            .namespaces
            .get(&self.namespace)
            .ok_or(TokenError::ContextUnavailable)?;
        if ns.identity.is_none()
            || ns.generation != self.generation
            || ns.policy_revision != binding.policy_revision
        {
            return Err(TokenError::ContextUnavailable);
        }
        let slice = ns.contexts.as_ref().map_or(&[][..], StoreVec::as_slice);
        let rec = slice
            .iter()
            .find_map(|entry| {
                let r = entry.get().ok()?;
                (r.response_digest == prev_digest
                    && r.authorizes_attempt(binding)
                    && r.policy_revision == binding.policy_revision)
                    .then_some(r)
            })
            .ok_or(TokenError::ContextUnavailable)?;
        for cd in call_digests.as_slice() {
            if rec.find_call(cd).is_none() {
                return Err(TokenError::ContextUnavailable);
            }
        }
        Ok(())
    }
}

// Exact UTF-8 and length framing distinguish app/session boundaries. The complete
// digest selects a namespace only; it never grants request restoration authority.
pub(super) fn namespace_identity_digest(
    key: &[u8; 32],
    app: &str,
    session: &str,
) -> Result<[u8; 32], TokenError> {
    let app_len = u64::try_from(app.len()).map_err(|_| TokenError::CapacityExceeded)?;
    let session_len = u64::try_from(session.len()).map_err(|_| TokenError::CapacityExceeded)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| TokenError::Unavailable)?;
    mac.update(b"vary-dlp-namespace-identity-v1\0");
    mac.update(&app_len.to_be_bytes());
    mac.update(app.as_bytes());
    mac.update(&session_len.to_be_bytes());
    mac.update(session.as_bytes());
    Ok(mac.finalize().into_bytes().into())
}

#[cfg(test)]
pub(super) mod identity_test_support {
    use super::TokenError;
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        Entropy,
        IdentityWork,
    }
    type Hook = Box<dyn FnMut(Phase, bool) -> Result<(), TokenError>>;
    thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
    pub(in super::super) struct Guard {
        previous: Option<Hook>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            HOOK.with(|slot| slot.replace(self.previous.take()));
        }
    }
    pub(in super::super) fn install(
        hook: impl FnMut(Phase, bool) -> Result<(), TokenError> + 'static,
    ) -> Guard {
        Guard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }
    pub(super) fn emit(phase: Phase, state_unlocked: bool) -> Result<(), TokenError> {
        HOOK.with(|slot| match slot.borrow_mut().as_mut() {
            Some(hook) => hook(phase, state_unlocked),
            None => Ok(()),
        })
    }
}

#[cfg(test)]
mod response_scan_tests {
    use super::*;

    fn response_lease() -> Result<(DlpRequestLease, String), TokenError> {
        let store = DlpTokenStore::with_test_key([0x73; 32]);
        let mut lease = store.admit("codex", Some("synthetic-scan"), 1)?;
        let reference = lease.prepare(
            &[TokenFragment {
                raw: "synthetic response scan value",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |tokens| tokens.first().cloned().ok_or(TokenError::Unavailable),
        )?;
        Ok((lease, reference))
    }

    #[test]
    fn reference_format_migration_all_categories_and_encoding() -> Result<(), TokenError> {
        let categories = [
            (TokenCategory::Phone, "cn_phone"),
            (TokenCategory::IdCard, "cn_id_card"),
            (TokenCategory::PrivateKey, "private_key"),
            (TokenCategory::ApiKey, "api_token"),
            (TokenCategory::Database, "db_conn"),
            (TokenCategory::Password, "password"),
            (TokenCategory::Infrastructure, "infra"),
            (TokenCategory::Personal, "personal"),
            (TokenCategory::Custom, "custom"),
            (TokenCategory::Mixed, "mixed"),
        ];
        for (category, name) in categories {
            let key = [0x61; 32];
            let raw = "synthetic migration value";
            let reference = make_token(&key, raw, category)?;
            let header = format!("rredact-{name}-");
            assert!(
                reference.starts_with(&header),
                "new semantic header required"
            );
            assert!(
                reference.len() == header.len() + 23,
                "bounded semantic length required"
            );
            let digest = token_digest(&key, raw)?;
            let expected: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            assert!(
                reference[header.len()..reference.len() - 3] == expected,
                "encoding must remain stable"
            );
            assert!(reference.ends_with("-ai"), "hyphen suffix required");
            let ranges = scan_token_references(&reference)?;
            assert!(
                ranges == [0..reference.len()],
                "new reference must be recognized exactly"
            );
        }
        Ok(())
    }

    #[test]
    fn reference_format_migration_authority_and_legacy_rejection() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        assert!(
            reference.starts_with("rredact-custom-"),
            "new reference generation required"
        );
        assert!(
            lease.resolve(&reference).is_ok(),
            "issued reference must resolve"
        );
        let digest = &reference["rredact-custom-".len()..reference.len() - 3];
        for name in ["cn_phone", "private_key", "unknown"] {
            let changed = format!("rredact-{name}-{digest}-ai");
            assert!(
                lease.resolve(&changed).is_err(),
                "type changes never grant authority"
            );
        }
        let legacy = format!("vary_cu_{digest}_ai");
        assert!(
            scan_token_references(&legacy)?.len() == 1,
            "legacy reference must remain recognized"
        );
        assert!(
            lease.resolve(&legacy).is_err(),
            "legacy aliases must not restore"
        );
        assert!(
            scan_token_references("vary_cu_bad").is_err(),
            "legacy strict damage must refuse"
        );
        let other = lease.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert!(
            other.resolve(&reference).is_err(),
            "membership remains lease scoped"
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_plain_and_authorized_text() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        for text in ["", "ordinary response", reference.as_str()] {
            assert_eq!(validate_response_references(text, &lease), Ok(false));
        }
        Ok(())
    }

    #[test]
    fn text_fallback_scan_reports_only_recognized_malformed_candidates() -> Result<(), TokenError> {
        let (lease, _) = response_lease()?;
        for text in [
            "rredact-custom-".to_owned(),
            format!("rredact-cn_phone-{}-ai", "A".repeat(20)),
            format!("rredact-password-{}-ax", "0".repeat(20)),
            format!("rredact-infra-{}-ai", "g".repeat(20)),
        ] {
            assert_eq!(validate_response_references(&text, &lease), Ok(true));
        }
        assert_eq!(
            validate_response_references("vary_zz_ vary_cuX vary_CU_ vary_c", &lease),
            Ok(false)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_authorized_and_malformed_in_either_order() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        for text in [
            format!("{reference} rredact-custom-"),
            format!("rredact-custom- {reference}"),
        ] {
            assert_eq!(validate_response_references(&text, &lease), Ok(true));
        }
        Ok(())
    }

    #[test]
    fn text_fallback_scan_unauthorized_before_malformed_is_hard() -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert_eq!(
            validate_response_references(&format!("{reference} rredact-custom-"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_unauthorized_after_malformed_is_hard() -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert_eq!(
            validate_response_references(&format!("rredact-custom- {reference}"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_nested_authorized_reference_is_checked() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        assert_eq!(
            validate_response_references(&format!("rredact-custom-{reference}"), &lease),
            Ok(true)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_nested_unauthorized_reference_is_hard() -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert_eq!(
            validate_response_references(&format!("rredact-custom-{reference}"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_category_mismatch_after_malformed_is_hard() -> Result<(), TokenError> {
        let (lease, mut reference) = response_lease()?;
        reference.replace_range(8..14, "cn_phone");
        assert_eq!(
            validate_response_references(&format!("rredact-custom-{reference}"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_foreign_namespace_reference_is_hard() -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner
            .store
            .admit("codex", Some("other-synthetic-scan"), 1)?;
        assert_eq!(
            validate_response_references(&format!("rredact-custom- {reference}"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_malformed_candidate_limit() -> Result<(), TokenError> {
        let (lease, _) = response_lease()?;
        let mut text = "rredact-custom-".repeat(4096);
        assert_eq!(validate_response_references(&text, &lease), Ok(true));
        text.push_str("rredact-custom-");
        assert_eq!(
            validate_response_references(&text, &lease),
            Err(TokenError::CapacityExceeded)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_complete_candidate_limit() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        let mut text = reference.repeat(4096);
        assert_eq!(validate_response_references(&text, &lease), Ok(false));
        text.push_str(&reference);
        assert_eq!(
            validate_response_references(&text, &lease),
            Err(TokenError::CapacityExceeded)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_mixed_candidate_limit_counts_nested_complete() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        let mut text = "rredact-custom-".repeat(4095);
        text.push_str(&reference);
        assert_eq!(validate_response_references(&text, &lease), Ok(true));
        text.push_str("rredact-custom-");
        assert_eq!(
            validate_response_references(&text, &lease),
            Err(TokenError::CapacityExceeded)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_unknown_prefixes_do_not_count_toward_limit() -> Result<(), TokenError> {
        let (lease, _) = response_lease()?;
        assert_eq!(
            validate_response_references(&"vary_zz_".repeat(4097), &lease),
            Ok(false)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_utf8_malformed_and_nested_complete() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        for width in 0..=TOKEN_BYTES {
            let text = format!(
                "前🔒rredact-custom-{}🔐{reference}尾vary_🦀_",
                "0".repeat(width)
            );
            assert_eq!(validate_response_references(&text, &lease), Ok(true));
        }
        assert_eq!(
            validate_response_references(&format!("前🔒{reference}尾"), &lease),
            Ok(false)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_utf8_does_not_hide_unauthorized_reference() -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert_eq!(
            validate_response_references(&format!("前🔒rredact-custom-🔐{reference}尾"), &lease),
            Err(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_strict_scanner_still_stops_at_malformed() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        let text = format!("{reference} rredact-custom-{reference}");
        assert_eq!(validate_response_references(&text, &lease), Ok(true));
        let mut visited = 0;
        assert_eq!(
            visit_token_references(&text, |_| {
                visited += 1;
                Ok(())
            }),
            Err(TokenError::ContextUnavailable)
        );
        assert_eq!(visited, 1);
        assert_eq!(
            scan_token_references(&text).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_strict_complete_limit_is_unchanged() -> Result<(), TokenError> {
        let (_, reference) = response_lease()?;
        let mut text = reference.repeat(4096);
        assert!(scan_token_references(&text).is_ok());
        text.push_str(&reference);
        assert_eq!(
            scan_token_references(&text).err(),
            Some(TokenError::CapacityExceeded)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_strict_resolve_is_unchanged() -> Result<(), TokenError> {
        let (lease, reference) = response_lease()?;
        assert!(lease.resolve(&reference).is_ok());
        assert_eq!(
            lease.resolve("rredact-custom-").err(),
            Some(TokenError::ContextUnavailable)
        );
        let mut wrong_category = reference.clone();
        wrong_category.replace_range(8..14, "cn_phone");
        assert_eq!(
            lease.resolve(&wrong_category).err(),
            Some(TokenError::ContextUnavailable)
        );
        let unauthorized = lease.store.admit("codex", Some("synthetic-scan"), 1)?;
        assert_eq!(
            unauthorized.resolve(&reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_liveness_never_grants_authority_or_refreshes_ttl(
    ) -> Result<(), TokenError> {
        let (owner, reference) = response_lease()?;
        let lease = owner.store.admit("codex", Some("synthetic-scan"), 1)?;
        let before = {
            let state = lease.store.lock()?;
            let namespace = state
                .namespaces
                .get(&lease.namespace)
                .ok_or(TokenError::Unavailable)?;
            (namespace.last_used, namespace.active_leases)
        };
        let stats = lease.store.stats()?;
        assert_eq!(lease.validate_response_liveness(), Ok(()));
        assert_eq!(
            validate_response_references("rredact-custom-", &lease),
            Ok(true)
        );
        let after = {
            let state = lease.store.lock()?;
            let namespace = state
                .namespaces
                .get(&lease.namespace)
                .ok_or(TokenError::Unavailable)?;
            (namespace.last_used, namespace.active_leases)
        };
        assert_eq!(before, after);
        assert_eq!(lease.store.stats()?, stats);
        assert_eq!(
            lease.resolve(&reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_stale_generation_is_hard_even_without_complete() -> Result<(), TokenError>
    {
        let (lease, reference) = response_lease()?;
        {
            let mut state = lease.store.lock()?;
            let namespace = state
                .namespaces
                .get_mut(&lease.namespace)
                .ok_or(TokenError::Unavailable)?;
            namespace.generation[0] ^= 1;
        }
        assert_eq!(
            lease.validate_response_liveness(),
            Err(TokenError::ContextUnavailable)
        );
        for text in [
            "",
            "ordinary response",
            "rredact-custom-",
            reference.as_str(),
        ] {
            assert_eq!(
                validate_response_references(text, &lease),
                Err(TokenError::ContextUnavailable)
            );
        }
        assert_eq!(
            lease.resolve(&reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_scan_missing_namespace_is_hard() -> Result<(), TokenError> {
        let (mut lease, _) = response_lease()?;
        let namespace = lease.namespace;
        lease.namespace = u64::MAX;
        assert_eq!(
            lease.validate_response_liveness(),
            Err(TokenError::ContextUnavailable)
        );
        assert_eq!(
            validate_response_references("rredact-custom-", &lease),
            Err(TokenError::ContextUnavailable)
        );
        lease.namespace = namespace;
        Ok(())
    }

    #[test]
    fn text_fallback_scan_unavailable_store_is_hard() -> Result<(), TokenError> {
        let (lease, _) = response_lease()?;
        lease.store.lock()?.unavailable = true;
        assert_eq!(
            lease.validate_response_liveness(),
            Err(TokenError::Unavailable)
        );
        for text in ["", "ordinary response", "rredact-custom-"] {
            assert_eq!(
                validate_response_references(text, &lease),
                Err(TokenError::Unavailable)
            );
        }
        Ok(())
    }
}

// Optional delivered-history APIs never participate in token authority.
impl DlpRequestLease {
    pub(crate) fn history_binding_live(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
    ) -> bool {
        binding.namespace == self.namespace
            && binding.generation == self.generation
            && self.store.history_binding_live(binding)
    }

    fn history_namespace(
        &self,
        create: bool,
    ) -> Option<SharedAllocation<super::history_provenance::HistoryNamespace>> {
        {
            let state = self.store.lock().ok()?;
            let ns = state.namespaces.get(&self.namespace)?;
            if ns.generation != self.generation || ns.identity.is_none() {
                return None;
            }
            if let Some(history) = &ns.history {
                return Some(history.clone());
            }
        }
        if !create {
            return None;
        }
        let global = self.store.history_allocation().ok()?;
        let staged = super::history_provenance::HistoryNamespace::new(global)?;
        let mut state = self.store.lock().ok()?;
        let ns = state.namespaces.get_mut(&self.namespace)?;
        if ns.generation != self.generation || ns.identity.is_none() {
            return None;
        }
        let history = ns.history.get_or_insert(staged).clone();
        Some(history)
    }

    pub(crate) fn capture_delivered_history(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        response: &serde_json::Value,
    ) -> Option<super::history_provenance::PendingDeliveredHistory> {
        if !self.history_binding_live(binding) {
            return None;
        }
        self.history_namespace(true)?
            .get()
            .ok()?
            .capture(binding, response)
    }

    /// Caller must finish mandatory validation, encoding and delivery first.
    pub(crate) fn publish_delivered_history(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        pending: super::history_provenance::PendingDeliveredHistory,
    ) -> bool {
        let Some(issued) = pending.binding() else {
            return false;
        };
        if !self.history_binding_live(binding)
            || issued.store_id != binding.store_id
            || issued.namespace != binding.namespace
            || issued.generation != binding.generation
            || issued.policy_revision != binding.policy_revision
            || issued.provider_fingerprint != binding.provider_fingerprint
            || !issued.reusable_principal
        {
            return false;
        }
        let Some(history) = self.history_namespace(false) else {
            return false;
        };
        self.store
            .with_live_history_binding(binding, || {
                history.get().map(|history| history.publish(pending))
            })
            .is_some_and(|result| result.is_ok_and(|published| published.is_ok()))
    }

    pub(crate) fn history_projection<'a>(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        request: &'a serde_json::Value,
    ) -> Option<super::history_provenance::DlpHistoryProjection<'a>> {
        if !self.history_binding_live(binding) {
            return None;
        }
        self.history_namespace(false)?.get().ok()?.projection(
            self.store.clone(),
            Some(binding),
            request,
        )
    }

    #[cfg(test)]
    pub(super) fn evict_delivered_history_for_test(&self) -> Result<(), TokenError> {
        self.history_namespace(false)
            .ok_or(TokenError::Unavailable)?
            .get()?
            .evict_records_for_test()
    }

    /// Deferred exact candidates cannot authorize sending: final mapping checks
    /// still require the actual provider/protocol/target/principal binding.
    pub(crate) fn history_admission_candidate_projection<'a>(
        &self,
        request: &'a serde_json::Value,
    ) -> Option<super::history_provenance::DlpHistoryProjection<'a>> {
        self.history_namespace(false)?
            .get()
            .ok()?
            .projection(self.store.clone(), None, request)
    }
}
impl DlpTokenStore {
    fn history_allocation(&self) -> Result<StoreAllocationLedger, TokenError> {
        self.inner()?
            .history_allocation
            .get_or_init(super::history_provenance::global_history_allocation)
            .as_ref()
            .cloned()
            .map_err(|error| *error)
    }

    fn with_live_history_binding<T>(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let inner = self.inner().ok()?;
        if binding.store_id != inner as *const StoreInner as usize || !binding.reusable_principal {
            return None;
        }
        // Read the immutable source, then release store state before taking the
        // policy read guard. All nested locking is policy -> store -> history.
        let policy = {
            let state = self.lock().ok()?;
            let ns = state.namespaces.get(&binding.namespace)?;
            if ns.generation != binding.generation
                || ns.policy_revision != binding.policy_revision
                || ns.identity.is_none()
            {
                return None;
            }
            ns.history_policy
        };
        policy
            .with_revision(binding.policy_revision, || {
                let state = self.lock().ok()?;
                let ns = state.namespaces.get(&binding.namespace)?;
                if ns.generation != binding.generation
                    || ns.policy_revision != binding.policy_revision
                    || ns.history_policy != policy
                    || ns.identity.is_none()
                {
                    return None;
                }
                let result = operation();
                drop(state);
                Some(result)
            })
            .flatten()
    }

    pub(super) fn history_binding_live(
        &self,
        binding: &super::completed_context::ActualAttemptBinding,
    ) -> bool {
        self.with_live_history_binding(binding, || ()).is_some()
    }

    #[cfg(test)]
    pub(crate) fn history_metadata_bytes_for_test(&self) -> Option<usize> {
        self.history_bytes_for_test().ok()
    }

    #[cfg(test)]
    pub(super) fn history_bytes_for_test(&self) -> Result<usize, TokenError> {
        match self.inner()?.history_allocation.get() {
            Some(Ok(ledger)) => {
                let used = ledger.used();
                used.plaintext
                    .checked_add(used.metadata)
                    .ok_or(TokenError::CapacityExceeded)
            }
            Some(Err(error)) => Err(*error),
            None => Ok(0),
        }
    }
}
