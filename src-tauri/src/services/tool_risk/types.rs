//! Tool-risk rule and decision types.
//!
//! Rules produce warnings only; they have no block, allow or redact action.
//! A non-match never exempts an observation from other rules.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    ProcessExec,
    ShellExec,
    FileRead,
    FileWrite,
    FileDelete,
    NetworkRequest,
    DataSend,
    DatabaseOperation,
    UnknownOperation,
}

impl OperationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProcessExec => "process_exec",
            Self::ShellExec => "shell_exec",
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::FileDelete => "file_delete",
            Self::NetworkRequest => "network_request",
            Self::DataSend => "data_send",
            Self::DatabaseOperation => "database_operation",
            Self::UnknownOperation => "unknown_operation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "process_exec" => Self::ProcessExec,
            "shell_exec" => Self::ShellExec,
            "file_read" => Self::FileRead,
            "file_write" => Self::FileWrite,
            "file_delete" => Self::FileDelete,
            "network_request" => Self::NetworkRequest,
            "data_send" => Self::DataSend,
            "database_operation" => Self::DatabaseOperation,
            "unknown_operation" => Self::UnknownOperation,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskCategory {
    DestructiveChange,
    SensitiveRead,
    DataExfiltration,
    RemoteCodeExecution,
    PrivilegeChange,
    SecurityConfigChange,
    DestructiveDatabaseChange,
}

impl RiskCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DestructiveChange => "destructive_change",
            Self::SensitiveRead => "sensitive_read",
            Self::DataExfiltration => "data_exfiltration",
            Self::RemoteCodeExecution => "remote_code_execution",
            Self::PrivilegeChange => "privilege_change",
            Self::SecurityConfigChange => "security_config_change",
            Self::DestructiveDatabaseChange => "destructive_database_change",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "destructive_change" => Self::DestructiveChange,
            "sensitive_read" => Self::SensitiveRead,
            "data_exfiltration" => Self::DataExfiltration,
            "remote_code_execution" => Self::RemoteCodeExecution,
            "privilege_change" => Self::PrivilegeChange,
            "security_config_change" => Self::SecurityConfigChange,
            "destructive_database_change" => Self::DestructiveDatabaseChange,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    None,
    Medium,
    High,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOrigin {
    Builtin,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalReview {
    Off,
    Eligible,
}

/// Exact tool identity; an omitted component is unknown, not a wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolRef {
    pub client: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_identity: Option<String>,
}

