//! Read-only size hints; never rewrite a request or move opaque continuation state.
use serde_json::Value;

pub(super) fn portable(body: &Value) -> bool {
    if body["_group_projection_incomplete"] == true
        || body
            .get("previous_response_id")
            .is_some_and(|v| !v.is_null())
        || body.get("conversation").is_some_and(|v| !v.is_null())
    {
        return false;
    }
    match &body["input"] {
        Value::String(_) => true,
        Value::Array(items) => items.iter().all(|item| {
            matches!(item["type"].as_str(), None | Some("message"))
                && item.get("encrypted_content").is_none()
                && match &item["content"] {
                    Value::String(_) => true,
                    Value::Array(parts) => parts.iter().all(|p| {
                        matches!(
                            p["type"].as_str(),
                            Some("input_text" | "output_text" | "text")
                        ) && p["text"].is_string()
                    }),
                    _ => false,
                }
        }),
        _ => false,
    }
}

pub(in crate::proxy) fn estimate(projection: &Value, body: &Value) -> Option<u64> {
    if projection["_group_projection_incomplete"] == true {
        return None;
    }
    let mut bytes = projection["instructions"].as_str().map_or(0, str::len) as u64;
    let text = |v: &Value| -> Option<u64> {
        match v {
            Value::String(s) => Some(s.len() as u64),
            Value::Array(parts) => {
                let mut n = 0u64;
                for p in parts {
                    match p["type"].as_str() {
                        Some("input_text" | "output_text" | "text") => {
                            n = n.saturating_add(p["text"].as_str()?.len() as u64)
                        }
                        Some("input_image") => {}
                        _ => return None,
                    }
                }
                Some(n)
            }
            _ => None,
        }
    };
    match &projection["input"] {
        Value::String(s) => bytes = bytes.saturating_add(s.len() as u64),
        Value::Array(items) => {
            for item in items {
                let n = match item["type"].as_str() {
                    None | Some("message") => text(&item["content"]),
                    Some("function_call") => Some(
                        item["name"].as_str()?.len() as u64
                            + item["arguments"].as_str()?.len() as u64,
                    ),
                    Some("function_call_output") => text(&item["output"]),
                    _ => return None,
                }?;
                bytes = bytes.saturating_add(n);
            }
        }
        _ => return None,
    }
    if let Some(tools) = body["tools"].as_array() {
        for tool in tools {
            let function = tool.get("function").unwrap_or(tool);
            for key in ["name", "description"] {
                bytes = bytes.saturating_add(function[key].as_str().map_or(0, str::len) as u64);
            }
            if let Some(schema) = function
                .get("parameters")
                .or_else(|| tool.get("input_schema"))
            {
                bytes = bytes.saturating_add(serde_json::to_vec(schema).ok()?.len() as u64);
            }
        }
    }
    Some(bytes / 4)
}

pub(super) fn overflow(status: u16, body: &[u8]) -> bool {
    if !matches!(status, 400 | 413 | 422) {
        return false;
    }
    if status == 413 {
        return true;
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let error = value.get("error").unwrap_or(&value);
    let code = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .unwrap_or("");
    if matches!(
        code,
        "context_length_exceeded"
            | "model_context_window_exceeded"
            | "prompt_too_long"
            | "input_too_long"
    ) {
        return true;
    }
    let message = error["message"].as_str().unwrap_or("").to_ascii_lowercase();
    [
        "maximum context length",
        "context window exceeded",
        "prompt is too long",
        "input is too long",
        "exceeds the context",
    ]
    .iter()
    .any(|s| message.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_complete_plain_history_can_move_between_members() {
        assert!(portable(
            &json!({"input":[{"role":"user","content":"synthetic"}]})
        ));
        for body in [
            json!({"input":"x","previous_response_id":"r"}),
            json!({"input":[{"type":"function_call_output","call_id":"c","output":"x"}]}),
            json!({"input":[{"type":"reasoning","encrypted_content":"synthetic"}]}),
            json!({"input":"x","_group_projection_incomplete":true}),
        ] {
            assert!(!portable(&body));
        }
    }
    #[test]
    fn count_only_supported_text_and_tool_schema_without_touching_vendor_values() {
        let body =
            json!({"instructions":"1234","input":"12345678","vendor":{"context":"not counted"}});
        assert_eq!(estimate(&body, &body), Some(3));
        assert_eq!(estimate(&json!({"input":false}), &body), None);
        assert!(overflow(
            400,
            br#"{"error":{"code":"context_length_exceeded","message":"synthetic"}}"#
        ));
        assert!(!overflow(
            400,
            br#"{"error":{"code":"invalid_api_key","message":"synthetic"}}"#
        ));
    }
}
