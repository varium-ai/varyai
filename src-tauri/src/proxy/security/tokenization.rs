//! Private, memory-only primitives for reversible DLP references.
//! Recognition never grants authorization to restore a reference.
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{fmt, ops::Range};

const TOKEN_BYTES: usize = 31;
const CATEGORY_CODES: [&str; 10] = ["ph", "id", "pk", "ak", "db", "pw", "ip", "pi", "cu", "mx"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TokenCategory {
    PrivateKey,
    Database,
    ApiKey,
    Password,
    IdCard,
    Phone,
    Infrastructure,
    Personal,
    Custom,
    Mixed,
}

impl TokenCategory {
    fn code(self) -> &'static str {
        match self {
            Self::PrivateKey => "pk",
            Self::Database => "db",
            Self::ApiKey => "ak",
            Self::Password => "pw",
            Self::IdCard => "id",
            Self::Phone => "ph",
            Self::Infrastructure => "ip",
            Self::Personal => "pi",
            Self::Custom => "cu",
            Self::Mixed => "mx",
        }
    }
}

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
    while let Some(relative) = text[offset..].find("vary_") {
        let start = offset + relative;
        let bytes = &text.as_bytes()[start..];
        let recognized = bytes.len() >= 8
            && bytes[7] == b'_'
            && CATEGORY_CODES
                .iter()
                .any(|code| code.as_bytes() == &bytes[5..7]);
        if !recognized {
            offset = start + 5;
            continue;
        }
        if bytes.len() < TOKEN_BYTES
            || !bytes[8..28]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            || &bytes[28..TOKEN_BYTES] != b"_ai"
        {
            return Err(TokenError::ContextUnavailable);
        }
        offset = start + TOKEN_BYTES;
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
}

impl Mapping {
    fn token(&self) -> Result<&str, TokenError> {
        std::str::from_utf8(&self.token).map_err(|_| TokenError::Unavailable)
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

struct Namespace {
    identity: Option<[u8; 32]>,
    policy_revision: u64,
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
pub(super) struct DlpTokenStore {
    inner: Result<SharedAllocation<StoreInner>, TokenError>,
}

/// Authority is request-local, not equivalent to membership in the namespace.
pub(super) struct DlpRequestLease {
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

    pub(super) fn admit(
        &self,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
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
                        && namespace.policy_revision == policy_revision)
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
                generation,
                key,
                version: 0,
                active_leases: 1,
                last_used: Instant::now(),
                mappings: MappingIndex::default(),
                contexts: None,
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
    let bytes = encode_token_bytes(digest, category);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn encode_token_bytes(digest: [u8; 10], category: TokenCategory) -> [u8; TOKEN_BYTES] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; TOKEN_BYTES];
    bytes[..5].copy_from_slice(b"vary_");
    bytes[5..7].copy_from_slice(category.code().as_bytes());
    bytes[7] = b'_';
    for (index, byte) in digest.into_iter().enumerate() {
        bytes[8 + index * 2] = HEX[usize::from(byte >> 4)];
        bytes[9 + index * 2] = HEX[usize::from(byte & 15)];
    }
    bytes[28..].copy_from_slice(b"_ai");
    bytes
}

impl DlpRequestLease {
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
                        let bytes = encode_token_bytes(
                            *digest,
                            select_category(fragment.whole_span_categories),
                        );
                        let text =
                            std::str::from_utf8(&bytes).map_err(|_| TokenError::Unavailable)?;
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
                let token = tokens.as_slice()[*index]
                    .as_str()
                    .as_bytes()
                    .try_into()
                    .map_err(|_| TokenError::Unavailable)?;
                let mapping = SharedAllocation::new(
                    Mapping {
                        raw: StoreText::try_from_str(
                            fragments[*index].raw,
                            ledger.clone(),
                            limits.fragment_bytes,
                        )?,
                        token,
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

    pub(super) fn resolve(&self, token: &str) -> Result<&str, TokenError> {
        if token.len() != TOKEN_BYTES {
            return Err(TokenError::ContextUnavailable);
        }
        let mut complete = false;
        visit_token_references(token, |range| {
            complete = range == (0..TOKEN_BYTES);
            Ok(())
        })?;
        if !complete {
            return Err(TokenError::ContextUnavailable);
        }
        let mut digest = [0u8; 10];
        for (index, byte) in digest.iter_mut().enumerate() {
            let offset = 8 + index * 2;
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
        calls: &[super::completed_context::CompletedCallSpec<'_>],
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
        if calls.len() > MAX_CALLS_PER_CONTEXT
            || output_tokens.len() > limits.entries_per_namespace
            || opaques.len() > MAX_CONTEXT_ENTRIES_PER_NAMESPACE
        {
            return Err(TokenError::CapacityExceeded);
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
                visit_token_references(spec.payload, |range| {
                    self.resolve(&spec.payload[range]).map(|_| ())
                })?;
                if let Some(reasoning) = spec.reasoning {
                    visit_token_references(reasoning, |range| {
                        self.resolve(&reasoning[range]).map(|_| ())
                    })?;
                }
                vec.push_without_growth(CompletedCallEntry {
                    call_digest: call_id_digest(key, spec.call_id)?,
                    snapshot_digest: call_snapshot_digest(key, spec)?,
                })?;
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
                        if found.snapshot_digest != snapshot_digest {
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
                        if found.snapshot_digest != snapshot_digest || found_idx.is_some() {
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
            || !binding.reusable_principal
        {
            return Err(TokenError::ContextUnavailable);
        }
        let limits = &self.store.inner()?.limits;
        let ledger = self.store.allocation_ledger()?;
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
            let idx = found_idx.ok_or(TokenError::ContextUnavailable)?;
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
