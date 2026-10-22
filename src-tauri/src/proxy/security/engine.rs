use serde_json::Value;
use std::borrow::Cow;

use super::builtin_rules::{BuiltinRule, BUILTIN_RULES, GLOBAL_AHO_CORASICK};
use super::extractor::extract_payload_texts;
use super::pii::{scan_chinese_id_cards, scan_chinese_phone_numbers};
use super::runtime::{get_security_runtime, SecurityRuntimeConfig};
use super::types::{AuditAction, AuditFinding, RiskLevel, RuleCategory, SecurityInspectionResult};

/// Security inspection engine.
pub struct SecurityAuditEngine;

impl SecurityAuditEngine {
    /// Inspect supported text payloads extracted from a JSON request body.
    pub fn inspect_request(body: &Value) -> SecurityInspectionResult {
        let runtime = get_security_runtime();
        Self::inspect_request_with_runtime(body, &runtime)
    }

    /// Inspect request payloads with the supplied runtime configuration.
    pub fn inspect_request_with_runtime(
        body: &Value,
        runtime: &SecurityRuntimeConfig,
    ) -> SecurityInspectionResult {
        if !runtime.enabled {
            return SecurityInspectionResult::pass();
        }

        let text_nodes = extract_payload_texts(body);
        if text_nodes.is_empty() {
            return SecurityInspectionResult::pass();
        }

        let mut all_findings = Vec::new();

        for node in text_nodes {
            let mut findings = Self::inspect_text_with_context(
                node.text,
                &node.location,
                node.is_password_field,
                runtime,
            );
            all_findings.append(&mut findings);
        }

        if all_findings.is_empty() {
            return SecurityInspectionResult::pass();
        }

        // Retain the first hit for each rule/content pair within this request.
        let mut seen = std::collections::HashSet::new();
        all_findings.retain(|f| {
            let Some(identity) = f.identity else {
                return true;
            };
            seen.insert((f.rule_id.clone(), identity))
        });

        // Compute the highest risk and whether the request must be blocked.
        let mut highest_risk: Option<RiskLevel> = None;
        let mut blocked = false;
        let mut block_rules = Vec::new();
        let mut seen_block_rules = std::collections::HashSet::new();

        for finding in &all_findings {
            match highest_risk {
                Some(current) => {
                    if finding.risk_level > current {
                        highest_risk = Some(finding.risk_level);
                    }
                }
                None => {
                    highest_risk = Some(finding.risk_level);
                }
            }

            if finding.action == AuditAction::Block {
                blocked = true;
                if seen_block_rules.insert(finding.rule_id.as_str()) {
                    // Custom rule names are user input: never include them in client errors or proxy warnings.
                    let safe_name = if finding.category == RuleCategory::Custom {
                        "自定义规则"
                    } else {
                        finding.rule_name.as_str()
                    };
                    block_rules.push(format!("{safe_name}（{}）", finding.risk_level.as_str()));
                }
            }
        }

        let block_reason = if blocked {
            let rules_list = block_rules
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("、");
            Some(format!(
                "检测到 {} 项敏感数据：{}。",
                block_rules.len(),
                rules_list
            ))
        } else {
            None
        };

        SecurityInspectionResult {
            blocked,
            findings: all_findings,
            highest_risk,
            block_reason,
        }
    }

    /// Inspect text with the current runtime configuration.
    pub fn inspect_text(text: &str, location: &str) -> Vec<AuditFinding> {
        let runtime = get_security_runtime();
        Self::inspect_text_with_runtime(text, location, &runtime)
    }

    /// Inspect text with an explicit runtime configuration.
    pub fn inspect_text_with_runtime(
        text: &str,
        location: &str,
        runtime: &SecurityRuntimeConfig,
    ) -> Vec<AuditFinding> {
        Self::inspect_text_with_context(text, location, false, runtime)
    }

    fn inspect_text_with_context(
        text: &str,
        location: &str,
        is_password_field: bool,
        runtime: &SecurityRuntimeConfig,
    ) -> Vec<AuditFinding> {
        let mut findings = Vec::new();
        if text.is_empty() {
            return findings;
        }

        // Normalize escaped underscores before matching.
        // Some clients escape underscores as \_, which would otherwise defeat matching.
        // Allocate a replacement only when the escaped sequence is present.
        let unescaped_text: Cow<'_, str> = if text.contains("\\_") {
            Cow::Owned(text.replace("\\_", "_"))
        } else {
            Cow::Borrowed(text)
        };
        let text: &str = &unescaped_text;

