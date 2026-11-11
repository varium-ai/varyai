use super::*;
use crate::services::agent_connections::Model;

fn model(window: Option<u64>) -> Model {
    Model {
        api: "openai-completions",
        native_model: None,
        id: "synthetic/model".into(),
        name: "Synthetic model".into(),
        context_window: window,
        compact_at: None,
        output_tokens: None,
        cost: None,
        group: "Synthetic".into(),
        reasoning_levels: vec![],
        reasoning: false,
        default_reasoning_level: None,
        images: false,
        icon: None,
    }
}

#[test]
fn native_profile_always_prefers_one_long_entry() {
    let spec = InferenceModelSpec {
        name: "claude-mythos-synthetic".into(),
        label_override: Some("Synthetic model".into()),
        supports_1m: true,
        max_effort: Some("high".into()),
    };
    let entry = inference_model_json(&spec);
    assert_eq!(entry["supports1m"], true);
    assert_eq!(entry["prefer1m"], true);
    assert_eq!(entry["name"], spec.name);
    assert_eq!(entry["labelOverride"], "Synthetic model");
    assert_eq!(entry["maxEffort"], "high");
    let short = InferenceModelSpec {
        supports_1m: false,
        ..spec
    };
    assert!(inference_model_json(&short).get("prefer1m").is_none());
}

#[test]
fn discovery_lists_one_entry_without_reducing_actual_capacity() {
    for window in [
        Some(1_000_000),
        Some(1_048_576),
        Some(2_000_000),
        Some(872_000),
        None,
    ] {
        let model = model(window);
        let wire = shared_model_wire(&model);
        let entry = desktop_model_entry(&model, json!({}));
        if window.is_some_and(|n| n >= 1_000_000) {
            assert_eq!(entry["id"], format!("{wire}[1m]"));
            assert_eq!(entry["description"], "1M context window");
        } else {
            assert_eq!(entry["id"], wire);
            assert!(entry.get("description").is_none());
        }
        assert_eq!(
            entry.get("max_input_tokens").and_then(Value::as_u64),
            window
        );
    }
}
