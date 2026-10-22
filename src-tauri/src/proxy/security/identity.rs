//! Process-local content identity. Neither secret bytes nor digest bytes leave this module.
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{fmt, sync::OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentFingerprint([u8; 32]);

impl fmt::Debug for ContentFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContentFingerprint(<redacted>)")
    }
}

static PROCESS_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();

pub fn fingerprint(text: &str) -> Option<ContentFingerprint> {
    let key = PROCESS_KEY
        .get_or_init(|| {
            let mut key = [0u8; 32];
            if getrandom::getrandom(&mut key).is_err() {
                log::warn!("[SecurityAudit] stage=identity category=entropy");
                return None;
            }
            Some(key)
        })
        .as_ref()?;
    fingerprint_with_key(text, key)
}

fn fingerprint_with_key(text: &str, key: &[u8]) -> Option<ContentFingerprint> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).ok()?;
    mac.update(text.as_bytes());
    Some(ContentFingerprint(mac.finalize().into_bytes().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_keyed_and_debug_is_private() {
        let a = fingerprint_with_key("short-secret", &[1; 32]).unwrap();
        assert_eq!(a, fingerprint_with_key("short-secret", &[1; 32]).unwrap());
        assert_ne!(a, fingerprint_with_key("short-secret", &[2; 32]).unwrap());
        assert_eq!(format!("{a:?}"), "ContentFingerprint(<redacted>)");
    }
}
