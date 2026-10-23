use serde_json::json;

use super::engine::SecurityAuditEngine;
use super::pii::is_valid_chinese_id_card;
use super::runtime::SecurityRuntimeConfig;
use super::types::{AuditAction, RiskLevel, RuleCategory};

#[test]
fn test_clean_request_passes() {
    let body = json!({
        "model": "claude-3-5-sonnet",
        "system": "You are a helpful coding assistant.",
        "messages": [
            {
                "role": "user",
                "content": "Please write a simple binary search function in Rust."
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked, "Clean request must not be blocked");
    assert!(res.findings.is_empty(), "No findings for clean request");
}

#[test]
fn test_rsa_private_key_hidden_and_sent_by_default() {
    let mut body = json!({
        "model": "claude-3-5-sonnet",
        "temperature": 0.7,
        "messages": [
            {
                "role": "user",
                "content": "Here is my private key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0...\n-----END RSA PRIVATE KEY-----"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_and_redact_with_runtime(
        &mut body,
        &SecurityRuntimeConfig::default(),
    );
    assert!(
        !res.blocked,
        "Complete private keys must be hidden and sent"
    );
    assert_eq!(
        body["messages"][0]["content"],
        "Here is my private key:\n[REDACTED_PRIVATE_KEY]"
    );
    assert_eq!(body["model"], "claude-3-5-sonnet");
    assert_eq!(body["temperature"], 0.7);
    assert_eq!(res.highest_risk, Some(RiskLevel::Critical));

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "rsa_ssh_private_key")
        .expect("Must find rsa_ssh_private_key");
    assert_eq!(finding.action, AuditAction::Redact);
    assert_eq!(finding.category, RuleCategory::PrivateKey);
    assert!(!finding.matched_snippet_masked.contains("MIIEow"));
}

#[test]
fn test_github_token_detected_with_default_redaction() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Can you check why ghp_111122223333444455556666777788889999 fails to clone?"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked);

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "github_token")
        .expect("Must find github_token");
    assert_eq!(finding.action, AuditAction::Redact);
    assert_eq!(finding.risk_level, RiskLevel::High);
    assert!(finding.matched_snippet_masked.contains("****"));
}

#[test]
fn test_aws_key_and_openai_key_detected_with_default_redaction() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Use AWS key AKIAIOSFODNN7EXAMPLE and OpenAI sk-proj-1234567890123456789012345678901234 to deploy."
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked);

    let has_aws = res.findings.iter().any(|f| f.rule_id == "aws_access_key");
    let has_openai = res.findings.iter().any(|f| f.rule_id == "openai_api_key");
    assert!(has_aws, "Must detect AWS key");
    assert!(has_openai, "Must detect OpenAI key");
}

#[test]
fn test_optional_catalog_rules_are_available_but_default_to_allow() {
    use super::builtin_rules::BUILTIN_RULES;

    let catalog_rule_count = BUILTIN_RULES
        .iter()
        .filter(|rule| rule.id.starts_with("catalog:"))
        .count();
    assert_eq!(
        catalog_rule_count, 21,
        "the optional catalog must contain 21 rules"
    );
    assert!(BUILTIN_RULES
        .iter()
        .filter(|rule| rule.id.starts_with("catalog:"))
        .all(|rule| rule.default_action == AuditAction::Allow));

    let text = format!(
        concat!(
            "glpat-abcdefghijklmnopqrst ",
            "sk_live_1234567890 ",
            "npm_{} ",
            "pypi-AgEIcHlwaS5vcmc{} ",
            "xoxb-1234567890-1234567890 ",
            "ABSK{} ",
            "AIza{} ",
            "SG.{} ",
            "hf_{} ",
            "dop_v1_{} ",
            "ntn_12345678901{}000 ",
            "lin_api_{} ",
            "shpat_{} ",
            "v1.0-{}-{} ",
            "sk-openrouter-mock ",
            "gsk_{} ",
            "pplx-{} ",
            "sk-demo ",
            "https://alice:login-secret@example.com/api ",
            "password = assignment-secret"
        ),
        "a".repeat(36),
        "a".repeat(50),
        "a".repeat(109),
        "A".repeat(35),
        "a".repeat(66),
        "a".repeat(34),
        "f".repeat(64),
        "A".repeat(32),
        "a".repeat(40),
        "a".repeat(32),
        "a".repeat(24),
        "b".repeat(146),
        "a".repeat(52),
        "a".repeat(48),
    );

    let findings = SecurityAuditEngine::inspect_text(&text, "content");
    let expected_ids = [
        "catalog:gitlab-pat",
        "catalog:stripe-key",
        "catalog:npm-token",
        "catalog:pypi-token",
        "catalog:slack-bot-token",
        "catalog:bedrock-key",
        "catalog:google-api-key",
        "catalog:sendgrid-token",
        "catalog:huggingface-token",
        "catalog:digitalocean-pat",
        "catalog:notion-token",
        "catalog:linear-key",
        "catalog:shopify-token",
        "catalog:cloudflare-origin-key",
        "catalog:openrouter-key",
        "catalog:groq-key",
        "catalog:perplexity-key",
        "catalog:generic-sk-key-notice",
        "catalog:login-url-password",
        "catalog:password-assignment",
    ];
    for id in expected_ids {
        let finding = findings
            .iter()
            .find(|finding| finding.rule_id == id)
            .unwrap_or_else(|| panic!("expected optional rule {id} to match"));
        assert_eq!(
            finding.action,
            AuditAction::Allow,
            "{id} must default to allow"
        );
    }
    assert!(findings.iter().all(|finding| {
        !finding.rule_id.starts_with("catalog:") || finding.action == AuditAction::Allow
    }));

    let allowlisted = "AIzaSyabcdefghijklmnopqrstuvwxyz1234567";
    assert!(!SecurityAuditEngine::inspect_text(allowlisted, "content")
        .iter()
        .any(|finding| finding.rule_id == "catalog:google-api-key"));
}

#[test]
fn test_openrouter_catalog_rule_matches_sk_and_v1_prefixes() {
    let findings =
        SecurityAuditEngine::inspect_text("sk-openrouter-mock v1-openrouter-mock", "content");
    let openrouter_findings: Vec<_> = findings
        .iter()
        .filter(|finding| finding.rule_id == "catalog:openrouter-key")
        .collect();

    assert_eq!(openrouter_findings.len(), 2);
    assert!(openrouter_findings
        .iter()
        .all(|finding| finding.action == AuditAction::Allow));
}

#[test]
fn test_catalog_password_field_is_detected_only_for_structured_password_values() {
    let body = json!({
        "messages": [{
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "name": "login",
                "input": {
                    "username": "alice",
                    "password": "structured-secret"
                }
            }]
        }]
    });

    let result = SecurityAuditEngine::inspect_request(&body);
    let finding = result
        .findings
        .iter()
        .find(|finding| finding.rule_id == "catalog:password-field")
        .expect("structured password field must be detected");
    assert_eq!(finding.action, AuditAction::Allow);
    assert!(!finding.matched_snippet_masked.contains("structured-secret"));
    assert!(result
        .findings
        .iter()
        .all(|finding| finding.rule_id != "catalog:password-assignment"));
}

