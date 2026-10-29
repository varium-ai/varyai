//! Private, bounded request-action summaries. Never retain plaintext or token references.
use std::{ops::Range, sync::Mutex, time::Instant};

use serde_json::Value;

use super::{
    aggregation::PendingAuditEvent,
    builtin_rules::BUILTIN_RULES,
    extractor::extract_payload_texts,
    identity::{fingerprint, ContentFingerprint},
    payload_budget::{PayloadBudget, Reservation},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    request_carriers::CodexRequestCarriers,
    request_detection::detect_spans_with_budget,
    request_plan::{plan_request_with_budget, EffectiveAction, PlanError, PlanField, SpanMatch},
    runtime::SecurityRuntimeConfig,
    tokenization::{visit_token_references, DlpRequestLease, TokenError},
};
use crate::{app_config::AppType, database::AuditLogEvent};

const MAX_FINDINGS: usize = 4096;
const MAX_METADATA: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FindingKey {
    rule: ContentFingerprint,
    content: ContentFingerprint,
}

struct FindingSummary {
    key: FindingKey,
    rule_id: PayloadText,
    name: &'static str,
    category: &'static str,
    risk: &'static str,
    action: &'static str,
    risk_epoch: u64,
    config_revision: u64,
    aggregate: bool,
}

pub(super) struct PendingRequestAudit {
    findings: PayloadVec<FindingSummary>,
}

struct LogicalField<'a> {
    text: &'a str,
    matches: PayloadVec<SpanMatch>,
    rules: PayloadVec<&'a str>,
}

impl PendingRequestAudit {
    pub(super) fn empty(budget: PayloadBudget) -> Result<Self, TokenError> {
        Ok(Self {
            findings: PayloadVec::new(budget)?,
        })
    }

    pub(super) fn capture(
        body: &Value,
        runtime: &SecurityRuntimeConfig,
        restore_ids: &[&str],
        lease: &DlpRequestLease,
        budget: PayloadBudget,
    ) -> Result<Self, PlanError> {
        let carriers = CodexRequestCarriers::from_body_with_budget(body, budget.clone())?;
        let mut logical = PayloadVec::new(budget.clone())?;
        let mut count = 0usize;
        carriers.visit(|leaf| {
            let mut references = PayloadVec::new(budget.clone())?;
            visit_token_references(leaf.text, |range| references.push(range))?;
            let mut matches = PayloadVec::new(budget.clone())?;
            let mut rules = PayloadVec::new(budget.clone())?;
            for found in detect_spans_with_budget(
                leaf.text,
                leaf.is_password_field,
                runtime,
                budget.clone(),
            )?
            .as_slice()
            {
                if found.range.is_empty()
                    || references.as_slice().iter().any(|range| {
                        range.start <= found.range.start
                            && found.range.end <= range.end
                            && lease.resolve(&leaf.text[range.clone()]).is_ok()
                    })
                {
                    continue;
                }
                count = count
                    .checked_add(1)
                    .filter(|n| *n <= MAX_FINDINGS)
                    .ok_or(TokenError::CapacityExceeded)?;
                let action = if found.action == EffectiveAction::Redact
                    && found.rule_id != "catalog:generic-sk-key-notice"
                    && restore_ids.contains(&found.rule_id)
                {
                    EffectiveAction::Restore
                } else {
                    found.action
                };
                matches.push(SpanMatch {
                    range: found.range.clone(),
                    action,
                    category: found.category,
                })?;
                rules.push(found.rule_id)?;
            }
            logical.push(LogicalField {
                text: leaf.text,
                matches,
                rules,
            })
        })?;
        let mut fields = PayloadVec::new(budget.clone())?;
        for field in logical.as_slice() {
            fields.push(PlanField {
                text: field.text,
                writable: true,
                matches: field.matches.as_slice(),
            })?;
        }
        let mut historical = PayloadVec::new(budget.clone())?;
        lease.visit_authorized_tokens(|token| historical.push(lease.resolve(token)?))?;
        let plan =
            plan_request_with_budget(fields.as_slice(), historical.as_slice(), budget.clone())?;
        let mut pending = Self::empty(budget.clone())?;
        for (index, field) in logical.as_slice().iter().enumerate() {
            for (found, rule) in field.matches.as_slice().iter().zip(field.rules.as_slice()) {
                let action = if found.action == EffectiveAction::Restore
                    && plan.as_slice()[index].spans.as_slice().iter().any(|span| {
                        span.action == EffectiveAction::Redact
                            && overlaps(&span.range, &found.range)
                    }) {
                    EffectiveAction::Redact
                } else {
                    found.action
                };
                pending.push(
                    field.text,
                    found.range.clone(),
                    rule,
                    action,
                    runtime,
                    &budget,
                )?;
            }
        }
        Ok(pending)
    }

