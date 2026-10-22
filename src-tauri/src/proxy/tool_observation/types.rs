//! In-process tool-observation contracts.
//!
//! These types are not database records or serializable frontend payloads.
//! Optional context can remain unknown; never invent client facts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    ModelResponse,
    ClientHook,
    McpRpc,
}

impl ObservationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ModelResponse => "model_response",
            Self::ClientHook => "client_hook",
            Self::McpRpc => "mcp_rpc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStage {
    Proposed,
    PreExecution,
    RpcSubmitted,
}

impl ObservationStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::PreExecution => "pre_execution",
            Self::RpcSubmitted => "rpc_submitted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    Client,
    Provider,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputEncoding {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    Complete,
    Incomplete,
    Invalid,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityQuality {
    SourceId,
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityOrigin {
    Native,
    Converted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTermination {
    Normal,
    Abnormal,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolIdentityQuality {
    Source,
    ValidatedMapping,
    Synthetic,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputFidelity {
    Preserved,
    ValidatedConversion,
    RepairedOrDropped,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextOrigin {
    ClientReported,
    LocalConfig,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct ContextValue {
    pub value: String,
    pub origin: ContextOrigin,
}

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Correlation {
    pub session_id: Option<String>,
    pub call_id: Option<String>,
    pub item_id: Option<String>,
    pub candidate_index: Option<u32>,
    pub tool_index: Option<u32>,
    pub identity_quality: Option<IdentityQuality>,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct ToolIdentity {
    pub client: Option<String>,
    pub name: String,
    pub namespace: Option<String>,
    pub server_id: Option<String>,
    pub definition_ref: Option<String>,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct ToolInput {
    pub encoding: InputEncoding,
    pub value: Option<serde_json::Value>,
    pub status: InputStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationQuality {
    pub origin: QualityOrigin,
    pub source_termination: SourceTermination,
    pub tool_identity: ToolIdentityQuality,
    pub input_fidelity: InputFidelity,
}

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ObservationContext {
    pub platform: Option<ContextValue>,
    pub shell_kind: Option<ContextValue>,
    pub cwd: Option<ContextValue>,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct ToolObservation {
    pub schema_version: u32,
    pub observation_id: String,
    pub source: ObservationSource,
    pub stage: ObservationStage,
    pub execution_scope: ExecutionScope,
    pub protocol: String,
    pub scope_id: String,
    pub correlation: Correlation,
    pub tool: ToolIdentity,
    pub input: ToolInput,
    pub quality: ObservationQuality,
    pub context: ObservationContext,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    UnsupportedSchemaVersion,
    EmptyField(&'static str),
    InputTooLarge,

    SourceStageMismatch,

    EncodingValueMismatch,

    CompleteWithoutEvidence,

    FieldTooLong(&'static str),

    JsonTooDeep,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion => write!(f, "unsupported schema version"),
            Self::EmptyField(field) => write!(f, "empty field: {field}"),
            Self::InputTooLarge => write!(f, "input too large"),
            Self::SourceStageMismatch => write!(f, "source/stage mismatch"),
            Self::EncodingValueMismatch => write!(f, "encoding/value mismatch"),
            Self::CompleteWithoutEvidence => write!(f, "complete without evidence"),
            Self::FieldTooLong(field) => write!(f, "field too long: {field}"),
            Self::JsonTooDeep => write!(f, "json too deep"),
        }
    }
}

#[allow(dead_code)]
pub const MAX_ID_BYTES: usize = 512;
#[allow(dead_code)]
pub const MAX_NAME_BYTES: usize = 512;
#[allow(dead_code)]
pub const MAX_CONTEXT_BYTES: usize = 4096;
#[allow(dead_code)]
pub const MAX_JSON_DEPTH: usize = 32;
pub const MAX_INPUT_BYTES: usize = 256 * 1024;

// Raw observations are deserialization-only; persistence and notifications require sanitized projections.
macro_rules! redacted_debug {
    ($($ty:ty),+ $(,)?) => {$ (
        impl std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($ty), " { <redacted> }"))
            }
        }
    )+ };
}
redacted_debug!(
    ContextValue,
    Correlation,
    ToolIdentity,
    ToolInput,
    ObservationContext,
    ToolObservation
);

impl ToolObservation {
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 {
            return Err(ContractError::UnsupportedSchemaVersion);
        }

        let stage_ok = matches!(
            (self.source, self.stage),
            (ObservationSource::ModelResponse, ObservationStage::Proposed)
                | (
                    ObservationSource::ClientHook,
                    ObservationStage::PreExecution
                )
                | (ObservationSource::McpRpc, ObservationStage::RpcSubmitted)
        );
        if !stage_ok {
            return Err(ContractError::SourceStageMismatch);
        }

        match (self.input.encoding, &self.input.value) {
            (InputEncoding::Text, Some(serde_json::Value::String(_))) => {}
            (InputEncoding::Text, None) => {}
            (InputEncoding::Json, _) => {}
            (InputEncoding::Text, Some(_)) => return Err(ContractError::EncodingValueMismatch),
        }

        // Complete input requires normal termination and preserved or validated-conversion fidelity.
        // A partial or unverified input must not be declared complete.
        if self.input.status == InputStatus::Complete {
            let term_ok = self.quality.source_termination == SourceTermination::Normal;
            let fidelity_ok = matches!(
                self.quality.input_fidelity,
                InputFidelity::Preserved | InputFidelity::ValidatedConversion
            );
            let conversion_ok = self.quality.origin != QualityOrigin::Converted
                || self.quality.input_fidelity == InputFidelity::ValidatedConversion;
            if !term_ok || !fidelity_ok || !conversion_ok || self.input.value.is_none() {
                return Err(ContractError::CompleteWithoutEvidence);
            }
        }

        for (field, value, limit, required) in [
            (
                "observation_id",
                Some(self.observation_id.as_str()),
                MAX_ID_BYTES,
                true,
            ),
            ("scope_id", Some(self.scope_id.as_str()), MAX_ID_BYTES, true),
            ("protocol", Some(self.protocol.as_str()), MAX_ID_BYTES, true),
            (
                "session_id",
                self.correlation.session_id.as_deref(),
                MAX_ID_BYTES,
                false,
            ),
            (
                "call_id",
                self.correlation.call_id.as_deref(),
                MAX_ID_BYTES,
                false,
            ),
            (
                "item_id",
                self.correlation.item_id.as_deref(),
                MAX_ID_BYTES,
                false,
            ),
            (
                "tool.name",
                Some(self.tool.name.as_str()),
                MAX_NAME_BYTES,
                true,
            ),
            (
                "tool.client",
                self.tool.client.as_deref(),
                MAX_NAME_BYTES,
                false,
            ),
            (
                "tool.namespace",
                self.tool.namespace.as_deref(),
                MAX_NAME_BYTES,
                false,
            ),
            (
                "tool.server_id",
                self.tool.server_id.as_deref(),
                MAX_ID_BYTES,
                false,
            ),
            (
                "tool.definition_ref",
                self.tool.definition_ref.as_deref(),
                MAX_ID_BYTES,
                false,
            ),
        ] {
            if let Some(value) = value {
                if required && value.trim().is_empty() {
                    return Err(ContractError::EmptyField(field));
                }
                if value.len() > limit {
                    return Err(ContractError::FieldTooLong(field));
                }
            }
        }
        for ctx in [
            &self.context.platform,
            &self.context.shell_kind,
            &self.context.cwd,
        ]
        .into_iter()
        .flatten()
        {
            if ctx.value.len() > MAX_CONTEXT_BYTES {
                return Err(ContractError::FieldTooLong("context.value"));
            }
        }
        if let Some(value) = &self.input.value {
            bounded_input_size(value)?;
        }

        Ok(())
    }
}

/// Check depth before serialization; bound recursion and count bytes without copying plaintext.
pub(crate) fn bounded_input_size(value: &serde_json::Value) -> Result<usize, ContractError> {
    fn check_depth(value: &serde_json::Value, depth: usize) -> Result<(), ContractError> {
        if depth > MAX_JSON_DEPTH {
            return Err(ContractError::JsonTooDeep);
        }
        match value {
            serde_json::Value::Array(values) => {
                for child in values {
                    check_depth(child, depth + 1)?;
                }
            }
            serde_json::Value::Object(values) => {
                for child in values.values() {
                    check_depth(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    struct BudgetWriter(usize);
    impl std::io::Write for BudgetWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_INPUT_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "input budget",
                ));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    check_depth(value, 1)?;
    let mut writer = BudgetWriter(0);
    serde_json::to_writer(&mut writer, value).map_err(|_| ContractError::InputTooLarge)?;
    Ok(writer.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_obs() -> ToolObservation {
        ToolObservation {
            schema_version: 1,
            observation_id: "obs-1".into(),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: ExecutionScope::Client,
            protocol: "anthropic_messages".into(),
            scope_id: "scope-1".into(),
            correlation: Correlation::default(),
            tool: ToolIdentity {
                client: Some("claude".into()),
                name: "Bash".into(),
                namespace: None,
                server_id: None,
                definition_ref: None,
            },
            input: ToolInput {
                encoding: InputEncoding::Json,
                value: Some(json!({"command": "ls"})),
                status: InputStatus::Complete,
            },
            quality: ObservationQuality {
                origin: QualityOrigin::Native,
                source_termination: SourceTermination::Normal,
                tool_identity: ToolIdentityQuality::Source,
                input_fidelity: InputFidelity::Preserved,
            },
            context: ObservationContext::default(),
        }
    }

    #[test]
    fn valid_observation_passes() {
        assert!(base_obs().validate().is_ok());
    }

    #[test]
    fn source_stage_mismatch_rejected() {
        let mut o = base_obs();
        o.stage = ObservationStage::PreExecution;
        assert_eq!(o.validate(), Err(ContractError::SourceStageMismatch));
    }

    #[test]
    fn text_encoding_with_non_string_rejected() {
        let mut o = base_obs();
        o.input.encoding = InputEncoding::Text;
        o.input.value = Some(json!({"a": 1}));
        assert_eq!(o.validate(), Err(ContractError::EncodingValueMismatch));
    }

    #[test]
    fn text_encoding_with_string_ok() {
        let mut o = base_obs();
        o.input.encoding = InputEncoding::Text;
        o.input.value = Some(json!("raw text"));
        assert!(o.validate().is_ok());
    }

    #[test]
    fn complete_without_normal_termination_rejected() {
        let mut o = base_obs();
        o.quality.source_termination = SourceTermination::Abnormal;
        assert_eq!(o.validate(), Err(ContractError::CompleteWithoutEvidence));
    }

    #[test]
    fn complete_without_preserved_fidelity_rejected() {
        let mut o = base_obs();
        o.quality.input_fidelity = InputFidelity::RepairedOrDropped;
        assert_eq!(o.validate(), Err(ContractError::CompleteWithoutEvidence));
    }

    #[test]
    fn incomplete_with_abnormal_termination_ok() {
        let mut o = base_obs();
        o.input.status = InputStatus::Incomplete;
        o.quality.source_termination = SourceTermination::Abnormal;
        assert!(o.validate().is_ok());
    }

    #[test]
    fn oversized_name_rejected() {
        let mut o = base_obs();
        o.tool.name = "x".repeat(MAX_NAME_BYTES + 1);
        assert_eq!(o.validate(), Err(ContractError::FieldTooLong("tool.name")));
    }

    #[test]
    fn deep_json_rejected() {
        let mut o = base_obs();
        let mut v = json!(1);
        for _ in 0..(MAX_JSON_DEPTH + 2) {
            v = json!([v]);
        }
        o.input.value = Some(v);
        assert_eq!(o.validate(), Err(ContractError::JsonTooDeep));
    }

    #[test]
    fn regression_schema_and_all_external_metadata_are_bounded() {
        let mut o = base_obs();
        o.schema_version = 2;
        assert!(o.validate().is_err());
        let oversized = "x".repeat(MAX_ID_BYTES + 1);
        for field in [
            "protocol",
            "session_id",
            "call_id",
            "item_id",
            "client",
            "namespace",
            "server_id",
            "definition_ref",
        ] {
            let mut o = base_obs();
            match field {
                "protocol" => o.protocol = oversized.clone(),
                "session_id" => o.correlation.session_id = Some(oversized.clone()),
                "call_id" => o.correlation.call_id = Some(oversized.clone()),
                "item_id" => o.correlation.item_id = Some(oversized.clone()),
                "client" => o.tool.client = Some(oversized.clone()),
                "namespace" => o.tool.namespace = Some(oversized.clone()),
                "server_id" => o.tool.server_id = Some(oversized.clone()),
                _ => o.tool.definition_ref = Some(oversized.clone()),
            }
            assert!(o.validate().is_err(), "{field}");
        }
        let mut o = base_obs();
        o.tool.name.clear();
        assert!(o.validate().is_err());
    }

    #[test]
    fn regression_complete_missing_value_and_unverified_conversion_rejected() {
        let mut o = base_obs();
        o.input.value = None;
        assert!(o.validate().is_err());
        let mut o = base_obs();
        o.quality.origin = QualityOrigin::Converted;
        assert!(o.validate().is_err());
        o.quality.input_fidelity = InputFidelity::ValidatedConversion;
        assert!(o.validate().is_ok());
    }

    #[test]
    fn regression_input_size_budget_includes_json_keys_and_escaping() {
        for value in [
            json!("x".repeat(256 * 1024 + 1)),
            json!({"x".repeat(256 * 1024):0}),
            json!("\n".repeat(140 * 1024)),
        ] {
            let mut o = base_obs();
            o.input.value = Some(value);
            assert!(o.validate().is_err());
        }
    }

    #[test]
    fn regression_debug_redacts_external_fields_and_input() {
        let mut o = base_obs();
        let secret = "fixture-private-marker";
        o.tool.name = secret.into();
        o.correlation.call_id = Some(secret.into());
        o.context.cwd = Some(ContextValue {
            value: secret.into(),
            origin: ContextOrigin::ClientReported,
        });
        o.input.value = Some(json!({"command":secret}));
        assert!(!format!("{o:?}").contains(secret));
        assert!(!format!("{:?}", o.input).contains(secret));
        assert!(!format!("{:?}", o.tool).contains(secret));
        let deserialized: ToolObservation = serde_json::from_value(json!({
            "schema_version":1,"observation_id":"o","source":"model_response","stage":"proposed","execution_scope":"client","protocol":"openai_chat","scope_id":"s",
            "correlation":{},"tool":{"name":"Bash"},"input":{"encoding":"json","value":{},"status":"complete"},
            "quality":{"origin":"native","source_termination":"normal","tool_identity":"source","input_fidelity":"preserved"},"context":{}
        })).unwrap();
        assert!(deserialized.validate().is_ok());
    }
}
