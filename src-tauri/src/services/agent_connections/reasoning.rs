//! Publish known capabilities using each agent's native model schema.
use super::Model;
use serde_json::{json, Value};

fn enabled(model: &Model) -> bool {
    model.reasoning || model.reasoning_levels.iter().any(|level| level != "none")
}

pub(super) fn workbuddy(row: &mut Value, model: &Model) {
    if let Some(tokens) = super::output_limit(model) {
        row["maxOutputTokens"] = json!(tokens);
    }
    let levels = model
        .reasoning_levels
        .iter()
        .filter(|level| level.as_str() != "none")
        .collect::<Vec<_>>();
    row["supportsImages"] = json!(model.images);
    row["supportsReasoning"] = json!(!levels.is_empty());
    if !levels.is_empty() {
        let default = levels
            .iter()
            .find(|level| model.default_reasoning_level.as_deref() == Some(level.as_str()))
            .or_else(|| {
                levels
                    .iter()
                    .find(|level| level.as_str() == "high")
                    .or_else(|| levels.first())
            });
        row["reasoning"] = json!({"supportedEfforts":levels,"canDisableThinking":model.reasoning_levels.iter().any(|level| level == "none"),"defaultEffort":default});
    }
}

pub(super) fn qoder(row: &mut Value, model: &Model) {
    let levels = model
        .reasoning_levels
        .iter()
        .filter(|level| matches!(level.as_str(), "low" | "medium" | "high" | "xhigh" | "max"))
        .collect::<Vec<_>>();
    row["capabilities"] = json!({"tools":true,"vision":model.images});
    if !levels.is_empty() {
        row["capabilities"]["thinking"] = json!({"modes":["enabled"],"supportsEffort":true,"supportedEffortLevels":levels,"requiresBudgetForEnabled":false});
    }
}

pub(super) fn kimi(row: &mut Value, model: &Model) {
    let levels = model
        .reasoning_levels
        .iter()
        .filter(|level| level.as_str() != "none")
        .collect::<Vec<_>>();
    let mut capabilities = vec![];
    if enabled(model) {
        capabilities.push("thinking");
    }
    if model.images {
        capabilities.push("image_in");
    }
    row["capabilities"] = json!(capabilities);
    if !levels.is_empty() {
        row["support_efforts"] = json!(levels);
        if let Some(level) = model
            .default_reasoning_level
            .as_ref()
            .filter(|level| levels.contains(level))
        {
            row["default_effort"] = json!(level);
        }
    }
}

pub(super) fn zcode(row: &mut Value, model: &Model) {
    if !model.reasoning_levels.is_empty() {
        row["optionSpecs"]["reasoningLevel"] = json!({"values":model.reasoning_levels.iter().map(|level| if level == "none" { "disabled" } else { level.as_str() }).collect::<Vec<_>>()});
    }
}