        // Phase 1: Aho-Corasick candidate matching.
        let has_pattern_candidate = GLOBAL_AHO_CORASICK
            .as_ref()
            .is_some_and(|matcher| matcher.is_match(text));

        // Run detailed expressions only for candidate rules.
        if has_pattern_candidate {
            let text_lower = text.to_ascii_lowercase();
            for rule in BUILTIN_RULES.iter() {
                if rule.id == "catalog:password-field" {
                    continue;
                }
                // Reuse one lowercase view for keyword checks instead of allocating per rule.
                let matches_trigger = rule
                    .trigger_keywords
                    .iter()
                    .any(|&kw| text.contains(kw) || text_lower.contains(&kw.to_ascii_lowercase()));

                if matches_trigger {
                    let Some(regex) = rule.regex.as_ref() else {
                        continue;
                    };
                    for captures in regex.captures_iter(text) {
                        let Some(mat) = sensitive_match(rule, &captures) else {
                            continue;
                        };
                        let matched_raw = mat.as_str();
                        if is_redacted_placeholder(matched_raw) {
                            continue;
                        }
                        if rule.allowlisted_values.contains(&matched_raw) {
                            continue;
                        }
                        let masked = mask_sensitive_value(matched_raw, &rule.category);
                        findings.push(AuditFinding {
                            identity: builtin_fingerprint(text, mat, rule),
                            rule_id: rule.id.to_string(),
                            rule_name: rule.name.to_string(),
                            category: rule.category.clone(),
                            risk_level: rule.risk_level,
                            action: builtin_action(rule, runtime),
                            matched_snippet_masked: masked,
                            location: location.to_string(),
                        });
                    }
                }
            }
        }

        // Leaf strings alone cannot identify password/passwd/pwd field names.
        // The extractor supplies field context for the password-field detector.
        if is_password_field && !is_redacted_placeholder(text) {
            if let Some(rule) = BUILTIN_RULES
                .iter()
                .find(|rule| rule.id == "catalog:password-field")
            {
                findings.push(AuditFinding {
                    identity: super::identity::fingerprint(text),
                    rule_id: rule.id.to_string(),
                    rule_name: rule.name.to_string(),
                    category: rule.category.clone(),
                    risk_level: rule.risk_level,
                    action: builtin_action(rule, runtime),
                    matched_snippet_masked: mask_sensitive_value(text, &rule.category),
                    location: location.to_string(),
                });
            }
        }

        // Phase 2: algorithmic PII validation.
        // Mainland China 18-digit identity numbers with the GB 11643 checksum.
        let id_cards = scan_chinese_id_cards(text);
        for (_start, _end, id_num) in id_cards {
            let masked = mask_chinese_id_card(&id_num);
            findings.push(AuditFinding {
                identity: super::identity::fingerprint(&id_num),
                rule_id: "chinese_id_card".to_string(),
                rule_name: "中国大陆居民身份证号".to_string(),
                category: RuleCategory::Pii,
                risk_level: RiskLevel::Medium,
                action: runtime
                    .builtin_action_overrides
                    .get("chinese_id_card")
                    .copied()
                    .unwrap_or(AuditAction::Redact),
                matched_snippet_masked: masked,
                location: location.to_string(),
            });
        }

        // Mainland China 11-digit mobile numbers.
        let phone_nums = scan_chinese_phone_numbers(text);
        for (_start, _end, phone) in phone_nums {
            let masked = mask_phone_number(&phone);
            findings.push(AuditFinding {
                identity: super::identity::fingerprint(&phone),
                rule_id: "chinese_phone_number".to_string(),
                rule_name: "中国大陆手机号码".to_string(),
                category: RuleCategory::Pii,
                risk_level: RiskLevel::Medium,
                action: runtime
                    .builtin_action_overrides
                    .get("chinese_phone_number")
                    .copied()
                    .unwrap_or(AuditAction::Redact),
                matched_snippet_masked: masked,
                location: location.to_string(),
            });
        }