    pub(super) fn capture_refusal(
        body: &Value,
        runtime: &SecurityRuntimeConfig,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        let carriers = CodexRequestCarriers::from_body_with_budget(body, budget.clone())?;
        let mut pending = Self::empty(budget.clone())?;
        carriers.visit(|leaf| {
            for found in detect_spans_with_budget(
                leaf.text,
                leaf.is_password_field,
                runtime,
                budget.clone(),
            )?
            .as_slice()
            {
                pending.push(
                    leaf.text,
                    found.range.clone(),
                    found.rule_id,
                    found.action,
                    runtime,
                    &budget,
                )?;
            }
            Ok(())
        })?;
        // Legacy protocol roots are still findings when restoration refuses an unsupported route.
        // Precharge the worst-case extracted-node vector and display paths before the extractor allocates.
        let mut extraction_charge = budget.reserve(0)?;
        charge_extraction(body, 0, 0, &mut 0, &mut extraction_charge)?;
        for field in extract_payload_texts(body) {
            for found in detect_spans_with_budget(
                field.text,
                field.is_password_field,
                runtime,
                budget.clone(),
            )?
            .as_slice()
            {
                pending.push(
                    field.text,
                    found.range.clone(),
                    found.rule_id,
                    found.action,
                    runtime,
                    &budget,
                )?;
            }
        }
        Ok(pending)
    }

    fn push(
        &mut self,
        text: &str,
        range: Range<usize>,
        rule: &str,
        action: EffectiveAction,
        runtime: &SecurityRuntimeConfig,
        budget: &PayloadBudget,
    ) -> Result<(), TokenError> {
        if action == EffectiveAction::Allow {
            return Ok(());
        }
        let content = text.get(range).ok_or(TokenError::RedactionFailed)?;
        let (Some(rule_identity), Some(content_identity)) =
            (fingerprint(rule), fingerprint(content))
        else {
            // Auxiliary identity loss skips only this summary; core preparation errors still propagate.
            log::warn!("[SecurityAudit] stage=request_summary category=identity_unavailable");
            return Ok(());
        };
        let key = FindingKey {
            rule: rule_identity,
            content: content_identity,
        };
        if self
            .findings
            .as_slice()
            .iter()
            .any(|summary| summary.key == key)
        {
            return Ok(());
        }
        if self.findings.len() >= MAX_FINDINGS {
            return Err(TokenError::CapacityExceeded);
        }
        let builtin = BUILTIN_RULES.iter().find(|builtin| builtin.id == rule);
        let custom = runtime.custom_rules.iter().find(|custom| custom.id == rule);
        let (name, category, risk) = if let Some(builtin) = builtin {
            (
                builtin.name,
                builtin.category.as_str(),
                builtin.risk_level.as_str(),
            )
        } else if matches!(rule, "chinese_phone_number" | "chinese_id_card") {
            ("Personal information", "pii", "HIGH")
        } else {
            (
                "Custom security rule",
                "custom",
                custom.map_or("HIGH", |rule| rule.risk_level.as_str()),
            )
        };
        // Rule labels/expressions and matched snippets are private configuration/content.
        let safe_rule = if safe_metadata(rule) && (content.is_empty() || !rule.contains(content)) {
            rule
        } else {
            "custom-rule"
        };
        self.findings.push(FindingSummary {
            key,
            rule_id: PayloadText::try_from_str(safe_rule, budget.clone(), MAX_METADATA)?,
            name,
            category,
            risk,
            action: match action {
                EffectiveAction::Audit => "audit",
                EffectiveAction::Redact => "redact",
                EffectiveAction::Restore => "redact_restore",
                EffectiveAction::Block => "block",
                EffectiveAction::Allow => "allow",
            },
            risk_epoch: runtime.risk_epoch(rule),
            config_revision: runtime.config_revision,
            aggregate: runtime.aggregation_enabled,
        })
    }
}

