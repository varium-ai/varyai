use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use super::checkpoint::{Context, Store as Codec};

const DOMAIN: &str = "varyai-continuation-v1:";
const ERROR: &str = "Continuation store unavailable";
const TTL: u64 = 24 * 60 * 60;
const MAX_RECORDS: usize = 512;
const MAX_MAPPINGS: usize = 4096;
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024;
const ENVELOPE_BYTES: usize = 64;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub targets: Vec<String>,
    pub responses: BTreeMap<String, Option<String>>,
    pub calls: BTreeMap<String, Option<String>>,
    pub sessions: BTreeMap<String, Option<String>>,
    pub opaque: BTreeMap<String, Option<String>>,
    pub items: BTreeMap<String, Option<String>>,
}

impl Snapshot {
    fn merge(&mut self, previous: &Self) {
        let mut current_targets = self.targets.clone();
        let mut previous_targets = previous.targets.clone();
        current_targets.sort();
        previous_targets.sort();
        if current_targets != previous_targets {
            return;
        }
        for (current, previous) in [
            (&mut self.responses, &previous.responses),
            (&mut self.calls, &previous.calls),
            (&mut self.opaque, &previous.opaque),
            (&mut self.items, &previous.items),
        ] {
            for (key, owner) in previous {
                current
                    .entry(key.clone())
                    .and_modify(|next| {
                        if next != owner {
                            *next = None;
                        }
                    })
                    .or_insert_with(|| owner.clone());
            }
        }
        for (key, owner) in &previous.sessions {
            self.sessions
                .entry(key.clone())
                .or_insert_with(|| owner.clone());
        }
    }
}

