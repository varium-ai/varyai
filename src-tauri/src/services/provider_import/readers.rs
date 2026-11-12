use super::*;
use crate::services::codex_provider::ModelMode;

pub(super) fn claude(path: &Path) -> Result<Vec<Candidate>, String> {
    let settings = json_file(path)?;
    Ok(vec![client(
        "settings".into(),
        "Claude Code",
        "Claude Code".into(),
        &settings,
    )])
}

fn client(reference: String, app: &str, name: String, settings: &Value) -> Candidate {
    let env = &settings["env"];
    let mut rows = models(&settings["modelCatalog"]["models"]);
    let (mut base, key, format) = match app {
        "claude" | "claude-desktop" | "Claude Code" => {
            for field in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_SMALL_FAST_MODEL",
                "ANTHROPIC_DEFAULT_SUBAGENT_MODEL",
            ] {
                let id = text(env, &[field]);
                if !id.is_empty() {
                    rows.push(json!({"model":id}));
                }
            }
            let format = text(settings, &["apiFormat", "api_format"]);
            let format = if format.is_empty() {
                "anthropic".into()
            } else {
                format
            };
            (
                text(env, &["ANTHROPIC_BASE_URL"]),
                text(env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]),
                format,
            )
        }
        "codex" => {
            let config = text(settings, &["config"]).parse::<toml::Value>().ok();
            let table = config.as_ref().and_then(|c| {
                c.get("model_providers")?
                    .get(c.get("model_provider")?.as_str()?)
            });
            let base = table
                .and_then(|c| c.get("base_url")?.as_str())
                .unwrap_or("")
                .into();
            let mut key = text(&settings["auth"], &["OPENAI_API_KEY"]);
            if key.is_empty() {
                key = table
                    .and_then(|c| c.get("experimental_bearer_token")?.as_str())
                    .unwrap_or("")
                    .into();
            }
            if let Some(model) = config.as_ref().and_then(|c| c.get("model")?.as_str()) {
                let mut row = json!({"model":model});
                reasoning::selected(
                    &mut row,
                    config
                        .as_ref()
                        .and_then(|c| c.get("model_reasoning_effort")?.as_str()),
                );
                rows.push(row);
            }
            let format = match table.and_then(|c| c.get("wire_api")?.as_str()) {
                Some("chat") => "openai_chat",
                _ => "openai_responses",
            };
            (base, key, format.into())
        }
        "gemini" => {
            let model = text(env, &["GEMINI_MODEL"]);
            if !model.is_empty() {
                rows.push(json!({"model":model}));
            }
            (
                text(env, &["GOOGLE_GEMINI_BASE_URL"]),
                text(env, &["GEMINI_API_KEY", "GOOGLE_API_KEY"]),
                "gemini_native".into(),
            )
        }
        _ => {
            rows.extend(models(&settings["models"]));
            let mut base = text(settings, &["baseUrl", "baseURL", "base_url"]);
            if base.is_empty() {
                base = text(&settings["options"], &["baseURL", "baseUrl", "base_url"]);
            }
            let mut key = text(settings, &["apiKey", "api_key"]);
            if key.is_empty() {
                key = text(&settings["options"], &["apiKey", "api_key"]);
            }
            let api = text(settings, &["apiFormat", "api_format", "api", "npm", "type"]);
            (base, key, format(&api).into())
        }
    };
    if base.is_empty()
        && !key.is_empty()
        && matches!(
            app,
            "claude" | "claude-desktop" | "Claude Code" | "codex" | "gemini"
        )
    {
        base = match format.as_str() {
            "anthropic" => "https://api.anthropic.com",
            "gemini_native" => "https://generativelanguage.googleapis.com/v1beta",
            _ => "https://api.openai.com/v1",
        }
        .into();
    }
    let mut rows = models(&json!(rows));
    let mut generated_gateway = false;
    if app == "codex" {
        if let Ok(config) = text(settings, &["config"]).parse::<toml::Value>() {
            generated_gateway = config
                .get("model_provider")
                .and_then(toml::Value::as_str)
                .is_some_and(gateways::generated_id);
            if let Some(model) = config.get("model").and_then(toml::Value::as_str) {
                for row in rows.iter_mut().filter(|row| row["model"] == model) {
                    reasoning::selected(
                        row,
                        config
                            .get("model_reasoning_effort")
                            .and_then(toml::Value::as_str),
                    );
                }
            }
        }
    }
    if matches!(app, "claude" | "claude-desktop" | "Claude Code") {
        reasoning::claude(&mut rows, settings);
    }
    let mut result = candidate(reference, app, name, base, key.clone(), &format, rows);
    result.generated_gateway |= generated_gateway;
    if key.is_empty() {
        result.skip = Some("subscription".into());
    }
    if let UpstreamSource::Api { auth_type, .. } = &mut result.record.source {
        if format == "anthropic"
            && text(env, &["ANTHROPIC_AUTH_TOKEN"]).is_empty()
            && !text(env, &["ANTHROPIC_API_KEY"]).is_empty()
        {
            *auth_type = Some("api-key".into());
        }
    }
    let headers = settings
        .get("headers")
        .or_else(|| settings.pointer("/options/headers"));
    if let Some(headers) = headers {
        set_headers(&mut result, headers);
    }
    result
}

