use std::{
    fs::{self, Metadata, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "varyai1:";
const VERSION: u8 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const MAX_SUMMARY_BYTES: usize = 128 * 1024;
const MAX_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_SEAL_BYTES: usize = 256 * 1024;
const MAX_PAYLOAD_BYTES: usize = 1 + NONCE_BYTES + MAX_SUMMARY_BYTES + TAG_BYTES;
const KEY_ERROR: &str = "Checkpoint key unavailable";
const INVALID_CHECKPOINT: &str = "Invalid checkpoint";

/// Identity and destination are opaque hashes, never credentials or endpoint URLs.
#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Context {
    pub identity: String,
    pub alias: String,
    pub destination: String,
}

// Deliberately no Debug implementation: the cipher owns the private key.
pub(super) struct Store {
    cipher: ChaCha20Poly1305,
}

impl Store {
    /// Read or atomically publish a private key. Run outside request forwarding.
    pub(super) fn load_or_create(path: &Path) -> Result<Self, &'static str> {
        if let Some(store) = read_key(path)? {
            return Ok(store);
        }
        path.file_name().ok_or(KEY_ERROR)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent).map_err(|_| KEY_ERROR)?;

        // Like config::atomic_write_private, stage a complete 0600 file in the
        // same directory. Unlike its replacing rename, publication must never
        // overwrite another creator's key or a malformed existing destination.
        let mut staged = tempfile::Builder::new()
            .prefix(".varyai-checkpoint-")
            .tempfile_in(parent)
            .map_err(|_| KEY_ERROR)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| KEY_ERROR)?;
        }
        let mut key = [0; KEY_BYTES];
        getrandom::getrandom(&mut key).map_err(|_| KEY_ERROR)?;
        staged.write_all(&key).map_err(|_| KEY_ERROR)?;
        staged.flush().map_err(|_| KEY_ERROR)?;
        staged.as_file().sync_all().map_err(|_| KEY_ERROR)?;
        match staged.persist_noclobber(path) {
            Ok(_) => {}
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                // The concurrent winner is authoritative; the losing temporary
                // key is removed when the persistence error drops.
            }
            Err(_) => return Err(KEY_ERROR),
        }
        read_key(path)?.ok_or(KEY_ERROR)
    }

    pub(super) fn seal(&self, context: &Context, summary: &str) -> Result<String, &'static str> {
        validate_summary(summary)?;
        let aad = associated_data(context)?;
        let mut nonce = [0; NONCE_BYTES];
        getrandom::getrandom(&mut nonce).map_err(|_| INVALID_CHECKPOINT)?;
        let ciphertext = self
            .cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: summary.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| INVALID_CHECKPOINT)?;
        // Only version, random nonce and authenticated ciphertext leave the codec.
        let mut payload = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
        payload.push(VERSION);
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        let seal = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(payload));
        if seal.len() > MAX_SEAL_BYTES {
            return Err(INVALID_CHECKPOINT);
        }
        Ok(seal)
    }

    pub(super) fn open(&self, context: &Context, seal: &str) -> Result<String, &'static str> {
        if seal.len() > MAX_SEAL_BYTES {
            return Err(INVALID_CHECKPOINT);
        }
        let encoded = seal.strip_prefix(PREFIX).ok_or(INVALID_CHECKPOINT)?;
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| INVALID_CHECKPOINT)?;
        if payload.len() <= 1 + NONCE_BYTES + TAG_BYTES
            || payload.len() > MAX_PAYLOAD_BYTES
            || payload.first() != Some(&VERSION)
        {
            return Err(INVALID_CHECKPOINT);
        }
        let nonce: [u8; NONCE_BYTES] = payload[1..1 + NONCE_BYTES]
            .try_into()
            .map_err(|_| INVALID_CHECKPOINT)?;
        let aad = associated_data(context)?;
        let plaintext = self
            .cipher
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &payload[1 + NONCE_BYTES..],
                    aad: &aad,
                },
            )
            .map_err(|_| INVALID_CHECKPOINT)?;
        let summary = String::from_utf8(plaintext).map_err(|_| INVALID_CHECKPOINT)?;
        validate_summary(&summary)?;
        Ok(summary)
    }
}

