use super::*;

const STARTING_MODEL_APPS: &[&str] = &[
    "claude",
    "codex",
    "pi",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
    "kimi",
    "qwen",
    "zcode",
    "deepseek-harness",
];

#[derive(Serialize)]
pub(crate) struct NativeModel {
    app: &'static str,
    model: Option<String>,
    available: bool,
}

/// Observe only model selection fields. No provider or authentication data leaves this module.
pub(crate) fn list() -> Vec<NativeModel> {
    STARTING_MODEL_APPS
        .iter()
        .map(|&app| match selection(app) {
            Ok(model) => NativeModel {
                app,
                model,
                available: true,
            },
            Err(_) => NativeModel {
                app,
                model: None,
                available: false,
            },
        })
        .collect()
}

fn selection(app: &str) -> Result<Option<String>, String> {
    let path = match app {
        "claude" => crate::config::get_claude_settings_path(),
        "codex" => crate::codex_config::get_codex_config_path(),
        "pi" => crate::pi_config::get_pi_settings_path().map_err(|_| FAILED)?,
        "gemini" => {
            let env = Format::Env
                .parse(read(&crate::gemini_config::get_gemini_env_path())?.as_deref())?;
            if let Some(model) = model_string(env.get("GEMINI_MODEL"))? {
                return Ok(Some(model));
            }
            crate::gemini_config::get_gemini_settings_path()
        }
        "grokbuild" => crate::grok_config::get_grok_config_path(),
        "opencode" => {
            let path = crate::opencode_config::get_opencode_config_path();
            if !path.exists() && path.with_extension("jsonc").exists() {
                path.with_extension("jsonc")
            } else {
                path
            }
        }
        "openclaw" => crate::openclaw_config::get_openclaw_config_path(),
        "hermes" => crate::hermes_config::get_hermes_config_path(),
        "kimi" | "qwen" | "zcode" | "deepseek-harness" => {
            crate::services::agent_clients::config_path(app).map_err(|_| FAILED)?
        }
        _ => return Err(FAILED.into()),
    };
    read_selection(app, &path)
}

pub(crate) fn starting_model(app: &str) -> Result<Option<String>, String> {
    if STARTING_MODEL_APPS.contains(&app) {
        selection(app)
    } else {
        Ok(None)
    }
}

fn model_string(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let text = value.as_str().ok_or(FAILED)?.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() > 512
        || text.contains("://")
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '?' | '#' | '@' | '\\'))
    {
        return Err(FAILED.into());
    }
    Ok(Some(text.to_owned()))
}

fn field<'a>(value: &'a Value, path: &[&str]) -> Result<Option<&'a Value>, String> {
    let mut node = value;
    for key in path {
        if node.is_null() {
            return Ok(None);
        }
        let Some(next) = node.as_object().ok_or(FAILED)?.get(*key) else {
            return Ok(None);
        };
        node = next;
    }
    Ok(Some(node))
}