fn format(api: &str) -> &str {
    let lower = api.to_ascii_lowercase();
    if lower.contains("anthropic") {
        "anthropic"
    } else if lower.contains("responses") || lower.contains("codex") {
        "openai_responses"
    } else if lower.contains("gemini") || lower.contains("google") {
        "gemini_native"
    } else {
        "openai_chat"
    }
}
fn set_headers(result: &mut Candidate, value: &Value) {
    if value.is_null() {
        return;
    }
    let Some(headers) = value.as_object() else {
        result.skip = Some("unsupported".into());
        return;
    };
    let mut parsed = BTreeMap::new();
    for (name, value) in headers {
        let Some(value) = value.as_str() else {
            result.skip = Some("unsupported".into());
            return;
        };
        parsed.insert(name.to_ascii_lowercase(), value.into());
    }
    result
        .record
        .transport
        .get_or_insert_with(Default::default)
        .headers = Some(parsed);
}

pub(super) fn cc_switch(path: &Path) -> Result<Vec<Candidate>, String> {
    let mut out = vec![];
    if path.extension().is_some_and(|ext| ext == "db") {
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| FAILED)?;
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| FAILED)?;
        let columns = conn
            .prepare("PRAGMA table_info(providers)")
            .map_err(|_| FAILED)?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|_| FAILED)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| FAILED)?;
        let meta = if columns.iter().any(|name| name == "meta") {
            "COALESCE(meta,'{}')"
        } else {
            "'{}'"
        };
        let icon = if columns.iter().any(|name| name == "icon") {
            "COALESCE(icon,'')"
        } else {
            "''"
        };
        let sql = format!("SELECT id,app_type,name,settings_config,{meta},{icon} FROM providers ORDER BY app_type,id LIMIT 513");
        let mut stmt = conn.prepare(&sql).map_err(|_| FAILED)?;
        let entries = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|_| FAILED)?;
        for entry in entries {
            let (id, app, name, raw, meta, icon) = entry.map_err(|_| FAILED)?;
            if raw.len() as u64 > MAX_FILE {
                return Err(INVALID.into());
            }
            let settings: Value = serde_json::from_str(&raw).map_err(|_| INVALID)?;
            let meta: Value = serde_json::from_str(&meta).map_err(|_| INVALID)?;
            let mut c = client(format!("{app}/{id}"), &app, name, &settings);
            c.record.icon = crate::services::provider_icons::imported(&icon, None);
            apply_meta(&mut c, &meta);
            out.push(c);
        }
    } else {
        let value = json_file(path)?;
        for (app, section) in value.as_object().ok_or(INVALID)? {
            if let Some(providers) = section["providers"].as_object() {
                for (id, provider) in providers {
                    let mut c = client(
                        format!("{app}/{id}"),
                        app,
                        text(provider, &["name"]),
                        &provider["settingsConfig"],
                    );
                    c.record.icon =
                        crate::services::provider_icons::imported(&text(provider, &["icon"]), None);
                    apply_meta(&mut c, &provider["meta"]);
                    out.push(c);
                }
            }
        }
    }
    Ok(out)
}

fn apply_meta(candidate: &mut Candidate, meta: &Value) {
    if let UpstreamSource::Api {
        api_format,
        auth_type,
        ..
    } = &mut candidate.record.source
    {
        let api = text(meta, &["apiFormat"]);
        if !api.is_empty() {
            if crate::services::routing_upstreams::valid_format(&api) {
                *api_format = api;
            } else {
                candidate.skip = Some("unsupported".into());
            }
        }
        if meta["apiKeyField"] == "ANTHROPIC_API_KEY" {
            *auth_type = Some("api-key".into());
        }
    }
    if meta["isFullUrl"] == true {
        candidate
            .record
            .transport
            .get_or_insert_with(Default::default)
            .full_url = true;
    }
    if let Some(overrides) = meta.get("localProxyRequestOverrides") {
        set_headers(candidate, &overrides["headers"]);
        if !overrides["body"].is_null() {
            candidate.skip = Some("unsupported".into());
        }
    }
}