impl ToolRef {
    pub fn is_valid(&self) -> bool {
        let bounded = |s: &str| !s.trim().is_empty() && s == s.trim() && s.len() <= 512;
        KNOWN_CLIENTS.contains(&self.client.as_str())
            && bounded(&self.name)
            && [&self.namespace, &self.definition_ref, &self.server_identity]
                .into_iter()
                .all(|s| s.as_deref().map(bounded).unwrap_or(true))
            && (self.definition_ref.is_some() || self.server_identity.is_some())
            && (self.namespace.as_deref() != Some("mcp") || self.server_identity.is_some())
            && (self.server_identity.is_none() || self.namespace.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleTarget {
    Operation {
        types: Vec<OperationType>,
    },
    /// Legacy tool_ref input has no valid identity and must fail validation.
    Tool {
        #[serde(default)]
        tool_refs: Vec<ToolRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<String>>,
    pub target: RuleTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq,

    In,

    PathCategory,

    PathGlob,

    Keyword,

    Regex,

    Detector,
}

/// Bounded declarative rule conditions; arbitrary scripts are unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleCondition {
    /// Explicit JSON Pointer, or $text for free text.
    pub field: String,
    pub op: ConditionOp,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRiskRule {
    pub schema_version: u32,
    pub id: String,
    pub revision: u32,
    pub origin: RuleOrigin,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub enabled: bool,
    pub selector: RuleSelector,
    pub conditions: Vec<RuleCondition>,
    pub risk_category: RiskCategory,
    pub severity: Severity,
    pub reason_code: String,
    pub local_review: LocalReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleValidationError {
    EmptySelector,
    EmptyConditions,
    TooManyConditions,
    UnknownOperationType(String),
    UnknownRiskCategory(String),
    InvalidReasonCode,
    PatternTooLong,
    TooManyValues,
    EmptyField,
    UnknownDetector(String),
    /// Custom rules cannot use the generic UnknownOperation selector.
    UnknownOperationNotAllowedForCustom,
    /// Custom rules cannot impersonate built-in IDs.
    BuiltinIdReserved,
    NameTooLong,
    DescriptionTooLong,
    InvalidSelector,
    InvalidField,
    InvalidCondition,
    InvalidRegex,
    UnsupportedSchemaVersion,
}

impl std::fmt::Display for RuleValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySelector => write!(f, "empty selector"),
            Self::EmptyConditions => write!(f, "empty conditions"),
            Self::TooManyConditions => write!(f, "too many conditions"),
            Self::UnknownOperationType(s) => write!(f, "unknown operation type: {s}"),
            Self::UnknownRiskCategory(s) => write!(f, "unknown risk category: {s}"),
            Self::InvalidReasonCode => write!(f, "invalid reason code"),
            Self::PatternTooLong => write!(f, "pattern too long"),
            Self::TooManyValues => write!(f, "too many values"),
            Self::EmptyField => write!(f, "empty field"),
            Self::UnknownDetector(s) => write!(f, "unknown detector: {s}"),
            Self::UnknownOperationNotAllowedForCustom => {
                write!(f, "unknown_operation not allowed for custom rule")
            }
            Self::BuiltinIdReserved => write!(f, "builtin id reserved"),
            Self::NameTooLong => write!(f, "name too long"),
            Self::DescriptionTooLong => write!(f, "description too long"),
            Self::InvalidSelector => write!(f, "invalid selector"),
            Self::InvalidField => write!(f, "invalid condition field"),
            Self::InvalidCondition => write!(f, "invalid condition configuration"),
            Self::InvalidRegex => write!(f, "invalid or unsupported regex"),
            Self::UnsupportedSchemaVersion => write!(f, "unsupported rule schema version"),
        }
    }
}

pub const MAX_CONDITIONS_PER_RULE: usize = 16;
pub const MAX_PATTERN_BYTES: usize = 4 * 1024;
pub const MAX_VALUES_PER_CONDITION: usize = 64;
pub const MAX_RULE_NAME_BYTES: usize = 256;
pub const MAX_RULE_DESCRIPTION_BYTES: usize = 1024;

pub const KNOWN_DETECTORS: &[&str] = &[
    "destructive_path",
    "sensitive_path",
    "exfil_destination",
    "remote_download_exec",
    "privilege_escalation",
    "security_config_path",
    "destructive_sql",
];

pub const KNOWN_REASON_CODES: &[&str] = &[
    "destructive_target",
    "sensitive_target",
    "exfil_to_remote",
    "remote_exec_chain",
    "privilege_escalation",
    "security_config_target",
    "destructive_sql",
];

pub const KNOWN_CLIENTS: &[&str] = &[
    "claude",
    "claude_code",
    "claude-desktop",
    "codex",
    "gemini",
    "openai",
    "pi",
];
pub const KNOWN_SOURCES: &[&str] = &["model_response", "client_hook", "mcp_rpc"];
pub const OPERATION_FIELDS: &[&str] = &[
    "/target_path",
    "/command",
    "/destination",
    "/op_type",
    "/recursive",
    "/overwrite",
    "/command_class",
];

/// RFC 6901 escaping only; no wildcard or implicit object traversal.
pub fn valid_input_field(field: &str) -> bool {
    if field == "$text" {
        return true;
    }
    if !field.starts_with('/') || field.len() > MAX_PATTERN_BYTES {
        return false;
    }
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

/// Compile with linear-time Rust regex and an explicit memory budget.
pub fn compile_regex(pattern: &str) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .size_limit(1024 * 1024)
        .dfa_size_limit(1024 * 1024)
        .build()
}

pub fn detector_field(id: &str) -> Option<&'static str> {
    match id {
        "destructive_path" | "sensitive_path" | "security_config_path" => Some("/target_path"),
        "exfil_destination" => Some("/destination"),
        "remote_download_exec" | "privilege_escalation" | "destructive_sql" => Some("/command"),
        _ => None,
    }
}

/// Compare declarative sets, ignoring ordering and repeated identical members.
fn same_members<T>(left: &[T], right: &[T], equal: impl Fn(&T, &T) -> bool) -> bool {
    left.iter().all(|a| right.iter().any(|b| equal(a, b)))
        && right.iter().all(|b| left.iter().any(|a| equal(a, b)))
}

fn same_optional_members<T: PartialEq>(left: &Option<Vec<T>>, right: &Option<Vec<T>>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => same_members(left, right, |a, b| a == b),
        _ => false,
    }
}

