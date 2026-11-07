//! Proxy Session - 请求会话管理
//!
//! 为每个代理请求创建会话上下文，在整个请求生命周期中跟踪状态和元数据。
//!
//! ## Session ID 提取
//!
//! 支持从客户端请求中提取 Session ID，用于关联同一对话的多个请求：
//! - Claude: 从 `metadata.user_id` (格式: `user_xxx_session_yyy`) 或 `metadata.session_id` 提取

//! - Grok Build: 从 headers 中的 `x-grok-conv-id` / `x-grok-session-id` 提取

//! - 其他: 生成新的 UUID

use axum::http::HeaderMap;
use uuid::Uuid;

// ============================================================================
// Session ID 提取器
// ============================================================================

/// Session ID 来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdSource {
    /// 从 metadata.user_id 提取 (Claude)
    MetadataUserId,
    /// 从 metadata.session_id 提取
    MetadataSessionId,
    /// 从 headers 提取
    Header,
    /// 新生成
    Generated,
}

/// Session ID 提取结果
#[derive(Debug, Clone)]
pub struct SessionIdResult {
    /// 提取或生成的 Session ID
    pub session_id: String,
    /// Session ID 来源
    pub source: SessionIdSource,
    /// 是否为客户端提供的 ID（非新生成）
    pub client_provided: bool,
}

/// 从请求中提取或生成 Session ID
///
/// 轻量化实现，仅提取 session_id 用于日志记录，不做复杂的 Session 管理。
///
/// ## 提取优先级
///
/// ### Claude 请求
/// 1. `metadata.user_id` (格式: `user_xxx_session_yyy`) → 提取 `yyy` 部分
/// 2. `metadata.session_id` → 直接使用
/// 3. 生成新 UUID
///
/// ### Codex 请求
/// 1. Headers: `thread-id` → `session-id` → `session_id` → `x-session-id`
/// 2. `metadata.session_id`
/// 3. 生成新 UUID
///
/// ### Grok Build 请求
/// 1. Headers: `x-grok-conv-id` 或 `x-grok-session-id`
/// 2. `metadata.session_id`
/// 3. 生成新 UUID
///
///
/// ## 示例
///
/// ```ignore
/// let result = extract_session_id(&headers, &body, "claude");
/// println!("Session ID: {} (from {:?})", result.session_id, result.source);
/// ```
pub fn extract_session_id(
    headers: &HeaderMap,
    body: &serde_json::Value,
    client_format: &str,
) -> SessionIdResult {
    extract_client_session_id(headers, body, client_format).unwrap_or_else(generate_new_session_id)
}

/// Borrow a strict raw Codex identity for restoration only; legacy extraction is unchanged.
/// A present invalid or repeated higher-precedence source disables continuity rather
/// than changing the identity source. No allocation or diagnostic includes the value.
#[allow(dead_code)]
pub(crate) fn extract_codex_restoration_session_id<'a>(
    headers: &'a HeaderMap,
    body: &'a serde_json::Value,
) -> Option<&'a str> {
    fn validate(raw: &str) -> Option<&str> {
        (!raw.is_empty()
            && raw.len() <= 256
            && !raw.chars().any(|ch| ch.is_whitespace() || ch.is_control()))
        .then_some(raw)
    }

    for name in ["thread-id", "session-id", "session_id", "x-session-id"] {
        let mut values = headers.get_all(name).iter();
        if let Some(value) = values.next() {
            if values.next().is_some() {
                return None;
            }
            return validate(std::str::from_utf8(value.as_bytes()).ok()?);
        }
    }

    let metadata = body.get("metadata")?.as_object()?;
    if let Some(value) = metadata.get("session_id") {
        return validate(value.as_str()?);
    }
    // Preserve only the existing recognized user-id suffix as the final source.
    let user_id = metadata.get("user_id")?.as_str()?;
    let (_, suffix) = user_id.split_once("_session_")?;
    validate(suffix)
}

/// Borrow a strict Chat identity using the unchanged Codex source and validation rules.
#[allow(dead_code)]
pub(crate) fn extract_chat_restoration_session_id<'a>(
    headers: &'a HeaderMap,
    body: &'a serde_json::Value,
) -> Option<&'a str> {
    extract_codex_restoration_session_id(headers, body)
}

/// Borrow only a strict native Anthropic identity; invalid present sources stop lookup.
#[allow(dead_code)]
pub(crate) fn extract_anthropic_restoration_session_id<'a>(
    headers: &'a HeaderMap,
    body: &'a serde_json::Value,
) -> Option<&'a str> {
    for name in ["x-claude-code-session-id", "claude-code-session-id"] {
        if headers.contains_key(name) {
            return extract_native_restoration_header(headers, name);
        }
    }

    let metadata = body.get("metadata")?.as_object()?;
    if let Some(value) = metadata.get("session_id") {
        return validate_native_restoration_identity(value.as_str()?);
    }
    let user_id = metadata.get("user_id")?.as_str()?;
    let (_, suffix) = user_id.split_once("_session_")?;
    validate_native_restoration_identity(suffix)
}

