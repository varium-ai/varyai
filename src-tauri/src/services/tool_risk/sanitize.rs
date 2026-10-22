use std::{ops::Range, sync::LazyLock};

use regex::Regex;

use crate::proxy::security::{
    builtin_rules::BUILTIN_RULES,
    pii::{scan_chinese_id_cards, scan_chinese_phone_numbers},
    types::RuleCategory,
};

pub const MAX_SUMMARY_CHARS: usize = 4096;
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;
const REDACTED: &str = "<redacted>";

struct SummaryPatterns {
    assignment: Regex,
    option: Regex,
    authorization: Regex,
    url_credentials: Regex,
    query: Regex,
    email: Regex,
}

// A compilation failure must never cause a raw-command fallback or a panic.
static PATTERNS: LazyLock<Option<SummaryPatterns>> = LazyLock::new(|| {
    let compile = || -> Result<SummaryPatterns, regex::Error> {
        Ok(SummaryPatterns {
            assignment: Regex::new(r"(?i)(?:^|[^a-z0-9_/?&.-])([a-z_][a-z0-9_]*)[ \t]*=[ \t]*")?,
            option: Regex::new(r"(?:^|[\s;|&])(--[a-zA-Z][a-zA-Z-]*|-u)(?:=|[ \t]+)")?,
            authorization: Regex::new(
                r"(?i)\bauthorization[ \t]*:[ \t]*(?:(?:bearer|basic)[ \t]+)?",
            )?,
            url_credentials: Regex::new(r#"(?i)\b[a-z][a-z0-9+.-]*://([^/\s'\"@]+)@"#)?,
            query: Regex::new(r"[?&]([a-zA-Z][a-zA-Z0-9_-]*)=")?,
            email: Regex::new(
                r"(?i)[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+",
            )?,
        })
    };
    match compile() {
        Ok(patterns) => Some(patterns),
        Err(error) => {
            log::error!("[ToolRisk] 摘要脱敏规则编译失败: {error}");
            None
        }
    }
});

pub fn summarize_command(command: &str) -> String {
    if command.len() > MAX_SOURCE_BYTES {
        return REDACTED.into();
    }
    if command.trim().is_empty() {
        return String::new();
    }
    let Some(patterns) = PATTERNS.as_ref() else {
        return REDACTED.into();
    };
    let mut summary = truncate_summary(redact(command, patterns));
    // A cut can create a new token boundary. Settle that bounded output before
    // persistence; an unexpected non-convergence fails closed without echoing.
    for _ in 0..4 {
        let next = truncate_summary(redact(&summary, patterns));
        if next == summary {
            return summary;
        }
        summary = next;
    }
    REDACTED.into()
}

fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace('-', "_");
    [
        "api_key",
        "auth_token",
        "token",
        "secret",
        "password",
        "passwd",
        "access_key",
        "access_token",
        "pwd",
        "passphrase",
        "passcode",
    ]
    .iter()
    .any(|suffix| {
        name == *suffix
            || name
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.ends_with('_'))
    })
}