pub(super) fn codex(path: &Path, home: &Path) -> Result<Vec<Candidate>, String> {
    let raw = bytes(path)?;
    let value = std::str::from_utf8(&raw)
        .map_err(|_| INVALID)?
        .trim_start_matches('\u{feff}')
        .parse::<toml::Value>()
        .map_err(|_| INVALID)?;
    let auth_path = path.with_file_name("auth.json");
    let auth = if auth_path.exists() {
        json_file(&auth_path)?
    } else {
        json!({})
    };
    let mut out = vec![];
    let Some(tables) = value.get("model_providers").and_then(toml::Value::as_table) else {
        return Ok(out);
    };
    for (id, table) in tables {
        let str_value = |field: &str| table.get(field).and_then(toml::Value::as_str).unwrap_or("");
        let base = str_value("base_url");
        let mut key = str_value("experimental_bearer_token").to_owned();
        if key.is_empty() {
            if let Some(env_key) = table.get("env_key").and_then(toml::Value::as_str) {
                key = std::env::var(env_key).unwrap_or_default();
            } else if table
                .get("requires_openai_auth")
                .and_then(toml::Value::as_bool)
                == Some(true)
                || value.get("model_provider").and_then(toml::Value::as_str) == Some(id)
            {
                key = text(&auth, &["OPENAI_API_KEY"]);
            }
        }
        let mut rows = vec![];
        let mut include = |config: &toml::Value| {
            let provider = config
                .get("model_provider")
                .and_then(toml::Value::as_str)
                .or_else(|| value.get("model_provider").and_then(toml::Value::as_str));
            if provider != Some(id) {
                return;
            }
            if let Some(model) = config.get("model").and_then(toml::Value::as_str) {
                let mut row = json!({"model":model});
                reasoning::selected(
                    &mut row,
                    config
                        .get("model_reasoning_effort")
                        .and_then(toml::Value::as_str),
                );
                rows.push(row);
            }
            if let Some(catalog) = config
                .get("model_catalog_json")
                .and_then(toml::Value::as_str)
            {
                let catalog = if let Some(relative) = catalog.strip_prefix("~/") {
                    home.join(relative)
                } else {
                    path.parent().unwrap_or(home).join(catalog)
                };
                if !catalog
                    .file_name()
                    .is_some_and(|n| n == "magpie-models.json" || n == "vary-models.json")
                {
                    if let Ok(value) = json_file(&catalog) {
                        rows.extend(
                            models(&value["models"])
                                .into_iter()
                                .filter(|r| r["visibility"] != "hide"),
                        );
                    }
                }
            }
        };
        include(&value);
        if let Some(profiles) = value.get("profiles").and_then(toml::Value::as_table) {
            for profile in profiles.values() {
                include(profile);
            }
        }
        let wire = str_value("wire_api");
        let format = if wire == "chat" {
            "openai_chat"
        } else {
            "openai_responses"
        };
        let name = if str_value("name").is_empty() {
            id.clone()
        } else {
            str_value("name").into()
        };
        let mut result = candidate(
            id.clone(),
            "config.toml",
            name,
            base.into(),
            key,
            format,
            models(&json!(rows)),
        );
        if let Some(headers) = table
            .get("http_headers")
            .and_then(|v| serde_json::to_value(v).ok())
        {
            set_headers(&mut result, &headers);
        }
        if table.get("env_http_headers").is_some() {
            result.skip = Some("unsupported".into());
        }
        out.push(result);
    }
    Ok(out)
}

