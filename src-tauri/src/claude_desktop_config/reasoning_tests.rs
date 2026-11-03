use super::*;
use crate::services::agent_connections::Model;

// Desktop 4.4212 normalizes profile IDs before consulting its native effort table.
fn desktop_offers_effort(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let normalized = id
        .split_once(".anthropic.")
        .map_or(id.as_str(), |(_, model)| model);
    matches!(
        normalized,
        "claude-sonnet-4-6" | "claude-sonnet-5" | "claude-opus-4-8" | "claude-opus-5"
    ) || normalized.starts_with("claude-mythos-")
        || normalized.starts_with("claude-fable-")
}

#[test]
fn shared_desktop_profile_exposes_native_effort_controls_and_caps() -> Result<(), String> {
    let home = tempfile::tempdir().map_err(|_| "fixture")?;
    let paths = paths_from_dirs(home.path().join("Claude"), home.path().join("Claude-3p"));
    let model = Model {
        id: "synthetic/first".into(),
        name: "Synthetic reasoning model".into(),
        context_window: None,
        compact_at: None,
        group: "Synthetic".into(),
        reasoning_levels: vec!["low".into(), "high".into()],
        reasoning: true,
        default_reasoning_level: Some("low".into()),
        images: false,
        icon: None,
    };
    assert!(!desktop_offers_effort(&shared_model_name(&model.id)));
    let (files, mapping) = shared_catalog_edits_at(
        paths,
        "http://127.0.0.1:1/connections/claude-desktop",
        "synthetic-local",
        std::slice::from_ref(&model),
    )?;
    let payload = serde_json::to_value(&files).map_err(|_| "fixture")?;
    let profile = payload
        .as_array()
        .and_then(|files| {
            files.iter().find_map(|file| {
                file["installed"]
                    .get("inferenceModels")
                    .map(|_| &file["installed"])
            })
        })
        .ok_or("missing profile")?;
    let entry = &profile["inferenceModels"][0];
    let wire = entry["name"].as_str().ok_or("missing wire")?;
    assert!(
        desktop_offers_effort(wire),
        "Desktop must recognize the native effort identity"
    );
    assert!(is_claude_safe_model_id(wire));
    assert!(!wire.contains("sonnet"));
    assert_eq!(mapping.get(wire), Some(&model.id));
    assert_eq!(entry["maxEffort"], "high");
    assert_eq!(profile["defaultModelEffort"], "low");
    Ok(())
}

#[test]
fn desktop_profile_refresh_preserves_user_effort_and_unknown_models_remain_plain(
) -> Result<(), String> {
    let home = tempfile::tempdir().map_err(|_| "fixture")?;
    let paths = paths_from_dirs(home.path().join("Claude"), home.path().join("Claude-3p"));
    write_json_file(&paths.profile_path, &json!({"defaultModelEffort":"high"}))
        .map_err(|_| "fixture")?;
    let mut model = Model {
        id: "synthetic/first".into(),
        name: "Synthetic".into(),
        context_window: None,
        compact_at: None,
        group: "Synthetic".into(),
        reasoning_levels: vec!["low".into(), "high".into()],
        reasoning: true,
        default_reasoning_level: Some("low".into()),
        images: false,
        icon: None,
    };
    let (files, _) = shared_catalog_edits_at(
        paths,
        "http://127.0.0.1:1/connections/claude-desktop",
        "synthetic-local",
        std::slice::from_ref(&model),
    )?;
    let payload = serde_json::to_value(&files).map_err(|_| "fixture")?;
    assert_eq!(payload[2]["installed"]["defaultModelEffort"], "high");

    for (reasoning, levels) in [(false, vec![]), (true, vec![]), (false, vec!["none"])] {
        model.reasoning = reasoning;
        model.reasoning_levels = levels.into_iter().map(str::to_owned).collect();
        assert_eq!(desktop_max_effort(&model), None);
        assert_eq!(shared_model_wire(&model), shared_model_name(&model.id));
    }
    Ok(())
}