fn redact(text: &str, patterns: &SummaryPatterns) -> String {
    let mut ranges = Vec::new();
    let (matching, offsets) = matching_text(text);
    for rule in BUILTIN_RULES.iter() {
        // Database/password rules can match an entire URL or arbitrary text;
        // their value contexts are handled below. IPs/paths are not credentials.
        if !matches!(
            rule.category,
            RuleCategory::ApiToken | RuleCategory::PrivateKey
        ) {
            continue;
        }
        let Some(regex) = rule.regex.as_ref() else {
            // A missing built-in detector cannot justify preserving the input.
            return REDACTED.into();
        };
        for capture in regex.captures_iter(&matching) {
            let Some(found) = capture.get(rule.capture_group.unwrap_or(0)) else {
                continue;
            };
            let end = if rule.category == RuleCategory::PrivateKey {
                let closing = found.as_str().replacen("BEGIN", "END", 1);
                matching[found.end()..]
                    .find(&closing)
                    .map_or(matching.len(), |position| {
                        found.end() + position + closing.len()
                    })
            } else {
                found.end()
            };
            if let (Some(&start), Some(&end)) = (offsets.get(found.start()), offsets.get(end)) {
                add_range(text, start..end, &mut ranges);
            }
        }
    }
    for (start, end, _) in scan_chinese_id_cards(text)
        .into_iter()
        .chain(scan_chinese_phone_numbers(text))
    {
        add_range(text, start..end, &mut ranges);
    }
    let url_credentials: Vec<Range<usize>> = patterns
        .url_credentials
        .captures_iter(text)
        .filter_map(|capture| capture.get(1).map(|found| found.range()))
        .collect();
    for found in patterns.email.find_iter(text) {
        // A password ending in @host resembles an email; never swallow the host.
        if !url_credentials
            .iter()
            .any(|credentials| found.start() < credentials.end && found.end() > credentials.start)
        {
            add_range(text, found.range(), &mut ranges);
        }
    }
    for capture in patterns.assignment.captures_iter(text) {
        if let (Some(name), Some(found)) = (capture.get(1), capture.get(0)) {
            if sensitive_name(name.as_str()) {
                value_ranges(text, found.end(), &mut ranges);
            }
        }
    }
    for capture in patterns.option.captures_iter(text) {
        if let (Some(name), Some(found)) = (capture.get(1), capture.get(0)) {
            let option = name.as_str();
            if sensitive_name(option.trim_start_matches('-'))
                || matches!(option, "-u" | "--user" | "--proxy-user")
            {
                value_ranges(text, found.end(), &mut ranges);
            }
        }
    }
    for found in patterns.authorization.find_iter(text) {
        // A quoted header value may contain spaces, unlike a shell word.
        let quote = enclosing_quote(text, found.start());
        let start = found.end();
        let end = if let Some(quote) = quote {
            quoted_end(text, start, quote).unwrap_or(text.len())
        } else {
            word_end(text, start)
        };
        add_range(text, start..end, &mut ranges);
    }
    for credentials in url_credentials {
        // Both username and password are authentication values; preserve @.
        add_range(text, credentials, &mut ranges);
    }
    for capture in patterns.query.captures_iter(text) {
        if let (Some(name), Some(found)) = (capture.get(1), capture.get(0)) {
            if sensitive_name(name.as_str()) {
                let end = query_end(text, found.end());
                add_range(text, found.end()..end, &mut ranges);
            }
        }
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut() {
            if range.start <= previous.end {
                previous.end = previous.end.max(range.end);
                continue;
            }
        }
        merged.push(range);
    }
    let mut output = String::with_capacity(text.len());
    let mut position = 0;
    for range in merged {
        output.push_str(&text[position..range.start]);
        output.push_str(REDACTED);
        position = range.end;
    }
    output.push_str(&text[position..]);
    output
}

fn add_range(text: &str, range: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    let Some(value) = text.get(range.clone()) else {
        return;
    };
    if value.is_empty() || value == "…" || value == REDACTED || value == "<redacted>…" {
        return;
    }
    // Never re-mask placeholders when another contextual detector overlaps.
    let mut start = range.start;
    for (offset, _) in value.match_indices(REDACTED) {
        let end = range.start + offset;
        if start < end {
            ranges.push(start..end);
        }
        start = end + REDACTED.len();
    }
    if start < range.end {
        ranges.push(start..range.end);
    }
}

fn escaped_advance(text: &str, position: usize) -> usize {
    let next = position + 1;
    next + text[next..].chars().next().map_or(0, char::len_utf8)
}

fn quoted_end(text: &str, start: usize, quote: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut position = start;
    while position < bytes.len() {
        if bytes[position] == quote {
            return Some(position);
        }
        if bytes[position] == b'\\' && quote == b'"' {
            position = escaped_advance(text, position);
        } else {
            position += text[position..].chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

fn delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b';' | b'|' | b'&' | b'<' | b'>')
}

fn word_end(text: &str, start: usize) -> usize {
    let mut position = start;
    let bytes = text.as_bytes();
    while position < bytes.len() {
        if text[position..].starts_with(REDACTED) {
            position += REDACTED.len();
        } else if delimiter(bytes[position]) || matches!(bytes[position], b'\'' | b'"') {
            break;
        } else if bytes[position] == b'\\' {
            position = escaped_advance(text, position);
        } else {
            position += text[position..].chars().next().map_or(1, char::len_utf8);
        }
    }
    position
}

fn value_ranges(text: &str, start: usize, ranges: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let mut position = start;
    while position < bytes.len() {
        if matches!(bytes[position], b'\'' | b'"') {
            let content_start = position + 1;
            let Some(end) = quoted_end(text, content_start, bytes[position]) else {
                add_range(text, content_start..text.len(), ranges);
                return;
            };
            add_range(text, content_start..end, ranges);
            position = end + 1;
        } else {
            let end = word_end(text, position);
            if end == position {
                break;
            }
            add_range(text, position..end, ranges);
            position = end;
        }
        if position >= bytes.len() || delimiter(bytes[position]) {
            break;
        }
    }
}

fn enclosing_quote(text: &str, end: usize) -> Option<u8> {
    let bytes = text.as_bytes();
    let mut quote = None;
    let mut position = 0;
    while position < end {
        let byte = bytes[position];
        if byte == b'\\' && quote != Some(b'\'') {
            position = escaped_advance(text, position);
        } else {
            if matches!(byte, b'\'' | b'"') {
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(byte);
                }
            }
            position += text[position..].chars().next().map_or(1, char::len_utf8);
        }
    }
    quote
}