fn validate_summary(summary: &str) -> Result<(), &'static str> {
    if summary.len() > MAX_SUMMARY_BYTES || summary.trim().is_empty() {
        return Err(INVALID_CHECKPOINT);
    }
    Ok(())
}

fn associated_data(context: &Context) -> Result<Vec<u8>, &'static str> {
    // Bound serialization before allocating, and encode field boundaries rather
    // than concatenating potentially ambiguous strings.
    let size = context
        .identity
        .len()
        .saturating_add(context.alias.len())
        .saturating_add(context.destination.len());
    if size > MAX_CONTEXT_BYTES {
        return Err(INVALID_CHECKPOINT);
    }
    serde_json::to_vec(&(PREFIX, VERSION, context)).map_err(|_| INVALID_CHECKPOINT)
}

fn validate_key_metadata(metadata: &Metadata) -> Result<(), &'static str> {
    if !metadata.is_file() || metadata.len() != KEY_BYTES as u64 {
        return Err(KEY_ERROR);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(KEY_ERROR);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(KEY_ERROR);
        }
    }
    Ok(())
}

fn read_key(path: &Path) -> Result<Option<Store>, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_key_metadata(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(KEY_ERROR),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Prevent a concurrent symlink or FIFO substitution from being followed
        // or blocking between the path check and opening the descriptor.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(path).map_err(|_| KEY_ERROR)?;
    validate_key_metadata(&file.metadata().map_err(|_| KEY_ERROR)?)?;
    let mut key = [0; KEY_BYTES];
    file.read_exact(&mut key).map_err(|_| KEY_ERROR)?;
    if file.read(&mut [0]).map_err(|_| KEY_ERROR)? != 0 {
        return Err(KEY_ERROR);
    }
    let cipher = ChaCha20Poly1305::new((&key).into());
    Ok(Some(Store { cipher }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chacha20poly1305::{
        aead::{Aead, Payload},
        Nonce,
    };
    use std::{fs, sync::Arc};
    use tempfile::TempDir;

    fn context() -> Context {
        Context {
            identity: "synthetic-opaque-identity-hash".into(),
            alias: "synthetic-model-alias".into(),
            destination: "synthetic-opaque-destination-hash".into(),
        }
    }

    fn fixture() -> (TempDir, Store) {
        let dir = tempfile::tempdir().expect("synthetic temporary directory");
        let store = Store::load_or_create(&dir.path().join("checkpoint.key"))
            .expect("create synthetic checkpoint key");
        (dir, store)
    }

    fn encode(payload: &[u8]) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(payload))
    }

    fn unchecked_seal(store: &Store, context: &Context, summary: &[u8]) -> String {
        let aad = serde_json::to_vec(&(PREFIX, VERSION, context)).expect("synthetic AAD");
        let nonce_bytes = [7; NONCE_BYTES];
        let ciphertext = store
            .cipher
            .encrypt(
                &Nonce::from(nonce_bytes),
                Payload {
                    msg: summary,
                    aad: &aad,
                },
            )
            .expect("encrypt synthetic invalid checkpoint");
        let mut payload = vec![VERSION];
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);
        encode(&payload)
    }

    fn write_private_key(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("write synthetic key fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .expect("make synthetic key private");
        }
    }

    #[test]
    fn round_trip_survives_restart_and_only_the_key_is_persisted() {
        let (dir, store) = fixture();
        let path = dir.path().join("checkpoint.key");
        let ctx = context();
        let summary = "  Synthetic checkpoint summary with Unicode: 日本語.\n";
        let seal = store.seal(&ctx, summary).expect("seal synthetic summary");
        assert!(seal.starts_with(PREFIX));
        assert!(seal.len() <= MAX_SEAL_BYTES);
        assert_eq!(store.open(&ctx, &seal), Ok(summary.to_string()));
        let payload = URL_SAFE_NO_PAD
            .decode(seal.strip_prefix(PREFIX).expect("synthetic prefix"))
            .expect("synthetic payload");
        for plaintext in [summary, &ctx.identity, &ctx.alias, &ctx.destination] {
            assert!(
                !seal.contains(plaintext),
                "seal exposed synthetic plaintext"
            );
            assert!(
                !payload
                    .windows(plaintext.len())
                    .any(|part| part == plaintext.as_bytes()),
                "payload exposed synthetic plaintext"
            );
        }
        let original_key = fs::read(&path).expect("read synthetic key");
        assert_eq!(original_key.len(), KEY_BYTES);
        drop(store);
        let reloaded = Store::load_or_create(&path).expect("reload synthetic key");
        assert_eq!(reloaded.open(&ctx, &seal), Ok(summary.to_string()));
        assert!(
            original_key == fs::read(&path).expect("read synthetic key after restart"),
            "reloading replaced the key"
        );
        assert_eq!(
            fs::read_dir(dir.path())
                .expect("temporary directory")
                .count(),
            1
        );
    }

    #[test]
    fn each_context_field_and_its_boundaries_are_authenticated() {
        let (_dir, store) = fixture();
        let ctx = context();
        let seal = store.seal(&ctx, "Synthetic summary").expect("seal fixture");
        let mut changed = ctx.clone();
        changed.identity.push('x');
        assert!(store.open(&changed, &seal).is_err());
        changed = ctx.clone();
        changed.alias.push('x');
        assert!(store.open(&changed, &seal).is_err());
        changed = ctx.clone();
        changed.destination.push('x');
        assert!(store.open(&changed, &seal).is_err());

        let first = Context {
            identity: "synthetic:a".into(),
            alias: "b:c".into(),
            destination: "d".into(),
        };
        let second = Context {
            identity: "synthetic:a:b".into(),
            alias: "c".into(),
            destination: "d".into(),
        };
        let seal = store
            .seal(&first, "Synthetic summary")
            .expect("seal fixture");
        assert!(store.open(&second, &seal).is_err());
    }

    #[test]
    fn a_different_key_cannot_open_a_checkpoint() {
        let (_first_dir, first) = fixture();
        let (_second_dir, second) = fixture();
        let seal = first
            .seal(&context(), "Synthetic summary")
            .expect("seal fixture");
        assert!(second.open(&context(), &seal).is_err());
    }

    #[test]
    fn sealing_identical_summaries_uses_distinct_random_nonces() {
        let (_dir, store) = fixture();
        let first = store
            .seal(&context(), "Synthetic summary")
            .expect("first seal");
        let second = store
            .seal(&context(), "Synthetic summary")
            .expect("second seal");
        let first = URL_SAFE_NO_PAD
            .decode(&first[PREFIX.len()..])
            .expect("first payload");
        let second = URL_SAFE_NO_PAD
            .decode(&second[PREFIX.len()..])
            .expect("second payload");
        assert_ne!(&first[1..1 + NONCE_BYTES], &second[1..1 + NONCE_BYTES]);
    }

    #[test]
    fn every_payload_byte_is_authenticated_including_version_and_nonce() {
        let (_dir, store) = fixture();
        let seal = store
            .seal(&context(), "Synthetic summary")
            .expect("seal fixture");
        let payload = URL_SAFE_NO_PAD
            .decode(&seal[PREFIX.len()..])
            .expect("payload");
        for offset in 0..payload.len() {
            let mut tampered = payload.clone();
            tampered[offset] ^= 1;
            assert!(store.open(&context(), &encode(&tampered)).is_err());
        }
        for size in 0..payload.len() {
            assert!(store.open(&context(), &encode(&payload[..size])).is_err());
        }
        assert!(store
            .open(&context(), &seal.replacen(PREFIX, "varyai2:", 1))
            .is_err());
    }

    #[test]
    fn malformed_and_oversized_seals_are_rejected() {
        let (_dir, store) = fixture();
        for seal in [
            "",
            "varyai1:",
            "varyai1:%%%",
            "varyai1:a",
            "VARYAI1:AAAA",
            "varyai1:AAAA=",
        ] {
            assert!(store.open(&context(), seal).is_err());
        }
        let oversized = format!("{PREFIX}{}", "A".repeat(MAX_SEAL_BYTES));
        assert!(store.open(&context(), &oversized).is_err());
        let too_large_payload = vec![0; 1 + NONCE_BYTES + MAX_SUMMARY_BYTES + TAG_BYTES + 1];
        assert!(store.open(&context(), &encode(&too_large_payload)).is_err());
        let valid = store
            .seal(&context(), "Synthetic summary")
            .expect("seal fixture");
        assert!(store.open(&context(), &(valid + "=")).is_err());
    }

    #[test]
    fn summary_limits_are_in_bytes_and_preserve_nonempty_text() {
        let (_dir, store) = fixture();
        for summary in ["", " ", "\t\r\n", "\u{2003}\u{3000}"] {
            assert!(store.seal(&context(), summary).is_err());
        }
        let maximum = "s".repeat(MAX_SUMMARY_BYTES);
        let seal = store
            .seal(&context(), &maximum)
            .expect("maximum synthetic summary");
        assert!(seal.len() <= MAX_SEAL_BYTES);
        assert_eq!(store.open(&context(), &seal), Ok(maximum));
        assert!(store
            .seal(&context(), &"s".repeat(MAX_SUMMARY_BYTES + 1))
            .is_err());
        let unicode = "\u{1d11e}".repeat(MAX_SUMMARY_BYTES / 4);
        let seal = store
            .seal(&context(), &unicode)
            .expect("maximum Unicode summary");
        assert_eq!(store.open(&context(), &seal), Ok(unicode.clone()));
        assert!(store.seal(&context(), &(unicode + "x")).is_err());
    }

    #[test]
    fn authenticated_but_invalid_plaintext_is_rejected_on_open() {
        let (_dir, store) = fixture();
        for summary in [b"".as_slice(), b" \t\n".as_slice(), &[0xff, 0xfe]] {
            let seal = unchecked_seal(&store, &context(), summary);
            assert!(store.open(&context(), &seal).is_err());
        }
        let seal = unchecked_seal(&store, &context(), &vec![b's'; MAX_SUMMARY_BYTES + 1]);
        assert!(store.open(&context(), &seal).is_err());
    }

    #[test]
    fn context_is_owned_and_serializable() {
        let ctx = context();
        let bytes = serde_json::to_vec(&ctx).expect("serialize synthetic context");
        let decoded: Context =
            serde_json::from_slice(&bytes).expect("deserialize synthetic context");
        assert_eq!(decoded.identity, ctx.identity);
        assert_eq!(decoded.alias, ctx.alias);
        assert_eq!(decoded.destination, ctx.destination);
    }

    #[test]
    fn existing_valid_key_is_never_replaced() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        let original = [0xa7; KEY_BYTES];
        write_private_key(&path, &original);
        let first = Store::load_or_create(&path).expect("load existing synthetic key");
        let seal = first
            .seal(&context(), "Synthetic summary")
            .expect("seal fixture");
        let second = Store::load_or_create(&path).expect("reload existing synthetic key");
        assert_eq!(
            second.open(&context(), &seal),
            Ok("Synthetic summary".into())
        );
        assert!(
            fs::read(&path).expect("read fixture") == original,
            "existing key changed"
        );
    }

    #[test]
    fn malformed_existing_keys_are_never_replaced() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        for length in [0, 1, KEY_BYTES - 1, KEY_BYTES + 1, 1024 * 1024] {
            let original = vec![0xa7; length];
            write_private_key(&path, &original);
            assert!(Store::load_or_create(&path).is_err());
            assert!(
                fs::read(&path).expect("read malformed fixture") == original,
                "malformed key changed"
            );
        }
        assert_eq!(
            fs::read_dir(dir.path())
                .expect("temporary directory")
                .count(),
            1
        );
    }

    #[test]
    fn oversized_sparse_key_is_rejected_without_replacement() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        write_private_key(&path, b"synthetic");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("synthetic file")
            .set_len(1024 * 1024 * 1024)
            .expect("sparse synthetic fixture");
        assert!(Store::load_or_create(&path).is_err());
        assert_eq!(
            fs::metadata(&path).expect("synthetic file metadata").len(),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn directories_are_not_key_files() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        fs::create_dir(&path).expect("synthetic directory key");
        assert!(Store::load_or_create(&path).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn missing_parent_directories_are_created() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("nested/private/checkpoint.key");
        let store = Store::load_or_create(&path).expect("create nested synthetic key");
        let seal = store
            .seal(&context(), "Synthetic summary")
            .expect("seal fixture");
        assert_eq!(
            store.open(&context(), &seal),
            Ok("Synthetic summary".into())
        );
        assert_eq!(
            fs::metadata(path).expect("synthetic key metadata").len(),
            KEY_BYTES as u64
        );
    }

    #[test]
    fn concurrent_creators_all_use_the_one_persisted_key() {
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = Arc::new(dir.path().join("checkpoint.key"));
        let barrier = Arc::new(std::sync::Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    Store::load_or_create(&path)
                        .expect("concurrent synthetic creator")
                        .seal(&context(), "Synthetic concurrent summary")
                        .expect("concurrent synthetic seal")
                })
            })
            .collect();
        let seals: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("synthetic creator thread"))
            .collect();
        let stored = Store::load_or_create(&path).expect("load winning synthetic key");
        for seal in seals {
            assert_eq!(
                stored.open(&context(), &seal),
                Ok("Synthetic concurrent summary".into())
            );
        }
        assert_eq!(
            fs::read_dir(dir.path())
                .expect("temporary directory")
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_dangling_symlinks_are_refused_without_touching_targets() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("synthetic directory");
        let target = dir.path().join("target.key");
        let path = dir.path().join("checkpoint.key");
        let original = [0xa7; KEY_BYTES];
        write_private_key(&target, &original);
        symlink(&target, &path).expect("synthetic key symlink");
        assert!(Store::load_or_create(&path).is_err());
        assert!(fs::symlink_metadata(&path)
            .expect("synthetic symlink")
            .file_type()
            .is_symlink());
        assert!(
            fs::read(&target).expect("synthetic target") == original,
            "symlink target changed"
        );
        fs::remove_file(&target).expect("make synthetic dangling symlink");
        assert!(Store::load_or_create(&path).is_err());
        assert!(!target.exists());
        assert!(fs::symlink_metadata(&path)
            .expect("synthetic dangling symlink")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_refused_without_opening_or_blocking() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        let encoded = CString::new(path.as_os_str().as_bytes()).expect("synthetic FIFO path");
        // SAFETY: the temporary fixture path is a valid NUL-terminated string.
        assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
        assert!(Store::load_or_create(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn new_key_and_parent_directories_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("synthetic directory");
        let parent = dir.path().join("nested/private");
        let path = parent.join("checkpoint.key");
        let _store = Store::load_or_create(&path).expect("private synthetic key");
        assert_eq!(
            fs::metadata(path)
                .expect("key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(parent)
                .expect("parent metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.path().join("nested"))
                .expect("ancestor metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn insecure_existing_permissions_are_refused_not_repaired_or_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("synthetic directory");
        let path = dir.path().join("checkpoint.key");
        let original = [0xa7; KEY_BYTES];
        write_private_key(&path, &original);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("synthetic insecure mode");
        assert!(Store::load_or_create(&path).is_err());
        assert!(
            fs::read(&path).expect("synthetic key") == original,
            "existing key changed"
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }
}
