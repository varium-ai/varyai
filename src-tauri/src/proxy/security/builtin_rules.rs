use aho_corasick::AhoCorasick;
use regex::Regex;
use std::sync::LazyLock;

use super::types::{AuditAction, RiskLevel, RuleCategory};

// The token expressions marked as Gitleaks-derived below are adapted from the
// pinned Gitleaks configuration at b58d3f102cf3a2c84cb7f923d05c25c9b1aed84b.
// Gitleaks is MIT-licensed:
// https://github.com/gitleaks/gitleaks/blob/b58d3f102cf3a2c84cb7f923d05c25c9b1aed84b/LICENSE
// The Groq and BasicAuth references are also MIT-licensed Secretlint rules.

pub struct BuiltinRule {
    pub id: &'static str,
    pub name: &'static str,
    pub category: RuleCategory,
    pub risk_level: RiskLevel,
    pub default_action: AuditAction,
    pub regex: Option<Regex>,

    pub capture_group: Option<usize>,

    pub allowlisted_values: &'static [&'static str],

    pub trigger_keywords: &'static [&'static str],
}

impl BuiltinRule {
    fn new(
        id: &'static str,
        name: &'static str,
        category: RuleCategory,
        risk_level: RiskLevel,
        default_action: AuditAction,
        pattern: &'static str,
        trigger_keywords: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            name,
            category,
            risk_level,
            default_action,
            regex: compile_regex(id, pattern),
            capture_group: None,
            allowlisted_values: &[],
            trigger_keywords,
        }
    }

    fn with_capture_group(mut self, capture_group: usize) -> Self {
        self.capture_group = Some(capture_group);
        self
    }

    fn with_allowlist(mut self, allowlisted_values: &'static [&'static str]) -> Self {
        self.allowlisted_values = allowlisted_values;
        self
    }
}

fn compile_regex(rule_id: &str, pattern: &str) -> Option<Regex> {
    match regex::RegexBuilder::new(pattern)
        .size_limit(100 * 1024 * 1024)
        .build()
    {
        Ok(regex) => Some(regex),
        Err(error) => {
            // A malformed built-in expression must disable only that rule,
            // never panic and take down the local proxy hot path.
            log::error!("[SecurityAudit] 内置规则 {rule_id} 正则编译失败: {error}");
            None
        }
    }
}

static GOOGLE_API_KEY_ALLOWLIST: &[&str] = &[
    "AIzaSyabcdefghijklmnopqrstuvwxyz1234567",
    "AIzaSyAnLA7NfeLquwW1tJFpx_eQCxoX-oo6YyIs",
    "AIzaSyCkEhVjf3pduRDt6d1yKOMitrUEke8agEM",
    "AIzaSyDMAScliyLx7F0NPDEJi1QmyCgHIAODrlU",
    "AIzaSyD3asb-2pEZVqMkmL6M9N6nHZRR_znhrh0",
    "AIzayDNSXIbFmlXbIE6mCzDLQAqITYefhixbX4A",
    "AIzaSyAdOS2zB6NCsk1pCdZ4-P6GBdi_UUPwX7c",
    "AIzaSyASWm6HmTMdYWpgMnjRBjxcQ9CKctWmLd4",
    "AIzaSyANUvH9H9BsUccjsu2pCmEkOPjjaXeDQgY",
    "AIzaSyA5_iVawFQ8ABuTZNUdcwERLJv_a_p4wtM",
    "AIzaSyA4UrcGxgwQFTfaI3no3t7Lt1sjmdnP5sQ",
    "AIzaSyDSb51JiIcB6OJpwwMicseKRhhrOq1cS7g",
    "AIzaSyBF2RrAIm4a0mO64EShQfqfd2AFnzAvvuU",
    "AIzaSyBcE-OOIbhjyR83gm4r2MFCu4MJmprNXsw",
    "AIzaSyB8qGxt4ec15vitgn44duC5ucxaOi4FmqE",
    "AIzaSyA8vmApnrHNFE0bApF4hoZ11srVL_n0nvY",
];