        // Phase 3: custom rules.
        for rule in &runtime.custom_rules {
            if !rule.enabled {
                continue;
            }
            if rule.rule_type == "regex" {
                if let Some(ref re) = rule.regex {
                    for mat in re.find_iter(text) {
                        let matched_raw = mat.as_str();
                        let masked = mask_sensitive_value(matched_raw, &RuleCategory::Custom);
                        findings.push(AuditFinding {
                            identity: super::identity::fingerprint(matched_raw),
                            rule_id: rule.id.clone(),
                            rule_name: rule.name.clone(),
                            category: RuleCategory::Custom,
                            risk_level: rule.risk_level,
                            action: rule.action,
                            matched_snippet_masked: masked,
                            location: location.to_string(),
                        });
                    }
                }
            } else if rule.rule_type == "keyword" {
                let pat = &rule.pattern;
                if !pat.is_empty() {
                    if let Ok(re) = regex::RegexBuilder::new(&regex::escape(pat))
                        .case_insensitive(true)
                        .build()
                    {
                        for mat in re.find_iter(text) {
                            findings.push(AuditFinding {
                                identity: super::identity::fingerprint(pat),
                                rule_id: rule.id.clone(),
                                rule_name: rule.name.clone(),
                                category: RuleCategory::Custom,
                                risk_level: rule.risk_level,
                                action: rule.action,
                                matched_snippet_masked: mask_sensitive_value(
                                    mat.as_str(),
                                    &RuleCategory::Custom,
                                ),
                                location: location.to_string(),
                            });
                        }
                    }
                }
            }
        }

        findings
    }
}

fn builtin_action(rule: &BuiltinRule, runtime: &SecurityRuntimeConfig) -> AuditAction {
    let action = runtime
        .builtin_action_overrides
        .get(rule.id)
        .copied()
        .unwrap_or(rule.default_action);

    if rule.id == "catalog:generic-sk-key-notice"
        && matches!(action, AuditAction::Block | AuditAction::Redact)
    {
        return rule.default_action;
    }
    action
}

fn sensitive_match<'a>(
    rule: &BuiltinRule,
    captures: &regex::Captures<'a>,
) -> Option<regex::Match<'a>> {
    captures.get(rule.capture_group.unwrap_or(0))
}

fn is_redacted_placeholder(value: &str) -> bool {
    matches!(
        value.trim(),
        "[REDACTED]" | "[REDACTED_PASSWORD]" | "[REDACTED_PRIVATE_KEY]" | "[REDACTED_DB_CONN]"
    )
}

/// Mask sensitive text; never echo its plaintext into logs or block messages.
pub fn mask_sensitive_value(val: &str, category: &RuleCategory) -> String {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match category {
        RuleCategory::PrivateKey => {
            if trimmed.starts_with("-----BEGIN") && trimmed.ends_with("-----") {
                let prefix_end = trimmed
                    .char_indices()
                    .nth(20)
                    .map(|(idx, _)| idx)
                    .unwrap_or(trimmed.len());
                format!(
                    "{}...[REDACTED_PRIVATE_KEY]...-----",
                    &trimmed[..prefix_end]
                )
            } else {
                "[REDACTED_PRIVATE_KEY]".to_string()
            }
        }
        RuleCategory::DbConnection => {
            // Replace only the password component of a database connection string.
            if let Some(at_idx) = trimmed.rfind('@') {
                let user_slice = &trimmed[..at_idx];
                if let Some(scheme_end) = user_slice.find("://") {
                    if let Some(colon_offset) = user_slice[scheme_end + 3..].find(':') {
                        let colon_idx = scheme_end + 3 + colon_offset;
                        return format!("{}:****{}", &trimmed[..colon_idx], &trimmed[at_idx..]);
                    }
                } else if let Some(colon_idx) = user_slice.find(':') {
                    return format!("{}:****{}", &trimmed[..colon_idx], &trimmed[at_idx..]);
                }
            }
            "[REDACTED_DB_CONN]".to_string()
        }
        RuleCategory::Password => "[REDACTED_PASSWORD]".to_string(),
        RuleCategory::ApiToken => {
            let char_count = trimmed.chars().count();
            if char_count <= 8 {
                "****".to_string()
            } else if char_count <= 16 {
                let prefix: String = trimmed.chars().take(3).collect();
                let suffix: String = trimmed
                    .chars()
                    .rev()
                    .take(2)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("{}****{}", prefix, suffix)
            } else {
                let prefix: String = trimmed.chars().take(6).collect();
                let suffix: String = trimmed
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("{}****{}", prefix, suffix)
            }
        }
        RuleCategory::Infrastructure => {
            // Mask IP addresses.
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() == 4 {
                format!("{}.{}.*.*", parts[0], parts[1])
            } else {
                "[REDACTED_IP]".to_string()
            }
        }
        RuleCategory::Pii | RuleCategory::Custom => {
            let char_count = trimmed.chars().count();
            if char_count <= 4 {
                "****".to_string()
            } else {
                let prefix: String = trimmed.chars().take(2).collect();
                let suffix: String = trimmed
                    .chars()
                    .rev()
                    .take(2)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("{}****{}", prefix, suffix)
            }
        }
    }
}

