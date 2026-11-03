//! Reference-style capacity publication; compaction never replaces the actual window.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DEFAULT_PERCENT: u64 = 95;
pub(crate) const WORKING_WINDOW: u64 = 272_000;
// Codex multiplies context sizes by percentages using signed integer arithmetic.
const MAX_TOKENS: u64 = i64::MAX as u64 / 100;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    #[serde(default)]
    pub full_window: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_at: Option<u64>,
}
impl Settings {
    pub fn valid(&self) -> bool {
        self.compact_at.is_none_or(|n| n > 0 && n <= MAX_TOKENS)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub effective_context_window_percent: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,
}

impl Metadata {
    pub fn parse(entry: &Value) -> Self {
        let number = |keys: &[&str]| {
            keys.iter().find_map(|key| {
                let n = match entry.get(*key)? {
                    Value::Number(n) => n.as_u64(),
                    Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                }?;
                (n > 0 && n <= MAX_TOKENS).then_some(n)
            })
        };
        Self {
            context_window: number(&[
                "contextWindow",
                "context_window",
                "context_length",
                "inputTokenLimit",
            ])
            .filter(|n| *n >= 2048),
            max_context_window: number(&["maxContextWindow", "max_context_window"])
                .filter(|n| *n >= 2048),
            effective_context_window_percent: [
                "effectiveContextWindowPercent",
                "effective_context_window_percent",
            ]
            .iter()
            .find_map(|key| number(&[key]).filter(|n| *n <= 100)),
            auto_compact_token_limit: number(&[
                "autoCompactTokenLimit",
                "auto_compact_token_limit",
            ]),
        }
    }

    pub fn valid(&self) -> bool {
        [
            self.context_window,
            self.max_context_window,
            self.auto_compact_token_limit,
        ]
        .into_iter()
        .flatten()
        .all(|n| n > 0 && n <= MAX_TOKENS)
            && self.context_window.is_none_or(|n| n >= 2048)
            && self.max_context_window.is_none_or(|n| n >= 2048)
            && self
                .effective_context_window_percent
                .is_none_or(|n| (1..=100).contains(&n))
    }

    pub fn with_fallback(self, fallback: Self) -> Self {
        Self {
            context_window: self.context_window.or(fallback.context_window),
            max_context_window: self.max_context_window.or(fallback.max_context_window),
            effective_context_window_percent: self
                .effective_context_window_percent
                .or(fallback.effective_context_window_percent),
            auto_compact_token_limit: self
                .auto_compact_token_limit
                .or(fallback.auto_compact_token_limit),
        }
    }