#[test]
fn test_optional_password_rules_can_be_changed_to_redact_without_leaking_values() {
    use std::collections::HashMap;

    let mut runtime = SecurityRuntimeConfig::default();
    runtime.builtin_action_overrides = HashMap::from([
        (
            "catalog:login-url-password".to_string(),
            AuditAction::Redact,
        ),
        (
            "catalog:password-assignment".to_string(),
            AuditAction::Redact,
        ),
        ("catalog:password-field".to_string(), AuditAction::Redact),
    ]);

    let mut body = json!({
        "messages": [{
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Open https://alice:login-secret@example.com/api and password = assignment-secret"
                },
                {
                    "type": "tool_use",
                    "name": "login",
                    "input": { "password": "structured-secret" }
                }
            ]
        }]
    });

    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(!result.blocked);
    let content = body["messages"][0]["content"]
        .as_array()
        .expect("content array");
    let text = content[0]["text"].as_str().expect("text block");
    assert!(text.contains("https://alice:[REDACTED]@example.com/api"));
    assert!(text.contains("password = [REDACTED]"));
    assert_eq!(content[1]["input"]["password"], "[REDACTED]");
    assert!(!body.to_string().contains("login-secret"));
    assert!(!body.to_string().contains("assignment-secret"));
    assert!(!body.to_string().contains("structured-secret"));
    assert!(
        SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime)
            .findings
            .is_empty()
    );
}

#[test]
fn test_generic_sk_catalog_rule_does_not_override_existing_openai_rule() {
    let token = format!("sk-proj-{}", "a".repeat(32));
    let body = json!({"messages": [{"role": "user", "content": token}]});
    let result = SecurityAuditEngine::inspect_request(&body);

    assert!(result.findings.iter().any(
        |finding| finding.rule_id == "openai_api_key" && finding.action == AuditAction::Redact
    ));
    assert!(result
        .findings
        .iter()
        .any(|finding| finding.rule_id == "catalog:generic-sk-key-notice"
            && finding.action == AuditAction::Allow));

    let mut runtime = SecurityRuntimeConfig::default();
    runtime.builtin_action_overrides.insert(
        "catalog:generic-sk-key-notice".to_string(),
        AuditAction::Block,
    );
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(result
        .findings
        .iter()
        .any(|finding| finding.rule_id == "catalog:generic-sk-key-notice"
            && finding.action == AuditAction::Allow));
}

#[test]
fn test_database_uri_detected_and_log_snippet_masks_password() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Debug connection error: postgres://dbuser:MySuperSecretPassword123@10.0.1.25:5432/orders"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked);

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "db_connection_uri")
        .expect("Must find db connection");
    assert_eq!(finding.action, AuditAction::Redact);
    assert!(finding
        .matched_snippet_masked
        .contains("postgres://dbuser:****@10.0.1.25:5432/orders"));
    assert!(!finding
        .matched_snippet_masked
        .contains("MySuperSecretPassword123"));
}

#[test]
fn test_tool_use_deep_input_inspected() {
    let body = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "name": "bash",
                        "input": {
                            "command": "git push https://ghp_0123456789abcdef0123456789abcdef0123@github.com/org/repo.git main"
                        }
                    }
                ]
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(
        !res.blocked,
        "Sensitive data in tool_use.input must be recognized for redaction"
    );

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "github_token")
        .expect("Must catch token in tool_use input");
    assert!(finding.location.contains("tool_use.input.{field}"));
}

#[test]
fn test_openai_tool_calls_arguments_inspected() {
    let body = json!({
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "execute_command",
                            "arguments": "{\"env\":\"production\",\"api_key\":\"sk-ant-api03-1234567890abcdef1234567890abcdef\"}"
                        }
                    }
                ]
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(
        !res.blocked,
        "Sensitive data in tool_calls arguments must be recognized for redaction"
    );

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "anthropic_api_key")
        .expect("Must catch Anthropic key in tool_calls arguments");
    assert!(finding
        .location
        .contains("tool_calls[0].function.arguments"));
    assert!(!finding.location.contains("execute_command"));
}

#[tokio::test]
async fn custom_rule_names_and_dynamic_paths_never_enter_block_reason_or_audit_location() {
    use super::runtime::CompiledCustomRule;

    let secret = format!("{}{}", concat!("gh", "p_"), "q".repeat(36));
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(CompiledCustomRule {
        id: "corp_rule".into(),
        name: secret.clone(),
        rule_type: "keyword".into(),
        pattern: "internal_marker".into(),
        regex: None,
        risk_level: RiskLevel::Low,
        action: AuditAction::Block,
        enabled: true,
    });
    let body = json!({"messages": [{"role": "assistant", "content": [{
        "type": "tool_use", "name": secret.clone(),
        "input": {secret.clone(): "internal_marker"}
    }]}]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(result.blocked);
    let reason = result.block_reason.unwrap();
    assert!(
        reason.contains("LOW"),
        "response must include the rule's risk level"
    );
    assert!(reason.contains("自定义规则"));
    assert!(
        !reason.contains(&secret),
        "untrusted rule name must not enter HTTP 400"
    );
    assert!(
        result
            .findings
            .iter()
            .all(|f| !f.location.contains(&secret)),
        "audit locations must not include user-controlled names or keys"
    );
    assert!(result
        .findings
        .iter()
        .any(|f| f.location.contains("tool_use.input.{field}")));
    let response =
        super::response::build_blocked_response(&crate::app_config::AppType::Claude, &reason)
            .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains(&secret),
        "HTTP 400 must never echo the custom rule name"
    );
}

#[test]
fn dynamic_openai_and_gemini_field_names_are_not_logged() {
    let secret = format!("{}{}", concat!("gh", "p_"), "q".repeat(36));
    let token = format!("{}{}", concat!("gh", "p_"), "a".repeat(36));
    let body = json!({
        "messages": [{"role": "assistant", "tool_calls": [{"function": {"name": secret.clone(), "arguments": token.clone()}}]}],
        "contents": [{"parts": [{"functionCall": {"args": {secret.clone(): token.clone()}}}]}],
        "input": {secret.clone(): token.clone()},
    });
    let locations: Vec<_> = super::extractor::extract_payload_texts(&body)
        .into_iter()
        .map(|node| node.location)
        .collect();
    assert!(!locations.is_empty());
    assert!(locations.iter().all(|location| !location.contains(&secret)));
    assert!(locations
        .iter()
        .any(|location| location.contains("tool_calls[0].function.arguments")));
    assert!(locations
        .iter()
        .any(|location| location.contains("functionCall.args.{field}")));
    assert!(locations
        .iter()
        .any(|location| location.contains("input.{field}")));
}

#[test]
fn test_chinese_id_card_checksum() {
    assert!(is_valid_chinese_id_card("11010519491231002X"));

    assert!(!is_valid_chinese_id_card("110105194912310021"));
    assert!(!is_valid_chinese_id_card("123456789012345678"));

    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "用户身份证号码为 11010519491231002X，联系电话 13800138000"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);

    assert!(!res.blocked);

    let id_finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "chinese_id_card")
        .expect("Must find valid id card");
    assert_eq!(id_finding.matched_snippet_masked, "110105********002X");

    let phone_finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "chinese_phone_number")
        .expect("Must find phone number");
    assert_eq!(id_finding.action, AuditAction::Redact);
    assert_eq!(phone_finding.action, AuditAction::Redact);
    assert_eq!(phone_finding.matched_snippet_masked, "138****8000");
}

#[test]
fn test_large_payload_fast_passthrough() {
    let large_code = "fn fibonacci(n: u64) -> u64 { match n { 0 => 0, 1 => 1, _ => fibonacci(n - 1) + fibonacci(n - 2) } }\n".repeat(5000);
    assert!(large_code.len() > 400_000, "Text size is > 400KB");

    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": large_code
            }
        ]
    });

    let start = std::time::Instant::now();
    let res = SecurityAuditEngine::inspect_request(&body);
    let elapsed = start.elapsed();

    assert!(!res.blocked);
    assert!(res.findings.is_empty());
    assert!(
        elapsed.as_millis() < 50,
        "Large payload inspection must be ultra-fast (was {:?})",
        elapsed
    );
}