/// Keep the first six and last four identity-number digits; mask the middle eight.
fn mask_chinese_id_card(id: &str) -> String {
    if id.len() == 18 {
        format!("{}********{}", &id[..6], &id[14..])
    } else {
        "******************".to_string()
    }
}

/// Keep the first three and last four phone digits; mask the middle four.
fn mask_phone_number(phone: &str) -> String {
    if phone.len() == 11 {
        format!("{}****{}", &phone[..3], &phone[7..])
    } else {
        "***********".to_string()
    }
}

impl SecurityAuditEngine {
    /// Redact a copy and inspect again; on failure, preserve the original body and block.
    pub fn inspect_and_redact_with_runtime(
        body: &mut Value,
        runtime: &SecurityRuntimeConfig,
    ) -> SecurityInspectionResult {
        let mut result = Self::inspect_request_with_runtime(body, runtime);
        if result.blocked
            || !result
                .findings
                .iter()
                .any(|f| f.action == AuditAction::Redact)
        {
            return result;
        }
        let mut candidate = body.clone();
        // Use selected string buffer addresses to locate JSON leaves.
        // Do not parse display paths, which may contain tool names or user-defined keys.
        let extracted = extract_payload_texts(&candidate);
        let selected: std::collections::HashSet<usize> =
            extracted.iter().map(|n| n.text.as_ptr() as usize).collect();
        let password_fields: std::collections::HashSet<usize> = extracted
            .iter()
            .filter(|n| n.is_password_field)
            .map(|n| n.text.as_ptr() as usize)
            .collect();
        let mut failed = false;
        redact_selected_leaves(
            &mut candidate,
            &selected,
            &password_fields,
            runtime,
            &mut failed,
        );
        if !failed {
            let remaining = Self::inspect_request_with_runtime(&candidate, runtime);
            failed = remaining
                .findings
                .iter()
                .any(|f| f.action == AuditAction::Redact || f.action == AuditAction::Block);
        }
        if failed {
            result.blocked = true;
            result.block_reason = Some("敏感信息自动隐藏未通过安全检查，请求已拦截".to_string());
        } else {
            *body = candidate;
        }
        result
    }
}

