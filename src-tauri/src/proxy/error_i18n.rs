//! Multi-language user-facing messages for local proxy/gateway blocking errors,
//! backed by embedded locale JSON files (`src-tauri/src/proxy/locales/*.json`).
//!
//! Machine-readable error `code`s and protocol-level strings (such as
//! `context_length_exceeded` and `request_too_large`) remain in standard English
//! so coding agents can still parse and auto-recover.

use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    En,
    Zh,
    ZhTw,
    Ja,
}

impl Lang {
    pub(crate) fn from_code(raw: &str) -> Self {
        let lower = raw.trim().to_ascii_lowercase();
        if lower.starts_with("zh-tw")
            || lower.starts_with("zh-hk")
            || lower.starts_with("zh-mo")
            || lower.contains("hant")
        {
            Self::ZhTw
        } else if lower.starts_with("zh") {
            Self::Zh
        } else if lower.starts_with("ja") {
            Self::Ja
        } else {
            Self::En
        }
    }
}

static EN: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| parse_locale(include_str!("locales/en.json")));
static ZH: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| parse_locale(include_str!("locales/zh.json")));
static ZH_TW: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| parse_locale(include_str!("locales/zh-TW.json")));
static JA: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| parse_locale(include_str!("locales/ja.json")));

fn parse_locale(raw: &str) -> HashMap<String, String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn table(lang: Lang) -> &'static HashMap<String, String> {
    match lang {
        Lang::En => &EN,
        Lang::Zh => &ZH,
        Lang::ZhTw => &ZH_TW,
        Lang::Ja => &JA,
    }
}

pub(crate) fn current_lang() -> Lang {
    let configured = crate::settings::get_settings().language;
    #[cfg(test)]
    let raw = configured.unwrap_or_else(|| "en".to_string());
    #[cfg(not(test))]
    let raw = configured
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en".to_string());
    Lang::from_code(&raw)
}

pub(crate) fn lookup_for(lang: Lang, key: &str) -> Option<&'static str> {
    table(lang)
        .get(key)
        .or_else(|| EN.get(key))
        .map(String::as_str)
}

pub(crate) fn routing_message(code: &str) -> &'static str {
    routing_message_for(current_lang(), code)
}

pub(crate) fn routing_message_for(lang: Lang, code: &str) -> &'static str {
    let canonical = match code {
        "routing_invalid_plan" | "routing_invalid_target" => "routing_invalid_input",
        "routing_catalog_unavailable" => "routing_catalog_stale",
        "routing_no_image_candidate" => "routing_target_does_not_support_images",
        "routing_ambiguous_continuation" => "routing_unknown_continuation",
        "routing_checkpoint_invalid"
        | "routing_checkpoint_ambiguous"
        | "routing_checkpoint_unavailable" => "routing_checkpoint_conflict",
        "routing_account_changed_or_expired" => "routing_account_unavailable",
        "routing_upstream_encoding_unsupported" => "routing_upstream_response_invalid",
        "routing_unsafe_payload_edit" => "routing_inspection_failed",
        "routing_token_estimate_unavailable" | "routing_compaction_unsupported" => {
            "routing_token_count_unsupported"
        }
        other => other,
    };
    lookup_for(lang, canonical)
        .or_else(|| lookup_for(lang, "routing_default"))
        .unwrap_or("Routing request failed.")
}

pub(crate) fn pi_message(code: &'static str) -> &'static str {
    pi_message_for(current_lang(), code)
}

pub(crate) fn pi_message_for(lang: Lang, code: &'static str) -> &'static str {
    if matches!(code, "context_length_exceeded" | "request_too_large") {
        return code;
    }
    let canonical = match code {
        "pi_unsafe_payload_edit" => "pi_inspection_failed",
        other => other,
    };
    lookup_for(lang, canonical).unwrap_or(code)
}

pub(crate) fn gateway_message(code: &str) -> &'static str {
    gateway_message_for(current_lang(), code)
}

pub(crate) fn gateway_message_for(lang: Lang, code: &str) -> &'static str {
    lookup_for(lang, code)
        .or_else(|| lookup_for(lang, "gateway_default"))
        .unwrap_or("Gateway error.")
}

pub(crate) fn dlp_routed_message(code: &str) -> &'static str {
    lookup_for(current_lang(), code).unwrap_or("Local security processing error.")
}

pub(crate) fn dlp_blocked_message(reason: &str) -> String {
    dlp_blocked_message_for(current_lang(), reason)
}

pub(crate) fn dlp_blocked_message_for(lang: Lang, reason: &str) -> String {
    if reason == "dlp_unsupported_route" {
        return lookup_for(lang, "dlp_unsupported_route")
            .unwrap_or("dlp_unsupported_route")
            .to_string();
    }
    lookup_for(lang, "dlp_blocked_template")
        .unwrap_or("{reason}")
        .replace("{reason}", reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn all_locale_json_files_have_identical_keys() {
        let en_keys: BTreeSet<&str> = EN.keys().map(String::as_str).collect();
        assert!(!en_keys.is_empty());
        for (name, map) in [("zh", &*ZH), ("zh-TW", &*ZH_TW), ("ja", &*JA)] {
            let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
            assert_eq!(keys, en_keys, "locale {name} keys mismatch en.json");
        }
    }

    #[test]
    fn resolves_all_four_languages() {
        for (input, expected) in [
            ("en", Lang::En),
            ("en-US", Lang::En),
            ("zh", Lang::Zh),
            ("zh-CN", Lang::Zh),
            ("zh-TW", Lang::ZhTw),
            ("zh-HK", Lang::ZhTw),
            ("zh-Hant", Lang::ZhTw),
            ("ja", Lang::Ja),
            ("ja-JP", Lang::Ja),
        ] {
            assert_eq!(Lang::from_code(input), expected, "{input}");
        }
    }

    #[test]
    fn routing_pi_gateway_and_dlp_messages_translate_for_all_languages() {
        for lang in [Lang::En, Lang::Zh, Lang::ZhTw, Lang::Ja] {
            assert_ne!(
                routing_message_for(lang, "routing_upstream_connection_failed"),
                "routing_upstream_connection_failed"
            );
            assert_ne!(
                pi_message_for(lang, "pi_model_unavailable"),
                "pi_model_unavailable"
            );
            assert!(!gateway_message_for(lang, "invalid_gateway_key").is_empty());
            assert!(dlp_blocked_message_for(lang, "secret").contains("secret"));
        }
        assert_eq!(
            pi_message_for(Lang::Zh, "context_length_exceeded"),
            "context_length_exceeded"
        );
    }
}