#[tokio::test]
async fn test_build_blocked_response_formats() {
    use crate::app_config::AppType;
    use crate::proxy::security::response::build_blocked_response;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    // 1. Claude / Anthropic
    let claude_resp = build_blocked_response(&AppType::Claude, "触发规则: RSA 私钥")
        .expect("build claude response");
    assert_eq!(claude_resp.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(claude_resp.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["type"], "error");
    assert_eq!(val["error"]["type"], "invalid_request_error");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("RSA 私钥"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[Vary AI 安全审计]"));
    assert!(!val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[CC Switch 安全审计]"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("当前处理方式为「拦截」"));
    assert!(!val["error"]["message"].as_str().unwrap().contains("出网前"));

    // 2. OpenAI / Codex
    let codex_resp = build_blocked_response(&AppType::Codex, "触发规则: GitHub Token")
        .expect("build codex response");
    assert_eq!(codex_resp.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(codex_resp.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], "sensitive_data_blocked");
    assert_eq!(val["error"]["type"], "invalid_request_error");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("GitHub Token"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[Vary AI 安全审计]"));
    assert!(!val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[CC Switch 安全审计]"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("当前处理方式为「拦截」"));
    assert!(!val["error"]["message"].as_str().unwrap().contains("出网前"));

    // 3. Gemini
    let gemini_resp = build_blocked_response(&AppType::Gemini, "触发规则: API Key")
        .expect("build gemini response");
    assert_eq!(gemini_resp.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(gemini_resp.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], 400);
    assert_eq!(val["error"]["status"], "INVALID_ARGUMENT");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("API Key"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[Vary AI 安全审计]"));
    assert!(!val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("[CC Switch 安全审计]"));
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("当前处理方式为「拦截」"));
    assert!(!val["error"]["message"].as_str().unwrap().contains("出网前"));
}

#[tokio::test]
async fn test_explicit_builtin_block_returns_protocol_error() {
    use crate::app_config::AppType;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    let mut runtime = SecurityRuntimeConfig::default();
    runtime
        .builtin_action_overrides
        .insert("github_token".into(), AuditAction::Block);
    let mut body = json!({"messages": [{"role": "user", "content": format!("Token {}{}", concat!("gh", "p_"), "a".repeat(36))}]});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(result.blocked);
    let response = super::response::build_blocked_response(
        &AppType::Claude,
        result.block_reason.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["type"], "invalid_request_error");
    assert!(val["error"]["message"].as_str().unwrap().contains("GitHub"));
}

#[test]
fn test_multi_rule_block_reason_lists_all_explicit_blocks() {
    let mut runtime = SecurityRuntimeConfig::default();
    for id in ["github_token", "anthropic_api_key", "db_connection_uri"] {
        runtime
            .builtin_action_overrides
            .insert(id.into(), AuditAction::Block);
    }
    let body = json!({"messages": [{"role": "user", "content": format!("{}{} sk-ant-api03-1234567890abcdef1234567890abcdef postgres://user:pass123@db.internal:5432/mydb", concat!("gh", "p_"), "a".repeat(36))}]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(result.blocked);
    let reason = result.block_reason.unwrap();
    assert!(reason.contains("检测到 3 项敏感数据"));
    assert!(reason.contains("GitHub 访问令牌 (Token)（HIGH）"));
    assert!(reason.contains("Anthropic API Key（HIGH）"));
    assert!(reason.contains("带密码的数据库连接串（HIGH）"));
}

#[tokio::test]
async fn blocked_response_uses_dynamic_rule_name_and_risk_level() {
    use crate::app_config::AppType;
    use axum::body::to_bytes;
    use std::collections::HashMap;

    let mut runtime = SecurityRuntimeConfig::default();
    runtime.builtin_action_overrides =
        HashMap::from([("chinese_phone_number".to_string(), AuditAction::Block)]);
    let body = json!({
        "messages": [{
            "role": "user",
            "content": "联系电话：13800138000"
        }]
    });

    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(result.blocked);
    assert_eq!(
        result.block_reason.as_deref(),
        Some("检测到 1 项敏感数据：中国大陆手机号码（MEDIUM）。")
    );

    let response = super::response::build_blocked_response(
        &AppType::Claude,
        result.block_reason.as_deref().unwrap_or_default(),
    )
    .expect("build blocked response");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("parse response body");
    assert_eq!(
        value["error"]["message"].as_str(),
        Some("[Vary AI 安全审计] 检测到敏感数据，请求已被拦截\n检测到 1 项敏感数据：中国大陆手机号码（MEDIUM）。\n当前处理方式为「拦截」，如需继续，请在安全审计设置中调整对应规则。")
    );
}

#[test]
fn test_chinese_character_masking_no_panic() {
    use super::engine::mask_sensitive_value;

    let samples = ["张三", "诸葛孔明", "机密项目ProjectX", "🎯绝密计划🔥", "李"];

    for s in samples {
        let masked = mask_sensitive_value(s, &RuleCategory::Custom);
        assert!(!masked.is_empty());
        assert!(masked.contains('*'));
    }
}

#[test]
fn test_compact_chinese_id_card_and_phone() {
    let text = "用户身份信息登记：身份证号11010519491231002X，联系电话13800138000,13900139000。";
    let findings = SecurityAuditEngine::inspect_text(text, "content");

    let id_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.rule_id == "chinese_id_card")
        .collect();
    assert_eq!(
        id_findings.len(),
        1,
        "Must find exactly 1 valid ID card in compact Chinese"
    );
    assert_eq!(id_findings[0].matched_snippet_masked, "110105********002X");

    let phone_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.rule_id == "chinese_phone_number")
        .collect();
    assert_eq!(
        phone_findings.len(),
        2,
        "Must find both adjacent phone numbers separated by comma"
    );
    assert_eq!(phone_findings[0].matched_snippet_masked, "138****8000");
    assert_eq!(phone_findings[1].matched_snippet_masked, "139****9000");
}

#[test]
fn test_gemini_camel_case_system_instruction_and_codex_instructions() {
    // 1. Gemini systemInstruction (camelCase)
    let gemini_body = json!({
        "systemInstruction": {
            "parts": [
                {"text": "Secret key is ghp_111122223333444455556666777788889999"}
            ]
        },
        "contents": []
    });
    let res = SecurityAuditEngine::inspect_request(&gemini_body);
    assert!(
        !res.blocked,
        "Must detect secret in Gemini camelCase systemInstruction"
    );

    // 2. Codex Responses top-level instructions
    let codex_body = json!({
        "instructions": "Database connection: POSTGRESQL://user:pass12345@db.internal:5432/main",
        "input": []
    });
    let res2 = SecurityAuditEngine::inspect_request(&codex_body);
    assert!(
        !res2.blocked,
        "Must detect secret in Codex top-level instructions with uppercase URI"
    );

    // 3. Modern GitHub PAT
    let pat_body = json!({
        "messages": [
            {"role": "user", "content": "Deploy token: github_pat_11AAAAAAA0000000000000000000000000000000000000000000000000000000000000000000000000"}
        ]
    });
    let res3 = SecurityAuditEngine::inspect_request(&pat_body);
    assert!(!res3.blocked, "Must detect modern github_pat_ token");
}
#[test]
fn test_security_runtime_toggle_and_custom_rules() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};

    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Secret internal project code is PROJ_CORP_9988"
            }
        ]
    });

    let default_cfg = SecurityRuntimeConfig::default();
    let res = SecurityAuditEngine::inspect_request_with_runtime(&body, &default_cfg);
    assert!(!res.blocked);
    assert!(res.findings.is_empty());

    let custom_rule = CompiledCustomRule {
        id: "corp_secret_code".to_string(),
        name: "内部绝密代号".to_string(),
        rule_type: "keyword".to_string(),
        pattern: "PROJ_CORP_9988".to_string(),
        regex: None,
        risk_level: RiskLevel::Critical,
        action: AuditAction::Block,
        enabled: true,
    };

    let blocking_cfg = SecurityRuntimeConfig {
        enabled: true,
        custom_rules: vec![custom_rule],
        builtin_action_overrides: Default::default(),
        ..Default::default()
    };

    let res = SecurityAuditEngine::inspect_request_with_runtime(&body, &blocking_cfg);
    assert!(res.blocked, "Custom rule must block request");
    assert_eq!(res.findings.len(), 1);
    assert_eq!(res.findings[0].rule_id, "corp_secret_code");

    let disabled_cfg = SecurityRuntimeConfig {
        enabled: false,
        custom_rules: vec![],
        builtin_action_overrides: Default::default(),
        ..Default::default()
    };

    let res = SecurityAuditEngine::inspect_request_with_runtime(&body, &disabled_cfg);
    assert!(!res.blocked);
    assert!(res.findings.is_empty());
}
#[test]
fn test_security_runtime_custom_regex_and_policy_actions() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};

    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Database secret connect string is postgres://admin:super_secret_pw@192.168.1.100:5432/finance"
            }
        ]
    });

    let order_body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Export data for order ORD-2026-99881234 now."
            }
        ]
    });

    let re = regex::Regex::new(r"ORD-\d{4}-\d{8}").unwrap();
    let custom_regex_rule = CompiledCustomRule {
        id: "order_id_leak".to_string(),
        name: "内部订单编号".to_string(),
        rule_type: "regex".to_string(),
        pattern: r"ORD-\d{4}-\d{8}".to_string(),
        regex: Some(re),
        risk_level: RiskLevel::Medium,
        action: AuditAction::Block,
        enabled: true,
    };

    let cfg1 = SecurityRuntimeConfig {
        enabled: true,
        custom_rules: vec![custom_regex_rule.clone()],
        builtin_action_overrides: Default::default(),
        ..Default::default()
    };

    let res = SecurityAuditEngine::inspect_request_with_runtime(&order_body, &cfg1);
    assert!(res.blocked, "Explicit custom block must not be downgraded");
    assert_eq!(res.findings.len(), 1);
    assert_eq!(res.findings[0].action, AuditAction::Block);

    let cfg2 = SecurityRuntimeConfig {
        enabled: true,
        custom_rules: vec![custom_regex_rule],
        builtin_action_overrides: Default::default(),
        ..Default::default()
    };

    let res = SecurityAuditEngine::inspect_request_with_runtime(&order_body, &cfg2);
    assert!(
        res.blocked,
        "Medium risk rule with explicit block MUST block"
    );
    assert_eq!(res.findings[0].action, AuditAction::Block);

    let cfg3 = SecurityRuntimeConfig {
        enabled: true,
        custom_rules: vec![],
        builtin_action_overrides: Default::default(),
        ..Default::default()
    };

    let res_db = SecurityAuditEngine::inspect_request_with_runtime(&body, &cfg3);
    assert!(
        !res_db.blocked,
        "High risk DB connection defaults to redaction"
    );
    assert!(!res_db.findings.is_empty());
    assert_eq!(
        res_db
            .findings
            .iter()
            .find(|f| f.rule_id == "db_connection_uri")
            .map(|f| f.action),
        Some(AuditAction::Redact),
    );
    assert_eq!(
        res_db
            .findings
            .iter()
            .find(|f| f.rule_id == "rfc1918_private_ip")
            .map(|f| f.action),
        Some(AuditAction::Audit),
    );
}