/// ObservationSource is a closed enum: an explicit full set is unrestricted.
/// Clients are not closed observation facts, so this normalization is sources-only.
fn same_source_scope(left: &Option<Vec<String>>, right: &Option<Vec<String>>) -> bool {
    let unrestricted = |sources: &Option<Vec<String>>| {
        sources.as_ref().is_none_or(|sources| {
            sources
                .iter()
                .all(|source| KNOWN_SOURCES.contains(&source.as_str()))
                && KNOWN_SOURCES
                    .iter()
                    .all(|source| sources.iter().any(|value| value == source))
        })
    };
    (unrestricted(left) && unrestricted(right)) || same_optional_members(left, right)
}

impl ToolRiskRule {
    /// Write-time policy: custom rules only add risk, never duplicate builtin predicates.
    /// Kept separate from schema validation so historical rules can still be read/deleted.
    pub fn validate_custom_policy(&self) -> Result<(), &'static str> {
        if self.severity == Severity::None {
            return Err("tool_risk_risk_severity_required");
        }
        if super::builtin::builtin_rules()
            .iter()
            .any(|builtin| self.same_detection_as(builtin))
        {
            return Err("tool_risk_builtin_duplicate");
        }
        Ok(())
    }

    fn same_detection_as(&self, other: &Self) -> bool {
        let same_target = match (&self.selector.target, &other.selector.target) {
            (RuleTarget::Operation { types: a }, RuleTarget::Operation { types: b }) => {
                same_members(a, b, |a, b| a == b)
            }
            (RuleTarget::Tool { tool_refs: a }, RuleTarget::Tool { tool_refs: b }) => {
                same_members(a, b, |a, b| a == b)
            }
            _ => false,
        };
        same_target
            && same_source_scope(&self.selector.sources, &other.selector.sources)
            && same_optional_members(&self.selector.clients, &other.selector.clients)
            && same_members(&self.conditions, &other.conditions, |a, b| {
                a.field == b.field
                    && a.op == b.op
                    && a.value == b.value
                    && same_optional_members(&a.values, &b.values)
            })
    }

    /// Validate before saving or enabling; unsupported operators cannot pretend to be executable.
    pub fn validate(&self) -> Result<(), RuleValidationError> {
        use RuleValidationError::*;
        if self.schema_version != 1 {
            return Err(UnsupportedSchemaVersion);
        }
        if self.name.len() > MAX_RULE_NAME_BYTES {
            return Err(NameTooLong);
        }
        if self
            .description
            .as_ref()
            .is_some_and(|d| d.len() > MAX_RULE_DESCRIPTION_BYTES)
        {
            return Err(DescriptionTooLong);
        }
        for (list, allowed) in [
            (&self.selector.sources, KNOWN_SOURCES),
            (&self.selector.clients, KNOWN_CLIENTS),
        ] {
            if let Some(list) = list {
                if list.is_empty()
                    || list.len() > MAX_VALUES_PER_CONDITION
                    || list.iter().any(|s| !allowed.contains(&s.as_str()))
                {
                    return Err(InvalidSelector);
                }
            }
        }
        match &self.selector.target {
            RuleTarget::Operation { types } => {
                if types.is_empty() {
                    return Err(EmptySelector);
                }
                if types.len() > 9 {
                    return Err(InvalidSelector);
                }
                if self.origin == RuleOrigin::Custom
                    && types.contains(&OperationType::UnknownOperation)
                {
                    return Err(UnknownOperationNotAllowedForCustom);
                }
            }
            RuleTarget::Tool { tool_refs } => {
                if tool_refs.is_empty() {
                    return Err(EmptySelector);
                }
                if tool_refs.len() > MAX_VALUES_PER_CONDITION
                    || tool_refs.iter().any(|r| !r.is_valid())
                {
                    return Err(InvalidSelector);
                }
            }
        }
        if self.conditions.is_empty() {
            return Err(EmptyConditions);
        }
        if self.conditions.len() > MAX_CONDITIONS_PER_RULE {
            return Err(TooManyConditions);
        }
        for cond in &self.conditions {
            if cond.field.trim().is_empty() {
                return Err(EmptyField);
            }
            let field_valid = match &self.selector.target {
                RuleTarget::Operation { .. } => OPERATION_FIELDS.contains(&cond.field.as_str()),
                RuleTarget::Tool { .. } => valid_input_field(&cond.field),
            };
            if !field_valid {
                return Err(InvalidField);
            }
            if cond
                .value
                .as_ref()
                .is_some_and(|v| v.len() > MAX_PATTERN_BYTES)
            {
                return Err(PatternTooLong);
            }
            if let Some(vals) = &cond.values {
                if vals.len() > MAX_VALUES_PER_CONDITION {
                    return Err(TooManyValues);
                }
                if vals.iter().any(|v| v.len() > MAX_PATTERN_BYTES) {
                    return Err(PatternTooLong);
                }
            }
            match cond.op {
                ConditionOp::In => {
                    if cond.value.is_some()
                        || cond
                            .values
                            .as_ref()
                            .is_none_or(|v| v.is_empty() || v.iter().any(String::is_empty))
                    {
                        return Err(InvalidCondition);
                    }
                }
                ConditionOp::PathGlob | ConditionOp::PathCategory => return Err(InvalidCondition),
                _ => {
                    if cond.values.is_some() || cond.value.as_ref().is_none_or(|v| v.is_empty()) {
                        return Err(InvalidCondition);
                    }
                }
            }
            if cond.op == ConditionOp::Regex {
                compile_regex(cond.value.as_deref().unwrap_or("")).map_err(|_| InvalidRegex)?;
            }
            if cond.op == ConditionOp::Detector {
                let id = cond.value.as_deref().unwrap_or("");
                if !KNOWN_DETECTORS.contains(&id) {
                    return Err(UnknownDetector(id.to_string()));
                }
                if !matches!(self.selector.target, RuleTarget::Operation { .. })
                    || detector_field(id) != Some(cond.field.as_str())
                {
                    return Err(InvalidCondition);
                }
            }
        }
        if !KNOWN_REASON_CODES.contains(&self.reason_code.as_str()) {
            return Err(InvalidReasonCode);
        }
        if self.origin == RuleOrigin::Custom && self.id.starts_with("builtin.") {
            return Err(BuiltinIdReserved);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(field: &str, op: ConditionOp, value: Option<&str>) -> RuleCondition {
        RuleCondition {
            field: field.into(),
            op,
            value: value.map(str::to_string),
            values: None,
        }
    }

    fn custom_rule() -> ToolRiskRule {
        ToolRiskRule {
            schema_version: 1,
            id: "custom-1".into(),
            revision: 1,
            origin: RuleOrigin::Custom,
            name: "my rule".into(),
            description: None,
            enabled: false,
            selector: RuleSelector {
                sources: None,
                clients: None,
                target: RuleTarget::Operation {
                    types: vec![OperationType::FileRead],
                },
            },
            conditions: vec![cond(
                "/target_path",
                ConditionOp::Detector,
                Some("sensitive_path"),
            )],
            risk_category: RiskCategory::SensitiveRead,
            severity: Severity::High,
            reason_code: "sensitive_target".into(),
            local_review: LocalReview::Off,
        }
    }

    #[test]
    fn valid_custom_rule_passes() {
        assert!(custom_rule().validate().is_ok());
    }

    #[test]
    fn empty_conditions_rejected() {
        let mut r = custom_rule();
        r.conditions.clear();
        assert_eq!(r.validate(), Err(RuleValidationError::EmptyConditions));
    }

    #[test]
    fn empty_selector_rejected() {
        let mut r = custom_rule();
        r.selector.target = RuleTarget::Operation { types: vec![] };
        assert_eq!(r.validate(), Err(RuleValidationError::EmptySelector));
    }

    #[test]
    fn custom_unknown_operation_rejected() {
        let mut r = custom_rule();
        r.selector.target = RuleTarget::Operation {
            types: vec![OperationType::UnknownOperation],
        };
        assert_eq!(
            r.validate(),
            Err(RuleValidationError::UnknownOperationNotAllowedForCustom)
        );
    }

    #[test]
    fn builtin_unknown_operation_allowed() {
        let mut r = custom_rule();
        r.origin = RuleOrigin::Builtin;
        r.id = "builtin.x".into();
        r.selector.target = RuleTarget::Operation {
            types: vec![OperationType::UnknownOperation],
        };
        assert!(r.validate().is_ok());
    }

    #[test]
    fn custom_builtin_id_rejected() {
        let mut r = custom_rule();
        r.id = "builtin.fake".into();
        assert_eq!(r.validate(), Err(RuleValidationError::BuiltinIdReserved));
    }

    #[test]
    fn unknown_reason_code_rejected() {
        let mut r = custom_rule();
        r.reason_code = "made_up".into();
        assert_eq!(r.validate(), Err(RuleValidationError::InvalidReasonCode));
    }

    #[test]
    fn unknown_detector_rejected() {
        let mut r = custom_rule();
        r.conditions = vec![cond("/target_path", ConditionOp::Detector, Some("nope"))];
        assert_eq!(
            r.validate(),
            Err(RuleValidationError::UnknownDetector("nope".into()))
        );
    }

    #[test]
    fn too_many_conditions_rejected() {
        let mut r = custom_rule();
        r.conditions = (0..(MAX_CONDITIONS_PER_RULE + 1))
            .map(|i| cond(&format!("/f{i}"), ConditionOp::Eq, Some("x")))
            .collect();
        assert_eq!(r.validate(), Err(RuleValidationError::TooManyConditions));
    }

    #[test]
    fn oversized_pattern_rejected() {
        let mut r = custom_rule();
        r.conditions = vec![cond(
            "/target_path",
            ConditionOp::Regex,
            Some(&"a".repeat(MAX_PATTERN_BYTES + 1)),
        )];
        assert_eq!(r.validate(), Err(RuleValidationError::PatternTooLong));
    }

    #[test]
    fn empty_field_rejected() {
        let mut r = custom_rule();
        r.conditions = vec![cond("  ", ConditionOp::Eq, Some("x"))];
        assert_eq!(r.validate(), Err(RuleValidationError::EmptyField));
    }

    #[test]
    fn operation_type_roundtrip() {
        for t in [
            OperationType::ShellExec,
            OperationType::FileRead,
            OperationType::DatabaseOperation,
        ] {
            assert_eq!(OperationType::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn risk_category_roundtrip() {
        for c in [
            RiskCategory::DestructiveChange,
            RiskCategory::SensitiveRead,
            RiskCategory::DataExfiltration,
        ] {
            assert_eq!(RiskCategory::parse(c.as_str()), Some(c));
        }
    }
    #[test]
    fn regression_invalid_fields_and_unsupported_conditions_rejected() {
        for field in ["path", "/unknown_operation_field", "/bad~2escape", "$all"] {
            let mut r = custom_rule();
            r.conditions = vec![cond(field, ConditionOp::Eq, Some("x"))];
            assert!(r.validate().is_err(), "accepted field: {field}");
        }
        for op in [ConditionOp::PathGlob, ConditionOp::PathCategory] {
            let mut r = custom_rule();
            r.conditions = vec![cond("/target_path", op, Some("*"))];
            assert!(r.validate().is_err(), "unsupported operator accepted");
        }
    }

    #[test]
    fn regression_invalid_and_backtracking_regex_rejected() {
        for pattern in ["[", "(?=secret)", r"(a)\1"] {
            let mut r = custom_rule();
            r.conditions = vec![cond("/target_path", ConditionOp::Regex, Some(pattern))];
            assert!(r.validate().is_err(), "accepted unsupported regex");
        }
    }

    #[test]
    fn regression_missing_condition_value_rejected() {
        let mut r = custom_rule();
        r.conditions = vec![cond("/target_path", ConditionOp::Eq, None)];
        assert!(r.validate().is_err());
    }

    #[test]
    fn regression_tool_refs_use_explicit_camel_case_identity() {
        let mut r = custom_rule();
        r.selector.target = serde_json::from_value(serde_json::json!({
            "kind":"tool", "tool_refs":[{"client":"claude", "name":"custom", "definitionRef":"local:schema:v1"}]
        })).expect("tool_refs contract");
        r.conditions = vec![cond(
            "/chosen/message",
            ConditionOp::Regex,
            Some("^danger$"),
        )];
        assert!(r.validate().is_ok());
        let encoded = serde_json::to_value(&r.selector.target).unwrap();
        assert_eq!(encoded["tool_refs"][0]["definitionRef"], "local:schema:v1");
    }

    #[test]
    fn regression_tool_refs_reject_legacy_and_incomplete_identity() {
        let mut r = custom_rule();
        r.selector.target =
            serde_json::from_value(serde_json::json!({"kind":"tool", "tool_ref":"Read"})).unwrap();
        assert!(r.validate().is_err());
        for identity in [
            serde_json::json!({"client":"claude", "name":"Read"}),
            serde_json::json!({"client":"unknown", "name":"Read", "definitionRef":"v1"}),
            serde_json::json!({"client":"claude", "name":"Read", "namespace":"mcp", "definitionRef":"v1"}),
            serde_json::json!({"client":"claude", "name":"Read", "serverIdentity":"local-server"}),
            serde_json::json!({"client":"claude", "name":"Read", "definitionRef":""}),
        ] {
            let parsed = serde_json::from_value::<RuleTarget>(
                serde_json::json!({"kind":"tool", "tool_refs":[identity]}),
            );
            if let Ok(target) = parsed {
                r.selector.target = target;
                assert!(r.validate().is_err());
            }
        }
    }

    #[test]
    fn regression_operator_arguments_and_selector_lists_are_strict() {
        for op in [
            ConditionOp::Eq,
            ConditionOp::Keyword,
            ConditionOp::Regex,
            ConditionOp::Detector,
        ] {
            let mut r = custom_rule();
            r.conditions = vec![cond("/target_path", op, Some(""))];
            assert!(r.validate().is_err());
            r.conditions[0].value = Some("sensitive_path".into());
            r.conditions[0].values = Some(vec!["unused".into()]);
            assert!(r.validate().is_err());
        }
        let mut r = custom_rule();
        r.conditions = vec![RuleCondition {
            field: "/target_path".into(),
            op: ConditionOp::In,
            value: None,
            values: Some(vec![]),
        }];
        assert!(r.validate().is_err());
        for sources in [vec![], vec!["arbitrary_source".into()]] {
            let mut r = custom_rule();
            r.selector.sources = Some(sources);
            assert!(r.validate().is_err());
        }
        for clients in [vec![], vec!["unknown_client".into()]] {
            let mut r = custom_rule();
            r.selector.clients = Some(clients);
            assert!(r.validate().is_err());
        }
    }

    #[test]
    fn regression_operation_detector_field_binding_is_validated() {
        let mut r = custom_rule();
        r.conditions = vec![cond(
            "/command",
            ConditionOp::Detector,
            Some("sensitive_path"),
        )];
        assert!(r.validate().is_err());
    }
}