fn preset(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "openai" => ("https://api.openai.com/v1", "openai_chat"),
        "anthropic" => ("https://api.anthropic.com", "anthropic"),
        "google" => (
            "https://generativelanguage.googleapis.com/v1beta",
            "gemini_native",
        ),
        "deepseek" => ("https://api.deepseek.com/v1", "openai_chat"),
        "openrouter" => ("https://openrouter.ai/api/v1", "openai_chat"),
        "groq" => ("https://api.groq.com/openai/v1", "openai_chat"),
        "xai" => ("https://api.x.ai/v1", "openai_chat"),
        "mistral" => ("https://api.mistral.ai/v1", "openai_chat"),
        "moonshotai" | "moonshot" => ("https://api.moonshot.ai/v1", "openai_chat"),
        "moonshotai-cn" | "moonshot-cn" => ("https://api.moonshot.cn/v1", "openai_chat"),
        "siliconflow-cn" | "siliconflow" => ("https://api.siliconflow.cn/v1", "openai_chat"),
        "togetherai" | "together" => ("https://api.together.xyz/v1", "openai_chat"),
        "fireworks-ai" | "fireworks" => ("https://api.fireworks.ai/inference/v1", "openai_chat"),
        "nvidia" => ("https://integrate.api.nvidia.com/v1", "openai_chat"),
        "opencode" | "opencode-zen" => ("https://opencode.ai/zen/v1", "openai_chat"),
        _ => return None,
    })
}
pub(super) fn opencode(paths: &ImportPaths) -> Result<Vec<Candidate>, String> {
    let settings = if paths.opencode.exists() {
        json_file(&paths.opencode)?
    } else {
        json!({})
    };
    let auth = if paths.opencode_auth.exists() {
        json_file(&paths.opencode_auth)?
    } else {
        json!({})
    };
    let mut out = vec![];
    let mut ids = std::collections::BTreeSet::new();
    if let Some(providers) = settings["provider"].as_object() {
        ids.extend(providers.keys().cloned());
    }
    if let Some(accounts) = auth.as_object() {
        ids.extend(accounts.keys().cloned());
    }
    for id in ids {
        let config = &settings["provider"][&id];
        let login = &auth[&id];
        let mut config = if config.is_object() {
            config.clone()
        } else {
            json!({})
        };
        let mut base = text(&config["options"], &["baseURL", "baseUrl"]);
        if base.is_empty() {
            if let Some((url, api)) = preset(&id) {
                base = url.into();
                config["apiFormat"] = json!(api);
            }
        }
        config["baseUrl"] = json!(base);
        if text(&config["options"], &["apiKey"]).is_empty() {
            config["apiKey"] = json!(text(login, &["key"]));
        }
        let name = text(&config, &["name"]);
        let mut result = client(
            id.clone(),
            "OpenCode",
            if name.is_empty() { id.clone() } else { name },
            &config,
        );
        result.record.icon =
            crate::services::provider_icons::imported(&text(&config, &["icon"]), None);
        if matches!(login["type"].as_str(), Some("oauth" | "wellknown"))
            && text(&config["options"], &["apiKey"]).is_empty()
        {
            result.skip = Some("subscription".into());
        }
        if text(&config["options"], &["apiKey"]).starts_with('{') {
            result.skip = Some("unresolved".into());
        }
        out.push(result);
    }
    Ok(out)
}

fn magpie_key_models(provider: &Value, key: &str) -> Option<Vec<String>> {
    let reference = format!("{:x}", Sha256::digest(key.as_bytes()))[..10].to_owned();
    if let Some(rows) = provider["accountModels"]
        .get(&reference)
        .and_then(Value::as_array)
    {
        if !rows.is_empty() {
            return Some(
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            );
        }
    }
    if provider["accountModelsAll"][&reference] == true {
        return None;
    }
    let rows = models(&provider["models"]);
    rows.iter().any(|row| row["keys"].is_array()).then(|| {
        rows.iter()
            .filter(|row| {
                row["keys"]
                    .as_array()
                    .is_some_and(|keys| keys.iter().any(|key| key.as_str() == Some(&reference)))
            })
            .filter_map(|row| crate::services::routing_upstreams::model_id(row).map(str::to_owned))
            .collect()
    })
}

