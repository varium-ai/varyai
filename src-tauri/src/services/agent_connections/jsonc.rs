//! OpenCode accepts JSON with comments and trailing commas, not JSON5.
use super::FAILED;
use json_five::{
    rt::parser::{from_str, JSONValue},
    tokenize::{tokenize_rt_str, TokType},
};
use serde_json::Value;

fn trivia(kind: &TokType) -> bool {
    matches!(
        kind,
        TokType::Whitespace | TokType::LineComment | TokType::BlockComment
    )
}

pub(crate) fn parse(source: &str) -> Result<Value, String> {
    let tokens = tokenize_rt_str(source).map_err(|_| FAILED)?.tok_spans;
    let mut json = String::with_capacity(source.len());
    for (index, (start, kind, end)) in tokens.iter().enumerate() {
        if matches!(kind, TokType::LineComment | TokType::BlockComment) {
            json.push(' ');
            continue;
        }
        if *kind == TokType::Comma {
            let previous = tokens[..index]
                .iter()
                .rev()
                .find(|(_, kind, _)| !trivia(kind));
            let next = tokens[index + 1..]
                .iter()
                .find(|(_, kind, _)| !trivia(kind));
            if next.is_some_and(|(_, kind, _)| {
                matches!(kind, TokType::RightBrace | TokType::RightBracket)
            }) && previous.is_some_and(|(_, kind, _)| {
                !matches!(
                    kind,
                    TokType::LeftBrace | TokType::LeftBracket | TokType::Comma | TokType::Colon
                )
            }) {
                continue;
            }
        }
        json.push_str(&source[*start..*end]);
    }
    serde_json::from_str(&json).map_err(|_| FAILED.into())
}

pub(crate) fn encode(source: &str, before: &Value, after: &Value) -> Result<String, String> {
    let mut shallow = after.clone();
    for (key, value) in after.as_object().ok_or(FAILED)? {
        if value.is_object() && before.get(key).is_some_and(Value::is_object) {
            shallow[key] = before[key].clone();
        }
    }
    let patched =
        crate::openclaw_config::patch_document(source, before, &shallow).map_err(|_| FAILED)?;
    let mut document = from_str(&patched).map_err(|_| FAILED)?;
    let JSONValue::JSONObject {
        key_value_pairs, ..
    } = &mut document.value
    else {
        return Err(FAILED.into());
    };
    // The JSON5 editor inserts bare root keys. Quote them before publishing to
    // OpenCode, including model keys left by older connection receipts.
    for pair in key_value_pairs {
        let key = match &pair.key {
            JSONValue::Identifier(key) | JSONValue::DoubleQuotedString(key) => key.clone(),
            _ => return Err(FAILED.into()),
        };
        if before.get(&key).is_some_and(Value::is_object)
            && after.get(&key).is_some_and(Value::is_object)
            && before[&key] != after[&key]
        {
            let nested = encode(&pair.value.to_string(), &before[&key], &after[&key])?;
            pair.value = from_str(&nested).map_err(|_| FAILED)?.value;
        }
        if let JSONValue::Identifier(name) = &pair.key {
            let quoted = serde_json::to_string(name).map_err(|_| FAILED)?;
            pair.key = JSONValue::DoubleQuotedString(quoted[1..quoted.len() - 1].to_owned());
        }
    }
    let mut result = document.to_string();
    let lines = source.matches('\n').count();
    if lines > 0 && source.matches("\r\n").count() == lines {
        result = result.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    if parse(&result)? != *after {
        return Err(FAILED.into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn jsonc_accepts_comments_trailing_commas_and_comment_markers_in_strings() -> Result<(), String>
    {
        assert_eq!(
            parse("{ // retained\n \"url\": \"https://synthetic.invalid/a/*b*/\", \"values\": [-1, 2, /* retained */], }")?,
            json!({"url":"https://synthetic.invalid/a/*b*/","values":[-1,2]})
        );
        Ok(())
    }

    #[test]
    fn jsonc_rejects_json5_and_lone_commas() {
        for source in [
            "{model:\"synthetic\"}",
            "{\"model\":'synthetic'}",
            "{\"value\":0xff}",
            "{\"value\":NaN}",
            "{,}",
            "{\"values\":[,]}",
            "{\"values\":[1,,]}",
        ] {
            assert!(parse(source).is_err());
        }
    }

    #[test]
    fn legacy_bare_model_key_is_repaired_without_changing_values() -> Result<(), String> {
        let source = "{ // retained\n \"theme\":\"synthetic\", model:\"varyai/old\"\n}";
        let before = json!({"theme":"synthetic","model":"varyai/old"});
        let after = json!({"theme":"synthetic","model":"varyai/new"});
        let result = encode(source, &before, &after)?;
        assert!(result.contains("// retained"));
        assert_eq!(parse(&result)?, after);
        Ok(())
    }

    #[test]
    fn unchanged_legacy_selection_is_repaired_on_write_and_restore() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{model:\"synthetic\"}").map_err(|_| FAILED)?;
        let file = super::super::FileEdit::prepare_format(
            path.clone(),
            super::super::Format::Jsonc,
            |_| Ok(()),
        )?;
        file.write()?;
        assert_eq!(
            parse(&std::fs::read_to_string(&path).map_err(|_| FAILED)?)?,
            json!({"model":"synthetic"})
        );
        file.restore()?;
        parse(&std::fs::read_to_string(&path).map_err(|_| FAILED)?)?;
        Ok(())
    }

    #[test]
    fn jsonc_receipts_keep_the_legacy_serialized_format() -> Result<(), String> {
        assert_eq!(
            serde_json::to_string(&super::super::Format::Jsonc).map_err(|_| FAILED)?,
            "\"Json5\""
        );
        Ok(())
    }

    #[test]
    fn foreign_provider_comments_and_crlf_are_retained() -> Result<(), String> {
        let source = "{\r\n  \"provider\": {\r\n    // native provider\r\n    \"mine\": {\"name\":\"Synthetic\"}\r\n  }\r\n}";
        let before = parse(source)?;
        let mut after = before.clone();
        after["provider"]["varyai"] = json!({"name":"Vary AI"});
        after["model"] = json!("varyai/synthetic");
        let result = encode(source, &before, &after)?;
        assert!(result.contains("// native provider\r\n"));
        assert_eq!(result.matches('\n').count(), result.matches("\r\n").count());
        assert_eq!(parse(&result)?, after);
        Ok(())
    }
}
