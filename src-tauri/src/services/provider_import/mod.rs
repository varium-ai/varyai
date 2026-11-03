//! Explicit, read-only app imports. Credentials never cross the preview boundary.
mod readers;
mod subscriptions;
pub(crate) use subscriptions::{import_with_accounts, preview_with_accounts};
#[cfg(test)]
mod tests;

use crate::{
    database::Database,
    services::routing_upstreams::{
        ImportedKey, ImportedTransport, RoutingUpstream, UpstreamSource,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

const FAILED: &str = "providerImport.failed";
const CONFLICT: &str = "providerImport.conflict";
const INVALID: &str = "providerImport.invalid";
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_ITEMS: usize = 512;

#[derive(Clone)]
pub(crate) struct ImportPaths {
    cc_switch: PathBuf,
    claude: PathBuf,
    codex: PathBuf,
    opencode: PathBuf,
    opencode_auth: PathBuf,
    provider_backup: PathBuf,
    home: PathBuf,
}
impl ImportPaths {
    pub(crate) fn local(source_path: Option<String>) -> Self {
        let home = crate::config::get_home_dir();
        let mut paths = Self::at(&home);
        paths.claude = crate::config::get_claude_settings_path();
        paths.codex = crate::codex_config::get_codex_config_path();
        paths.opencode = crate::opencode_config::get_opencode_config_path();
        if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            paths.opencode_auth = PathBuf::from(data).join("opencode/auth.json");
        }
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            paths.provider_backup = PathBuf::from(config).join("magpie/providers.json");
        }
        if let Some(path) = source_path {
            paths.provider_backup = PathBuf::from(path);
            if paths
                .provider_backup
                .file_name()
                .is_some_and(|name| name == "logins.json")
            {
                paths.provider_backup.set_file_name("providers.json");
            }
        }
        if !paths.cc_switch.exists() && paths.cc_switch.with_file_name("config.json").exists() {
            paths.cc_switch = paths.cc_switch.with_file_name("config.json");
        }
        if !paths.opencode.exists() && paths.opencode.with_extension("jsonc").exists() {
            paths.opencode = paths.opencode.with_extension("jsonc");
        }
        paths
    }
    fn at(home: &Path) -> Self {
        Self {
            cc_switch: home.join(".cc-switch/cc-switch.db"),
            claude: home.join(".claude/settings.json"),
            codex: home.join(".codex/config.toml"),
            opencode: home.join(".config/opencode/opencode.json"),
            opencode_auth: home.join(".local/share/opencode/auth.json"),
            provider_backup: home.join(".config/magpie/providers.json"),
            home: home.into(),
        }
    }
    fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.home)
            .map(|p| format!("~/{}", p.display()))
            .unwrap_or_else(|_| path.to_string_lossy().into_owned())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Source {
    pub id: String,
    pub name: String,
    pub path: String,
    pub found: bool,
    pub error: Option<String>,
    pub items: Vec<Item>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    pub reference: String,
    pub fingerprint: String,
    pub name: String,
    pub from: String,
    pub host: String,
    pub model_count: usize,
    pub status: String,
    pub existing: Option<String>,
    pub skip: Option<String>,
    pub enabled: bool,
    pub subscription: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ImportMode {
    Add,
    Key,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Pick {
    pub source: String,
    pub reference: String,
    pub fingerprint: String,
    pub mode: ImportMode,
    pub name: Option<String>,
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportResult {
    pub added: usize,
    pub skipped: usize,
}

pub(crate) struct Candidate {
    reference: String,
    from: String,
    record: RoutingUpstream,
    skip: Option<String>,
    subscription: Option<Value>,
}
struct PrivateSource {
    public: Source,
    candidates: Vec<Candidate>,
}

fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| FAILED)?;
    if !file.metadata().map_err(|_| FAILED)?.is_file() {
        return Err(INVALID.into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FAILED)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(INVALID.into());
    }
    Ok(bytes)
}
fn json_file(path: &Path) -> Result<Value, String> {
    let raw = bytes(path)?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| INVALID)?
        .trim_start_matches('\u{feff}');
    json5::from_str(text).map_err(|_| INVALID.into())
}
fn text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| value.get(k)?.as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or("")
        .trim()
        .into()
}
fn models(value: &Value) -> Vec<Value> {
    let rows = if let Some(rows) = value.as_array() {
        rows.iter()
            .filter_map(|row| {
                let id = row
                    .as_str()
                    .or_else(|| row["model"].as_str())
                    .or_else(|| row["id"].as_str())
                    .or_else(|| row["slug"].as_str())?;
                let mut result = if row.is_object() {
                    row.clone()
                } else {
                    json!({})
                };
                result["model"] = json!(id);
                Some(result)
            })
            .collect::<Vec<_>>()
    } else if let Some(rows) = value.as_object() {
        rows.iter()
            .map(|(id, row)| {
                let mut result = if row.is_object() {
                    row.clone()
                } else {
                    json!({})
                };
                result["model"] = json!(id);
                result
            })
            .collect()
    } else {
        vec![]
    };
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| {
            crate::services::routing_upstreams::model_id(row)
                .is_some_and(|id| seen.insert(id.to_owned()))
        })
        .collect()
}

