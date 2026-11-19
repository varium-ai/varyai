use super::{build, parse, Part, Protocol, ToolChoice};
use serde_json::json;

#[test]
fn magpie_allowed_tools_filter_definitions_without_rewriting_history(
) -> Result<(), Box<dyn std::error::Error>> {
    for mode in ["auto", "required"] {
        for input in [
            json!("Synthetic request"),
            json!([
                {"type":"function_call","call_id":"old-call","name":"excluded","arguments":"{}"},
                {"type":"function_call_output","call_id":"old-call","output":"Synthetic result"},
                {"role":"user","content":"Synthetic request"}
            ]),
        ] {
            let body = json!({"model":"synthetic","input":input,
                "tools":[
                    {"type":"function","name":"excluded","parameters":{"type":"object"}},
                    {"type":"function","name":"lookup","parameters":{"type":"object"}},
                    {"type":"namespace","name":"agent","tools":[
                        {"type":"function","name":"lookup","parameters":{"type":"object"}},
                        {"type":"custom","name":"patch","format":{"type":"text"}},
                        {"type":"function","name":"excluded","parameters":{"type":"object"}}
                    ]}
                ],
                "tool_choice":{"type":"allowed_tools","mode":mode,"tools":[
                    {"type":"function","name":"lookup","namespace":"agent"},
                    {"type":"custom","name":"patch","namespace":"agent"}
                ]}
            });
            let original = body.clone();
            let parsed = parse::request(Protocol::Responses, &body, "/v1/responses")?;
            assert_eq!(body, original);
            assert_eq!(parsed.tools.len(), 1);
            assert_eq!(
                parsed.tools[0].native.as_ref().ok_or("namespace")?["tools"]
                    .as_array()
                    .ok_or("children")?
                    .len(),
                2
            );
            assert_eq!(
                parsed.tool_choice,
                Some(if mode == "auto" {
                    ToolChoice::Auto
                } else {
                    ToolChoice::Required
                })
            );
            if input.is_array() {
                assert!(
                    matches!(&parsed.messages[0].parts[0], Part::ToolCall { name, .. } if name == "excluded")
                );
                assert!(
                    matches!(&parsed.messages[1].parts[0], Part::ToolResult { call_id, .. } if call_id == "old-call")
                );
            }
            for target in [
                Protocol::Chat,
                Protocol::Anthropic,
                Protocol::Gemini,
                Protocol::Responses,
            ] {
                let encoded = build::request(&parsed, target)?;
                let endpoint = if target == Protocol::Gemini {
                    "/v1beta/models/synthetic:generateContent"
                } else {
                    "/v1/responses"
                };
                let roundtrip = parse::request(target, &encoded, endpoint)?;
                assert_eq!(roundtrip.tool_choice, parsed.tool_choice);
                assert!(!encoded["tools"].to_string().contains("excluded"));
                assert!(encoded["tools"].to_string().contains("lookup"));
                assert!(encoded["tools"].to_string().contains("patch"));
            }
        }
    }
    Ok(())
}

#[test]
fn magpie_allowed_tools_validate_modes_and_empty_required_sets() {
    for choice in [
        json!({"type":"allowed_tools","mode":"none","tools":[]}),
        json!({"type":"allowed_tools","mode":"required","tools":[]}),
        json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"absent"}]}),
        json!({"type":"allowed_tools","mode":"auto","tools":{}}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function"}]}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"unknown"}]}),
    ] {
        assert!(parse::request(
            Protocol::Responses,
            &json!({"model":"synthetic","input":"Synthetic","tool_choice":choice}),
            "/v1/responses"
        )
        .is_err());
    }
}

#[test]
fn magpie_allowed_tools_auto_can_disable_all_tools() -> Result<(), Box<dyn std::error::Error>> {
    let body = json!({"model":"synthetic","input":"Synthetic","tools":[{"type":"function","name":"excluded","parameters":{"type":"object"}}],"parallel_tool_calls":false,"tool_choice":{"type":"allowed_tools","mode":"auto","tools":[]}});
    let parsed = parse::request(Protocol::Responses, &body, "/v1/responses")?;
    assert!(parsed.tools.is_empty());
    let chat = build::request(&parsed, Protocol::Chat)?;
    assert!(chat.get("tool_choice").is_none());
    assert!(chat.get("parallel_tool_calls").is_none());
    let messages = build::request(&parsed, Protocol::Anthropic)?;
    assert!(messages.get("tool_choice").is_none());
    let responses = build::request(&parsed, Protocol::Responses)?;
    assert_eq!(responses["tool_choice"], body["tool_choice"]);
    assert_eq!(responses["parallel_tool_calls"], false);
    Ok(())
}

#[test]
fn magpie_allowed_tools_restrict_legacy_codex_adapters() -> Result<(), Box<dyn std::error::Error>> {
    use crate::proxy::providers::{
        transform_codex_anthropic::responses_request_to_anthropic,
        transform_codex_chat::responses_to_chat_completions,
    };
    let mut body = json!({
        "model":"synthetic",
        "input":[
            {"type":"additional_tools","tools":[{"type":"namespace","name":"loaded","tools":[
                {"type":"function","name":"lookup","parameters":{"type":"object"}},
                {"type":"function","name":"excluded","parameters":{"type":"object"}}
            ]}]},
            {"type":"message","role":"user","content":"Synthetic request"}
        ],
        "tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}],
        "tool_choice":{"type":"allowed_tools","mode":"required","tools":[
            {"type":"function","namespace":"loaded","name":"lookup"}
        ]}
    });
    let chat = responses_to_chat_completions(body.clone())?;
    let messages = responses_request_to_anthropic(body.clone(), 4096)?;
    assert_eq!(chat["tool_choice"], "required");
    assert_eq!(messages["tool_choice"], json!({"type":"any"}));
    for (wire, name) in [(&chat, "/0/function/name"), (&messages, "/0/name")] {
        assert_eq!(wire["tools"].as_array().ok_or("missing tools")?.len(), 1);
        assert_eq!(wire["tools"].pointer(name), Some(&json!("loaded__lookup")));
    }
    body["tool_choice"]["tools"] = json!([]);
    assert!(responses_to_chat_completions(body.clone()).is_err());
    assert!(responses_request_to_anthropic(body, 4096).is_err());
    Ok(())
}
