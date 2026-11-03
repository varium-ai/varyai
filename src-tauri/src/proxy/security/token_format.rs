//! Shared bounded grammar only. Recognition never grants restoration authority.

pub(super) const MAX_REFERENCE_BYTES: usize = 43;
pub(super) const DIGEST_HEX_BYTES: usize = 20;
pub(super) const PREFIX: &str = "rredact-";
pub(super) const LEGACY_PREFIX: &str = "vary_";
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
    fn header(self) -> &'static str {
        match self {
            Self::PrivateKey => "rredact-private_key-",
            Self::Database => "rredact-db_conn-",
            Self::ApiKey => "rredact-api_token-",
            Self::Password => "rredact-password-",
            Self::IdCard => "rredact-cn_id_card-",
            Self::Phone => "rredact-cn_phone-",
            Self::Infrastructure => "rredact-infra-",
            Self::Personal => "rredact-personal-",
            Self::Custom => "rredact-custom-",
            Self::Mixed => "rredact-mixed-",
        }
    }

    pub(super) fn code(self) -> &'static str {
        let header = self.header();
        &header[PREFIX.len()..header.len() - 1]
    }
}

const CATEGORIES: [TokenCategory; 10] = [
    TokenCategory::Phone,
    TokenCategory::IdCard,
    TokenCategory::PrivateKey,
    TokenCategory::ApiKey,
    TokenCategory::Database,
    TokenCategory::Password,
    TokenCategory::Infrastructure,
    TokenCategory::Personal,
    TokenCategory::Custom,
    TokenCategory::Mixed,
];

fn headers() -> impl Iterator<Item = (&'static [u8], bool)> {
    CATEGORIES
        .into_iter()
        .map(|category| (category.header().as_bytes(), false))
        .chain(LEGACY_HEADERS.into_iter().map(|header| (header, true)))
}

const LEGACY_HEADERS: [&[u8]; 10] = [
    b"vary_ph_",
    b"vary_id_",
    b"vary_pk_",
    b"vary_ak_",
    b"vary_db_",
    b"vary_pw_",
    b"vary_ip_",
    b"vary_pi_",
    b"vary_cu_",
    b"vary_mx_",
];

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub digest_start: usize,
    pub len: usize,
    pub legacy: bool,
}

impl Layout {
    pub(super) fn expected_suffix(self) -> &'static [u8] {
        if self.legacy {
            b"_ai"
        } else {
            b"-ai"
        }
    }

    pub(super) fn valid_byte(self, index: usize, byte: u8) -> bool {
        if index < self.digest_start + DIGEST_HEX_BYTES {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
        } else {
            self.expected_suffix()
                .get(index - self.digest_start - DIGEST_HEX_BYTES)
                == Some(&byte)
        }
    }

    pub(super) fn complete(self, bytes: &[u8]) -> bool {
        bytes.len() >= self.len
            && bytes[self.digest_start..self.len]
                .iter()
                .enumerate()
                .all(|(index, byte)| self.valid_byte(self.digest_start + index, *byte))
    }
}

pub(super) fn header(bytes: &[u8]) -> Option<Layout> {
    for (candidate, legacy) in headers() {
        if bytes.starts_with(candidate) {
            return Some(Layout {
                digest_start: candidate.len(),
                len: candidate.len() + DIGEST_HEX_BYTES + 3,
                legacy,
            });
        }
    }
    None
}

/// Return a UTF-8 boundary and the matched ASCII prefix length.
pub(super) fn next_candidate(text: &str, offset: usize) -> Option<(usize, usize)> {
    let tail = &text.as_bytes()[offset..];
    // A single forward scan avoids rescanning the remaining body for an absent
    // alternate prefix on every reference. ASCII matches are UTF-8 boundaries.
    for (relative, byte) in tail.iter().enumerate() {
        let prefix = match byte {
            b'r' => PREFIX,
            b'v' => LEGACY_PREFIX,
            _ => continue,
        };
        if tail[relative..].starts_with(prefix.as_bytes()) {
            return Some((offset + relative, prefix.len()));
        }
    }
    None
}

/// A viable incomplete prefix; invalid candidates remain a caller policy decision.
pub(super) fn is_prefix(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() >= MAX_REFERENCE_BYTES {
        return false;
    }
    if headers().any(|(candidate, _)| candidate.starts_with(bytes)) {
        return true;
    }
    header(bytes).is_some_and(|layout| {
        bytes.len() < layout.len
            && bytes[layout.digest_start..]
                .iter()
                .enumerate()
                .all(|(index, byte)| layout.valid_byte(layout.digest_start + index, *byte))
    })
}
