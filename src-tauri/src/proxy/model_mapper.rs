//! 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换请求中的模型名称

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use crate::provider::Provider;
use serde_json::Value;

/// 模型映射配置
pub struct ModelMapping {
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub fable_model: Option<String>,
    pub subagent_model: Option<String>,
    pub default_model: Option<String>,
    picker_models: Vec<String>,
}

impl ModelMapping {
    /// 从 Provider 配置中提取模型映射
    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");

        Self {
            picker_models: provider
                .settings_config
                .pointer("/modelPicker/options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|row| row.get("model").and_then(Value::as_str))
                .filter(|model| !model.trim().is_empty())
                .map(String::from)
                .collect(),
            haiku_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            fable_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_FABLE_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            subagent_model: env
                .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了任何模型映射
    pub fn has_mapping(&self) -> bool {
        self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.fable_model.is_some()
            || self.subagent_model.is_some()
            || self.default_model.is_some()
    }

    /// 根据原始模型名称获取映射后的模型
    pub fn map_model(&self, original_model: &str) -> String {
        // Explicit picker selections take precedence over role and fallback mappings.
        if self.picker_models.iter().any(|model| {
            strip_one_m_suffix_for_upstream(model)
                == strip_one_m_suffix_for_upstream(original_model)
        }) {
            return original_model.to_string();
        }
        let model_lower = original_model.to_lowercase();

        // 1. 按模型类型匹配
        if model_lower.contains("fable") {
            if let Some(ref m) = self.fable_model {
                return m.clone();
            }
            // 未单独配置 fable 档时归入 opus 档，与 Claude Code 官方
            // 分类器降级方向一致（fable→opus），避免落到 default 失去层级。
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("haiku") {
            if let Some(ref m) = self.haiku_model {
                return m.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref m) = self.sonnet_model {
                return m.clone();
            }
        }

        if let Some(ref m) = self.subagent_model {
            if strip_one_m_suffix_for_upstream(original_model) == strip_one_m_suffix_for_upstream(m)
            {
                return original_model.to_string();
            }
        }

        // 2. 默认模型
        if let Some(ref m) = self.default_model {
            return m.clone();
        }

        // 3. 无映射，保持原样
        original_model.to_string()
    }
}

/// Resolve the legacy mapping without copying configuration or model strings.
/// Unicode lowercase expansion is streamed; ASCII keywords need no contextual
/// non-ASCII casing state and must not allocate a lowercase scratch string.
pub(crate) fn borrowed_mapped_model<'a>(original: &'a str, provider: &'a Provider) -> &'a str {
    fn contains_lowercase(text: &str, keyword: &str) -> bool {
        let mut lower = text.chars().flat_map(char::to_lowercase);
        while let Some(first) = lower.next() {
            if first == char::from(keyword.as_bytes()[0])
                && lower
                    .clone()
                    .zip(keyword.bytes().skip(1))
                    .take_while(|(a, b)| *a == char::from(*b))
                    .count()
                    == keyword.len() - 1
            {
                return true;
            }
        }
        false
    }
    let env = provider.settings_config.get("env");
    let configured = |key: &str| {
        env.and_then(|env| env.get(key))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    if contains_lowercase(original, "fable") {
        if let Some(model) = configured("ANTHROPIC_DEFAULT_FABLE_MODEL")
            .or_else(|| configured("ANTHROPIC_DEFAULT_OPUS_MODEL"))
        {
            return model;
        }
    }
    for (keyword, key) in [
        ("haiku", "ANTHROPIC_DEFAULT_HAIKU_MODEL"),
        ("opus", "ANTHROPIC_DEFAULT_OPUS_MODEL"),
        ("sonnet", "ANTHROPIC_DEFAULT_SONNET_MODEL"),
    ] {
        if contains_lowercase(original, keyword) {
            if let Some(model) = configured(key) {
                return model;
            }
        }
    }
    if configured("CLAUDE_CODE_SUBAGENT_MODEL").is_some_and(|model| {
        strip_one_m_suffix_for_upstream(original) == strip_one_m_suffix_for_upstream(model)
    }) {
        return original;
    }
    configured("ANTHROPIC_MODEL").unwrap_or(original)
}

/// 对请求体应用模型映射
///
/// 返回 (映射后的请求体, 原始模型名, 映射后模型名)
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 如果没有配置映射，直接返回
    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 提取原始模型名
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    if let Some(ref original) = original_model {
        let mapped = mapping.map_model(original);

        if mapped != *original {
            log::debug!("[ModelMapper] 模型映射: {original} → {mapped}");
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.clone()), Some(mapped));
        }
    }

    (body, original_model, None)
}

/// Claude Code 通过 `[1M]` 后缀声明 100 万上下文能力；上游 API
/// 通常不接受这个本地能力标记，转发前需要剥离。
pub fn strip_one_m_suffix_for_upstream(model: &str) -> &str {
    let trimmed = model.trim_end();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    model
}

pub fn strip_one_m_suffix_for_upstream_from_body(mut body: Value) -> Value {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return body;
    };

