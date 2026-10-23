use serde_json::Value;

/// Historical text still needs inspection/redaction, but must not be reported as
/// a new user action when a client resends its conversation on a later turn.
pub(crate) struct CurrentTurn {
    starts: Vec<(&'static str, usize)>,
}

impl CurrentTurn {
    pub(crate) fn new(body: &Value) -> Self {
        let starts = ["messages", "input", "contents"]
            .into_iter()
            .filter_map(|key| {
                let items = body.get(key)?.as_array()?;
                let index = items.iter().rposition(|item| item["role"] == "user")?;
                Some((key, index))
            })
            .collect();
        Self { starts }
    }

    pub(crate) fn is_historical(&self, location: &str) -> bool {
        self.starts.iter().any(|(key, start)| {
            location
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('['))
                .and_then(|rest| rest.split_once(']'))
                .and_then(|(index, _)| index.parse::<usize>().ok())
                .is_some_and(|index| index < *start)
        })
    }
}

/// A text node extracted for inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedTextNode<'a> {
    pub location: String,

    pub text: &'a str,

    pub is_password_field: bool,
}

pub fn extract_payload_texts(body: &Value) -> Vec<ExtractedTextNode<'_>> {
    let mut nodes = Vec::new();

    let Value::Object(root) = body else {
        return nodes;
    };

    if let Some(system_val) = root.get("system") {
        extract_system_texts(system_val, "system", &mut nodes);
    }

    let gemini_sys = root
        .get("system_instruction")
        .or_else(|| root.get("systemInstruction"));
    if let Some(sys_instruction) = gemini_sys {
        if let Some(parts) = sys_instruction.get("parts").and_then(Value::as_array) {
            for (idx, part) in parts.iter().enumerate() {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    nodes.push(ExtractedTextNode {
                        location: format!("system_instruction.parts[{idx}].text"),
                        text,
                        is_password_field: false,
                    });
                }
            }
        } else {
            extract_any_text(sys_instruction, "system_instruction", &mut nodes);
        }
    }

    if let Some(messages) = root.get("messages").and_then(Value::as_array) {
        for (m_idx, msg) in messages.iter().enumerate() {
            let msg_loc = format!("messages[{m_idx}]");
            extract_message_node(msg, &msg_loc, &mut nodes);
        }
    }

    // Qoder CN repeats prompt text in native context and contents blocks. Inspect
    // every copy, but only when the native envelope is present; metadata and model
    // configuration are not prompt text and must remain untouched.
    if root.get("model_config").is_some_and(Value::is_object) {
        if let Some(context) = root.get("chat_context").filter(|v| v.is_object()) {
            for (pointer, location) in [
                ("/text", "chat_context.text"),
                (
                    "/extra/originalContent",
                    "chat_context.extra.originalContent",
                ),
            ] {
                if let Some(text) = context.pointer(pointer).and_then(Value::as_str) {
                    nodes.push(ExtractedTextNode {
                        location: location.into(),
                        text,
                        is_password_field: false,
                    });
                }
            }
            if let Some(messages) = root.get("messages").and_then(Value::as_array) {
                for (m_idx, message) in messages.iter().enumerate() {
                    if let Some(contents) = message.get("contents").and_then(Value::as_array) {
                        for (c_idx, block) in contents.iter().enumerate() {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                nodes.push(ExtractedTextNode {
                                    location: format!("messages[{m_idx}].contents[{c_idx}].text"),
                                    text,
                                    is_password_field: false,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(prompt) = root.get("prompt") {
        extract_any_text(prompt, "prompt", &mut nodes);
    }
    if let Some(input) = root.get("input") {
        extract_any_text(input, "input", &mut nodes);
    }
    if let Some(instructions) = root.get("instructions") {
        extract_any_text(instructions, "instructions", &mut nodes);
    }

    if let Some(query) = root.get("query") {
        extract_any_text(query, "query", &mut nodes);
    }
    if let Some(queries) = root.get("queries") {
        extract_any_text(queries, "queries", &mut nodes);
    }

    if let Some(contents) = root.get("contents").and_then(Value::as_array) {
        for (c_idx, content) in contents.iter().enumerate() {
            let c_loc = format!("contents[{c_idx}]");
            if let Some(parts) = content.get("parts").and_then(Value::as_array) {
                for (p_idx, part) in parts.iter().enumerate() {
                    let p_loc = format!("{c_loc}.parts[{p_idx}]");
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        nodes.push(ExtractedTextNode {
                            location: format!("{p_loc}.text"),
                            text,
                            is_password_field: false,
                        });
                    }
                    if let Some(func_call) = part.get("functionCall") {
                        if let Some(args) = func_call.get("args") {
                            extract_recursive_strings(
                                args,
                                &format!("{p_loc}.functionCall.args"),
                                &mut nodes,
                            );
                        }
                    }
                    if let Some(func_resp) = part.get("functionResponse") {
                        if let Some(resp) = func_resp.get("response") {
                            extract_recursive_strings(
                                resp,
                                &format!("{p_loc}.functionResponse.response"),
                                &mut nodes,
                            );
                        }
                    }
                }
            }
        }
    }

    nodes
}

fn extract_system_texts<'a>(val: &'a Value, path: &str, nodes: &mut Vec<ExtractedTextNode<'a>>) {
    match val {
        Value::String(s) => {
            nodes.push(ExtractedTextNode {
                location: path.to_string(),
                text: s.as_str(),
                is_password_field: false,
            });
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let item_path = format!("{path}[{i}]");
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    nodes.push(ExtractedTextNode {
                        location: format!("{item_path}.text"),
                        text,
                        is_password_field: false,
                    });
                }
            }
        }
        _ => {}
    }
}

fn extract_message_node<'a>(msg: &'a Value, path: &str, nodes: &mut Vec<ExtractedTextNode<'a>>) {
    let Value::Object(m_obj) = msg else {
        return;
    };

    if let Some(content) = m_obj.get("content") {
        match content {
            Value::String(s) => {
                nodes.push(ExtractedTextNode {
                    location: format!("{path}.content"),
                    text: s.as_str(),
                    is_password_field: false,
                });
            }
            Value::Array(blocks) => {
                for (b_idx, block) in blocks.iter().enumerate() {
                    let b_path = format!("{path}.content[{b_idx}]");
                    if let Some(b_obj) = block.as_object() {
                        let b_type = b_obj
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match b_type {
                            "text" => {
                                if let Some(text) = b_obj.get("text").and_then(Value::as_str) {
                                    nodes.push(ExtractedTextNode {
                                        location: format!("{b_path}.text"),
                                        text,
                                        is_password_field: false,
                                    });
                                }
                            }
                            "tool_result" => {
                                if let Some(res_content) = b_obj.get("content") {
                                    extract_any_text(
                                        res_content,
                                        &format!("{b_path}.tool_result.content"),
                                        nodes,
                                    );
                                }
                            }
                            "tool_use" => {
                                if let Some(input) = b_obj.get("input") {
                                    extract_recursive_strings(
                                        input,
                                        &format!("{b_path}.tool_use.input"),
                                        nodes,
                                    );
                                }
                            }
                            "thinking" => {
                                if let Some(thinking_text) =
                                    b_obj.get("thinking").and_then(Value::as_str)
                                {
                                    nodes.push(ExtractedTextNode {
                                        location: format!("{b_path}.thinking"),
                                        text: thinking_text,
                                        is_password_field: false,
                                    });
                                }
                            }
                            _ => {
                                if let Some(text) = b_obj.get("text").and_then(Value::as_str) {
                                    nodes.push(ExtractedTextNode {
                                        location: format!("{b_path}.text"),
                                        text,
                                        is_password_field: false,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(tool_calls) = m_obj.get("tool_calls").and_then(Value::as_array) {
        for (t_idx, tc) in tool_calls.iter().enumerate() {
            let tc_path = format!("{path}.tool_calls[{t_idx}]");
            if let Some(func) = tc.get("function") {
                if let Some(args_str) = func.get("arguments").and_then(Value::as_str) {
                    nodes.push(ExtractedTextNode {
                        location: format!("{tc_path}.function.arguments"),
                        text: args_str,
                        is_password_field: false,
                    });
                }
            }
        }
    }
}

fn extract_recursive_strings<'a>(
    val: &'a Value,
    path: &str,
    nodes: &mut Vec<ExtractedTextNode<'a>>,
) {
    extract_recursive_strings_with_context(val, path, nodes, false);
}

fn extract_recursive_strings_with_context<'a>(
    val: &'a Value,
    path: &str,
    nodes: &mut Vec<ExtractedTextNode<'a>>,
    is_password_field: bool,
) {
    match val {
        Value::String(s) => {
            nodes.push(ExtractedTextNode {
                location: path.to_string(),
                text: s.as_str(),
                is_password_field,
            });
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                extract_recursive_strings_with_context(
                    item,
                    &format!("{path}[{idx}]"),
                    nodes,
                    is_password_field,
                );
            }
        }
        Value::Object(map) => {
            for (field, value) in map {
                let child_is_password_field =
                    is_password_field || (is_password_field_name(field) && value.is_string());
                extract_recursive_strings_with_context(
                    value,
                    &format!("{path}.{{field}}"),
                    nodes,
                    child_is_password_field,
                );
            }
        }
        _ => {}
    }
}

fn is_password_field_name(field: &str) -> bool {
    matches!(
        field.to_ascii_lowercase().as_str(),
        "password" | "passwd" | "pwd" | "passphrase" | "passcode"
    )
}

fn extract_any_text<'a>(val: &'a Value, path: &str, nodes: &mut Vec<ExtractedTextNode<'a>>) {
    match val {
        Value::String(s) => {
            nodes.push(ExtractedTextNode {
                location: path.to_string(),
                text: s.as_str(),
                is_password_field: false,
            });
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                extract_any_text(item, &format!("{path}[{idx}]"), nodes);
            }
        }
        Value::Object(_) => {
            extract_recursive_strings(val, path, nodes);
        }
        _ => {}
    }
}