#[test]
fn test_dedup_same_rule_same_snippet() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "My token is ghp_abcdef0123456789abcdef0123456789abcd and also ghp_abcdef0123456789abcdef0123456789abcd"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    let github_findings: Vec<_> = res
        .findings
        .iter()
        .filter(|f| f.rule_id == "github_token")
        .collect();
    assert_eq!(
        github_findings.len(),
        1,
        "Same rule + same snippet must be deduplicated to 1 finding"
    );
}

#[test]
fn test_dedup_different_rules_both_exposed() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "Keys: ghp_abcdef0123456789abcdef0123456789abcd and sk-ant-api03-1234567890abcdef1234567890abcdef"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked);
    let rule_ids: Vec<&str> = res.findings.iter().map(|f| f.rule_id.as_str()).collect();
    assert!(rule_ids.contains(&"github_token"), "Must find github_token");
    assert!(
        rule_ids.contains(&"anthropic_api_key"),
        "Must find anthropic_api_key"
    );
}

#[test]
fn test_dedup_different_locations_same_snippet() {
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": "ghp_abcdef0123456789abcdef0123456789abcd"
            },
            {
                "role": "assistant",
                "content": "I see ghp_abcdef0123456789abcdef0123456789abcd in your message"
            }
        ]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    let github_findings: Vec<_> = res
        .findings
        .iter()
        .filter(|f| f.rule_id == "github_token")
        .collect();

    assert_eq!(
        github_findings.len(),
        1,
        "Same snippet in different locations must be deduplicated"
    );
}