fn charge_extraction(
    value: &Value,
    depth: usize,
    path_bytes: usize,
    nodes: &mut usize,
    charge: &mut Reservation,
) -> Result<(), TokenError> {
    *nodes = nodes
        .checked_add(1)
        .filter(|nodes| *nodes <= 65_536)
        .ok_or(TokenError::CapacityExceeded)?;
    if depth > 64 {
        return Err(TokenError::CapacityExceeded);
    }
    charge.grow(
        path_bytes
            .checked_mul(4)
            .and_then(|n| n.checked_add(256))
            .ok_or(TokenError::CapacityExceeded)?,
    )?;
    match value {
        Value::Array(values) => {
            for value in values {
                charge_extraction(value, depth + 1, path_bytes + 32, nodes, charge)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let path = path_bytes
                    .checked_add(key.len())
                    .and_then(|n| n.checked_add(32))
                    .ok_or(TokenError::CapacityExceeded)?;
                charge_extraction(value, depth + 1, path, nodes, charge)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn safe_metadata(value: &str) -> bool {
    value.len() <= MAX_METADATA
        && !value.contains("vary_")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:.".contains(&byte))
}

pub(super) struct RequestAuditState {
    app: PayloadText,
    session: Option<PayloadText>,
    seen: Mutex<PayloadVec<FindingKey>>,
    _allocation: Reservation,
    #[cfg(test)]
    recorded: Mutex<Vec<PendingAuditEvent>>,
}
impl RequestAuditState {
    #[cfg(test)]
    pub(super) fn recorded_for_test(&self) -> Vec<PendingAuditEvent> {
        self.recorded
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
    pub(super) fn new(
        app_type: &AppType,
        session_id: Option<&str>,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        let allocation =
            budget.reserve(std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>())?;
        let session = session_id
            .filter(|id| safe_metadata(id))
            .map(|id| PayloadText::try_from_str(id, budget.clone(), MAX_METADATA))
            .transpose()?;
        let app = PayloadText::try_from_str(app_type.as_str(), budget.clone(), MAX_METADATA)?;
        Ok(Self {
            app,
            session,
            seen: Mutex::new(PayloadVec::new(budget)?),
            _allocation: allocation,
            #[cfg(test)]
            recorded: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn record_permitted_attempt(
        &self,
        pending: &PendingRequestAudit,
        provider_id: &str,
        request_id: Option<&str>,
    ) {
        self.record_with(
            pending,
            provider_id,
            request_id,
            false,
            super::emit_audit_event,
        );
    }

    pub(super) fn record_refusal(
        &self,
        pending: &PendingRequestAudit,
        provider_id: &str,
        request_id: Option<&str>,
    ) {
        self.record_with(
            pending,
            provider_id,
            request_id,
            true,
            super::emit_audit_event,
        );
    }

    pub(super) fn record_with(
        &self,
        pending: &PendingRequestAudit,
        provider_id: &str,
        request_id: Option<&str>,
        refused: bool,
        mut emit: impl FnMut(PendingAuditEvent),
    ) {
        for finding in pending.findings.as_slice() {
            // Only protect the small request-local identity set; never perform queue/DB work under it.
            let Ok(mut seen) = self.seen.lock() else {
                return;
            };
            if seen.as_slice().contains(&finding.key) {
                continue;
            }
            if seen.len() >= MAX_FINDINGS || seen.push(finding.key).is_err() {
                log::warn!("[SecurityAudit] stage=request_summary category=capacity");
                return;
            }
            drop(seen);
            let event = PendingAuditEvent {
                log: AuditLogEvent {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    request_id: request_id.filter(|id| safe_metadata(id)).map(str::to_owned),
                    session_id: self
                        .session
                        .as_ref()
                        .map(|session| session.as_str().to_owned()),
                    app_type: self.app.as_str().to_owned(),
                    provider_id: if safe_metadata(provider_id) {
                        provider_id.to_owned()
                    } else {
                        String::new()
                    },
                    rule_id: finding.rule_id.as_str().to_owned(),
                    rule_name: finding.name.to_owned(),
                    rule_category: finding.category.to_owned(),
                    risk_level: finding.risk.to_owned(),
                    action_taken: if refused { "block" } else { finding.action }.to_owned(),
                    matched_snippet_masked: "[redacted]".to_owned(),
                    location: None,
                },
                identity: finding.aggregate.then_some(finding.key.content),
                detected_at: Instant::now(),
                risk_epoch: finding.risk_epoch,
                config_revision: finding.config_revision,
            };
            #[cfg(test)]
            if let Ok(mut recorded) = self.recorded.lock() {
                recorded.push(event.clone());
            }
            emit(event);
        }
    }
}