fn query_end(text: &str, start: usize) -> usize {
    let mut position = start;
    while position < text.len() {
        if text[position..].starts_with(REDACTED) {
            position += REDACTED.len();
            continue;
        }
        let byte = text.as_bytes()[position];
        if delimiter(byte) || matches!(byte, b'#' | b'\'' | b'"') {
            break;
        }
        position += text[position..].chars().next().map_or(1, char::len_utf8);
    }
    position
}

// Only matcher text collapses escaped underscores. Offsets always address the
// original UTF-8 bytes, so public text/paths never get normalized in the summary.
fn matching_text(text: &str) -> (String, Vec<usize>) {
    let mut matching = String::with_capacity(text.len());
    let mut offsets = vec![0];
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '\\' && matches!(chars.peek(), Some((_, '_'))) {
            chars.next();
            matching.push('_');
            offsets.push(start + 2);
        } else {
            matching.push(ch);
            for byte in 1..=ch.len_utf8() {
                offsets.push(start + byte);
            }
        }
    }
    (matching, offsets)
}

fn truncate_summary(mut summary: String) -> String {
    if let Some((mut cut, _)) = summary.char_indices().nth(MAX_SUMMARY_CHARS - 1) {
        // Reserve one character for the ellipsis, but don't split a placeholder.
        if summary.chars().count() <= MAX_SUMMARY_CHARS {
            return summary;
        }
        for (start, _) in summary.match_indices(REDACTED) {
            if start < cut && cut < start + REDACTED.len() {
                cut = start;
                break;
            }
        }
        summary.truncate(cut);
        summary.push('…');
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_summary(command: &str, expected: &str) {
        let summary = summarize_command(command);
        assert_eq!(summary, expected);
        assert_eq!(
            summarize_command(&summary),
            summary,
            "summary must be idempotent"
        );
    }

    #[test]
    fn ordinary_commands_are_preserved_byte_for_byte() {
        for command in [
            "ls -la /workspace/project",
            "curl https://example.com/path?name=public",
            "unknown-program --public-option=value arbitrary-public",
            "  echo 'hello' | cat -n; printf \"world\\n\"\n",
            "echo arbitrary-secret",
            "echo 'quoted secret token'",
            "cat secrets/credentials.json",
            "echo $(cat /private/credentials)",
            "unrecognized-program --private-flag=hidden-value bare-secret",
            "ls -la; echo hidden-secret",
            "echo a\\_b /workspace/a\\ b https://example.com/path#fragment",
            "curl http://192.168.1.10/public --data 'hello world'",
        ] {
            assert_summary(command, command);
        }
    }

    #[test]
    fn empty_command_yields_empty_without_trimming_nonempty_commands() {
        for command in ["", " \t\r\n", "\u{2003}"] {
            assert_summary(command, "");
        }
        assert_summary("  ls  \n", "  ls  \n");
    }

    #[test]
    fn sensitive_env_assignments_hide_only_values() {
        for name in [
            "API_KEY",
            "AUTH_TOKEN",
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "PASSWD",
            "ACCESS_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "OPENAI_API_KEY",
            "MY_AUTH_TOKEN",
        ] {
            let command = format!(
                "export {name}={} curl https://example.com; echo hello",
                "abc123"
            );
            assert!(
                command.contains("abc123"),
                "fixture must include a real value"
            );
            assert_summary(
                &command,
                &format!("export {name}=<redacted> curl https://example.com; echo hello"),
            );
        }
        let command = format!("env {}='{}' OTHER=public tool", "PASSWORD", "a b;c|d");
        assert_summary(
            &command,
            &format!("env {}='<redacted>' OTHER=public tool", "PASSWORD"),
        );
    }

    #[test]
    fn quotes_escapes_and_command_separators_survive_redaction() {
        let command = format!("{}=\"{}\" curl a\\ b;\nls | cat", "API_KEY", "a\\\"b c");
        assert_summary(
            &command,
            &format!("{}=\"<redacted>\" curl a\\ b;\nls | cat", "API_KEY"),
        );
        let command = format!("{}={} curl", "SECRET", "a\\ b");
        assert_summary(&command, &format!("{}=<redacted> curl", "SECRET"));
        let command = format!("{}={}{}", "API_KEY", "'abc'", "\"def\"");
        assert_summary(
            &command,
            &format!("{}='<redacted>'\"<redacted>\"", "API_KEY"),
        );
    }

    #[test]
    fn malformed_sensitive_quotes_hide_the_sensitive_tail() {
        for quote in ["'", "\""] {
            let command = format!(
                "echo hello; {}={quote}{}\nnext-command",
                "API_KEY", "not-closed"
            );
            assert_summary(
                &command,
                &format!("echo hello; {}={quote}<redacted>", "API_KEY"),
            );
        }
    }

    #[test]
    fn sensitive_options_support_separate_equal_and_quoted_values() {
        for option in [
            "--password",
            "--token",
            "--api-key",
            "--secret",
            "--access-key",
            "--auth-token",
        ] {
            let command = format!("unknown-program {option} {} --public=yes", "abc123");
            assert_summary(
                &command,
                &format!("unknown-program {option} <redacted> --public=yes"),
            );
            let command = format!("unknown-program {option}=\"{}\" | cat", "abc 123");
            assert_summary(
                &command,
                &format!("unknown-program {option}=\"<redacted>\" | cat"),
            );
        }
    }

    #[test]
    fn authorization_preserves_header_scheme_and_quotes() {
        for scheme in ["Bearer", "Basic"] {
            let command = format!(
                "curl -H \"{}: {scheme} {}\" https://example.com",
                "Authorization", "abc123"
            );
            assert!(command.contains("abc123"));
            assert_summary(
                &command,
                &format!("curl -H \"Authorization: {scheme} <redacted>\" https://example.com"),
            );
        }
        let command = format!("curl -H '{}: {}'", "Authorization", "custom-token");
        assert_summary(&command, "curl -H 'Authorization: <redacted>'");
        let command = format!(
            "curl -H '{}: Bearer {}' https://example.com",
            "Authorization", "opaque with spaces"
        );
        assert_summary(
            &command,
            "curl -H 'Authorization: Bearer <redacted>' https://example.com",
        );
    }

    #[test]
    fn curl_auth_options_hide_credentials_without_hiding_options() {
        for option in ["-u", "--user", "--proxy-user"] {
            let command = format!("curl {option} '{}' https://example.com", "user:pass");
            assert_summary(
                &command,
                &format!("curl {option} '<redacted>' https://example.com"),
            );
        }
        let command = format!("curl --user={} https://example.com", "user:pass");
        assert_summary(&command, "curl --user=<redacted> https://example.com");
    }

    #[test]
    fn urls_keep_hosts_paths_and_public_queries() {
        for scheme in ["https", "postgres", "mongodb"] {
            let command = format!(
                "curl '{scheme}://{}@example.com/path?name=public'",
                "alice:p%40ss"
            );
            assert_summary(
                &command,
                &format!("curl '{scheme}://<redacted>@example.com/path?name=public'"),
            );
        }
        for key in [
            "token",
            "api_key",
            "api-key",
            "password",
            "access_token",
            "secret",
            "access_key",
        ] {
            let command = format!(
                "curl 'https://example.com/path?name=public&{key}={}&page=2'",
                "abc%20123"
            );
            assert_summary(
                &command,
                &format!("curl 'https://example.com/path?name=public&{key}=<redacted>&page=2'"),
            );
        }
    }

    #[test]
    fn provider_tokens_are_redacted_even_for_default_allow_catalog_rules() {
        for token in [
            format!("{}{}", "ghp_", "a".repeat(36)),
            format!("{}{}", "glpat-", "b".repeat(20)),
            format!("{}{}", "gsk_", "c".repeat(52)),
            format!("{}{}", "sk-ant-", "d".repeat(32)),
            format!("{}{}", "AKIA", "E".repeat(16)),
        ] {
            assert_summary(&format!("echo '{token}' | cat"), "echo '<redacted>' | cat");
        }
    }

    #[test]
    fn escaped_token_detection_does_not_normalize_public_text() {
        let escaped_token = ["ghp", "\\", "_", &"a".repeat(36)].concat();
        let command = format!("echo a\\_b '{escaped_token}' /workspace/a\\ b");
        assert_summary(&command, "echo a\\_b '<redacted>' /workspace/a\\ b");
    }

    #[test]
    fn pii_values_are_redacted_and_invalid_id_is_not() {
        let email = ["alice", "@", "example.com"].concat();
        let phone = ["138", "00138000"].concat();
        let valid_id = ["110105", "19491231002X"].concat();
        let invalid_id = ["110105", "194912310020"].concat();
        let command = format!("echo 邮箱{email} 手机{phone} 身份证{valid_id} invalid:{invalid_id}");
        assert_summary(
            &command,
            &format!("echo 邮箱<redacted> 手机<redacted> 身份证<redacted> invalid:{invalid_id}"),
        );
    }

    #[test]
    fn private_key_redaction_covers_full_block_not_just_old_marker_regex() {
        for kind in ["", "RSA ", "EC ", "OPENSSH "] {
            let begin = format!("-----{} {kind}PRIVATE KEY-----", "BEGIN");
            let end = format!("-----{} {kind}PRIVATE KEY-----", "END");
            let command = format!("echo '{begin}\nMII-secret-material\n{end}'; ls");
            assert_summary(&command, "echo '<redacted>'; ls");
        }
    }

    #[test]
    fn incomplete_private_key_conservatively_hides_remaining_input() {
        let begin = ["-----BEGIN ", "RSA PRIVATE KEY-----"].concat();
        let command = format!("echo hello; echo '{begin}\nprivate-material; next command");
        assert_summary(&command, "echo hello; echo '<redacted>");
    }

    #[test]
    fn placeholders_remain_intact_on_repeated_dao_reads() {
        let env = format!(
            "{}=<redacted> curl -H '{}: Bearer <redacted>'",
            "API_KEY", "Authorization"
        );
        let url = format!(
            "curl https://<redacted>@example.com/?{}=<redacted>&name=public",
            "token"
        );
        for command in ["echo <redacted>", &env, &url, "echo [redacted] <command>"] {
            assert_summary(command, command);
        }
    }

    #[test]
    fn summary_is_unicode_safe_bounded_and_idempotent() {
        let command = format!("echo {}", "界🙂".repeat(4096));
        let summary = summarize_command(&command);
        assert_eq!(summary.chars().count(), 4096);
        assert!(summary.starts_with("echo 界🙂"));
        assert!(summary.ends_with('…'));
        assert_eq!(summarize_command(&summary), summary);
    }

    #[test]
    fn truncation_does_not_split_placeholders_or_break_sensitive_context_idempotence() {
        for offset in 4060..4095 {
            let command = format!(
                "echo {}; https://example.com/?{}={}&name={}",
                "x".repeat(offset),
                "token",
                "abc123",
                "y".repeat(200)
            );
            let summary = summarize_command(&command);
            assert!(summary.chars().count() <= 4096);
            assert!(!summary.contains("abc123"));
            assert_eq!(
                summarize_command(&summary),
                summary,
                "cut position {offset}"
            );
        }
        let credential = ["gl", "pat", "-", &"b".repeat(1000)].concat();
        let command = format!("echo {} {credential}", "x".repeat(4063));
        let summary = summarize_command(&command);
        assert_eq!(
            summarize_command(&summary),
            summary,
            "truncation must not expose a newly matchable token"
        );
    }

    #[test]
    fn redaction_happens_before_truncation_so_no_partial_secret_is_shown() {
        let prefix = format!("echo {}; ", "x".repeat(4057));
        let key = "API_KEY";
        let command = format!("{prefix}{key}={}", "s".repeat(500));
        let summary = summarize_command(&command);
        assert_eq!(summary, format!("{prefix}{key}=<redacted>"));
        assert!(!summary.contains("ssss"));
        assert_eq!(summarize_command(&summary), summary);
    }

    #[test]
    fn source_input_is_bounded_before_scanning_with_no_echo() {
        let summary = summarize_command(&format!("{}private-material", "x".repeat(256 * 1024)));
        assert_eq!(summary, "<redacted>");
        assert_eq!(summarize_command(&summary), summary);
        let at_limit = "x".repeat(256 * 1024);
        assert!(summarize_command(&at_limit).starts_with("xxxx"));
    }
}