#[test]
fn test_unescape_underscore_before_matching() {
    let body = json!({
        "messages": [{
            "role": "user",
            "content": "Token: ghp\\_abcdef0123456789abcdef0123456789abcd"
        }]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(
        !res.blocked,
        "Escaped token ghp\\_ must be detected and blocked"
    );

    let finding = res
        .findings
        .iter()
        .find(|f| f.rule_id == "github_token")
        .expect("Must find github_token after unescaping");
    assert!(finding.matched_snippet_masked.contains("ghp_"));
}

#[test]
fn test_unescape_preserves_normal_text() {
    let body = json!({
        "messages": [{
            "role": "user",
            "content": "This is a normal text with underscores like file_name and some_var"
        }]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked);
    assert!(
        res.findings.is_empty(),
        "Normal text must not trigger any rules"
    );
}

#[test]
fn test_unescape_multiple_underscores() {
    let body = json!({
        "messages": [{
            "role": "user",
            "content": "Keys: ghp\\_abcdef0123456789abcdef0123456789abcd and sk-ant-api03-1234567890abcdef1234567890abcdef"
        }]
    });

    let res = SecurityAuditEngine::inspect_request(&body);
    assert!(!res.blocked, "Multiple escaped tokens must be detected");
    let rule_ids: Vec<&str> = res.findings.iter().map(|f| f.rule_id.as_str()).collect();
    assert!(rule_ids.contains(&"github_token"));
    assert!(rule_ids.contains(&"anthropic_api_key"));
}

#[test]
fn test_redact_rewrites_only_extracted_string_leaves_and_preserves_structure() {
    use super::runtime::SecurityRuntimeConfig;
    let token = "ghp_111122223333444455556666777788889999";
    let mut body = json!({
        "model": "ghp_111122223333444455556666777788889999",
        "messages": [{"role":"user", "content": format!("中文🚀 {} and ghp\\_111122223333444455556666777788889999", token), "metadata": {"flag": true}}],
        "temperature": 0.5
    });
    let original_model = body["model"].clone();
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(
        &mut body,
        &SecurityRuntimeConfig::default(),
    );
    assert!(!result.blocked);
    assert!(result
        .findings
        .iter()
        .any(|f| f.action == AuditAction::Redact));
    assert_eq!(body["model"], original_model);
    assert_eq!(body["temperature"], json!(0.5));
    assert_eq!(body["messages"][0]["metadata"]["flag"], json!(true));
    let content = body["messages"][0]["content"]
        .as_str()
        .expect("string content");
    assert!(content.starts_with("中文🚀 "));
    assert!(!content.contains(token));
    assert!(!content.contains("ghp\\_"));
    assert!(SecurityAuditEngine::inspect_request_with_runtime(
        &body,
        &SecurityRuntimeConfig::default()
    )
    .findings
    .is_empty());
}

#[test]
fn test_redact_private_key_preserves_surrounding_text_and_overlapping_pii() {
    use super::runtime::SecurityRuntimeConfig;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime
        .builtin_action_overrides
        .insert("rsa_ssh_private_key".into(), AuditAction::Redact);
    let mut body = json!({"input": ["prefix -----BEGIN RSA PRIVATE KEY-----\nMIIE-secret-content\n-----END RSA PRIVATE KEY----- suffix", "号码 13800138000 与 11010519491231002X"]});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(!result.blocked);
    assert_eq!(body["input"][0], "prefix [REDACTED_PRIVATE_KEY] suffix");
    assert!(!body.to_string().contains("MIIE-secret-content"));
    assert!(!body.to_string().contains("13800138000"));
    assert!(!body.to_string().contains("11010519491231002X"));
    assert!(
        SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime)
            .findings
            .is_empty()
    );
}

#[test]
fn test_unclosed_private_keys_hide_through_end_of_text() {
    let runtime = SecurityRuntimeConfig::default();
    for key_type in ["RSA", "OPENSSH", "EC", "DSA", "ENCRYPTED"] {
        for tail in [
            "",
            " VARYAI_SYNTHETIC_TEST",
            "\nSYNTHETIC_KEY_BODY\n后续文字🔒",
        ] {
            let mut body = json!({"prompt": format!(
                "前缀🔒 keep\\_this -----BEGIN {key_type} PRIVATE KEY-----{tail}"
            )});
            let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
            assert!(!result.blocked, "Unclosed {key_type} key must be hidden");
            assert_eq!(body["prompt"], "前缀🔒 keep\\_this [REDACTED_PRIVATE_KEY]");
            assert!(result.findings.iter().any(|f| {
                f.rule_id == "rsa_ssh_private_key" && f.action == AuditAction::Redact
            }));
            assert!(
                SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime)
                    .findings
                    .is_empty()
            );
        }
    }
}

#[test]
fn test_unclosed_private_key_history_allows_normal_followups_without_leaking() {
    let runtime = SecurityRuntimeConfig::default();
    let original = json!({
        "model": "test-model", "stream": true, "unknown_option": {"keep": true},
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "合成测试：-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_KEY_BODY"}]},
            {"role": "assistant", "content": "Previous request failed"},
            {"role": "user", "content": "你好"}
        ]
    });
    // Clients resend their unchanged local history on every turn.
    for followup in ["你好", "请继续回答普通问题"] {
        let mut expected = original.clone();
        expected["messages"][0]["content"][0]["text"] = json!("合成测试：[REDACTED_PRIVATE_KEY]");
        expected["messages"][2]["content"] = json!(followup);
        let mut body = original.clone();
        body["messages"][2]["content"] = json!(followup);
        let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
        assert!(
            !result.blocked,
            "Clean followup must not be blocked by history"
        );
        assert_eq!(body, expected);
        assert!(!body.to_string().contains("SYNTHETIC_KEY_BODY"));
    }
}

#[test]
fn test_unclosed_private_key_respects_non_redaction_actions() {
    let original = json!({"messages": [
        {"role": "user", "content": "-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_KEY_BODY"},
        {"role": "user", "content": "你好"}
    ]});
    for action in [AuditAction::Audit, AuditAction::Allow, AuditAction::Block] {
        let mut runtime = SecurityRuntimeConfig::default();
        runtime
            .builtin_action_overrides
            .insert("rsa_ssh_private_key".into(), action);
        let mut body = original.clone();
        let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
        assert_eq!(result.blocked, action == AuditAction::Block);
        assert_eq!(body, original, "Non-redaction actions must preserve text");
        assert!(result
            .findings
            .iter()
            .any(|f| { f.rule_id == "rsa_ssh_private_key" && f.action == action }));
    }
}

#[test]
fn test_mixed_closed_and_unclosed_private_keys_do_not_leak_either_body() {
    let mut body = json!({"prompt": "before -----BEGIN EC PRIVATE KEY-----\nSYNTHETIC_CLOSED\n-----END EC PRIVATE KEY----- middle -----BEGIN RSA PRIVATE KEY-----\nSYNTHETIC_OPEN"});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(
        &mut body,
        &SecurityRuntimeConfig::default(),
    );
    assert!(!result.blocked);
    assert_eq!(
        body["prompt"],
        "before [REDACTED_PRIVATE_KEY] middle [REDACTED_PRIVATE_KEY]"
    );
}

#[test]
fn test_redaction_preserves_unmatched_escaped_underscores() {
    let mut body = json!({"prompt": "keep\\_this and ghp\\_111122223333444455556666777788889999 and keep\\_that"});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(
        &mut body,
        &super::runtime::SecurityRuntimeConfig::default(),
    );
    assert!(!result.blocked);
    assert_eq!(body["prompt"], "keep\\_this and [REDACTED] and keep\\_that");
}

#[test]
fn test_custom_explicit_block_and_redact_overlap() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules = vec![
        CompiledCustomRule {
            id: "redact".into(),
            name: "redact".into(),
            rule_type: "regex".into(),
            pattern: "secret".into(),
            regex: regex::Regex::new("secret").ok(),
            risk_level: RiskLevel::High,
            action: AuditAction::Redact,
            enabled: true,
        },
        CompiledCustomRule {
            id: "block".into(),
            name: "block".into(),
            rule_type: "keyword".into(),
            pattern: "secret".into(),
            regex: None,
            risk_level: RiskLevel::High,
            action: AuditAction::Block,
            enabled: true,
        },
    ];
    let mut body = json!({"prompt": "secret"});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(result.blocked);
    assert_eq!(body["prompt"], "secret", "block must not partially rewrite");
}

#[test]
fn test_custom_allow_passes_without_rewriting_or_blocking() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};

    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(CompiledCustomRule {
        id: "allow".into(),
        name: "allow".into(),
        rule_type: "keyword".into(),
        pattern: "secret".into(),
        regex: None,
        risk_level: RiskLevel::High,
        action: AuditAction::Allow,
        enabled: true,
    });

    let mut body = json!({"prompt": "secret"});
    let original = body.clone();
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);

    assert!(!result.blocked);
    assert_eq!(body, original);
    assert_eq!(result.findings.len(), 1);
    assert_eq!(result.findings[0].action, AuditAction::Allow);
}

#[tokio::test]
async fn test_mut_intercept_redacts_by_default() {
    use crate::app_config::AppType;
    let mut body = json!({"messages": [{"role":"user", "content":"ghp_111122223333444455556666777788889999"}]});
    let response =
        super::inspect_and_intercept_mut(&AppType::Claude, &mut body).expect("inspection");
    assert!(response.is_none());
    assert!(!body
        .to_string()
        .contains("ghp_111122223333444455556666777788889999"));
}

#[test]
fn test_unicode_keyword_redact_multiple_matches_safely() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(CompiledCustomRule {
        id: "unicode".into(),
        name: "unicode".into(),
        rule_type: "keyword".into(),
        pattern: "İ".into(),
        regex: None,
        risk_level: RiskLevel::High,
        action: AuditAction::Redact,
        enabled: true,
    });
    let mut body = json!({"prompt": "前İ后İ终"});
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(!result.blocked);
    assert_eq!(body["prompt"], "前[REDACTED]后[REDACTED]终");
}

#[test]
fn test_redaction_verification_failure_blocks_without_partial_commit() {
    use super::runtime::{CompiledCustomRule, SecurityRuntimeConfig};
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(CompiledCustomRule {
        id: "any".into(),
        name: "any".into(),
        rule_type: "regex".into(),
        pattern: ".".into(),
        regex: regex::Regex::new(".").ok(),
        risk_level: RiskLevel::High,
        action: AuditAction::Redact,
        enabled: true,
    });
    let mut body = json!({"messages": [{"role": "user", "content": "private"}]});
    let original = body.clone();
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(
        result.blocked,
        "replacement remains detectable, so fail closed"
    );
    assert_eq!(
        result.block_reason.as_deref(),
        Some("敏感信息自动隐藏未通过安全检查，请求已拦截")
    );
    assert_eq!(body, original);
}