/// Borrow an explicit local Gemini session header only; never infer a fallback identity.
#[allow(dead_code)]
pub(crate) fn extract_gemini_restoration_session_id(headers: &HeaderMap) -> Option<&str> {
    extract_native_restoration_header(headers, "x-session-id")
}

fn validate_native_restoration_identity(raw: &str) -> Option<&str> {
    (!raw.is_empty()
        && raw.len() <= 256
        && !raw.chars().any(|ch| ch.is_whitespace() || ch.is_control()))
    .then_some(raw)
}

fn extract_native_restoration_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    validate_native_restoration_identity(std::str::from_utf8(value.as_bytes()).ok()?)
}

/// Extract an actual client identity without generating a per-request UUID.
pub(crate) fn extract_client_session_id(
    headers: &HeaderMap,
    body: &serde_json::Value,
    client_format: &str,
) -> Option<SessionIdResult> {
    if client_format == "zcode" {
        if let Some(id) = headers
            .get("x-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            return Some(SessionIdResult {
                session_id: id.to_owned(),
                source: SessionIdSource::Header,
                client_provided: true,
            });
        }
    }
    if client_format == "qoder" {
        for name in ["x-qoder-session-id", "x-session-id", "session-id"] {
            if let Some(id) = headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(SessionIdResult {
                    session_id: format!("qoder_{id}"),
                    source: SessionIdSource::Header,
                    client_provided: true,
                });
            }
        }
        if let Some(id) = body
            .get("session_id")
            .or_else(|| body.pointer("/metadata/session_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(SessionIdResult {
                session_id: format!("qoder_{id}"),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
        return None;
    }
    if client_format == "workbuddy" {
        // WorkBuddy sets this to its current session.id. Request/message/parent
        // IDs describe different scopes and must not identify this conversation.
        if let Some(session_id) = headers
            .get("x-conversation-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            return Some(SessionIdResult {
                session_id: session_id.to_string(),
                source: SessionIdSource::Header,
                client_provided: true,
            });
        }
    }

    if client_format == "claude" {
        if let Some(result) = extract_claude_session(headers, body) {
            return Some(result);
        }
    }

    // Responses 请求特殊处理。Grok Build 使用与 Codex 相同的客户端协议，
    // 但保留独立前缀，避免统计和缓存键跨应用碰撞。
    if matches!(client_format, "codex" | "openai" | "grokbuild") {
        let prefix = if client_format == "grokbuild" {
            "grokbuild"
        } else {
            "codex"
        };
        if let Some(result) = extract_responses_session(headers, body, prefix) {
            return Some(result);
        }
    }

    // Other clients may also supply metadata; absence stays unknown.
    extract_from_metadata(body)
}

/// 提取 Claude Session ID
fn extract_claude_session(
    headers: &HeaderMap,
    body: &serde_json::Value,
) -> Option<SessionIdResult> {
    for header_name in &["x-claude-code-session-id", "claude-code-session-id"] {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(session_id) = value.to_str() {
                if !session_id.is_empty() {
                    return Some(SessionIdResult {
                        session_id: session_id.to_string(),
                        source: SessionIdSource::Header,
                        client_provided: true,
                    });
                }
            }
        }
    }

    extract_from_metadata(body)
}