pub static BUILTIN_RULES: LazyLock<Vec<BuiltinRule>> = LazyLock::new(|| {
    vec![
        // Credentials are hidden and sent by default; private IPs remain audit-only.
        BuiltinRule::new(
            "rsa_ssh_private_key",
            "RSA/SSH/EC/DSA 私钥凭据",
            RuleCategory::PrivateKey,
            RiskLevel::Critical,
            AuditAction::Redact,
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----",
            &["-----BEGIN", "PRIVATE KEY-----"],
        ),
        BuiltinRule::new(
            "github_token",
            "GitHub 访问令牌 (Token)",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Redact,
            r"\b(?:gh[pousr]_[A-Za-z0-9_]{36,}|github_pat_[A-Za-z0-9_]{80,})\b",
            &["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"],
        ),
        BuiltinRule::new(
            "aws_access_key",
            "AWS AccessKey 身份凭证",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Redact,
            r"\b(?:AKIA|ABIA|ACCA|ASIA)[0-9A-Z]{16}\b",
            &["AKIA", "ABIA", "ACCA", "ASIA"],
        ),
        BuiltinRule::new(
            "openai_api_key",
            "OpenAI API Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Redact,
            r"\bsk-(?:proj-|live-)?[a-zA-Z0-9]{32,}\b",
            &["sk-"],
        ),
        BuiltinRule::new(
            "anthropic_api_key",
            "Anthropic API Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Redact,
            r"\bsk-ant-[a-zA-Z0-9_-]{32,}\b",
            &["sk-ant-"],
        ),
        BuiltinRule::new(
            "db_connection_uri",
            "带密码的数据库连接串",
            RuleCategory::DbConnection,
            RiskLevel::High,
            AuditAction::Redact,
            r#"(?i)\b(?:postgres|postgresql|mysql|redis|mongodb):\/\/[^:\s]+:[^@\s]+@[^\s,'"\)\]\}]+"#,
            &[
                "postgres://",
                "postgresql://",
                "mysql://",
                "redis://",
                "mongodb://",
            ],
        ),
        BuiltinRule::new(
            "rfc1918_private_ip",
            "RFC1918 局域网私有 IP",
            RuleCategory::Infrastructure,
            RiskLevel::Medium,
            AuditAction::Audit,
            r"\b(?:10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])|192\.168)\.\d{1,3}\.\d{1,3}\b",
            &["10.", "172.", "192.168."],
        ),
        // Optional catalog: platform tokens (default action = Allow).
        BuiltinRule::new(
            "catalog:gitlab-pat",
            "GitLab PAT",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\b(?:glpat-[0-9A-Za-z_-]{27,300}\.[0-9a-z]{9}|glpat-[\w-]{20})\b",
            &["glpat-"],
        ),
        BuiltinRule::new(
            "catalog:stripe-key",
            "Stripe 密钥",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\b(?:sk|rk)_(?:test|live|prod)_[A-Za-z0-9]{10,99}\b",
            &[
                "sk_test", "sk_live", "sk_prod", "rk_test", "rk_live", "rk_prod",
            ],
        ),
        BuiltinRule::new(
            "catalog:npm-token",
            "npm Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"(?i)\bnpm_[a-z0-9]{36}\b",
            &["npm_"],
        ),
        BuiltinRule::new(
            "catalog:pypi-token",
            "PyPI Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bpypi-AgEIcHlwaS5vcmc[\w-]{50,1000}",
            &["pypi-AgEIcHlwaS5vcmc"],
        ),
        BuiltinRule::new(
            "catalog:slack-bot-token",
            "Slack Bot Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bxoxb-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9-]*\b",
            &["xoxb-"],
        ),
        BuiltinRule::new(
            "catalog:bedrock-key",
            "AWS Bedrock 长期 API Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bABSK[A-Za-z0-9+/]{109,269}={0,2}",
            &["ABSK"],
        ),
        BuiltinRule::new(
            "catalog:google-api-key",
            "Google API Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bAIza[\w-]{35}\b",
            &["AIza"],
        )
        .with_allowlist(GOOGLE_API_KEY_ALLOWLIST),
        BuiltinRule::new(
            "catalog:sendgrid-token",
            "SendGrid Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bSG\.(?i:[a-z0-9=_.-]{66})\b",
            &["SG."],
        ),
        BuiltinRule::new(
            "catalog:huggingface-token",
            "Hugging Face Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bhf_(?i:[a-z]{34})\b",
            &["hf_"],
        ),
        BuiltinRule::new(
            "catalog:digitalocean-pat",
            "DigitalOcean PAT",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bdop_v1_[a-f0-9]{64}\b",
            &["dop_v1_"],
        ),
        BuiltinRule::new(
            "catalog:notion-token",
            "Notion API Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bntn_[0-9]{11}[A-Za-z0-9]{32}[A-Za-z0-9]{3}\b",
            &["ntn_"],
        ),
        BuiltinRule::new(
            "catalog:linear-key",
            "Linear API Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\blin_api_(?i:[a-z0-9]{40})\b",
            &["lin_api_"],
        ),
        BuiltinRule::new(
            "catalog:shopify-token",
            "Shopify Access Token",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bshpat_[a-fA-F0-9]{32}\b",
            &["shpat_"],
        ),
        BuiltinRule::new(
            "catalog:cloudflare-origin-key",
            "Cloudflare Origin CA Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bv1\.0-[a-f0-9]{24}-[a-f0-9]{146}\b",
            &["v1.0-", "cloudflare"],
        ),
        // Optional catalog: AI providers and gateways (default action = Allow).
        BuiltinRule::new(
            "catalog:openrouter-key",
            "OpenRouter Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\b(?:sk|v1)[-_][A-Za-z0-9_-]+\b",
            &["sk", "v1"],
        ),
        BuiltinRule::new(
            "catalog:groq-key",
            "Groq Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bgsk_[A-Za-z0-9]{52}\b",
            &["gsk_"],
        ),
        BuiltinRule::new(
            "catalog:perplexity-key",
            "Perplexity Key",
            RuleCategory::ApiToken,
            RiskLevel::High,
            AuditAction::Allow,
            r"\bpplx-[A-Za-z0-9]{48}\b",
            &["pplx-"],
        ),
        BuiltinRule::new(
            "catalog:generic-sk-key-notice",
            "通用 sk- 疑似 API Key 提醒",
            RuleCategory::ApiToken,
            RiskLevel::Medium,
            AuditAction::Allow,
            r"\bsk-[A-Za-z0-9_-]+\b",
            &["sk-"],
        ),
        // Optional catalog: passwords (default action = Allow).
        BuiltinRule::new(
            "catalog:login-url-password",
            "登录 URL 中的账号密码",
            RuleCategory::Password,
            RiskLevel::High,
            AuditAction::Allow,
            r#"(?i)\b[a-z][a-z0-9+.-]*:\/\/[^:\/\s@]+:([^@\s\/?#]+)@[^ \s<>"']+"#,
            &["://"],
        )
        .with_capture_group(1),
        BuiltinRule::new(
            "catalog:password-assignment",
            "配置文本中的密码赋值",
            RuleCategory::Password,
            RiskLevel::High,
            AuditAction::Allow,
            r#"(?i)["']?(?:password|passwd|pwd|passphrase|passcode)["']?\s*(?:=>|=|:)\s*["']?([^\s,\[\]"'}]+)"#,
            &["password", "passwd", "pwd", "passphrase", "passcode"],
        )
        .with_capture_group(1),
        BuiltinRule::new(
            "catalog:password-field",
            "结构化参数中的密码字段",
            RuleCategory::Password,
            RiskLevel::High,
            AuditAction::Allow,
            r"(?s).+",
            &[],
        ),
    ]
});

pub static GLOBAL_AHO_CORASICK: LazyLock<Option<AhoCorasick>> = LazyLock::new(|| {
    let mut patterns = Vec::new();
    for rule in BUILTIN_RULES.iter() {
        patterns.extend(rule.trigger_keywords.iter().copied());
    }

    match AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(patterns)
    {
        Ok(matcher) => Some(matcher),
        Err(error) => {
            log::error!("[SecurityAudit] Aho-Corasick 规则索引构建失败: {error}");
            None
        }
    }
});
