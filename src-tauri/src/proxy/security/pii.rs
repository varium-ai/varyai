use regex::Regex;
use std::sync::LazyLock;

const ID_CARD_WEIGHTS: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];

const ID_CARD_CHECK_CODES: [char; 11] = ['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'];

static ID_CARD_CANDIDATE_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    match Regex::new(
        r"(?-u:\b)([1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx])(?-u:\b)",
    ) {
        Ok(regex) => Some(regex),
        Err(error) => {
            log::error!("[SecurityAudit] 身份证候选正则编译失败: {error}");
            None
        }
    }
});

static PHONE_NUMBER_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| match Regex::new(r"(?-u:\b)(1[3-9]\d{9})(?-u:\b)") {
        Ok(regex) => Some(regex),
        Err(error) => {
            log::error!("[SecurityAudit] 手机号正则编译失败: {error}");
            None
        }
    });

pub fn is_valid_chinese_id_card(id: &str) -> bool {
    let mut chars = id.chars().inspect(|_| {
        #[cfg(test)]
        validation_tests::observe_character();
    });
    let mut sum = 0;
    for weight in ID_CARD_WEIGHTS {
        let Some(digit) = chars.next().and_then(|character| character.to_digit(10)) else {
            return false;
        };
        sum += digit * weight;
    }
    let Some(last_char) = chars.next() else {
        return false;
    };
    chars.next().is_none()
        && last_char.to_ascii_uppercase() == ID_CARD_CHECK_CODES[(sum % 11) as usize]
}

pub fn probe_pii_candidates(text: &str) -> (bool, bool) {
    let bytes = text.as_bytes();
    let mut current_digits = 0;
    let mut max_digits = 0;

    for &b in bytes {
        if b.is_ascii_digit() || (current_digits == 17 && (b == b'X' || b == b'x')) {
            current_digits += 1;
            if current_digits > max_digits {
                max_digits = current_digits;
            }
        } else {
            current_digits = 0;
        }
    }

    (max_digits >= 11, max_digits >= 18)
}

pub fn scan_chinese_id_cards(text: &str) -> Vec<(usize, usize, String)> {
    let (_, may_have_id) = probe_pii_candidates(text);
    if !may_have_id {
        return Vec::new();
    }

    let mut results = Vec::new();
    let Some(regex) = ID_CARD_CANDIDATE_RE.as_ref() else {
        return Vec::new();
    };
    for mat in regex.find_iter(text) {
        let candidate = mat.as_str();
        if is_valid_chinese_id_card(candidate) {
            results.push((mat.start(), mat.end(), candidate.to_string()));
        }
    }
    results
}

pub fn scan_chinese_phone_numbers(text: &str) -> Vec<(usize, usize, String)> {
    let (may_have_phone, _) = probe_pii_candidates(text);
    if !may_have_phone {
        return Vec::new();
    }

    let mut results = Vec::new();
    let Some(regex) = PHONE_NUMBER_RE.as_ref() else {
        return Vec::new();
    };
    for cap in regex.captures_iter(text) {
        if let Some(m) = cap.get(1) {
            results.push((m.start(), m.end(), m.as_str().to_string()));
        }
    }
    results
}

/// Visit validated ranges without retaining copies of detected plaintext.
/// The private bounded planner can stop scanning before growing its next item.
pub(super) fn visit_id_card_ranges(
    text: &str,
    mut visitor: impl FnMut(std::ops::Range<usize>) -> Result<(), super::tokenization::TokenError>,
) -> Result<(), super::tokenization::TokenError> {
    if !probe_pii_candidates(text).1 {
        return Ok(());
    }
    let regex = ID_CARD_CANDIDATE_RE
        .as_ref()
        .ok_or(super::tokenization::TokenError::RedactionFailed)?;
    for found in regex.find_iter(text) {
        if is_valid_chinese_id_card(found.as_str()) {
            visitor(found.range())?;
        }
    }
    Ok(())
}

/// Visit phone ranges without allocating a vector or plaintext strings.
pub(super) fn visit_phone_ranges(
    text: &str,
    mut visitor: impl FnMut(std::ops::Range<usize>) -> Result<(), super::tokenization::TokenError>,
) -> Result<(), super::tokenization::TokenError> {
    if !probe_pii_candidates(text).0 {
        return Ok(());
    }
    let regex = PHONE_NUMBER_RE
        .as_ref()
        .ok_or(super::tokenization::TokenError::RedactionFailed)?;
    for captures in regex.captures_iter(text) {
        if let Some(found) = captures.get(1) {
            visitor(found.range())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod validation_tests {
    use super::is_valid_chinese_id_card;
    use std::cell::Cell;

    thread_local! { static VISITED: Cell<usize> = const { Cell::new(0) }; }

    pub(super) fn observe_character() {
        VISITED.with(|count| count.set(count.get() + 1));
    }

    #[test]
    fn id_validation_consumes_at_most_nineteen_characters_without_collecting_the_tail() {
        for prefix in ["11010519491231002X", "synthetic-invalid-"] {
            let input = format!("{prefix}{}", "尾".repeat(1024 * 1024));
            VISITED.with(|count| count.set(0));
            assert!(!is_valid_chinese_id_card(&input));
            assert!(
                VISITED.with(Cell::get) <= 19,
                "ID validation traversed the unbounded tail"
            );
        }
    }

    #[test]
    fn id_validation_fixed_space_preserves_exact_character_and_checksum_semantics() {
        fn legacy(input: &str) -> bool {
            let chars: Vec<char> = input.chars().collect();
            if chars.len() != 18 {
                return false;
            }
            let mut sum = 0;
            for (character, weight) in chars.iter().zip(super::ID_CARD_WEIGHTS) {
                let Some(digit) = character.to_digit(10) else {
                    return false;
                };
                sum += digit * weight;
            }
            chars[17].to_ascii_uppercase() == super::ID_CARD_CHECK_CODES[(sum % 11) as usize]
        }
        for input in [
            "",
            "11010519491231002X",
            "11010519491231002x",
            "110105194912310021",
            "11010519491231002",
            "11010519491231002XX",
            "１1010519491231002X",
            "11010519491231002尾",
            " 11010519491231002X",
            "11010519491231002X\0",
        ] {
            assert_eq!(
                is_valid_chinese_id_card(input),
                legacy(input),
                "ID validation semantics changed"
            );
        }
    }
}