fn read_selection(app: &str, path: &Path) -> Result<Option<String>, String> {
    let format = match app {
        "codex" | "grokbuild" | "kimi" => Format::Toml,
        "hermes" => Format::Yaml,
        "deepseek-harness" => Format::Cordis,
        "opencode" | "openclaw" | "pi" => Format::Json5,
        _ => Format::Json,
    };
    let value = format.parse(read(path)?.as_deref())?;
    if !value.is_object() {
        return Err(FAILED.into());
    }
    match app {
        "deepseek-harness" => {
            model_string(crate::services::deepseek_harness::selection(&value).get("model"))
        }
        "claude" => model_string(field(&value, &["env", "ANTHROPIC_MODEL"])?)?
            .map_or_else(|| model_string(value.get("model")), |model| Ok(Some(model))),
        "codex" => {
            let name = model_string(value.get("profile"))?;
            let profile = name
                .as_deref()
                .map(|name| field(&value, &["profiles", name]))
                .transpose()?
                .flatten();
            model_string(profile.and_then(|profile| profile.get("model")))?
                .map_or_else(|| model_string(value.get("model")), |model| Ok(Some(model)))
        }
        "gemini" | "qwen" => model_string(field(&value, &["model", "name"])?),
        "grokbuild" => {
            let profile = model_string(field(&value, &["models", "default"])?)?;
            match profile {
                Some(profile) => model_string(field(&value, &["model", &profile, "model"])?)?
                    .map_or(Ok(Some(profile)), |model| Ok(Some(model))),
                None => Ok(None),
            }
        }
        "opencode" => model_string(value.get("model")),
        "openclaw" => {
            let model = field(&value, &["agents", "defaults", "model"])?;
            model_string(if model.is_some_and(Value::is_object) {
                model.and_then(|m| m.get("primary"))
            } else {
                model
            })
        }
        "hermes" => {
            let model = value.get("model");
            model_string(if model.is_some_and(Value::is_object) {
                model.and_then(|m| m.get("default"))
            } else {
                model
            })
        }
        "pi" => model_string(value.get("defaultModel")),
        "kimi" => model_string(value.get("default_model")),
        "zcode" => model_string(field(
            &value,
            &["config", "defaultModelSelection", "modelId"],
        )?),
        _ => Err(FAILED.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_starting_models_are_read_without_importing_or_writing() -> Result<(), String> {
        for (app, text, expected) in [
            ("claude", r#"{"model":"sonnet","env":{"ANTHROPIC_AUTH_TOKEN":"synthetic-only"}}"#, "sonnet"),
            ("codex", "model = 'native-sol'\n", "native-sol"),
            ("gemini", r#"{"model":{"name":"native-gemini"}}"#, "native-gemini"),
            ("grokbuild", "[models]\ndefault = 'profile'\n[model.profile]\nmodel = 'native-grok'\napi_key = 'synthetic-only'\n", "native-grok"),
            ("opencode", "{model:'typex/zai-org/GLM-5-FP8',}", "typex/zai-org/GLM-5-FP8"),
            ("openclaw", "{agents:{defaults:{model:{primary:'native-claw'}}}}", "native-claw"),
            ("hermes", "model:\n  default: native-hermes\n", "native-hermes"),
            ("pi", r#"{"defaultModel":"native-pi","defaultProvider":"synthetic-provider"}"#, "native-pi"),
            ("kimi", "default_model = 'native-kimi'\n", "native-kimi"),
            ("kimi", "default_model = 'GPT Example · Test provider · VaryAI'\n", "GPT Example · Test provider · VaryAI"),
            ("qwen", r#"{"model":{"name":"native-qwen"}}"#, "native-qwen"),
            ("qwen", r#"{"model":{"name":"GPT Example · Test provider · VaryAI"}}"#, "GPT Example · Test provider · VaryAI"),
            ("zcode", r#"{"config":{"defaultModelSelection":{"modelId":"native-zcode"}}}"#, "native-zcode"),
            ("zcode", r#"{"config":{"defaultModelSelection":{"modelId":"GPT Example · Test provider · VaryAI"}}}"#, "GPT Example · Test provider · VaryAI"),
            ("deepseek-harness", "- id: agent-default-model\n  config:\n    provider: deepseek-official\n    model: native-deepseek\n", "native-deepseek"),
        ] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let path = dir.path().join("config");
            fs::write(&path, text).map_err(|_| FAILED)?;
            assert_eq!(read_selection(app, &path)?, Some(expected.to_owned()), "{app}");
            assert_eq!(fs::read_to_string(&path).map_err(|_| FAILED)?, text);
        }
        Ok(())
    }

    #[test]
    fn defaults_and_read_errors_are_distinct_and_bounded() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("config.json");
        assert_eq!(read_selection("claude", &path)?, None);
        fs::write(
            &path,
            r#"{"theme":"light","env":{"ANTHROPIC_AUTH_TOKEN":"synthetic-only"}}"#,
        )
        .map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path)?, None);
        fs::write(&path, r#"{"model":{"unexpected":true}}"#).map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path), Err(FAILED.into()));
        for app in ["gemini", "qwen"] {
            fs::write(&path, r#"{"model":42}"#).map_err(|_| FAILED)?;
            assert_eq!(read_selection(app, &path), Err(FAILED.into()));
        }
        fs::write(&path, "[]").map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path), Err(FAILED.into()));
        fs::write(&path, "malformed synthetic config").map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path), Err(FAILED.into()));
        fs::write(
            &path,
            r#"{"model":"https://synthetic.invalid/?key=synthetic-only"}"#,
        )
        .map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path), Err(FAILED.into()));
        fs::write(&path, vec![b' '; 4 * 1024 * 1024 + 1]).map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path), Err(FAILED.into()));
        Ok(())
    }

    #[test]
    fn codex_profile_and_claude_env_overrides_are_respected() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("config");
        fs::write(
            &path,
            "profile = 'work'\nmodel = 'base'\n[profiles.work]\nmodel = 'profile-model'\n",
        )
        .map_err(|_| FAILED)?;
        assert_eq!(
            read_selection("codex", &path)?,
            Some("profile-model".into())
        );
        fs::write(
            &path,
            r#"{"model":"base","env":{"ANTHROPIC_MODEL":"env-model"}}"#,
        )
        .map_err(|_| FAILED)?;
        assert_eq!(read_selection("claude", &path)?, Some("env-model".into()));
        Ok(())
    }
}