    let stripped = strip_one_m_suffix_for_upstream(model);
    if stripped != model {
        log::debug!("[ModelMapper] 去除本地 1M 标记: {model} → {stripped}");
        body["model"] = serde_json::json!(stripped);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn imported_picker_models_preserve_the_selected_model_over_role_and_default_mappings() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config["modelPicker"] = json!({"options": [
            {"model": "fixture-large", "label": "Large"},
            {"model": "gateway/claude-sonnet-fixture", "label": "Custom Sonnet"}
        ]});
        for model in [
            "fixture-large",
            "gateway/claude-sonnet-fixture",
            "fixture-large[1M]",
        ] {
            let body = json!({"model": model, "temperature": 0.2});
            let (actual, _, mapped) = apply_model_mapping(body.clone(), &provider);
            assert_eq!(actual, body);
            assert_eq!(mapped, None);
        }
        let (actual, _, _) = apply_model_mapping(json!({"model": "claude-sonnet-5"}), &provider);
        assert_eq!(actual["model"], "sonnet-mapped");
        let (actual, _, _) = apply_model_mapping(json!({"model": "unknown"}), &provider);
        assert_eq!(actual["model"], "default-model");
    }

    fn create_provider_with_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped",
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-mapped"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_without_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_sonnet_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-sonnet-4-5-20250929"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(original, Some("claude-sonnet-4-5-20250929".to_string()));
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_haiku_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-haiku-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_opus_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-opus-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_fable_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "fable-mapped");
        assert_eq!(mapped, Some("fable-mapped".to_string()));
    }

    #[test]
    fn test_fable_with_one_m_suffix_mapping() {
        // Claude Code 实际会发 claude-fable-5[1m] 形态（issue #3980）
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-fable-5[1m]"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "fable-mapped");
        assert_eq!(mapped, Some("fable-mapped".to_string()));
    }

    #[test]
    fn test_fable_falls_back_to_opus_when_unset() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
            }
        });
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_fable_falls_back_to_default_without_opus() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model"
            }
        });
        let body = json!({"model": "claude-fable-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_thinking_does_not_affect_model_mapping() {
        // Issue #2081: thinking 参数不应影响模型映射
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_adaptive_does_not_affect_model_mapping() {
        // Issue #2081: adaptive thinking 也不应影响模型映射
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "adaptive"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_disabled() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "disabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_unknown_model_uses_default() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "some-unknown-model"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_subagent_model_preserved_before_default_fallback() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
            }
        });

        let body = json!({"model": "gpt-5.4-mini"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.4-mini");
        assert_eq!(original, Some("gpt-5.4-mini".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_subagent_model_preserved_with_one_m_suffix_before_default_fallback() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
            }
        });

        let body = json!({"model": "gpt-5.4-mini[1M]"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.4-mini[1M]");
        assert_eq!(original, Some("gpt-5.4-mini[1M]".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_no_mapping_configured() {
        let provider = create_provider_without_mapping();
        let body = json!({"model": "claude-sonnet-4-5"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "claude-sonnet-4-5");
        assert_eq!(original, Some("claude-sonnet-4-5".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "Claude-SONNET-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn strips_one_m_suffix_before_upstream() {
        let body = json!({"model": "deepseek-v4-pro[1M]"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn strips_one_m_suffix_after_mapping() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro [1M]"
            }
        });

        let body = json!({"model": "claude-sonnet-4-6"});
        let (mapped, _, _) = apply_model_mapping(body, &provider);
        let result = strip_one_m_suffix_for_upstream_from_body(mapped);

        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn keeps_model_without_one_m_suffix() {
        let body = json!({"model": "deepseek-v4-pro"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }
    #[test]
    fn borrowed_mapping_matches_legacy_and_borrows_the_selected_allocation() {
        let originals = [
            "claude-FABLE-haiku-opus-sonnet[1M]",
            "HAIKU",
            "SONNET",
            "OPUS",
            "FABLE",
            "fable-haiku",
            "İSONNET",
            "ΣSONNETΣ",
            "ſonnet",
            "ＳONNET",
            "soNNeTİ",
            "gpt-subagent [1m]  ",
            "unknown",
            "",
            "sonne",
            "fabl",
            "haik",
            "opu",
        ];
        let configurations = [
            json!({}),
            create_provider_with_mapping().settings_config,
            json!({"env":{"ANTHROPIC_MODEL":"default","ANTHROPIC_DEFAULT_OPUS_MODEL":"opus"}}),
            json!({"env":{"ANTHROPIC_MODEL":"default","CLAUDE_CODE_SUBAGENT_MODEL":"gpt-subagent"}}),
            json!({"env":{"ANTHROPIC_MODEL":"","ANTHROPIC_DEFAULT_HAIKU_MODEL":17,"ANTHROPIC_DEFAULT_FABLE_MODEL":""}}),
            json!({"env":{"ANTHROPIC_MODEL":"default","ANTHROPIC_DEFAULT_HAIKU_MODEL":"haiku"}}),
        ];
        for config in configurations {
            let mut provider = create_provider_without_mapping();
            provider.settings_config = config;
            let legacy = ModelMapping::from_provider(&provider);
            for original in originals {
                let resolved = borrowed_mapped_model(original, &provider);
                assert_eq!(resolved, legacy.map_model(original));
                assert!(
                    std::ptr::eq(resolved, original)
                        || provider.settings_config["env"]
                            .as_object()
                            .is_some_and(|env| env
                                .values()
                                .filter_map(Value::as_str)
                                .any(|value| std::ptr::eq(value, resolved))),
                    "resolver must borrow, not allocate model/configuration strings"
                );
            }
        }
    }
}