fn candidate(
    reference: String,
    from: &str,
    name: String,
    base: String,
    key: String,
    format: &str,
    rows: Vec<Value>,
) -> Candidate {
    use crate::services::codex_provider::ModelMode;
    let mut record = RoutingUpstream {
        schema_version: 1,
        id: "imported".into(),
        name: if name.is_empty() {
            "Imported provider".into()
        } else {
            name
        },
        revision: 0,
        source: UpstreamSource::Api {
            base_url: base.trim_end_matches('/').into(),
            api_key: Some(key),
            api_format: format.into(),
            mode: if rows.is_empty() {
                ModelMode::Vendor
            } else {
                ModelMode::Picked
            },
            catalog: rows.clone(),
            models: rows,
            groups_only: false,
            auth_type: None,
        },
        has_api_key: false,
        enabled: true,
        preset_id: None,
        region: None,
        codex_settings: None,
        transport: None,
    };
    let skip = if let UpstreamSource::Api {
        base_url, api_key, ..
    } = &record.source
    {
        let own = url::Url::parse(base_url).ok().is_some_and(|url| {
            matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
                && (matches!(
                    url.port(),
                    Some(15721) | Some(crate::proxy::types::DEFAULT_PROXY_PORT)
                ) || url.path().contains("/connections/")
                    || url.path().contains("/codex-menu"))
        });
        if own {
            Some("selfRoute")
        } else if base_url.is_empty() {
            Some("missingUrl")
        } else if api_key.as_deref().is_none_or(str::is_empty) {
            Some("missingKey")
        } else {
            None
        }
    } else {
        None
    };
    let skip = skip.map(str::to_owned);
    if record.name.chars().count() > 120 {
        record.name = record.name.chars().take(120).collect();
    }
    Candidate {
        reference,
        from: from.into(),
        record,
        skip,
        subscription: None,
    }
}
fn fingerprint(candidate: &Candidate) -> Result<String, String> {
    let value = serde_json::to_vec(&(&candidate.record, &candidate.skip, &candidate.subscription))
        .map_err(|_| FAILED)?;
    Ok(format!("{:x}", Sha256::digest(value)))
}
fn endpoints(record: &RoutingUpstream) -> Vec<String> {
    let mut urls = record
        .transport
        .as_ref()
        .map(|t| t.endpoints.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if let UpstreamSource::Api { base_url, .. } = &record.source {
        urls.push(base_url.clone());
    }
    urls.into_iter()
        .filter_map(|url| {
            url::Url::parse(&url)
                .ok()
                .map(|u| u.as_str().trim_end_matches('/').to_owned())
        })
        .collect()
}
fn keys(record: &RoutingUpstream) -> Vec<&str> {
    let mut keys = vec![];
    if let UpstreamSource::Api {
        api_key: Some(key), ..
    } = &record.source
    {
        keys.push(key.as_str());
    }
    if let Some(transport) = &record.transport {
        keys.extend(transport.keys.iter().filter_map(|k| k.key.as_deref()));
    }
    keys
}
fn protocol_endpoints(record: &RoutingUpstream) -> BTreeMap<String, String> {
    let mut urls = record
        .transport
        .as_ref()
        .map(|t| t.endpoints.clone())
        .unwrap_or_default();
    if let UpstreamSource::Api {
        base_url,
        api_format,
        ..
    } = &record.source
    {
        urls.insert(api_format.clone(), base_url.clone());
    }
    urls
}
fn self_route(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
        ) && (matches!(
            url.port(),
            Some(15721) | Some(crate::proxy::types::DEFAULT_PROXY_PORT)
        ) || url.path().contains("/connections/")
            || url.path().contains("/codex-menu"))
    })
}
pub(crate) fn same_vendor(a: &RoutingUpstream, b: &RoutingUpstream) -> bool {
    let headers = |r: &RoutingUpstream| {
        r.transport
            .as_ref()
            .and_then(|t| t.headers.clone())
            .unwrap_or_default()
    };
    matches!((&a.source, &b.source), (UpstreamSource::Api { auth_type: aa, .. }, UpstreamSource::Api { auth_type: ba, .. }) if aa == ba)
        && a.transport.as_ref().is_some_and(|t| t.full_url)
            == b.transport.as_ref().is_some_and(|t| t.full_url)
        && headers(a) == headers(b)
        && endpoints(a).iter().any(|u| endpoints(b).contains(u))
}
pub(crate) fn already_added(a: &RoutingUpstream, b: &RoutingUpstream) -> bool {
    if matches!(
        (&a.source, &b.source),
        (UpstreamSource::Codex, UpstreamSource::Codex)
    ) {
        return match (&a.codex_settings, &b.codex_settings) {
            (Some(a), Some(b)) => {
                !b.accounts.is_empty() && b.accounts.iter().all(|id| a.accounts.contains(id))
            }
            _ => false,
        };
    }
    same_vendor(a, b)
        && keys(b).iter().all(|key| keys(a).contains(key))
        && protocol_endpoints(b).iter().all(|(format, url)| {
            protocol_endpoints(a)
                .get(format)
                .is_some_and(|have| have.trim_end_matches('/') == url.trim_end_matches('/'))
        })
}
fn scan(paths: &ImportPaths, have: &[RoutingUpstream]) -> Vec<PrivateSource> {
    let definitions = [
        ("cc-switch", "CC Switch", &paths.cc_switch),
        ("claude-code", "Claude Code", &paths.claude),
        ("codex", "Codex", &paths.codex),
        ("opencode", "OpenCode", &paths.opencode),
        ("magpie", "Provider backup", &paths.provider_backup),
    ];
    definitions
        .into_iter()
        .map(|(id, name, path)| {
            let found = path.try_exists().unwrap_or(true)
                || (id == "opencode" && paths.opencode_auth.try_exists().unwrap_or(true))
                || (id == "magpie"
                    && path
                        .with_file_name("logins.json")
                        .try_exists()
                        .unwrap_or(true));
            let result = if found {
                match id {
                    "cc-switch" => readers::cc_switch(path),
                    "claude-code" => readers::claude(path),
                    "codex" => readers::codex(path, &paths.home),
                    "opencode" => readers::opencode(paths),
                    _ => subscriptions::read(path),
                }
            } else {
                Ok(vec![])
            };
            let mut public = Source {
                id: id.into(),
                name: name.into(),
                path: paths.display(path),
                found,
                error: None,
                items: vec![],
            };
            let mut candidates = match result {
                Ok(c) if c.len() <= MAX_ITEMS => c,
                _ => {
                    public.error = Some(FAILED.into());
                    vec![]
                }
            };
            for candidate in &mut candidates {
                if protocol_endpoints(&candidate.record)
                    .values()
                    .any(|url| self_route(url))
                {
                    candidate.skip = Some("selfRoute".into());
                }
                if candidate.skip.is_none() && candidate.record.validate().is_err() {
                    candidate.skip = Some("unsupported".into());
                }
                let same = have.iter().find(|r| already_added(r, &candidate.record));
                let vendor = have.iter().find(|r| same_vendor(r, &candidate.record));
                let collision = have.iter().find(|r| r.name == candidate.record.name);
                let (status, existing) = if let Some(r) = same {
                    ("same", Some(r.name.clone()))
                } else if let Some(r) = vendor {
                    ("vendor", Some(r.name.clone()))
                } else if let Some(r) = collision {
                    ("taken", Some(r.name.clone()))
                } else {
                    ("new", None)
                };
                let host = endpoints(&candidate.record)
                    .first()
                    .and_then(|url| url::Url::parse(url).ok())
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .unwrap_or_default();
                let model_count = match &candidate.record.source {
                    UpstreamSource::Api { models, .. } => models.len(),
                    _ => 0,
                };
                public.items.push(Item {
                    reference: candidate.reference.clone(),
                    fingerprint: fingerprint(candidate).unwrap_or_default(),
                    name: candidate.record.name.clone(),
                    from: candidate.from.clone(),
                    host,
                    model_count,
                    status: status.into(),
                    existing,
                    skip: candidate.skip.clone(),
                    enabled: candidate.record.enabled,
                    subscription: candidate.subscription.is_some(),
                });
            }
            PrivateSource { public, candidates }
        })
        .collect()
}
#[cfg(test)]
pub(crate) fn preview(db: &Database, paths: &ImportPaths) -> Result<Vec<Source>, String> {
    let have = db.private_routing_upstreams()?;
    Ok(scan(paths, &have).into_iter().map(|s| s.public).collect())
}
pub(crate) struct Selected {
    pub record: RoutingUpstream,
    pub join: bool,
}
#[cfg(test)]
pub(crate) fn import(
    db: &Database,
    paths: &ImportPaths,
    picks: Vec<Pick>,
) -> Result<ImportResult, String> {
    if picks.is_empty() || picks.len() > 256 {
        return Err(INVALID.into());
    }
    let sources = scan(paths, &db.private_routing_upstreams()?);
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for pick in picks {
        if !seen.insert((pick.source.clone(), pick.reference.clone())) {
            return Err(INVALID.into());
        }
        let candidate = sources
            .iter()
            .find(|s| s.public.id == pick.source)
            .and_then(|s| s.candidates.iter().find(|c| c.reference == pick.reference))
            .ok_or(CONFLICT)?;
        if candidate.skip.is_some() || fingerprint(candidate)? != pick.fingerprint {
            return Err(CONFLICT.into());
        }
        let mut record = candidate.record.clone();
        record.id.clear();
        if let Some(name) = pick.name {
            if !name.trim().is_empty() {
                record.name = name.trim().into();
            }
        }
        selected.push(Selected {
            record,
            join: matches!(pick.mode, ImportMode::Key),
        });
    }
    db.import_native_providers(selected)
}