/// 提取 Responses 客户端的 Session ID
fn extract_responses_session(
    headers: &HeaderMap,
    body: &serde_json::Value,
    prefix: &str,
) -> Option<SessionIdResult> {
    // 1. 从 headers 提取
    let header_names: &[&str] = if prefix == "grokbuild" {
        // Conversation ID 跨多轮请求保持稳定；session ID 作为客户端缺少
        // conversation ID 时的回退。x-grok-req-id 是逐请求 ID，不能用于聚合。
        &["x-grok-conv-id", "x-grok-session-id"]
    } else {
        &["thread-id", "session-id", "session_id", "x-session-id"]
    };
    for header_name in header_names {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(session_id) = value.to_str() {
                let session_id = session_id.trim();
                // Responses 客户端的 Session ID 通常较长（UUID 格式）
                if session_id.len() > 20 {
                    return Some(SessionIdResult {
                        session_id: format!("{prefix}_{session_id}"),
                        source: SessionIdSource::Header,
                        client_provided: true,
                    });
                }
            }
        }
    }

    // 2. 从 body.metadata.session_id 提取
    if let Some(session_id) = body
        .get("metadata")
        .and_then(|m| m.get("session_id"))
        .and_then(|v| v.as_str())
    {
        let session_id = session_id.trim();
        if session_id.len() > 10 {
            return Some(SessionIdResult {
                session_id: format!("{prefix}_{session_id}"),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
    }

    // previous_response_id 是 Responses 协议里的响应游标，不是稳定会话身份。
    // Chat/Responses 桥接时该值通常来自上游每轮返回的随机 response id；
    // 若把它当 prompt_cache_key 或 Codex session header，会导致每轮请求换缓存 key。

    None
}

/// 从 metadata 提取 Session ID (Claude)
fn extract_from_metadata(body: &serde_json::Value) -> Option<SessionIdResult> {
    let metadata = body.get("metadata")?;

    // 1. 从 metadata.user_id 提取（格式: user_xxx_session_yyy）
    if let Some(user_id) = metadata.get("user_id").and_then(|v| v.as_str()) {
        if let Some(session_id) = parse_session_from_user_id(user_id) {
            return Some(SessionIdResult {
                session_id,
                source: SessionIdSource::MetadataUserId,
                client_provided: true,
            });
        }
    }

    // 2. 直接从 metadata.session_id 提取
    if let Some(session_id) = metadata.get("session_id").and_then(|v| v.as_str()) {
        if !session_id.is_empty() {
            return Some(SessionIdResult {
                session_id: session_id.to_string(),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
    }

    None
}

/// 从 user_id 解析 session_id
///
/// 格式: `user_identifier_session_actual_session_id`
pub(super) fn parse_session_from_user_id(user_id: &str) -> Option<String> {
    // 查找 "_session_" 分隔符
    if let Some(pos) = user_id.find("_session_") {
        let session_id = &user_id[pos + 9..]; // "_session_" 长度为 9
        if !session_id.is_empty() {
            return Some(session_id.to_string());
        }
    }
    None
}

/// 生成新的 Session ID
fn generate_new_session_id() -> SessionIdResult {
    SessionIdResult {
        session_id: Uuid::new_v4().to_string(),
        source: SessionIdSource::Generated,
        client_provided: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native_restoration_header(raw: &[u8]) -> axum::http::HeaderValue {
        match axum::http::HeaderValue::from_bytes(raw) {
            Ok(value) => value,
            Err(_) => panic!("synthetic header must be representable"),
        }
    }

    fn native_restoration_assert_borrowed(actual: Option<&str>, expected: &str) {
        assert!(actual.is_some_and(|raw| {
            std::ptr::eq(raw.as_ptr(), expected.as_ptr()) && raw.len() == expected.len()
        }));
    }

    fn native_restoration_all_identities<'a>(
        headers: &'a HeaderMap,
        body: &'a serde_json::Value,
    ) -> [Option<&'a str>; 3] {
        [
            extract_chat_restoration_session_id(headers, body),
            extract_anthropic_restoration_session_id(headers, body),
            extract_gemini_restoration_session_id(headers),
        ]
    }

    #[test]
    fn native_restoration_chat_preserves_codex_source_order_and_borrowing() {
        let names = ["thread-id", "session-id", "session_id", "x-session-id"];
        let mut headers = HeaderMap::new();
        for (name, value) in names.iter().zip(["chat-a", "chat-b", "chat-c", "chat-d"]) {
            headers.insert(*name, native_restoration_header(value.as_bytes()));
        }
        let body = json!({"metadata":{"session_id":"chat-metadata", "user_id":"user_session_chat-suffix"}});
        for name in names {
            let expected = match headers.get(name).and_then(|value| value.to_str().ok()) {
                Some(value) => value,
                None => panic!("synthetic source must exist"),
            };
            let actual = extract_chat_restoration_session_id(&headers, &body);
            native_restoration_assert_borrowed(actual, expected);
            assert!(actual == extract_codex_restoration_session_id(&headers, &body));
            headers.remove(name);
        }
        native_restoration_assert_borrowed(
            extract_chat_restoration_session_id(&headers, &body),
            body["metadata"]["session_id"].as_str().unwrap_or_default(),
        );
        let body = json!({"metadata":{"user_id":"user_session_chat-suffix_session_tail"}});
        let expected = body["metadata"]["user_id"]
            .as_str()
            .and_then(|value| value.split_once("_session_"))
            .map(|(_, suffix)| suffix)
            .unwrap_or_default();
        native_restoration_assert_borrowed(
            extract_chat_restoration_session_id(&headers, &body),
            expected,
        );
    }

    #[test]
    fn native_restoration_chat_rejects_invalid_higher_priority_headers() {
        let names = ["thread-id", "session-id", "session_id", "x-session-id"];
        let body =
            json!({"metadata":{"session_id":"chat-fallback", "user_id":"user_session_suffix"}});
        for (index, name) in names.iter().enumerate() {
            for invalid in [
                "",
                " ",
                " leading",
                "trailing ",
                "a\tb",
                "a\u{85}b",
                "x".repeat(257).as_str(),
            ] {
                let mut headers = HeaderMap::new();
                for &lower in &names[index + 1..] {
                    headers.insert(lower, native_restoration_header(b"lower-source"));
                }
                headers.insert(*name, native_restoration_header(invalid.as_bytes()));
                assert!(extract_chat_restoration_session_id(&headers, &body).is_none());
                assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
            }
        }
    }

    #[test]
    fn native_restoration_chat_rejects_duplicate_headers_even_if_identical() {
        let body = json!({"metadata":{"session_id":"chat-fallback"}});
        for name in ["thread-id", "session-id", "session_id", "x-session-id"] {
            for second in ["duplicate-source", "different-source"] {
                let mut headers = HeaderMap::new();
                headers.append(name, native_restoration_header(b"duplicate-source"));
                headers.append(name, native_restoration_header(second.as_bytes()));
                assert!(extract_chat_restoration_session_id(&headers, &body).is_none());
            }
        }
    }

    #[test]
    fn native_restoration_chat_requires_strict_metadata_and_recognized_suffix() {
        let headers = HeaderMap::new();
        for invalid in [
            json!(null),
            json!(false),
            json!(7),
            json!([]),
            json!({}),
            json!(""),
            json!(" leading"),
            json!("a\nb"),
            json!("a\u{7f}b"),
            json!("x".repeat(257)),
        ] {
            let body =
                json!({"metadata":{"session_id":invalid, "user_id":"user_session_valid-suffix"}});
            assert!(extract_chat_restoration_session_id(&headers, &body).is_none());
        }
        for invalid in [
            json!(null),
            json!([]),
            json!(5),
            json!("unrecognized-user"),
            json!("user_session_"),
            json!("user_session_a b"),
            json!("user_session_a\nb"),
            json!(format!("user_session_{}", "é".repeat(129))),
        ] {
            let body = json!({"metadata":{"user_id":invalid}});
            assert!(extract_chat_restoration_session_id(&headers, &body).is_none());
        }
        for metadata in [json!(null), json!(false), json!([]), json!("not-an-object")] {
            assert!(
                extract_chat_restoration_session_id(&headers, &json!({"metadata":metadata}))
                    .is_none()
            );
        }
    }

    #[test]
    fn native_restoration_chat_ignores_unrecognized_sources_without_generation() {
        let mut headers = HeaderMap::new();
        for name in [
            "x-request-id",
            "x-conversation-id",
            "x-claude-code-session-id",
            "x-grok-conv-id",
        ] {
            headers.insert(name, native_restoration_header(b"unsupported-source"));
        }
        let body = json!({"session_id":"root-source", "prompt_cache_key":"cache-source", "previous_response_id":"response-source", "metadata":{"user_id":"unrecognized-user"}});
        for _ in 0..2 {
            assert!(extract_chat_restoration_session_id(&headers, &body).is_none());
        }
        assert!(extract_chat_restoration_session_id(&HeaderMap::new(), &json!({})).is_none());
    }

    #[test]
    fn native_restoration_anthropic_source_order_and_borrowing() {
        let mut headers = HeaderMap::new();
        let names = ["x-claude-code-session-id", "claude-code-session-id"];
        for (name, value) in names.iter().zip(["anthropic-a", "anthropic-b"]) {
            headers.insert(*name, native_restoration_header(value.as_bytes()));
        }
        let body = json!({"metadata":{"session_id":"anthropic-metadata", "user_id":"user_session_anthropic-suffix"}});
        for name in names {
            let expected = match headers.get(name).and_then(|value| value.to_str().ok()) {
                Some(value) => value,
                None => panic!("synthetic source must exist"),
            };
            native_restoration_assert_borrowed(
                extract_anthropic_restoration_session_id(&headers, &body),
                expected,
            );
            headers.remove(name);
        }
        native_restoration_assert_borrowed(
            extract_anthropic_restoration_session_id(&headers, &body),
            body["metadata"]["session_id"].as_str().unwrap_or_default(),
        );
        let body = json!({"metadata":{"user_id":"uninspected prefix_session_anthropic-suffix_session_tail"}});
        let expected = body["metadata"]["user_id"]
            .as_str()
            .and_then(|value| value.split_once("_session_"))
            .map(|(_, suffix)| suffix)
            .unwrap_or_default();
        native_restoration_assert_borrowed(
            extract_anthropic_restoration_session_id(&headers, &body),
            expected,
        );
    }

    #[test]
    fn native_restoration_anthropic_rejects_invalid_higher_priority_headers() {
        let names = ["x-claude-code-session-id", "claude-code-session-id"];
        let body = json!({"metadata":{"session_id":"anthropic-fallback", "user_id":"user_session_suffix"}});
        for (index, name) in names.iter().enumerate() {
            for invalid in [
                "",
                " ",
                " leading",
                "trailing ",
                "a\tb",
                "a\u{85}b",
                "x".repeat(257).as_str(),
            ] {
                let mut headers = HeaderMap::new();
                for &lower in &names[index + 1..] {
                    headers.insert(lower, native_restoration_header(b"lower-source"));
                }
                headers.insert(*name, native_restoration_header(invalid.as_bytes()));
                assert!(extract_anthropic_restoration_session_id(&headers, &body).is_none());
            }
        }
    }

    #[test]
    fn native_restoration_anthropic_rejects_duplicate_headers_even_if_identical() {
        let body = json!({"metadata":{"session_id":"anthropic-fallback"}});
        for name in ["x-claude-code-session-id", "claude-code-session-id"] {
            for second in ["duplicate-source", "different-source"] {
                let mut headers = HeaderMap::new();
                headers.append(name, native_restoration_header(b"duplicate-source"));
                headers.append(name, native_restoration_header(second.as_bytes()));
                assert!(extract_anthropic_restoration_session_id(&headers, &body).is_none());
            }
        }
    }

    #[test]
    fn native_restoration_anthropic_requires_strict_metadata_and_recognized_suffix() {
        let headers = HeaderMap::new();
        for invalid in [
            json!(null),
            json!(false),
            json!(7),
            json!([]),
            json!({}),
            json!(""),
            json!(" leading"),
            json!("a\nb"),
            json!("a\u{7f}b"),
            json!("x".repeat(257)),
        ] {
            let body =
                json!({"metadata":{"session_id":invalid, "user_id":"user_session_valid-suffix"}});
            assert!(extract_anthropic_restoration_session_id(&headers, &body).is_none());
        }
        for invalid in [
            json!(null),
            json!([]),
            json!(5),
            json!("unrecognized-user"),
            json!("user_session_"),
            json!("user_session_a b"),
            json!("user_session_a\nb"),
            json!(format!("user_session_{}", "é".repeat(129))),
        ] {
            let body = json!({"metadata":{"user_id":invalid}});
            assert!(extract_anthropic_restoration_session_id(&headers, &body).is_none());
        }
        for metadata in [json!(null), json!(false), json!([]), json!("not-an-object")] {
            assert!(extract_anthropic_restoration_session_id(
                &headers,
                &json!({"metadata":metadata})
            )
            .is_none());
        }
        let body = json!({"metadata":{"session_id":"metadata-wins", "user_id":false}});
        native_restoration_assert_borrowed(
            extract_anthropic_restoration_session_id(&headers, &body),
            body["metadata"]["session_id"].as_str().unwrap_or_default(),
        );
    }

    #[test]
    fn native_restoration_anthropic_ignores_codex_and_other_identity_sources() {
        let body = json!({"session_id":"root-source", "prompt_cache_key":"cache-source", "previous_response_id":"response-source"});
        for name in [
            "thread-id",
            "session-id",
            "session_id",
            "x-session-id",
            "x-request-id",
            "x-conversation-id",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(name, native_restoration_header(b"unsupported-source"));
            assert!(extract_anthropic_restoration_session_id(&headers, &body).is_none());
        }
        assert!(extract_anthropic_restoration_session_id(&HeaderMap::new(), &body).is_none());
    }

    #[test]
    fn native_restoration_gemini_borrows_only_explicit_local_session_header() {
        let mut headers = HeaderMap::new();
        for name in [
            "thread-id",
            "session-id",
            "session_id",
            "x-claude-code-session-id",
        ] {
            headers.insert(name, native_restoration_header(b"unsupported-source"));
        }
        headers.insert(
            "x-session-id",
            native_restoration_header(b"gemini-explicit"),
        );
        let expected = match headers
            .get("x-session-id")
            .and_then(|value| value.to_str().ok())
        {
            Some(value) => value,
            None => panic!("synthetic source must exist"),
        };
        native_restoration_assert_borrowed(
            extract_gemini_restoration_session_id(&headers),
            expected,
        );
    }

    #[test]
    fn native_restoration_gemini_rejects_invalid_explicit_session_header() {
        for invalid in [
            "",
            " ",
            " leading",
            "trailing ",
            "a\tb",
            "a\u{85}b",
            "x".repeat(257).as_str(),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "thread-id",
                native_restoration_header(b"unsupported-source"),
            );
            headers.insert(
                "x-session-id",
                native_restoration_header(invalid.as_bytes()),
            );
            assert!(extract_gemini_restoration_session_id(&headers).is_none());
        }
    }

    #[test]
    fn native_restoration_gemini_rejects_duplicate_headers_even_if_identical() {
        for second in ["duplicate-source", "different-source"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "thread-id",
                native_restoration_header(b"unsupported-source"),
            );
            headers.append(
                "x-session-id",
                native_restoration_header(b"duplicate-source"),
            );
            headers.append("x-session-id", native_restoration_header(second.as_bytes()));
            assert!(extract_gemini_restoration_session_id(&headers).is_none());
        }
    }

    #[test]
    fn native_restoration_gemini_does_not_infer_identity_or_generate_fallback() {
        for name in [
            "thread-id",
            "session-id",
            "session_id",
            "x-claude-code-session-id",
            "x-request-id",
            "x-conversation-id",
            "x-goog-request-id",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(name, native_restoration_header(b"unsupported-source"));
            assert!(extract_gemini_restoration_session_id(&headers).is_none());
        }
        for _ in 0..2 {
            assert!(extract_gemini_restoration_session_id(&HeaderMap::new()).is_none());
        }
    }

    #[test]
    fn native_restoration_all_parsers_enforce_utf8_byte_limit_without_trimming() {
        let body = json!({"metadata":{"session_id":"metadata-fallback"}});
        let empty_headers = HeaderMap::new();
        for raw in [
            "a".repeat(256),
            "é".repeat(128),
            "界".repeat(85),
            "short-identity".to_owned(),
        ] {
            let mut headers = HeaderMap::new();
            for name in ["thread-id", "x-claude-code-session-id", "x-session-id"] {
                headers.insert(name, native_restoration_header(raw.as_bytes()));
            }
            assert!(native_restoration_all_identities(&headers, &body)
                .iter()
                .all(|actual| *actual == Some(raw.as_str())));
            let body = json!({"metadata":{"session_id":raw}});
            for actual in [
                extract_chat_restoration_session_id(&empty_headers, &body),
                extract_anthropic_restoration_session_id(&empty_headers, &body),
            ] {
                assert!(actual == body["metadata"]["session_id"].as_str());
            }
        }
        for raw in [
            "a".repeat(257),
            "é".repeat(129),
            format!("{}a", "é".repeat(128)),
            " leading".to_owned(),
            "trailing ".to_owned(),
            "a\tb".to_owned(),
            "a\u{2003}b".to_owned(),
            "a\u{80}b".to_owned(),
            String::new(),
        ] {
            let mut headers = HeaderMap::new();
            for name in ["thread-id", "x-claude-code-session-id", "x-session-id"] {
                headers.insert(name, native_restoration_header(raw.as_bytes()));
            }
            assert!(native_restoration_all_identities(&headers, &body)
                .iter()
                .all(Option::is_none));
        }
    }

    #[test]
    fn native_restoration_all_parsers_reject_non_utf8_headers_without_fallback() {
        let body = json!({"metadata":{"session_id":"metadata-fallback"}});
        for invalid in [b"\xff".as_slice(), b"\xc3\x28".as_slice()] {
            let mut headers = HeaderMap::new();
            for name in ["thread-id", "x-claude-code-session-id", "x-session-id"] {
                headers.insert(name, native_restoration_header(invalid));
            }
            assert!(native_restoration_all_identities(&headers, &body)
                .iter()
                .all(Option::is_none));
        }
    }

    #[test]
    fn native_restoration_parsers_preserve_input_and_legacy_session_behavior() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "thread-id",
            native_restoration_header(b"  synthetic-legacy-thread-identifier  "),
        );
        headers.insert(
            "x-claude-code-session-id",
            native_restoration_header(b"  synthetic-legacy-anthropic-identifier  "),
        );
        headers.insert(
            "x-session-id",
            native_restoration_header(b"  synthetic-legacy-gemini-identifier  "),
        );
        let body = json!({"metadata":{"session_id":"synthetic-metadata", "user_id":"user_session_synthetic-suffix"}, "messages":[{"role":"user", "content":"synthetic-text"}]});
        let original_headers = headers.clone();
        let original_body = body.clone();
        assert!(native_restoration_all_identities(&headers, &body)
            .iter()
            .all(Option::is_none));
        assert!(extract_codex_restoration_session_id(&headers, &body).is_none());
        let codex = extract_session_id(&headers, &body, "codex");
        assert!(codex.session_id == "codex_synthetic-legacy-thread-identifier");
        assert!(codex.source == SessionIdSource::Header && codex.client_provided);
        let claude = extract_session_id(&headers, &body, "claude");
        assert!(claude.session_id == "  synthetic-legacy-anthropic-identifier  ");
        assert!(claude.source == SessionIdSource::Header && claude.client_provided);
        assert!(headers == original_headers);
        assert!(body == original_body);
    }

    // ========== Session ID 提取测试 ==========

    #[test]
    fn zcode_native_header_is_stable_and_missing_identity_stays_unknown() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-session-id",
            "native-zcode-session".parse().expect("header"),
        );
        headers.insert("x-request-id", "request-a".parse().expect("header"));
        let body = json!({"messages": []});
        let original = headers.clone();
        let result = extract_client_session_id(&headers, &body, "zcode").expect("session");
        assert_eq!(result.session_id, "native-zcode-session");
        assert_eq!(result.source, SessionIdSource::Header);
        assert_eq!(headers, original);
        headers.remove("x-session-id");
        assert!(extract_client_session_id(&headers, &body, "zcode").is_none());
    }

    #[test]
    fn workbuddy_uses_native_conversation_id_across_turns_without_rewriting_requests() {
        let body = json!({
            "messages": [{"role":"user", "content":"Hello"}],
            "metadata": {"session_id":"metadata-fallback"}
        });
        let original_body = body.clone();
        for conversation in ["native-session-a", "11111111-2222-3333-4444-555555555555"] {
            for turn in ["first-turn", "second-turn"] {
                let mut headers = HeaderMap::new();
                headers.insert(
                    "X-Conversation-ID",
                    conversation.parse().expect("conversation"),
                );
                headers.insert("x-conversation-request-id", turn.parse().expect("turn"));
                headers.insert("x-conversation-message-id", turn.parse().expect("message"));
                headers.insert("x-request-id", turn.parse().expect("request"));
                headers.insert(
                    "x-parent-conversation-id",
                    "parent-session".parse().expect("parent"),
                );
                let original_headers = headers.clone();
                let result = extract_client_session_id(&headers, &body, "workbuddy")
                    .expect("native WorkBuddy conversation");
                assert_eq!(result.session_id, conversation);
                assert_eq!(result.source, SessionIdSource::Header);
                assert!(result.client_provided);
                assert_eq!(headers, original_headers);
                assert_eq!(body, original_body);
            }
        }
    }

    #[test]
    fn workbuddy_does_not_invent_sessions_from_request_or_parent_ids() {
        let mut headers = HeaderMap::new();
        for name in [
            "x-request-id",
            "x-conversation-request-id",
            "x-conversation-message-id",
            "x-parent-conversation-id",
        ] {
            headers.insert(
                name,
                "11111111-2222-3333-4444-555555555555".parse().expect("ID"),
            );
        }
        for value in ["", "   "] {
            headers.insert(
                "x-conversation-id",
                value.parse().expect("empty conversation"),
            );
            assert!(extract_client_session_id(&headers, &json!({}), "workbuddy").is_none());
            let result = extract_client_session_id(
                &headers,
                &json!({"metadata":{"session_id":"metadata-fallback"}}),
                "workbuddy",
            )
            .expect("metadata fallback");
            assert_eq!(result.session_id, "metadata-fallback");
        }
        headers.insert(
            "x-conversation-id",
            "workbuddy-only".parse().expect("conversation"),
        );
        for client in ["codex", "claude", "gemini", "grokbuild"] {
            assert!(extract_client_session_id(&headers, &json!({}), client).is_none());
        }
    }

    #[test]
    fn test_extract_session_from_claude_metadata_user_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "user_id": "user_john_doe_session_abc123def456"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "abc123def456");
        assert_eq!(result.source, SessionIdSource::MetadataUserId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_metadata_session_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "session_id": "my-session-123"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "my-session-123");
        assert_eq!(result.source, SessionIdSource::MetadataSessionId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "d937243f-2702-4f20-97b6-c9682235ab81".parse().unwrap(),
        );
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "d937243f-2702-4f20-97b6-c9682235ab81");
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_header_precedes_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "header-session-123".parse().unwrap(),
        );
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "session_id": "my-session-123"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "header-session-123");
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    #[test]
    fn test_codex_previous_response_id_is_not_stable_session_identity() {
        let headers = HeaderMap::new();
        let body = json!({
            "input": "Write a function",
            "previous_response_id": "resp_abc123def456789"
        });

        let result = extract_session_id(&headers, &body, "codex");

        assert!(!result.session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_codex_keeps_existing_response_session_headers() {
        let body = json!({ "input": "Write a function" });

        for header_name in ["session_id", "x-session-id"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header_name,
                "d937243f-2702-4f20-97b6-c9682235ab81".parse().unwrap(),
            );

            let result = extract_session_id(&headers, &body, "codex");

            assert_eq!(
                result.session_id,
                "codex_d937243f-2702-4f20-97b6-c9682235ab81"
            );
            assert_eq!(result.source, SessionIdSource::Header);
            assert!(result.client_provided);
        }
    }

    #[test]
    fn test_codex_current_client_headers_identify_same_thread_across_requests() {
        for format in ["codex", "openai"] {
            for header_name in ["thread-id", "session-id"] {
                let mut headers = HeaderMap::new();
                headers.insert(
                    header_name,
                    axum::http::HeaderValue::from_static("11111111-2222-3333-4444-555555555555"),
                );
                let original_headers = headers.clone();
                for cursor in ["resp_first", "resp_second"] {
                    let body = json!({"input":"Hello", "previous_response_id":cursor});
                    let original_body = body.clone();
                    let result = extract_session_id(&headers, &body, format);
                    assert_eq!(
                        result.session_id,
                        "codex_11111111-2222-3333-4444-555555555555"
                    );
                    assert_eq!(result.source, SessionIdSource::Header);
                    assert!(result.client_provided);
                    assert_eq!(body, original_body);
                    assert_eq!(headers, original_headers);
                }
            }
        }
    }

    #[test]
    fn test_codex_prefers_thread_identity_and_falls_back_when_it_is_empty() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "thread-id",
            axum::http::HeaderValue::from_static("11111111-2222-3333-4444-555555555555"),
        );
        headers.insert(
            "session-id",
            axum::http::HeaderValue::from_static("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        );
        headers.insert(
            "session_id",
            axum::http::HeaderValue::from_static("99999999-8888-7777-6666-555555555555"),
        );
        let body = json!({"metadata":{"session_id":"metadata-session-123456789"}});
        assert_eq!(
            extract_session_id(&headers, &body, "codex").session_id,
            "codex_11111111-2222-3333-4444-555555555555"
        );
        headers.insert("thread-id", axum::http::HeaderValue::from_static("   "));
        assert_eq!(
            extract_session_id(&headers, &body, "codex").session_id,
            "codex_aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
    }

    #[test]
    fn test_codex_cache_and_turn_keys_alone_do_not_identify_a_thread() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-client-request-id",
            axum::http::HeaderValue::from_static("11111111-2222-3333-4444-555555555555"),
        );
        headers.insert(
            "x-codex-turn-metadata",
            axum::http::HeaderValue::from_static("{\"turn_id\":\"turn-local\"}"),
        );
        let body = json!({"prompt_cache_key":"shared-cache-11111111-2222-3333", "previous_response_id":"resp_local"});
        assert!(extract_client_session_id(&headers, &body, "codex").is_none());
    }

    #[test]
    fn test_grokbuild_prefers_conversation_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-grok-conv-id",
            "conv-724f4275-584e-43af-ad46-b5e7509a3ca2".parse().unwrap(),
        );
        headers.insert(
            "x-grok-session-id",
            "session-d937243f-2702-4f20-97b6-c9682235ab81"
                .parse()
                .unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "grokbuild");

        assert_eq!(
            result.session_id,
            "grokbuild_conv-724f4275-584e-43af-ad46-b5e7509a3ca2"
        );
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    #[test]
    fn test_grokbuild_falls_back_to_session_header() {
        let body = json!({ "input": "Write a function" });

        for conversation_id in ["", "                         "] {
            let mut headers = HeaderMap::new();
            headers.insert("x-grok-conv-id", conversation_id.parse().unwrap());
            headers.insert(
                "x-grok-session-id",
                "session-d937243f-2702-4f20-97b6-c9682235ab81"
                    .parse()
                    .unwrap(),
            );

            let result = extract_session_id(&headers, &body, "grokbuild");

            assert_eq!(
                result.session_id,
                "grokbuild_session-d937243f-2702-4f20-97b6-c9682235ab81"
            );
            assert_eq!(result.source, SessionIdSource::Header);
            assert!(result.client_provided);
        }
    }

    #[test]
    fn test_grokbuild_ignores_request_and_codex_session_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-grok-req-id",
            "request-724f4275-584e-43af-ad46-b5e7509a3ca2"
                .parse()
                .unwrap(),
        );
        headers.insert(
            "x-session-id",
            "codex-d937243f-2702-4f20-97b6-c9682235ab81"
                .parse()
                .unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "grokbuild");

        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_extract_session_generates_new_when_not_found() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert!(!result.session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_parse_session_from_user_id() {
        assert_eq!(
            parse_session_from_user_id("user_john_session_abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            parse_session_from_user_id("my_app_session_xyz789"),
            Some("xyz789".to_string())
        );
        // 注意: "_session_" 是分隔符，所以下面的字符串会匹配
        assert_eq!(
            parse_session_from_user_id("no_session_marker"),
            Some("marker".to_string())
        );
        // 没有 "_session_" 分隔符的情况
        assert_eq!(parse_session_from_user_id("user_john_abc123"), None);
        assert_eq!(parse_session_from_user_id("_session_"), None);
    }
}