    pub fn for_provider(settings: &Value, model: &str) -> Self {
        let declared = settings
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["model"].as_str().is_some_and(|id| id.trim() == model))
            })
            .map(Self::parse)
            .unwrap_or_default();
        let configured = settings
            .get("config")
            .and_then(Value::as_str)
            .and_then(|text| text.parse::<toml::Value>().ok());
        let positive = |key: &str| {
            configured
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(toml::Value::as_integer)
                .and_then(|n| u64::try_from(n).ok())
                .filter(|n| *n > 0 && *n <= MAX_TOKENS)
        };
        let mut vendor = settings
            .pointer("/routingContext/vendor")
            .and_then(|v| v.get(model))
            .map(Self::parse)
            .unwrap_or_default();
        if !Self::typed_row(settings, model) {
            vendor = declared.clone().with_fallback(vendor);
        }
        let vendor_window = vendor
            .context_window
            .into_iter()
            .chain(vendor.max_context_window)
            .max();
        let provider_window = positive("model_context_window")
            .filter(|n| *n >= 2048)
            .map(|n| vendor_window.map_or(n, |most| n.min(most)));
        let exact_override = Self::typed_row(settings, model);
        let mut metadata = declared.clone().with_fallback(vendor.clone());
        metadata.context_window = if exact_override {
            declared.context_window.or(provider_window)
        } else {
            provider_window.or(metadata.context_window)
        };
        metadata.auto_compact_token_limit = declared
            .auto_compact_token_limit
            .or_else(|| positive("model_auto_compact_token_limit"))
            .or(vendor.auto_compact_token_limit);
        if let Some(fallback) = settings
            .pointer("/routingContext/models")
            .and_then(|v| v.get(model))
        {
            metadata = metadata.with_fallback(Self::parse(fallback));
        }
        metadata
    }

    pub fn typed_window(settings: &Value, model: &str) -> bool {
        Self::typed_row(settings, model)
            || settings
                .get("config")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<toml::Value>().ok())
                .and_then(|v| {
                    v.get("model_context_window")
                        .and_then(toml::Value::as_integer)
                })
                .is_some_and(|n| n >= 2048 && n as u64 <= MAX_TOKENS)
    }

    fn typed_row(settings: &Value, model: &str) -> bool {
        if let Some(typed) = settings
            .pointer("/routingContext/typed")
            .and_then(Value::as_array)
        {
            return typed.iter().any(|m| m.as_str() == Some(model));
        }
        settings
            .pointer("/modelCatalog/models")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter().any(|row| {
                    row["model"].as_str().is_some_and(|id| id.trim() == model)
                        && row["contextSource"] != "vendor"
                        && Self::parse(row).context_window.is_some()
                })
            })
    }

    pub fn session_typed_window(session: &crate::proxy::intent::Session) -> bool {
        if session.group.context_window.is_some_and(|n| n > 0) {
            return true;
        }
        let window = Self::for_session(session).context_window;
        window.is_some()
            && session.menu_targets().any(|target| {
                target.effort.context.resolved().context_window == window
                    && target.effort.typed_window
            })
    }

    /// Remove publication-only fields from the frozen transport snapshot, so a
    /// catalog refresh does not revoke opaque continuations at the same target.
    pub fn strip_from_settings(settings: &mut Value) {
        if let Some(object) = settings.as_object_mut() {
            object.remove("routingContext");
            object.remove("routingModelFacts");
        }
        if let Some(config) = settings.get("config").and_then(Value::as_str) {
            if let Ok(mut doc) = config.parse::<toml_edit::DocumentMut>() {
                // Canonicalize every parsed snapshot, including ones without these
                // fields, so adding an override does not change formatting identity.
                doc.as_table_mut().remove("model_context_window");
                doc.as_table_mut().remove("model_auto_compact_token_limit");
                if let Ok(value) = doc.to_string().parse::<toml::Value>() {
                    if let Ok(canonical) = toml::to_string(&value) {
                        settings["config"] = json!(canonical);
                    }
                }
            }
        }
        if let Some(entries) = settings
            .pointer_mut("/modelCatalog/models")
            .and_then(Value::as_array_mut)
        {
            for entry in entries {
                if let Some(object) = entry.as_object_mut() {
                    object.remove("displayName");
                    object.remove("display_name");
                    if let Some(prefs) = object
                        .get_mut("modelPreferences")
                        .and_then(Value::as_object_mut)
                    {
                        prefs.remove("name");
                        prefs.remove("sameAs");
                        if prefs.is_empty() {
                            object.remove("modelPreferences");
                        }
                    }
                    for field in [
                        "contextWindow",
                        "contextSource",
                        "context_window",
                        "context_length",
                        "inputTokenLimit",
                        "maxContextWindow",
                        "max_context_window",
                        "effectiveContextWindowPercent",
                        "effective_context_window_percent",
                        "autoCompactTokenLimit",
                        "auto_compact_token_limit",
                    ] {
                        object.remove(field);
                    }
                }
            }
        }
    }

    /// Copy only validated, supported fields; never persist arbitrary upstream metadata.
    pub fn write(&self, entry: &mut Value) {
        for (key, value) in [
            ("context_window", self.context_window),
            ("max_context_window", self.max_context_window),
            (
                "effective_context_window_percent",
                self.effective_context_window_percent,
            ),
            ("auto_compact_token_limit", self.auto_compact_token_limit),
        ] {
            if let Some(value) = value {
                entry[key] = json!(value);
            } else if let Some(object) = entry.as_object_mut() {
                object.remove(key);
            }
        }
    }

    /// Unknown capacity stays unknown; a small or unknown member cannot shrink a group.
    pub fn shared<'a>(members: impl Iterator<Item = &'a Self>) -> Self {
        let members = members.collect::<Vec<_>>();
        if let [one] = members.as_slice() {
            return one.resolved();
        }
        let resolved = members.iter().map(|m| m.resolved()).collect::<Vec<_>>();
        let window = resolved.iter().filter_map(|m| m.context_window).max();
        Self {
            context_window: window,
            max_context_window: None,
            effective_context_window_percent: Some(
                resolved
                    .iter()
                    .filter_map(|m| m.effective_context_window_percent)
                    .min()
                    .unwrap_or(DEFAULT_PERCENT),
            ),
            auto_compact_token_limit: resolved
                .iter()
                .filter_map(|m| m.auto_compact_token_limit)
                .min()
                .map(|n| window.map_or(n, |window| n.min(window))),
        }
    }

    fn resolved(&self) -> Self {
        let mut result = self.clone();
        result.context_window = self.context_window.or(self.max_context_window);
        if let (Some(window), Some(max)) = (result.context_window, result.max_context_window) {
            result.max_context_window = Some(max.max(window));
        }
        result.effective_context_window_percent = Some(
            self.effective_context_window_percent
                .unwrap_or(DEFAULT_PERCENT),
        );
        result
    }

    pub fn window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    pub fn publish(
        &self,
        settings: &Settings,
        typed_window: bool,
        vendor_catalog: bool,
        entry: &mut Value,
    ) {
        let mut metadata = self.resolved();
        if let Some(window) = metadata.context_window {
            let threshold = metadata.auto_compact_token_limit.or({
                if settings.full_window {
                    None
                } else if let Some(n) = settings.compact_at {
                    Some(n)
                } else if typed_window || vendor_catalog {
                    None
                } else {
                    Some(WORKING_WINDOW)
                }
            });
            metadata.auto_compact_token_limit = threshold.filter(|n| *n < window);
        }
        metadata.write(entry);
    }

    pub fn for_session(session: &crate::proxy::intent::Session) -> Self {
        let mut metadata = Self::shared(session.menu_targets().map(|t| &t.effort.context));
        match session.group.context_window {
            Some(-1) => {
                metadata.context_window = session
                    .menu_targets()
                    .filter_map(|t| t.effort.context.resolved().context_window)
                    .min();
                metadata.max_context_window = None;
            }
            Some(n) if n > 0 => {
                metadata.context_window = Some(n as u64);
                metadata.max_context_window = None;
            }
            _ => {}
        }
        metadata
    }

    pub fn valid_group_window(window: Option<i64>) -> bool {
        window.is_none_or(|n| n == -1 || n == 0 || (n >= 2048 && n as u64 <= MAX_TOKENS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_override_is_capped_only_by_vendor_facts_and_never_public_guesses() {
        let mut settings = json!({"config":"model_context_window=1000000",
            "modelCatalog":{"models":[{"model":"fixture","contextWindow":128000,"contextSource":"vendor"}]},
            "routingContext":{"models":{"fixture":{"context_window":64000}}}});
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(128000)
        );
        settings["modelCatalog"]["models"][0]["contextSource"] = json!("user");
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(128000)
        );
        settings["modelCatalog"]["models"][0]["contextWindow"] = json!(2000000);
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(2000000)
        );
        settings["modelCatalog"]["models"] = json!([]);
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(1000000)
        );
        settings["config"] = json!("");
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(64000)
        );
        assert!(!Metadata::typed_window(&settings, "fixture"));
        settings["routingContext"]["vendor"] = json!({"fixture":{"context_window":256000}});
        assert_eq!(
            Metadata::for_provider(&settings, "fixture").context_window,
            Some(256000)
        );
        assert!(!Metadata::typed_window(&settings, "fixture"));
    }

    #[test]
    fn compaction_policy_never_substitutes_the_actual_window() {
        let m = Metadata::parse(&json!({"context_window":1000000}));
        let mut entry = json!({});
        m.publish(&Settings::default(), false, false, &mut entry);
        assert_eq!(entry["context_window"], 1000000);
        assert_eq!(entry["auto_compact_token_limit"], 272000);
        for (settings, typed, vendor) in [
            (
                Settings {
                    full_window: true,
                    compact_at: None,
                },
                false,
                false,
            ),
            (Settings::default(), true, false),
            (Settings::default(), false, true),
        ] {
            m.publish(&settings, typed, vendor, &mut entry);
            assert_eq!(entry["context_window"], 1000000);
            assert!(entry.get("auto_compact_token_limit").is_none());
        }
        m.publish(
            &Settings {
                full_window: false,
                compact_at: Some(300000),
            },
            true,
            true,
            &mut entry,
        );
        assert_eq!(entry["auto_compact_token_limit"], 300000);
        Metadata::parse(&json!({"context_window":1000000,"auto_compact_token_limit":200000}))
            .publish(
                &Settings {
                    full_window: true,
                    compact_at: None,
                },
                false,
                false,
                &mut entry,
            );
        assert_eq!(entry["auto_compact_token_limit"], 200000);
        Metadata::default().publish(&Settings::default(), false, false, &mut entry);
        assert!(entry.get("context_window").is_none());
        assert!(entry.get("auto_compact_token_limit").is_none());
    }

    #[test]
    fn context_metadata_parses_only_explicit_valid_independent_fields() {
        let m = Metadata::parse(&json!({
            "contextWindow":null,"context_window":"1000000",
            "maxContextWindow":1200000,"auto_compact_token_limit":"200000",
            "effectiveContextWindowPercent":101,"effective_context_window_percent":90
        }));
        assert_eq!(m.context_window, Some(1000000));
        assert_eq!(m.max_context_window, Some(1200000));
        assert_eq!(m.auto_compact_token_limit, Some(200000));
        assert_eq!(m.effective_context_window_percent, Some(90));
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1M"),
            json!(u64::MAX),
            json!(true),
        ] {
            assert_eq!(
                Metadata::parse(&json!({"context_window":value})),
                Metadata::default()
            );
        }
        assert_eq!(
            Metadata::parse(&json!({"model":"large-model-1m"})),
            Metadata::default()
        );
        assert_eq!(
            Metadata::parse(&json!({"contextWindow":128000,"context_window":256000}))
                .context_window,
            Some(128000)
        );
    }

    #[test]
    fn context_group_capacity_uses_largest_known_member_without_inventing_unknown_windows() {
        let large =
            Metadata::parse(&json!({"context_window":1000000,"auto_compact_token_limit":200000}));
        let small = Metadata::parse(
            &json!({"context_window":128000,"effective_context_window_percent":90}),
        );
        let unknown = Metadata::default();
        let group = Metadata::shared([&large, &small].into_iter());
        assert_eq!(group.context_window, Some(1000000));
        assert_eq!(group.max_context_window, None);
        assert_eq!(group.auto_compact_token_limit, Some(200000));
        assert_eq!(group.effective_context_window_percent, Some(90));
        assert_eq!(
            Metadata::shared([&large, &unknown].into_iter()).context_window,
            Some(1000000)
        );
        assert_eq!(
            Metadata::shared([&large].into_iter()).context_window,
            Some(1000000)
        );
        assert_eq!(
            Metadata::shared([&small].into_iter()).auto_compact_token_limit,
            None
        );
        assert_eq!(
            Metadata::shared([&unknown].into_iter()).context_window,
            None
        );
        assert_eq!(Metadata::shared(std::iter::empty()).context_window, None);
    }

    #[test]
    fn context_length_is_a_supported_vendor_capacity_field() {
        assert_eq!(
            Metadata::parse(&json!({"context_length":1000000})).context_window,
            Some(1000000)
        );
    }

    #[test]
    fn provider_context_uses_exact_row_then_positive_top_level_overrides() {
        let settings = json!({"config":"model_context_window=256000\nmodel_auto_compact_token_limit=100000",
            "modelCatalog":{"models":[{"model":"exact","contextWindow":1000000}]}});
        assert_eq!(
            Metadata::for_provider(&settings, "exact").context_window,
            Some(1000000)
        );
        assert_eq!(
            Metadata::for_provider(&settings, "other").context_window,
            Some(256000)
        );
        assert_eq!(
            Metadata::for_provider(&settings, "exact").auto_compact_token_limit,
            Some(100000)
        );
    }
}
