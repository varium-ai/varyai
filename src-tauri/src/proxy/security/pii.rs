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
    let chars: Vec<char> = id.chars().collect();
    if chars.len() != 18 {
        return false;
    }

    let mut sum: u32 = 0;
    for i in 0..17 {
        match chars[i].to_digit(10) {
            Some(d) => sum += d * ID_CARD_WEIGHTS[i],
            None => return false,
        }
    }

    let remainder = (sum % 11) as usize;
    let expected_check_char = ID_CARD_CHECK_CODES[remainder];

    let last_char = chars[17].to_ascii_uppercase();
    last_char == expected_check_char
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