#[test]
fn test_builtin_override_actions_match_redaction_and_block_policy() {
    use std::collections::HashMap;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.builtin_action_overrides = HashMap::from([
        ("github_token".to_string(), AuditAction::Redact),
        ("chinese_phone_number".to_string(), AuditAction::Block),
    ]);
    let token = "gh".to_owned() + "p_" + &"a".repeat(36);
    let mut body =
        json!({"messages": [{"role": "user", "content": format!("{token} 13800138000")}]});
    let original = body.clone();
    let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
    assert!(result.blocked, "explicit medium block must block");
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "github_token")
            .unwrap()
            .action,
        AuditAction::Redact
    );
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "chinese_phone_number")
            .unwrap()
            .action,
        AuditAction::Block
    );
    assert_eq!(
        body, original,
        "do not partially redact when another rule blocks"
    );
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(result.blocked);
}

#[test]
fn builtin_rules_use_independent_actions_regardless_of_risk() {
    let mut runtime = SecurityRuntimeConfig::default();
    let expected_defaults = [
        ("rsa_ssh_private_key", AuditAction::Redact),
        ("github_token", AuditAction::Redact),
        ("aws_access_key", AuditAction::Redact),
        ("openai_api_key", AuditAction::Redact),
        ("anthropic_api_key", AuditAction::Redact),
        ("db_connection_uri", AuditAction::Redact),
        ("rfc1918_private_ip", AuditAction::Audit),
    ];
    for (rule_id, expected_action) in expected_defaults {
        assert_eq!(
            super::builtin_rules::BUILTIN_RULES
                .iter()
                .find(|rule| rule.id == rule_id)
                .map(|rule| rule.default_action),
            Some(expected_action),
            "unexpected default action for {rule_id}",
        );
    }
    let token = format!("{}{}", concat!("gh", "p_"), "a".repeat(36));
    let body = json!({"messages": [{"role": "user", "content": format!("{token} 13800138000 192.168.1.10")} ]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(!result.blocked);
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "github_token")
            .map(|f| f.action),
        Some(AuditAction::Redact)
    );
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "chinese_phone_number")
            .map(|f| f.action),
        Some(AuditAction::Redact)
    );
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "rfc1918_private_ip")
            .map(|f| f.action),
        Some(AuditAction::Audit)
    );

    runtime
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), AuditAction::Block);
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime);
    assert!(
        result.blocked,
        "per-rule block must work for medium severity"
    );
    assert_eq!(
        result
            .findings
            .iter()
            .find(|f| f.rule_id == "github_token")
            .map(|f| f.action),
        Some(AuditAction::Redact)
    );
}

#[tokio::test]
async fn read_only_intercept_blocks_if_redaction_would_be_required() {
    let token = format!("{}{}", concat!("gh", "p_"), "a".repeat(36));
    let body = json!({"messages": [{"role": "user", "content": token}]});
    let response =
        super::inspect_and_intercept(&crate::app_config::AppType::Claude, &body).unwrap();
    assert_eq!(
        response.map(|r| r.status()),
        Some(axum::http::StatusCode::BAD_REQUEST)
    );
}

#[test]
fn content_dedup_distinguishes_colliding_masks_and_private_keys() {
    let config = SecurityRuntimeConfig::default();
    let body = json!({"messages": [{"role": "user", "content":
        "ghp_abcdef00000000000000000000000000abcd ghp_abcdef11111111111111111111111111abcd ghp_abcdef00000000000000000000000000abcd"}]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &config);
    let tokens: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.rule_id == "github_token")
        .collect();
    assert_eq!(
        tokens.len(),
        2,
        "Different secrets with equal masks must remain distinct"
    );
    let body = json!({"messages": [{"role": "user", "content":
        "-----BEGIN RSA PRIVATE KEY-----\nAAA\n-----END RSA PRIVATE KEY-----\n-----BEGIN RSA PRIVATE KEY-----\nBBB\n-----END RSA PRIVATE KEY-----"}]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&body, &config);
    assert_eq!(
        result
            .findings
            .iter()
            .filter(|f| f.rule_id == "rsa_ssh_private_key")
            .count(),
        2
    );
}

#[test]
fn content_identity_is_internal_and_normalized_without_context() {
    let config = SecurityRuntimeConfig::default();
    let one = SecurityAuditEngine::inspect_text_with_runtime(
        "before ghp_abcdef0123456789abcdef0123456789abcd after",
        "a",
        &config,
    );
    let two = SecurityAuditEngine::inspect_text_with_runtime(
        "ghp\\_abcdef0123456789abcdef0123456789abcd",
        "b",
        &config,
    );
    let first = one.iter().find(|f| f.rule_id == "github_token").unwrap();
    let second = two.iter().find(|f| f.rule_id == "github_token").unwrap();
    assert_eq!(first.identity, second.identity);
    assert!(first.identity.is_some());
    let json = serde_json::to_value(first).unwrap();
    assert!(json.get("identity").is_none());
    assert!(!format!("{first:?}").contains("abcdef0123456789"));
    let incomplete = json!({"messages": [{"role": "user", "content": "-----BEGIN RSA PRIVATE KEY-----\nAAA"},
        {"role": "assistant", "content": "-----BEGIN RSA PRIVATE KEY-----\nBBB"}]});
    let result = SecurityAuditEngine::inspect_request_with_runtime(&incomplete, &config);
    let keys: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.rule_id == "rsa_ssh_private_key")
        .collect();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|f| f.identity.is_none()));
}

