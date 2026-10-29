use serde::{Deserialize, Serialize};
use std::fmt;

/// Risk severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Policy action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditAction {
    /// Block forwarding and return HTTP 400.
    Block,
    /// Record a security event and continue forwarding.
    Audit,
    /// Redact sensitive text before forwarding.
    Redact,
    /// Replace supported sensitive text with locally restorable references.
    #[serde(rename = "redact_restore")]
    RedactRestore,
    /// Forward unchanged without a security event or notification.
    Allow,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Audit => "audit",
            Self::Redact => "redact",
            Self::RedactRestore => "redact_restore",
            Self::Allow => "allow",
        }
    }
}

impl fmt::Display for AuditAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Rule category.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCategory {
    PrivateKey,
    ApiToken,
    DbConnection,
    Password,
    Infrastructure,
    Pii,
    Custom,
}

impl RuleCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PrivateKey => "private_key",
            Self::ApiToken => "api_token",
            Self::DbConnection => "db_connection",
            Self::Password => "password",
            Self::Infrastructure => "infrastructure",
            Self::Pii => "pii",
            Self::Custom => "custom",
        }
    }
}

impl fmt::Display for RuleCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// One detected audit item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFinding {
    /// Internal identity only; never serialized or displayed.
    #[serde(skip)]
    pub(crate) identity: Option<super::identity::ContentFingerprint>,
    pub rule_id: String,
    pub rule_name: String,
    pub category: RuleCategory,
    pub risk_level: RiskLevel,
    pub action: AuditAction,
    /// Masked content summary; never expose the original sensitive plaintext.
    pub matched_snippet_masked: String,
    /// Display location, such as messages[0].content or tool_use.input.command.
    pub location: String,
}

/// Final request-body inspection result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityInspectionResult {
    /// Whether forwarding must be blocked.
    pub blocked: bool,
    /// All detected items.
    pub findings: Vec<AuditFinding>,
    /// Highest detected risk.
    pub highest_risk: Option<RiskLevel>,
    /// Sanitized block reason for logs and client errors.
    pub block_reason: Option<String>,
}

impl SecurityInspectionResult {
    pub fn pass() -> Self {
        Self {
            blocked: false,
            findings: Vec::new(),
            highest_risk: None,
            block_reason: None,
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked
    }
}

#[cfg(test)]
mod redact_restore_tests {
    use super::*;

    #[test]
    fn redact_restore_has_exact_public_serde_spelling() -> Result<(), serde_json::Error> {
        let action: AuditAction = serde_json::from_str("\"redact_restore\"")?;
        assert_eq!(action.as_str(), "redact_restore");
        assert_eq!(serde_json::to_string(&action)?, "\"redact_restore\"");
        for invalid in [
            "redactrestore",
            "redact-restore",
            "RedactRestore",
            "restore",
        ] {
            assert!(serde_json::from_value::<AuditAction>(serde_json::json!(invalid)).is_err());
        }
        Ok(())
    }
}