pub(super) fn hash(domain: &str, value: &str) -> String {
    let mut digest = Sha256::new();
    for part in [domain, value] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// Hex-only strings have fixed JSON widths: bound allocation without serializing
// or encrypting on the forwarding thread. The worker checks the real size too.
fn snapshot_size(snapshot: &Snapshot) -> Option<usize> {
    if snapshot.targets.len() > 256 {
        return None;
    }
    let targets: BTreeSet<_> = snapshot.targets.iter().collect();
    if targets.len() != snapshot.targets.len() || targets.iter().any(|target| !hex(target)) {
        return None;
    }
    let maps = [
        &snapshot.responses,
        &snapshot.calls,
        &snapshot.sessions,
        &snapshot.opaque,
        &snapshot.items,
    ];
    if maps.iter().map(|map| map.len()).sum::<usize>() > MAX_MAPPINGS {
        return None;
    }
    let mut size =
        r#"{"targets":[],"responses":{},"calls":{},"sessions":{},"opaque":{},"items":{}}"#.len();
    size += targets.len() * 66 + targets.len().saturating_sub(1);
    for map in maps {
        size += map.len().saturating_sub(1);
        for (key, owner) in map {
            if !hex(key) || owner.as_ref().is_some_and(|owner| !targets.contains(owner)) {
                return None;
            }
            size += 67 + if owner.is_some() { 66 } else { 4 };
        }
    }
    (size + ENVELOPE_BYTES <= MAX_SNAPSHOT_BYTES).then_some(size)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

fn fresh(at: u64, time: u64) -> bool {
    at <= time && time - at < TTL
}

fn context(scope: &str, id: &str) -> Context {
    Context {
        identity: format!("{DOMAIN}{scope}"),
        alias: id.into(),
        destination: "continuation-v1".into(),
    }
}

#[derive(Clone)]
struct Record {
    at: u64,
    order: u64,
    snapshot: Option<Snapshot>,
    budget: usize,
}

type Records = BTreeMap<(String, String), Record>;

struct State {
    records: Records,
    revision: u64,
    completed: u64,
    result: Result<(), &'static str>,
}

struct Inner {
    state: Mutex<State>,
    revisions: watch::Sender<u64>,
    completions: watch::Sender<u64>,
    path: PathBuf,
    codec: Arc<Codec>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

pub(super) struct Store {
    inner: Arc<Inner>,
}

impl Store {
    pub async fn load(path: PathBuf, key_path: PathBuf) -> Result<Arc<Self>, &'static str> {
        let read_path = path.clone();
        let (codec, records) = tokio::task::spawn_blocking(move || {
            let codec = Codec::load_or_create(&key_path).map_err(|_| ERROR)?;
            let records = read_records(&read_path, &codec)?;
            Ok::<_, &'static str>((Arc::new(codec), records))
        })
        .await
        .map_err(|_| ERROR)??;
        let revision = records
            .values()
            .map(|record| record.order)
            .max()
            .unwrap_or(0);
        let (revisions, receiver) = watch::channel(revision);
        let (completions, _) = watch::channel(revision);
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                records,
                revision,
                completed: revision,
                result: Ok(()),
            }),
            revisions,
            completions,
            path,
            codec,
        });
        tokio::spawn(worker(Arc::downgrade(&inner), receiver));
        Ok(Arc::new(Self { inner }))
    }

    pub fn scope(&self, scope: &str) -> Vec<(String, Snapshot)> {
        let time = now();
        self.inner
            .state()
            .records
            .iter()
            .filter(|((saved_scope, _), record)| saved_scope == scope && fresh(record.at, time))
            // Empty targets preserve fail-closed invalidation across restart.
            .map(|((_, id), record)| (id.clone(), record.snapshot.clone().unwrap_or_default()))
            .collect()
    }

    pub fn record(&self, scope: &str, id: &str, snapshot: Snapshot) {
        if !hex(scope) || !hex(id) {
            return;
        }
        let key = (scope.into(), id.into());
        let mut snapshot = snapshot_size(&snapshot).map(|_| snapshot);
        let mut state = self.inner.state();
        if let Some(previous) = state
            .records
            .get(&key)
            .filter(|record| fresh(record.at, now()))
        {
            match (snapshot.as_mut(), previous.snapshot.as_ref()) {
                (Some(current), Some(previous)) => {
                    // Compatible generations overlap while an old stream finishes.
                    // Union ownership and preserve collisions, never last-writer-wins.
                    current.merge(previous);
                }
                (_, None) => snapshot = None,
                _ => {}
            }
        }
        let size = snapshot.as_ref().and_then(snapshot_size);
        let snapshot = size.and(snapshot);
        // Conservative ciphertext/base64, metadata and wrapper allowance.
        let budget = 512 + (size.unwrap_or(4) + ENVELOPE_BYTES + 29).div_ceil(3) * 4;
        state.revision = state.revision.saturating_add(1);
        let revision = state.revision;
        state.records.insert(
            key,
            Record {
                at: now(),
                order: revision,
                snapshot,
                budget,
            },
        );
        prune(&mut state.records);
        drop(state);
        self.inner.revisions.send_replace(revision);
    }

    pub async fn flush(&self) -> Result<(), &'static str> {
        let mut completions = self.inner.completions.subscribe();
        let revision = self.inner.state().revision;
        loop {
            {
                let state = self.inner.state();
                if state.completed >= revision {
                    return state.result;
                }
            }
            completions.changed().await.map_err(|_| ERROR)?;
        }
    }
}

fn prune(records: &mut Records) {
    let time = now();
    records.retain(|_, record| fresh(record.at, time));
    let mut budget = 512 + records.values().map(|record| record.budget).sum::<usize>();
    while records.len() > MAX_RECORDS || budget > MAX_FILE_BYTES {
        let oldest = records
            .iter()
            .min_by_key(|(_, record)| (record.at, record.order))
            .map(|(key, _)| key.clone());
        if let Some(record) = oldest.and_then(|key| records.remove(&key)) {
            budget -= record.budget;
        } else {
            break;
        }
    }
}