pub(crate) fn join_keys(
    target: &mut RoutingUpstream,
    incoming: &RoutingUpstream,
) -> Result<(), String> {
    if !same_vendor(target, incoming) {
        return Err(CONFLICT.into());
    }
    let mut have: HashSet<String> = keys(target).into_iter().map(str::to_owned).collect();
    let mut additions = vec![];
    if let UpstreamSource::Api {
        api_key: Some(key),
        api_format,
        ..
    } = &incoming.source
    {
        if have.insert(key.clone()) {
            additions.push(ImportedKey {
                id: uuid::Uuid::new_v4().to_string(),
                name: incoming.name.clone(),
                key: Some(key.clone()),
                enabled: incoming.enabled,
                api_format: incoming
                    .transport
                    .as_ref()
                    .map_or_else(|| Some(api_format.clone()), |t| t.primary_protocol.clone()),
            });
        }
    }
    if let Some(transport) = &incoming.transport {
        for key in &transport.keys {
            if key.key.as_ref().is_some_and(|key| have.insert(key.clone())) {
                let mut key = key.clone();
                key.id = uuid::Uuid::new_v4().to_string();
                additions.push(key);
            }
        }
    }
    let mut combined = target.transport.clone().unwrap_or_default();
    if let UpstreamSource::Api {
        base_url,
        api_format,
        ..
    } = &target.source
    {
        combined
            .endpoints
            .insert(api_format.clone(), base_url.clone());
    }
    if let UpstreamSource::Api {
        base_url,
        api_format,
        ..
    } = &incoming.source
    {
        if combined
            .endpoints
            .get(api_format)
            .is_some_and(|url| url.trim_end_matches('/') != base_url.trim_end_matches('/'))
        {
            return Err(CONFLICT.into());
        }
        combined
            .endpoints
            .insert(api_format.clone(), base_url.clone());
    }
    if let Some(transport) = &incoming.transport {
        for (format, url) in &transport.endpoints {
            if combined
                .endpoints
                .get(format)
                .is_some_and(|current| current.trim_end_matches('/') != url.trim_end_matches('/'))
            {
                return Err(CONFLICT.into());
            }
            combined.endpoints.insert(format.clone(), url.clone());
        }
    }
    combined.keys.extend(additions);
    target.transport = Some(combined);
    if let (
        UpstreamSource::Api {
            models, catalog, ..
        },
        UpstreamSource::Api {
            models: incoming_models,
            catalog: incoming_catalog,
            ..
        },
    ) = (&mut target.source, &incoming.source)
    {
        for (target, incoming) in [(models, incoming_models), (catalog, incoming_catalog)] {
            for row in incoming {
                if !target.iter().any(|have| {
                    crate::services::routing_upstreams::model_id(have)
                        == crate::services::routing_upstreams::model_id(row)
                }) {
                    target.push(row.clone());
                }
            }
        }
    }
    Ok(())
}