fn redact_selected_leaves(
    value: &mut Value,
    selected: &std::collections::HashSet<usize>,
    password_fields: &std::collections::HashSet<usize>,
    runtime: &SecurityRuntimeConfig,
    failed: &mut bool,
) {
    match value {
        Value::String(text) if selected.contains(&(text.as_ptr() as usize)) => {
            let is_password_field = password_fields.contains(&(text.as_ptr() as usize));
            let findings = SecurityAuditEngine::inspect_text_with_context(
                text,
                "",
                is_password_field,
                runtime,
            );
            if !findings.iter().any(|f| f.action == AuditAction::Redact) {
                return;
            }
            // Match against the normalized spelling, but map each match back to the
            // original string: only matched bytes may change in the outbound body.
            let (normalized, offsets) = normalized_match_text(text);
            let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
            for rule in BUILTIN_RULES.iter() {
                if rule.id == "catalog:password-field" {
                    continue;
                }
                if builtin_action(rule, runtime) == AuditAction::Redact {
                    let Some(regex) = rule.regex.as_ref() else {
                        continue;
                    };
                    for captures in regex.captures_iter(&normalized) {
                        let Some(mat) = sensitive_match(rule, &captures) else {
                            continue;
                        };
                        if is_redacted_placeholder(mat.as_str()) {
                            continue;
                        }
                        if rule
                            .allowlisted_values
                            .iter()
                            .any(|allowed| *allowed == mat.as_str())
                        {
                            continue;
                        }
                        if rule.category == RuleCategory::PrivateKey {
                            let marker = mat.as_str().replacen("-----BEGIN ", "-----END ", 1);
                            // An unterminated key may contain secret bytes through the
                            // end of this text node; never remove just its BEGIN marker.
                            let end = normalized[mat.end()..]
                                .find(&marker)
                                .map(|offset| mat.end() + offset + marker.len())
                                .unwrap_or(normalized.len());
                            ranges.push((mat.start(), end, true));
                        } else {
                            ranges.push((mat.start(), mat.end(), false));
                        }
                    }
                }
            }
            if is_password_field {
                if let Some(rule) = BUILTIN_RULES
                    .iter()
                    .find(|rule| rule.id == "catalog:password-field")
                {
                    if builtin_action(rule, runtime) == AuditAction::Redact {
                        ranges.push((0, normalized.len(), false));
                    }
                }
            }
            if runtime
                .builtin_action_overrides
                .get("chinese_id_card")
                .copied()
                .unwrap_or(AuditAction::Redact)
                == AuditAction::Redact
            {
                ranges.extend(
                    scan_chinese_id_cards(&normalized)
                        .into_iter()
                        .map(|(s, e, _)| (s, e, false)),
                );
            }
            if runtime
                .builtin_action_overrides
                .get("chinese_phone_number")
                .copied()
                .unwrap_or(AuditAction::Redact)
                == AuditAction::Redact
            {
                ranges.extend(
                    scan_chinese_phone_numbers(&normalized)
                        .into_iter()
                        .map(|(s, e, _)| (s, e, false)),
                );
            }
            for rule in runtime
                .custom_rules
                .iter()
                .filter(|r| r.enabled && r.action == AuditAction::Redact)
            {
                if rule.rule_type == "regex" {
                    if let Some(re) = &rule.regex {
                        ranges.extend(
                            re.find_iter(&normalized)
                                .map(|m| (m.start(), m.end(), false)),
                        );
                    }
                } else if rule.rule_type == "keyword" && !rule.pattern.is_empty() {
                    let pattern = regex::escape(&rule.pattern);
                    match regex::RegexBuilder::new(&pattern)
                        .case_insensitive(true)
                        .build()
                    {
                        Ok(re) => ranges.extend(
                            re.find_iter(&normalized)
                                .map(|m| (m.start(), m.end(), false)),
                        ),
                        Err(_) => {
                            *failed = true;
                            return;
                        }
                    }
                }
            }
            let mut original_ranges = Vec::new();
            for (start, stop, private_key) in ranges {
                let (Some(&start), Some(&stop)) = (offsets.get(start), offsets.get(stop)) else {
                    *failed = true;
                    return;
                };
                if start >= stop || !text.is_char_boundary(start) || !text.is_char_boundary(stop) {
                    *failed = true;
                    return;
                }
                original_ranges.push((start, stop, private_key));
            }
            original_ranges.sort_unstable();
            let mut merged: Vec<(usize, usize, bool)> = Vec::new();
            for (start, stop, private_key) in original_ranges {
                if let Some(last) = merged.last_mut() {
                    if start <= last.1 {
                        last.1 = last.1.max(stop);
                        last.2 |= private_key;
                        continue;
                    }
                }
                merged.push((start, stop, private_key));
            }
            if merged.is_empty() {
                *failed = true;
                return;
            }
            let mut output = String::new();
            let mut end = 0;
            for (start, stop, private_key) in merged {
                output.push_str(&text[end..start]);
                output.push_str(if private_key {
                    "[REDACTED_PRIVATE_KEY]"
                } else {
                    "[REDACTED]"
                });
                end = stop;
            }
            output.push_str(&text[end..]);
            *text = output;
        }
        Value::Array(items) => {
            for item in items {
                redact_selected_leaves(item, selected, password_fields, runtime, failed);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_selected_leaves(item, selected, password_fields, runtime, failed);
            }
        }
        _ => {}
    }
}

/// Normalized matcher offsets mapped back to original UTF-8 byte boundaries.
/// A backslash-escaped underscore consumes two original bytes but matches one.
fn normalized_match_text(text: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(text.len());
    let mut offsets = vec![0];
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '\\' && matches!(chars.peek(), Some((_, '_'))) {
            chars.next();
            normalized.push('_');
            offsets.push(start + 2);
        } else {
            normalized.push(ch);
            for byte in 1..=ch.len_utf8() {
                offsets.push(start + byte);
            }
        }
    }
    (normalized, offsets)
}

fn builtin_fingerprint(
    text: &str,
    mat: regex::Match<'_>,
    rule: &BuiltinRule,
) -> Option<super::identity::ContentFingerprint> {
    if rule.category == RuleCategory::PrivateKey {
        let marker = mat.as_str().replacen("-----BEGIN ", "-----END ", 1);
        let tail = text.get(mat.end()..)?;
        let end = tail.find(&marker)?;
        // A second BEGIN before END makes the range ambiguous; do not aggregate.
        if tail.get(..end)?.contains("-----BEGIN ") {
            return None;
        }
        super::identity::fingerprint(text.get(mat.start()..mat.end() + end + marker.len())?)
    } else {
        super::identity::fingerprint(mat.as_str())
    }
}