async fn worker(weak: Weak<Inner>, mut revisions: watch::Receiver<u64>) {
    while revisions.changed().await.is_ok() {
        let Some(inner) = weak.upgrade() else { return };
        let (revision, records) = {
            let mut state = inner.state();
            if state.completed >= state.revision {
                continue;
            }
            prune(&mut state.records);
            (state.revision, state.records.clone())
        };
        let path = inner.path.clone();
        let codec = inner.codec.clone();
        drop(inner);
        let result = tokio::task::spawn_blocking(move || write_records(&path, &codec, records))
            .await
            .unwrap_or(Err(ERROR));
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut state = inner.state();
            state.completed = revision;
            state.result = result;
        }
        // A failed generation is acknowledged once; only a new revision retries.
        inner.completions.send_replace(revision);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    version: u8,
    records: Vec<DiskRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    at: u64,
    snapshot: Option<Snapshot>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskRecord {
    scope: String,
    id: String,
    at: u64,
    seal: String,
}

fn validate_metadata(metadata: &Metadata) -> Result<(), &'static str> {
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(ERROR);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(ERROR);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ERROR);
        }
    }
    Ok(())
}

fn preflight(path: &Path) -> Result<bool, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_metadata(&metadata)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ERROR),
    }
}

fn read_records(path: &Path, codec: &Codec) -> Result<Records, &'static str> {
    if !preflight(path)? {
        return Ok(Records::new());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(|_| ERROR)?;
    validate_metadata(&file.metadata().map_err(|_| ERROR)?)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ERROR)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ERROR);
    }
    let disk: Disk = serde_json::from_slice(&bytes).map_err(|_| ERROR)?;
    if disk.version != 1 || disk.records.len() > MAX_RECORDS {
        return Err(ERROR);
    }
    let mut records = Records::new();
    let mut seen = BTreeSet::new();
    let time = now();
    for (order, record) in disk.records.into_iter().enumerate() {
        if !hex(&record.scope)
            || !hex(&record.id)
            || !seen.insert((record.scope.clone(), record.id.clone()))
        {
            return Err(ERROR);
        }
        let json = codec
            .open(&context(&record.scope, &record.id), &record.seal)
            .map_err(|_| ERROR)?;
        let envelope: Envelope = serde_json::from_str(&json).map_err(|_| ERROR)?;
        // Authenticate time before filtering: outer edits cannot revive or hide a pin.
        if envelope.at != record.at {
            return Err(ERROR);
        }
        if !fresh(record.at, time) {
            continue;
        }
        let snapshot = envelope.snapshot;
        let size = match &snapshot {
            Some(snapshot) => snapshot_size(snapshot).ok_or(ERROR)?,
            None => 4,
        };
        let budget = 512 + (size + ENVELOPE_BYTES + 29).div_ceil(3) * 4;
        records.insert(
            (record.scope, record.id),
            Record {
                at: record.at,
                order: order as u64,
                snapshot,
                budget,
            },
        );
    }
    prune(&mut records);
    Ok(records)
}