pub(super) fn provider_backup(path: &Path) -> Result<Vec<Candidate>, String> {
    let value = json_file(path)?;
    let settings_path = path.with_file_name("settings.json");
    let settings = if settings_path.try_exists().map_err(|_| FAILED)? {
        json_file(&settings_path)?
    } else {
        json!({})
    };
    if !settings.is_object() {
        return Err(INVALID.into());
    }
    let providers = value
        .get("providers")
        .and_then(Value::as_array)
        .ok_or(INVALID)?;
    let mut out = vec![];
    for (index, p) in providers.iter().enumerate() {
        if p["hidden"] == true {
            continue;
        }
        let id = text(p, &["id"]);
        let reference = format!("{index}/{id}");
        let mut endpoints = BTreeMap::new();
        for (field, format) in [
            ("chat", "openai_chat"),
            ("responses", "openai_responses"),
            ("anthropic", "anthropic"),
            ("gemini", "gemini_native"),
        ] {
            let url = text(p, &[field]);
            if !url.is_empty() {
                endpoints.insert(format.into(), url);
            }
        }
        if endpoints.is_empty() {
            if let Some((url, api)) = preset(&text(p, &["preset"])) {
                endpoints.insert(api.into(), url.into());
            }
        }
        let base_api = format(&text(p, &["baseAPI"])).to_owned();
        let primary_protocol = text(p, &["keyProtocol"]);
        let selected = if !primary_protocol.is_empty() {
            format(&primary_protocol).into()
        } else if endpoints.contains_key(&base_api) {
            base_api
        } else {
            endpoints
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "openai_chat".into())
        };
        let mut result = candidate(
            reference,
            "providers.json",
            text(p, &["name", "id"]),
            endpoints.get(&selected).cloned().unwrap_or_default(),
            text(p, &["key"]),
            &selected,
            models(&p["models"]),
        );
        result.record.id = id.clone();
        result.record.icon =
            crate::services::provider_icons::imported(&text(p, &["icon"]), path.parent());
        result.record.enabled = p["off"] != true;
        let mut transport = ImportedTransport {
            primary_name: p
                .get("keyName")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
            primary_models: magpie_key_models(p, &text(p, &["key"])),
            endpoints,
            primary_protocol: (!primary_protocol.is_empty()).then_some(selected.clone()),
            ..Default::default()
        };
        if let Some(keys) = p["keys"].as_array() {
            for (index, key) in keys.iter().enumerate() {
                let protocol = text(key, &["protocol"]);
                transport.keys.push(ImportedKey {
                    id: format!("import-key-{index}"),
                    name: text(key, &["name"]),
                    key: Some(text(key, &["key"])),
                    enabled: key["off"] != true,
                    api_format: (!protocol.is_empty()).then(|| format(&protocol).into()),
                    models: magpie_key_models(p, &text(key, &["key"])),
                });
            }
        }
        result.record.transport = Some(transport);
        set_headers(&mut result, &p["headers"]);
        let anthropic = result
            .record
            .transport
            .as_ref()
            .and_then(|t| t.endpoints.get("anthropic"));
        let official_anthropic = anthropic
            .and_then(|u| url::Url::parse(u).ok())
            .is_some_and(|u| u.host_str() == Some("api.anthropic.com"));
        if let UpstreamSource::Api {
            mode,
            groups_only,
            auth_type,
            models,
            catalog,
            ..
        } = &mut result.record.source
        {
            *groups_only = p["unlisted"] == true;
            if p["noModels"] == true {
                *mode = ModelMode::Usage;
            } else if p["bestInFamily"] == true {
                *mode = ModelMode::Best;
            }
            if p["authHeader"] == "x-api-key" || official_anthropic {
                *auth_type = Some("api-key".into());
            } else if anthropic.is_some() {
                *auth_type = Some(
                    if p["authHeader"] == "bearer" {
                        "bearer"
                    } else {
                        "both"
                    }
                    .into(),
                );
            }
            let mut vendor = super::models(&p["pickedFrom"]);
            for row in models.iter() {
                if let Some(existing) = vendor.iter_mut().find(|v| {
                    crate::services::routing_upstreams::model_id(v)
                        == crate::services::routing_upstreams::model_id(row)
                }) {
                    if let (Some(existing), Some(row)) = (existing.as_object_mut(), row.as_object())
                    {
                        for (key, value) in row {
                            existing.entry(key).or_insert_with(|| value.clone());
                        }
                    }
                } else {
                    vendor.push(row.clone());
                }
            }
            *catalog = vendor;
            for row in models.iter_mut().chain(catalog.iter_mut()) {
                if let Some(id) = row["model"].as_str().map(str::to_owned) {
                    if let Some(context) = p["contexts"]
                        .get(&id)
                        .or_else(|| p["contexts"].get("*"))
                        .and_then(Value::as_u64)
                        .filter(|n| *n > 0 && *n <= (i64::MAX as u64 / 100))
                    {
                        row["contextWindow"] = json!(context);
                        row["contextSource"] = json!("custom");
                    }
                    let prefs = reasoning::preferences(
                        &text(p, &["id"]),
                        &id,
                        &p["modelPrefs"][&id],
                        &settings,
                    )?;
                    if prefs.as_object().is_some_and(|prefs| !prefs.is_empty()) {
                        let mut existing = row["modelPreferences"]
                            .as_object()
                            .cloned()
                            .unwrap_or_default();
                        if let Some(prefs) = prefs.as_object() {
                            existing.extend(prefs.clone());
                        }
                        row["modelPreferences"] = Value::Object(existing);
                    }
                }
            }
        }
        if p["hidden"] == true
            || (text(p, &["key"]).is_empty()
                && matches!(id.as_str(), "codex" | "claude" | "copilot" | "workbuddy"))
        {
            result.skip = Some("subscription".into());
        }
        out.push(result);
    }
    Ok(out)
}
