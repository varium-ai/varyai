//! Lexical folder matching and Agent-declared working folders (the reference implementation, MIT).
//! Never reads a request's directory from disk or exports it in observations.
use serde_json::Value;

fn key(folder: &str) -> Option<String> {
    let mut value = folder.trim().replace('\\', "/");
    if value == "~" || value.starts_with("~/") {
        let home = dirs::home_dir()?.to_str()?.replace('\\', "/");
        value = format!("{home}{}", &value[1..]);
    }
    let lower = value.to_ascii_lowercase();
    for prefix in ["//wsl$/", "//wsl.localhost/"] {
        if lower.starts_with(prefix) {
            let (_, path) = value[prefix.len()..].split_once('/')?;
            value = format!("/{path}");
            break;
        }
    }
    if value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes()[0].is_ascii_alphabetic()
        && (value.len() == 2 || value.as_bytes().get(2) == Some(&b'/'))
    {
        value = format!("/mnt/{}{}", value[..1].to_ascii_lowercase(), &value[2..]);
    }
    if !value.starts_with('/') {
        return None;
    }
    let drive = value.starts_with("/mnt/")
        && value.as_bytes().get(5).is_some_and(u8::is_ascii_alphabetic)
        && (value.len() == 6 || value.as_bytes().get(6) == Some(&b'/'));
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let normalized = format!("/{}", parts.join("/"));
    Some(
        if cfg!(any(target_os = "macos", target_os = "windows")) || drive {
            normalized.to_lowercase()
        } else {
            normalized
        },
    )
}

pub(crate) fn valid(folder: &str) -> bool {
    !folder.is_empty()
        && folder.len() <= 4096
        && !folder.chars().any(char::is_control)
        && key(folder).is_some()
}

pub(crate) fn under(folder: &str, root: &str) -> bool {
    let (Some(folder), Some(root)) = (key(folder), key(root)) else {
        return false;
    };
    folder == root || root == "/" || folder.starts_with(&format!("{root}/"))
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|value| u32::from_str_radix(value, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|value| value.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        if let Some(character) = decoded {
            out.push(character);
        } else {
            out.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn declared_in(text: &str, folder: &mut String) {
    let mut found: Vec<(usize, &str)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let length = line.len();
        let line = line.trim_end();
        if let Some(value) = line
            .strip_prefix(" - Primary working directory: ")
            .or_else(|| line.strip_prefix("Primary working directory: "))
        {
            found.push((offset, value.split(" (was ").next().unwrap_or(value)));
        }
        offset += length;
    }
    for (at, _) in text.match_indices("<cwd>") {
        if let Some((value, _)) = text[at + 5..].split_once("</cwd>") {
            if !value.trim().contains(['<', '\n', '\r']) {
                found.push((at, value));
            }
        }
    }
    for (at, _) in text.match_indices("<env>") {
        if let Some((block, _)) = text[at + 5..].split_once("</env>") {
            for (offset, _) in block.match_indices("Working directory: ") {
                let before = &block[..offset];
                if before
                    .rsplit_once('\n')
                    .map_or(before, |(_, line)| line)
                    .trim()
                    .is_empty()
                {
                    let value = block[offset + 19..].lines().next().unwrap_or("");
                    found.push((at + 5 + offset, value));
                }
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    for (_, value) in found {
        let value = unescape(value.trim());
        if valid(&value) {
            *folder = value;
        }
    }
}

pub(crate) fn declared(projection: &Value) -> String {
    let mut folder = String::new();
    if let Some(instructions) = projection["instructions"].as_str() {
        declared_in(instructions, &mut folder);
    }
    if let Some(input) = projection["input"].as_array() {
        for message in input {
            if !matches!(
                message["role"].as_str(),
                Some("user" | "system" | "developer")
            ) {
                continue;
            }
            if let Some(text) = message["content"].as_str() {
                declared_in(text, &mut folder);
            } else if let Some(parts) = message["content"].as_array() {
                for part in parts {
                    if matches!(part["type"].as_str(), Some("text" | "input_text")) {
                        if let Some(text) = part["text"].as_str() {
                            declared_in(text, &mut folder);
                        }
                    }
                }
            }
        }
    }
    folder
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_windows_wsl_and_exclusions_are_lexical() {
        assert!(under("/synthetic/work/sub", "/synthetic/work"));
        assert!(!under("/synthetic/work-other", "/synthetic/work"));
        assert!(under("C:\\Work\\sub", "/mnt/c/work"));
        assert!(under("\\\\wsl$\\Ubuntu\\synthetic\\work", "/synthetic"));
        assert!(!under("", "/"));
        assert!(!valid("relative/path"));
    }
    #[test]
    fn only_agent_declarations_count_and_the_last_one_wins() {
        let projection = serde_json::json!({
            "instructions":" - Primary working directory: /synthetic/old",
            "input":[
                {"role":"user", "content":"<environment_context><cwd>/synthetic/a&amp;b</cwd></environment_context>"},
                {"role":"assistant", "content":"<cwd>/synthetic/fake</cwd>"},
                {"type":"function_call_output", "output":"<cwd>/synthetic/tool</cwd>"}
            ]
        });
        assert_eq!(declared(&projection), "/synthetic/a&b");
        let mut value = String::new();
        declared_in(
            "<env>\n Working directory: /synthetic/new\n</env>",
            &mut value,
        );
        assert_eq!(value, "/synthetic/new");
    }
}