fn write_records(path: &Path, codec: &Codec, records: Records) -> Result<(), &'static str> {
    let records = records
        .into_iter()
        .map(|((scope, id), record)| {
            let json = serde_json::to_string(&Envelope {
                at: record.at,
                snapshot: record.snapshot,
            })
            .map_err(|_| ERROR)?;
            if json.len() > MAX_SNAPSHOT_BYTES {
                return Err(ERROR);
            }
            let seal = codec
                .seal(&context(&scope, &id), &json)
                .map_err(|_| ERROR)?;
            Ok(DiskRecord {
                scope,
                id,
                at: record.at,
                seal,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let bytes = serde_json::to_vec(&Disk {
        version: 1,
        records,
    })
    .map_err(|_| ERROR)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ERROR);
    }
    preflight(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(parent).map_err(|_| ERROR)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".varyai-continuation-")
        .tempfile_in(parent)
        .map_err(|_| ERROR)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ERROR)?;
    }
    staged
        .write_all(&bytes)
        .and_then(|_| staged.flush())
        .map_err(|_| ERROR)?;
    staged.as_file().sync_all().map_err(|_| ERROR)?;
    preflight(path)?;
    staged.persist(path).map_err(|_| ERROR)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use tempfile::TempDir;

    type TestResult = Result<(), Box<dyn Error>>;

    fn fixture() -> Result<(TempDir, PathBuf, PathBuf), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("continuations.json");
        let key = dir.path().join("checkpoint.key");
        Ok((dir, path, key))
    }

    fn snapshot() -> Snapshot {
        let target = hash("target", "synthetic-account");
        Snapshot {
            targets: vec![target.clone()],
            responses: [(hash("response", "synthetic-response"), Some(target.clone()))].into(),
            calls: [(hash("call", "synthetic-call"), None)].into(),
            sessions: [(hash("session", "synthetic-session"), Some(target.clone()))].into(),
            opaque: [(hash("opaque", "synthetic-opaque"), Some(target.clone()))].into(),
            items: [(hash("item", "synthetic-item"), Some(target))].into(),
        }
    }

    #[test]
    fn hashes_separate_domains_and_field_boundaries() {
        assert!(hex(&hash("synthetic", "value")));
        assert_eq!(hash("synthetic", "value"), hash("synthetic", "value"));
        assert_ne!(hash("a", "bc"), hash("ab", "c"));
        assert_ne!(hash("response", "same"), hash("call", "same"));
    }

    #[tokio::test]
    async fn reload_preserves_interleaved_conversations_and_collision_owners() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic-scope");
        let ids: Vec<_> = (0..24)
            .map(|i| hash("conversation", &i.to_string()))
            .collect();
        for id in &ids {
            store.record(&scope, id, snapshot());
        }
        store.flush().await?;
        let bytes = fs::read_to_string(&path)?;
        for private in [
            "synthetic-account",
            "synthetic-response",
            "targets",
            "responses",
        ] {
            assert!(!bytes.contains(private));
        }
        assert!(!bytes.contains(&snapshot().targets[0]));
        drop(store);
        let reloaded = Store::load(path, key).await?;
        let records = reloaded.scope(&scope);
        assert_eq!(records.len(), ids.len());
        for (id, restored) in records {
            assert!(ids.contains(&id));
            assert_eq!(
                restored.calls.get(&hash("call", "synthetic-call")),
                Some(&None)
            );
            assert_eq!(
                serde_json::to_value(restored)?,
                serde_json::to_value(snapshot())?
            );
        }
        assert!(reloaded.scope(&hash("scope", "other")).is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_or_oversized_updates_replace_prior_records_with_tombstones() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic");
        let mut bad_owner = snapshot();
        bad_owner.responses.insert(
            hash("response", "bad-owner"),
            Some(hash("target", "absent")),
        );
        let mut bad_key = snapshot();
        bad_key.items.insert("synthetic-invalid-key".into(), None);
        let mut duplicate_target = snapshot();
        duplicate_target
            .targets
            .push(duplicate_target.targets[0].clone());
        let mut excess_targets = snapshot();
        excess_targets.targets = (0..257).map(|i| hash("target", &i.to_string())).collect();
        let mut excess_maps = snapshot();
        excess_maps.responses = (0..4097)
            .map(|i| (hash("response", &i.to_string()), None))
            .collect();
        let mut excess_bytes = snapshot();
        excess_bytes.responses = (0..1200)
            .map(|i| {
                (
                    hash("response", &i.to_string()),
                    Some(excess_bytes.targets[0].clone()),
                )
            })
            .collect();
        for (index, invalid) in [
            bad_owner,
            bad_key,
            duplicate_target,
            excess_targets,
            excess_maps,
            excess_bytes,
        ]
        .into_iter()
        .enumerate()
        {
            let id = hash("conversation", &index.to_string());
            store.record(&scope, &id, snapshot());
            store.flush().await?;
            assert_eq!(store.scope(&scope).len(), index + 1);
            store.record(&scope, &id, invalid);
            assert!(store
                .scope(&scope)
                .iter()
                .any(|(saved, snapshot)| saved == &id && snapshot.targets.is_empty()));
            store.record(&scope, &id, snapshot());
            assert!(store
                .scope(&scope)
                .iter()
                .any(|(saved, snapshot)| saved == &id && snapshot.targets.is_empty()));
            store.flush().await?;
            let loaded = Store::load(path.clone(), key.clone()).await?;
            assert!(loaded
                .scope(&scope)
                .iter()
                .any(|(saved, snapshot)| saved == &id && snapshot.targets.is_empty()));
            let wrapper: Disk = serde_json::from_slice(&fs::read(&path)?)?;
            assert_eq!(wrapper.records.len(), index + 1);
            let record = wrapper
                .records
                .iter()
                .find(|record| record.id == id)
                .ok_or("missing record")?;
            let codec = Codec::load_or_create(&key)?;
            let envelope: Envelope = serde_json::from_str(
                &codec.open(&context(&record.scope, &record.id), &record.seal)?,
            )?;
            assert_eq!(envelope.at, record.at);
            assert!(envelope.snapshot.is_none());
        }
        Ok(())
    }

    #[tokio::test]
    async fn expired_and_future_records_are_ignored_and_context_tampering_is_refused() -> TestResult
    {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic");
        store.record(&scope, &hash("conversation", "synthetic"), snapshot());
        store.flush().await?;
        drop(store);
        let original = fs::read(&path)?;
        let codec = Codec::load_or_create(&key)?;
        for at in [now().saturating_sub(TTL + 1), now() + TTL] {
            let mut disk: Disk = serde_json::from_slice(&original)?;
            let record = &mut disk.records[0];
            record.at = at;
            fs::write(&path, serde_json::to_vec(&disk)?)?;
            assert!(matches!(
                Store::load(path.clone(), key.clone()).await,
                Err(ERROR)
            ));
            let record = &mut disk.records[0];
            record.seal = codec.seal(
                &context(&record.scope, &record.id),
                &serde_json::to_string(&Envelope {
                    at,
                    snapshot: Some(snapshot()),
                })?,
            )?;
            fs::write(&path, serde_json::to_vec(&disk)?)?;
            let loaded = Store::load(path.clone(), key.clone()).await?;
            assert!(loaded.scope(&scope).is_empty());
            disk.records[0].at = now();
            fs::write(&path, serde_json::to_vec(&disk)?)?;
            assert!(matches!(
                Store::load(path.clone(), key.clone()).await,
                Err(ERROR)
            ));
        }
        let mut disk: Disk = serde_json::from_slice(&original)?;
        disk.records[0].id = hash("conversation", "changed");
        fs::write(&path, serde_json::to_vec(&disk)?)?;
        assert!(matches!(Store::load(path, key).await, Err(ERROR)));
        Ok(())
    }

    #[tokio::test]
    async fn malformed_and_oversized_files_are_unavailable() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        store.record(
            &hash("scope", "synthetic"),
            &hash("conversation", "synthetic"),
            snapshot(),
        );
        store.flush().await?;
        drop(store);
        for contents in [b"not json".as_slice(), br#"{"version":2,"records":[]}"#] {
            fs::write(&path, contents)?;
            assert!(matches!(
                Store::load(path.clone(), key.clone()).await,
                Err(ERROR)
            ));
        }
        fs::OpenOptions::new()
            .write(true)
            .open(&path)?
            .set_len(MAX_FILE_BYTES as u64 + 1)?;
        assert!(matches!(Store::load(path, key).await, Err(ERROR)));
        Ok(())
    }

    #[tokio::test]
    async fn records_are_bounded_and_flush_acknowledges_all_generations() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic");
        for i in 0..600 {
            store.record(&scope, &hash("conversation", &i.to_string()), snapshot());
        }
        assert_eq!(store.scope(&scope).len(), MAX_RECORDS);
        let flushing = store.flush();
        let updating = async {
            tokio::task::yield_now().await;
            for i in 0..32 {
                store.record(&scope, &hash("latest", &i.to_string()), snapshot());
                tokio::task::yield_now().await;
            }
            store.flush().await
        };
        let (first, last) = tokio::join!(flushing, updating);
        first?;
        last?;
        let expected = store.scope(&scope).len();
        let loaded = Store::load(path, key).await?;
        assert_eq!(loaded.scope(&scope).len(), expected);
        for i in 0..32 {
            assert!(loaded
                .scope(&scope)
                .iter()
                .any(|(id, _)| id == &hash("latest", &i.to_string())));
        }
        Ok(())
    }

    #[tokio::test]
    async fn global_byte_budget_prunes_old_records_and_keeps_latest_invalidation() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic");
        let mut large = snapshot();
        large.responses = (0..950)
            .map(|i| {
                (
                    hash("response", &i.to_string()),
                    Some(large.targets[0].clone()),
                )
            })
            .collect();
        assert!(snapshot_size(&large).is_some());
        for i in 0..120 {
            store.record(&scope, &hash("conversation", &i.to_string()), large.clone());
        }
        let count = store.scope(&scope).len();
        assert!(count < 120 && count > 1);
        let latest = hash("conversation", "119");
        large.items.insert("synthetic-invalid".into(), None);
        store.record(&scope, &latest, large);
        store.flush().await?;
        assert!(fs::metadata(&path)?.len() <= MAX_FILE_BYTES as u64);
        let loaded = Store::load(path, key).await?;
        assert!(loaded
            .scope(&scope)
            .iter()
            .any(|(id, snapshot)| id == &latest && snapshot.targets.is_empty()));
        assert!(!loaded
            .scope(&scope)
            .iter()
            .any(|(id, _)| id == &hash("conversation", "0")));
        Ok(())
    }

    #[tokio::test]
    async fn write_failure_acknowledges_flush_and_a_future_update_retries() -> TestResult {
        let (_dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        fs::create_dir(&path)?;
        let scope = hash("scope", "synthetic");
        let id = hash("conversation", "synthetic");
        // A current-thread runtime cannot run the worker until this yields.
        store.record(&scope, &id, snapshot());
        assert_eq!(store.scope(&scope).len(), 1);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), store.flush()).await?,
            Err(ERROR)
        );
        assert_eq!(store.flush().await, Err(ERROR));
        fs::remove_dir(&path)?;
        store.record(&scope, &id, snapshot());
        store.flush().await?;
        assert_eq!(Store::load(path, key).await?.scope(&scope).len(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn insecure_files_symlinks_and_fifos_are_refused() -> TestResult {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{symlink, PermissionsExt},
        };
        let (dir, path, key) = fixture()?;
        let store = Store::load(path.clone(), key.clone()).await?;
        let scope = hash("scope", "synthetic");
        let id = hash("conversation", "synthetic");
        store.record(&scope, &id, snapshot());
        store.flush().await?;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert!(matches!(
            Store::load(path.clone(), key.clone()).await,
            Err(ERROR)
        ));
        fs::remove_file(&path)?;
        let target = dir.path().join("synthetic-target");
        fs::write(&target, b"synthetic untouched data")?;
        symlink(&target, &path)?;
        assert!(matches!(
            Store::load(path.clone(), key.clone()).await,
            Err(ERROR)
        ));
        store.record(&scope, &id, snapshot());
        assert_eq!(store.flush().await, Err(ERROR));
        assert_eq!(fs::read(&target)?, b"synthetic untouched data");
        fs::remove_file(&path)?;
        fs::remove_file(&key)?;
        symlink(&target, &key)?;
        assert!(matches!(
            Store::load(path.clone(), key.clone()).await,
            Err(ERROR)
        ));
        fs::remove_file(&key)?;
        let fifo = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: the synthetic temporary path is a valid NUL-terminated string.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(matches!(Store::load(path, key).await, Err(ERROR)));
        Ok(())
    }
}