#[test]
fn audit_session_source_uses_client_ids_and_rejects_generated_or_invalid_ids() {
    use crate::app_config::AppType;
    use axum::http::HeaderMap;
    let mut headers = HeaderMap::new();
    headers.insert(
        "session_id",
        "11111111-2222-3333-4444-555555555555".parse().unwrap(),
    );
    assert_eq!(
        super::audit_session_id(&AppType::Codex, &headers, &json!({})).as_deref(),
        Some("codex_11111111-2222-3333-4444-555555555555")
    );
    let empty = HeaderMap::new();
    assert_eq!(
        super::audit_session_id(
            &AppType::Claude,
            &empty,
            &json!({"metadata":{"user_id":"user_device_session_session-a"}})
        )
        .as_deref(),
        Some("session-a")
    );
    headers.insert("x-claude-code-session-id", "  session-b  ".parse().unwrap());
    assert_eq!(
        super::audit_session_id(
            &AppType::Claude,
            &headers,
            &json!({"metadata":{"session_id":"session-a"}})
        )
        .as_deref(),
        Some("session-b")
    );
    assert_eq!(
        super::audit_session_id(&AppType::ClaudeDesktop, &headers, &json!({})).as_deref(),
        Some("session-b")
    );
    headers.insert(
        "x-grok-conv-id",
        "11111111-2222-3333-4444-555555555555".parse().unwrap(),
    );
    assert_eq!(
        super::audit_session_id(&AppType::GrokBuild, &headers, &json!({})).as_deref(),
        Some("grokbuild_11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(
        super::audit_session_id(
            &AppType::Gemini,
            &empty,
            &json!({"metadata":{"session_id":"session-a"}})
        )
        .as_deref(),
        Some("session-a")
    );
    for body in [
        json!({}),
        json!({"previous_response_id":"response-123"}),
        json!({"metadata":{"session_id":"   "}}),
        json!({"metadata":{"session_id":"invalid\nsession"}}),
        json!({"metadata":{"session_id":"x".repeat(257)}}),
    ] {
        assert_eq!(
            super::audit_session_id(&AppType::Codex, &empty, &body),
            None
        );
    }
}

#[test]
fn audit_session_recognizes_current_codex_thread_headers_without_using_cache_keys() {
    use crate::app_config::AppType;
    use axum::http::{HeaderMap, HeaderValue};
    let body = json!({"input":"Hello", "prompt_cache_key":"shared-cache-key"});
    for header_name in ["thread-id", "session-id"] {
        let mut headers = HeaderMap::new();
        headers.insert(
            header_name,
            HeaderValue::from_static("11111111-2222-3333-4444-555555555555"),
        );
        assert_eq!(
            super::audit_session_id(&AppType::Codex, &headers, &body).as_deref(),
            Some("codex_11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(
            super::audit_session_id(&AppType::GrokBuild, &headers, &body),
            None
        );
    }
    assert_eq!(
        super::audit_session_id(&AppType::Codex, &HeaderMap::new(), &body),
        None
    );
}

#[test]
fn audit_session_workbuddy_uses_valid_native_conversations_and_rejects_invalid_ids() {
    use crate::app_config::AppType;
    use axum::http::HeaderMap;
    let body = json!({"messages":[{"role":"user","content":"Hello"}]});
    let mut headers = HeaderMap::new();
    for value in [
        "native-session-a",
        "native-session-b",
        "  native-session-a  ",
    ] {
        headers.insert("x-conversation-id", value.parse().expect("conversation"));
        let expected = format!("workbuddy_{}", value.trim());
        assert_eq!(
            super::audit_session_id(&AppType::WorkBuddy, &headers, &body).as_deref(),
            Some(expected.as_str())
        );
    }
    for value in [
        "".to_string(),
        "   ".to_string(),
        "invalid session".to_string(),
        "x".repeat(247),
    ] {
        headers.insert("x-conversation-id", value.parse().expect("header"));
        assert_eq!(
            super::audit_session_id(&AppType::WorkBuddy, &headers, &body),
            None
        );
    }
    headers.clear();
    assert_eq!(
        super::audit_session_id(&AppType::WorkBuddy, &headers, &body),
        None
    );

    let native_id = "x".repeat(246);
    let body = json!({"metadata":{"session_id":native_id},"messages":[]});
    let original_body = body.clone();
    let expected = format!("workbuddy_{native_id}");
    assert_eq!(expected.len(), 256);
    assert_eq!(
        super::audit_session_id(&AppType::WorkBuddy, &headers, &body).as_deref(),
        Some(expected.as_str())
    );
    headers.insert(
        "x-conversation-id",
        native_id.parse().expect("conversation"),
    );
    let original_headers = headers.clone();
    assert_eq!(
        super::audit_session_id(&AppType::WorkBuddy, &headers, &body).as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(headers, original_headers);
    assert_eq!(body, original_body);
}

// Qoder CN 0.4.8 repeats prompt text in its native context and message blocks.
fn qoder_request_with_copies(text: &str) -> serde_json::Value {
    json!({
        "session_id": "synthetic-qoder-session",
        "model_config": {"key":"synthetic-model","source":"system"},
        "chat_context": {"text":text,"extra":{"originalContent":text,"keep":true}},
        "messages": [
            {"role":"user","content":text,"contents":[{"type":"text","text":text}]},
            {"role":"assistant","contents":[{"type":"text","text":text}]}
        ],
        "parameters": {"temperature":0.2,"max_tokens":1024},
        "tools": [{"name":"synthetic_tool","parameters":{"type":"object"}}],
        "future_field": {"keep":"unchanged"}
    })
}

#[test]
fn qoder_native_prompt_copies_follow_each_selected_rule_action() {
    let samples = [
        (
            "rsa_ssh_private_key",
            "-----BEGIN RSA PRIVATE KEY-----\n1234556\n-----END RSA PRIVATE KEY-----",
        ),
        ("chinese_phone_number", "13800138000"),
        ("chinese_id_card", "11010519491231002X"),
        ("rfc1918_private_ip", "10.123.45.67"),
    ];
    let pointers = [
        "/chat_context/text",
        "/chat_context/extra/originalContent",
        "/messages/0/content",
        "/messages/0/contents/0/text",
        "/messages/1/contents/0/text",
    ];
    for (rule, sample) in samples {
        for action in [
            AuditAction::Block,
            AuditAction::Redact,
            AuditAction::Audit,
            AuditAction::Allow,
        ] {
            let mut runtime = SecurityRuntimeConfig::default();
            runtime.builtin_action_overrides.insert(rule.into(), action);
            let mut body = qoder_request_with_copies(sample);
            let original = body.clone();
            let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
            assert_eq!(
                result.blocked,
                action == AuditAction::Block,
                "{rule} {action}"
            );
            assert!(
                result
                    .findings
                    .iter()
                    .any(|f| f.rule_id == rule && f.action == action),
                "{rule} {action}"
            );
            if action == AuditAction::Redact {
                for pointer in pointers {
                    assert_ne!(
                        body.pointer(pointer),
                        original.pointer(pointer),
                        "unredacted Qoder copy: {rule} {pointer}"
                    );
                }
                let mut expected = original.clone();
                for pointer in pointers {
                    *expected.pointer_mut(pointer).expect("fixture pointer") =
                        body.pointer(pointer).expect("fixture pointer").clone();
                }
                assert_eq!(body, expected, "only prompt leaves may change");
                assert!(
                    SecurityAuditEngine::inspect_request_with_runtime(&body, &runtime)
                        .findings
                        .is_empty()
                );
            } else {
                assert_eq!(body, original, "{rule} {action} must preserve the payload");
            }
        }
    }
}

#[test]
fn qoder_native_text_extraction_is_read_only_and_includes_contents_only_messages() {
    let body = qoder_request_with_copies("hello");
    let original = body.clone();
    let nodes = super::extractor::extract_payload_texts(&body);
    for location in [
        "chat_context.text",
        "chat_context.extra.originalContent",
        "messages[0].content",
        "messages[0].contents[0].text",
        "messages[1].contents[0].text",
    ] {
        assert!(
            nodes
                .iter()
                .any(|node| node.location == location && node.text == "hello"),
            "missing native prompt location: {location}"
        );
    }
    assert_eq!(body, original);
}

#[test]
fn qoder_native_fields_do_not_expand_inspection_for_other_protocols() {
    let mut body = qoder_request_with_copies("hello");
    body.as_object_mut()
        .expect("fixture")
        .remove("model_config");
    let nodes = super::extractor::extract_payload_texts(&body);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].location, "messages[0].content");
}

#[test]
fn native_agent_audit_sessions_have_independent_prefixes_without_fabricated_ids() {
    use crate::app_config::AppType;
    use axum::http::HeaderMap;
    let mut headers = HeaderMap::new();
    headers.insert("x-session-id", "native-session".parse().expect("header"));
    assert_eq!(
        super::audit_session_id(&AppType::ZCode, &headers, &json!({})).as_deref(),
        Some("zcode_native-session")
    );
    for (app, prefix) in [
        (AppType::Kimi, "kimi"),
        (AppType::Qwen, "qwen"),
        (AppType::ZCode, "zcode"),
    ] {
        let body = json!({"metadata":{"session_id":"native-session"}});
        assert_eq!(
            super::audit_session_id(&app, &HeaderMap::new(), &body),
            Some(format!("{prefix}_native-session"))
        );
        assert!(super::audit_session_id(
            &app,
            &HeaderMap::new(),
            &json!({"request_id":"request-a"})
        )
        .is_none());
    }
}

#[test]
fn repeating_a_secret_in_the_current_turn_still_produces_a_current_finding() {
    let secret = format!("ghp_{}", "a".repeat(36));
    let body = json!({"messages": [{"role":"user","content":secret}, {"role":"assistant","content":"ok"}, {"role":"user","content":secret}]});
    let result = SecurityAuditEngine::inspect_request(&body);
    let finding = result
        .findings
        .iter()
        .find(|finding| finding.rule_id == "github_token")
        .expect("current token");
    assert_eq!(finding.location, "messages[2].content");
}

#[test]
fn clean_followup_redacts_history_without_reporting_it_as_current_content() {
    use super::extractor::CurrentTurn;
    let secret = format!("ghp_{}", "a".repeat(36));
    for mut body in [
        json!({"messages": [{"role":"user","content":secret}, {"role":"assistant","content":"ok"}, {"role":"user","content":"你好呀"}]}),
        json!({"input": [{"role":"user","content":secret}, {"role":"assistant","content":"ok"}, {"role":"user","content":"你好呀"}]}),
        json!({"contents": [{"role":"user","parts":[{"text":secret}]}, {"role":"model","parts":[{"text":"ok"}]}, {"role":"user","parts":[{"text":"你好呀"}]}]}),
    ] {
        let turn = CurrentTurn::new(&body);
        let result = SecurityAuditEngine::inspect_and_redact_with_runtime(
            &mut body,
            &SecurityRuntimeConfig::default(),
        );
        assert!(!result.blocked);
        assert!(!body.to_string().contains(&secret));
        assert!(!result.findings.is_empty());
        assert!(result
            .findings
            .iter()
            .all(|finding| turn.is_historical(&finding.location)));
    }
    // Responses delta requests may contain only a fresh tool result, with no user role.
    let delta =
        json!({"input":[{"type":"function_call_output","call_id":"call-1","output":secret}]});
    let turn = CurrentTurn::new(&delta);
    let result = SecurityAuditEngine::inspect_request(&delta);
    assert!(result
        .findings
        .iter()
        .any(|finding| !turn.is_historical(&finding.location)));
}

#[test]
fn current_identity_card_is_not_reported_as_a_historical_phone_number() {
    use super::extractor::CurrentTurn;
    let phone = ["138001", "38000"].concat();
    let id = ["110105", "19491231", "002X"].concat();
    let body = json!({"input": [
        {"role":"user","content":format!("synthetic phone: {phone}")},
        {"role":"assistant","content":"ok"},
        {"role":"user","content":format!("synthetic ID: {id}")}
    ]});
    let turn = CurrentTurn::new(&body);
    let result =
        SecurityAuditEngine::inspect_request_with_runtime(&body, &SecurityRuntimeConfig::default());
    let current: Vec<_> = result
        .findings
        .iter()
        .filter(|finding| !turn.is_historical(&finding.location))
        .map(|finding| finding.rule_id.as_str())
        .collect();
    assert_eq!(current, ["chinese_id_card"]);
}

#[test]
fn current_structured_password_does_not_use_the_historical_assignment_rule() {
    let body = json!({"messages": [
        {"role":"user", "content":"password = synthetic-old-password"},
        {"role":"assistant", "content":"ok"},
        {"role":"user", "content":"Use the login tool"},
        {"role":"assistant", "content":[{"type":"tool_use", "name":"login", "input":{"password":"synthetic-new-password"}}]}
    ]});
    let turn = super::extractor::CurrentTurn::new(&body);
    let result =
        SecurityAuditEngine::inspect_request_with_runtime(&body, &SecurityRuntimeConfig::default());
    let current: Vec<_> = result
        .findings
        .iter()
        .filter(|finding| !turn.is_historical(&finding.location))
        .map(|finding| finding.rule_id.as_str())
        .collect();
    assert_eq!(current, ["catalog:password-field"]);
}

#[test]
fn json_pasted_as_chat_text_uses_password_assignment_instead_of_structured_field() {
    let body = json!({"input":[{"role":"user", "content":json!({
        "username":"synthetic-user", "password":"synthetic-password"
    }).to_string()}]});
    let result =
        SecurityAuditEngine::inspect_request_with_runtime(&body, &SecurityRuntimeConfig::default());
    let rules: Vec<_> = result
        .findings
        .iter()
        .map(|finding| finding.rule_id.as_str())
        .collect();
    assert_eq!(rules, ["catalog:password-assignment"]);
}

#[test]
fn private_key_alert_and_hide_have_distinct_wire_behavior_but_mask_both_audits() {
    for kind in ["RSA", "OPENSSH", "EC", "DSA"] {
        let key = format!(
            "-----BEGIN {kind} PRIVATE KEY-----\nSYNTHETIC_KEY_BODY\n-----END {kind} PRIVATE KEY-----"
        );
        for action in [AuditAction::Audit, AuditAction::Redact] {
            let mut runtime = SecurityRuntimeConfig::default();
            runtime
                .builtin_action_overrides
                .insert("rsa_ssh_private_key".into(), action);
            let original = json!({"messages":[{"role":"user", "content":key}], "stream":true});
            let mut body = original.clone();
            let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
            assert!(!result.blocked);
            let finding = result
                .findings
                .iter()
                .find(|finding| finding.rule_id == "rsa_ssh_private_key")
                .expect("synthetic private key");
            assert_eq!(finding.action, action);
            assert!(!finding
                .matched_snippet_masked
                .contains("SYNTHETIC_KEY_BODY"));
            if action == AuditAction::Audit {
                assert_eq!(body, original);
            } else {
                assert_eq!(body["messages"][0]["content"], "[REDACTED_PRIVATE_KEY]");
                assert_eq!(body["stream"], true);
            }
        }
    }
}

#[test]
fn reported_token_rules_detect_valid_samples_with_the_configured_action() {
    let cases = [
        (
            "catalog:pypi-token",
            format!("pypi-AgEIcHlwaS5vcmc{}", "a".repeat(50)),
        ),
        ("catalog:bedrock-key", format!("ABSK{}", "a".repeat(109))),
        ("catalog:google-api-key", format!("AIza{}", "A".repeat(35))),
        ("catalog:sendgrid-token", format!("SG.{}", "a".repeat(66))),
        (
            "catalog:digitalocean-pat",
            format!("dop_v1_{}", "f".repeat(64)),
        ),
        (
            "catalog:huggingface-token",
            format!("hf_{}", "a".repeat(34)),
        ),
        (
            "catalog:notion-token",
            format!("ntn_12345678901{}000", "A".repeat(32)),
        ),
        ("catalog:shopify-token", format!("shpat_{}", "a".repeat(32))),
        (
            "catalog:cloudflare-origin-key",
            format!("v1.0-{}-{}", "a".repeat(24), "b".repeat(146)),
        ),
        ("catalog:openrouter-key", "v1-openrouter-synthetic".into()),
        ("catalog:groq-key", format!("gsk_{}", "a".repeat(52))),
        ("catalog:perplexity-key", format!("pplx-{}", "a".repeat(48))),
    ];
    for (rule, token) in cases {
        for action in [AuditAction::Audit, AuditAction::Redact] {
            let mut runtime = SecurityRuntimeConfig::default();
            runtime.builtin_action_overrides.insert(rule.into(), action);
            let original = json!({"input":[{"role":"user","content":token}],"temperature":0.2,"future_option":{"keep":true}});
            let mut body = original.clone();
            let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &runtime);
            assert!(!result.blocked, "{rule}");
            let finding = result
                .findings
                .iter()
                .find(|finding| finding.rule_id == rule)
                .expect("valid synthetic token");
            assert_eq!(finding.action, action, "{rule}");
            assert!(!finding.matched_snippet_masked.contains(&token), "{rule}");
            if action == AuditAction::Audit {
                assert_eq!(body, original, "{rule}");
            } else {
                assert!(!body.to_string().contains(&token), "{rule}");
                assert_eq!(body["future_option"], original["future_option"]);
            }
        }
    }
}