pub(super) fn claude_capabilities(models: &[Model]) -> String {
    let mut result = String::new();
    for model in models {
        if model
            .id
            .chars()
            .any(|c| c.is_control() || matches!(c, ';' | '=' | ',' | '*'))
        {
            continue;
        }
        if !model
            .reasoning_levels
            .iter()
            .any(|level| matches!(level.as_str(), "low" | "medium" | "high" | "xhigh" | "max"))
        {
            continue;
        }
        let mut caps = vec!["effort"];
        if model
            .reasoning_levels
            .iter()
            .any(|level| level == "xhigh" || level == "max")
        {
            caps.push("xhigh_effort");
        }
        if model.reasoning_levels.iter().any(|level| level == "max") {
            caps.push("max_effort");
        }
        let segment = format!("{}={}", model.id.to_lowercase(), caps.join(","));
        if result.len() + segment.len() + usize::from(!result.is_empty()) > 8000 {
            continue;
        }
        if !result.is_empty() {
            result.push(';');
        }
        result.push_str(&segment);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_known_output_and_cache_prices_reach_native_agents() {
        let capability =
            crate::services::intent_routing::effort::Capability::parse(&json!({"output":32768}));
        assert_eq!(capability.output_tokens, Some(32768));
        let model = Model {
            output_tokens: Some(32768),
            cost: Some(json!({"input":1,"output":2,"cacheRead":0.1,"cacheWrite":1.25})),
            id: "synthetic".into(),
            name: "Synthetic".into(),
            context_window: Some(128000),
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: false,
            icon: None,
        };
        let pi = super::super::pi_model(&model);
        assert_eq!(pi["maxTokens"], 32768);
        assert_eq!(pi["cost"]["cacheRead"], 0.1);
        let opencode = super::super::opencode_model(&model);
        assert_eq!(opencode["limit"]["output"], 32768);
        assert_eq!(opencode["cost"]["cache_write"], 1.25);
        let mut buddy = json!({});
        workbuddy(&mut buddy, &model);
        assert_eq!(buddy["maxOutputTokens"], 32768);
    }

    #[test]
    fn parity_opencode_publishes_image_modalities() {
        let model = Model {
            output_tokens: None,
            cost: None,
            id: "vary-provider/synthetic".into(),
            name: "Synthetic".into(),
            context_window: None,
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: true,
            icon: None,
        };
        assert_eq!(
            super::super::opencode_model(&model)["modalities"]["input"],
            json!(["text", "image"])
        );
    }

    #[test]
    fn native_agents_receive_only_their_supported_reasoning_fields() {
        let model = Model {
            output_tokens: None,
            cost: None,
            id: "vary-provider/test".into(),
            name: "Synthetic".into(),
            context_window: None,
            compact_at: None,
            group: "Fixture".into(),
            reasoning_levels: vec!["none".into(), "low".into(), "high".into(), "max".into()],
            reasoning: true,
            default_reasoning_level: Some("low".into()),
            images: false,
            icon: None,
        };
        let mut buddy = json!({});
        workbuddy(&mut buddy, &model);
        assert_eq!(
            buddy["reasoning"]["supportedEfforts"],
            json!(["low", "high", "max"])
        );
        assert_eq!(buddy["reasoning"]["canDisableThinking"], true);
        assert_eq!(buddy["reasoning"]["defaultEffort"], "low");
        let mut qoder_model = json!({});
        qoder(&mut qoder_model, &model);
        assert_eq!(
            qoder_model["capabilities"]["thinking"]["supportedEffortLevels"],
            json!(["low", "high", "max"])
        );
        let mut kimi_model = json!({});
        kimi(&mut kimi_model, &model);
        assert_eq!(kimi_model["support_efforts"], json!(["low", "high", "max"]));
        assert_eq!(kimi_model["default_effort"], "low");
        let mut zcode_model = json!({});
        zcode(&mut zcode_model, &model);
        assert_eq!(
            zcode_model["optionSpecs"]["reasoningLevel"]["values"],
            json!(["disabled", "low", "high", "max"])
        );
        assert_eq!(
            claude_capabilities(&[model]),
            "vary-provider/test=effort,xhigh_effort,max_effort"
        );
    }

    #[test]
    fn toggle_only_models_do_not_acquire_fabricated_levels() {
        let model = Model {
            output_tokens: None,
            cost: None,
            id: "synthetic".into(),
            name: "Synthetic".into(),
            context_window: None,
            compact_at: None,
            group: "Fixture".into(),
            reasoning_levels: vec![],
            reasoning: true,
            default_reasoning_level: None,
            images: false,
            icon: None,
        };
        let pi = super::super::pi_model(&model);
        assert_eq!(pi["reasoning"], true);
        assert!(pi.get("thinkingLevelMap").is_none());
        let opencode = super::super::opencode_model(&model);
        assert_eq!(opencode["reasoning"], true);
        assert!(opencode.get("variants").is_none());
        let mut kimi_model = json!({});
        kimi(&mut kimi_model, &model);
        assert_eq!(kimi_model["capabilities"], json!(["thinking"]));
        assert!(kimi_model.get("support_efforts").is_none());
        assert!(claude_capabilities(&[model]).is_empty());
    }
}
