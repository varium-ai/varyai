//! Local gateway credentials and completed-usage token budgets.
use super::usage::parser::TokenUsage;
use crate::{database::Database, error::AppError};
use chrono::{Datelike, TimeZone, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};

mod ingress;
pub use ingress::handle;
pub(crate) use ingress::{error as policy_error, meter as meter_response};
mod bindings;
pub use bindings::{catalog, enforce_agent, GatewayCatalog, GatewayUsage};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Never,
    Daily,
    Monthly,
}
impl Period {
    fn window(self, now: i64) -> (i64, Option<i64>) {
        match self {
            Self::Never => (0, None),
            Self::Daily => {
                let start = now.div_euclid(86400) * 86400;
                (start, Some(start + 86400))
            }
            Self::Monthly => {
                let Some(date) = Utc.timestamp_opt(now, 0).single() else {
                    return (0, None);
                };
                let start = Utc
                    .with_ymd_and_hms(date.year(), date.month(), 1, 0, 0, 0)
                    .single();
                let (year, month) = if date.month() == 12 {
                    (date.year() + 1, 1)
                } else {
                    (date.year(), date.month() + 1)
                };
                (
                    start.map(|d| d.timestamp()).unwrap_or(0),
                    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
                        .single()
                        .map(|d| d.timestamp()),
                )
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPolicy {
    pub name: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub all_models: bool,
    pub token_limit: Option<u64>,
    pub period: Period,
}
impl KeyPolicy {
    fn validate(&self) -> Result<(), AppError> {
        if self.name.trim().is_empty()
            || self.name.len() > 120
            || (!self.all_models && self.models.is_empty())
            || self.models.len() > 200
            || self
                .models
                .iter()
                .any(|m| m.trim() != m || m.is_empty() || m.len() > 200)
            || self.token_limit.is_some_and(|v| v > i64::MAX as u64)
        {
            return Err(AppError::Config("Invalid gateway key policy".into()));
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayKey {
    pub id: String,
    pub policy: KeyPolicy,
    pub revoked: bool,
    pub used_tokens: u64,
    pub resets_at: Option<i64>,
    pub agents: Vec<String>,
}
#[derive(Serialize)]
pub struct CreatedKey {
    pub key: GatewayKey,
    pub secret: String,
}
#[derive(Clone, Debug)]
pub struct Grant {
    pub id: String,
    pub window: i64,
    pub agent: String,
    pub model: String,
    pub upstream_model: GatewayUpstreamModel,
}

#[derive(Clone, Debug, Default)]
pub struct GatewayUpstreamModel(Arc<OnceLock<String>>);

impl GatewayUpstreamModel {
    pub(crate) fn record(&self, model: Option<&str>) {
        if let Some(model) = model.filter(|m| !m.is_empty() && m.len() <= 200) {
            let _ = self.0.set(model.to_owned());
        }
    }

    fn get(&self) -> Option<&str> {
        self.0.get().map(String::as_str)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Invalid or revoked gateway credential")]
    Unauthorized,
    #[error("Model is not permitted by this gateway key")]
    Forbidden,
    #[error("This operation is not supported by gateway budget policies")]
    UnsupportedOperation,
    #[error("Gateway token budget exhausted")]
    Exhausted(Option<i64>),
    #[error("Gateway policy unavailable")]
    Storage,
}
fn digest(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

impl Database {
    pub fn create_gateway_key(&self, policy: KeyPolicy) -> Result<CreatedKey, AppError> {
        policy.validate()?;
        let mut random = [0u8; 32];
        getrandom::getrandom(&mut random)
            .map_err(|_| AppError::Config("Credential generation failed".into()))?;
        let secret = format!(
            "vary-gw-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let id = uuid::Uuid::new_v4().to_string();
        let conn = crate::database::lock_conn!(self.extension_connection());
        conn.execute(
            "INSERT INTO gateway_keys (id, secret_hash, policy, revoked) VALUES (?1,?2,?3,0)",
            rusqlite::params![
                id,
                digest(&secret),
                crate::database::to_json_string(&policy)?
            ],
        )?;
        Ok(CreatedKey {
            key: GatewayKey {
                id,
                resets_at: policy.period.window(Utc::now().timestamp()).1,
                policy,
                revoked: false,
                used_tokens: 0,
                agents: Vec::new(),
            },
            secret,
        })
    }
    pub fn update_gateway_key(&self, id: &str, policy: KeyPolicy) -> Result<(), AppError> {
        policy.validate()?;
        let conn = crate::database::lock_conn!(self.extension_connection());
        if conn.execute(
            "UPDATE gateway_keys SET policy=?2 WHERE id=?1",
            rusqlite::params![id, crate::database::to_json_string(&policy)?],
        )? == 0
        {
            return Err(AppError::Config("Gateway key not found".into()));
        }
        Ok(())
    }
    pub fn revoke_gateway_key(&self, id: &str) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        conn.execute("UPDATE gateway_keys SET revoked=1 WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn delete_gateway_key(&self, id: &str) -> Result<(), AppError> {
        let mut conn = crate::database::lock_conn!(self.extension_connection());
        let tx = conn.transaction()?;
        let eligible: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM gateway_keys WHERE id=?1 AND revoked=1) AND NOT EXISTS(SELECT 1 FROM gateway_agent_bindings WHERE key_id=?1)",
            [id], |r| r.get(0),
        )?;
        if !eligible {
            return Err(AppError::Config(
                "Only revoked, unbound gateway keys can be deleted".into(),
            ));
        }
        tx.execute("DELETE FROM gateway_usage_details WHERE key_id=?1", [id])?;
        tx.execute("DELETE FROM gateway_usage_models WHERE key_id=?1", [id])?;
        tx.execute("DELETE FROM gateway_usage WHERE key_id=?1", [id])?;
        tx.execute("DELETE FROM gateway_keys WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn list_gateway_keys(&self) -> Result<Vec<GatewayKey>, AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        let mut stmt =
            conn.prepare("SELECT id, policy, revoked FROM gateway_keys ORDER BY rowid DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, raw, revoked)| {
                let policy: KeyPolicy = serde_json::from_str(&raw)
                    .map_err(|_| AppError::Config("Invalid gateway policy".into()))?;
                let (window, resets_at) = policy.period.window(Utc::now().timestamp());
                let used_tokens = conn
                    .query_row(
                        "SELECT tokens FROM gateway_usage WHERE key_id=?1 AND window=?2",
                        rusqlite::params![id, window],
                        |r| r.get::<_, u64>(0),
                    )
                    .optional()?
                    .unwrap_or(0);
                Ok(GatewayKey {
                    agents: conn
                        .prepare(
                            "SELECT app FROM gateway_agent_bindings WHERE key_id=?1 ORDER BY app",
                        )?
                        .query_map([&id], |r| r.get(0))?
                        .collect::<Result<Vec<_>, _>>()?,
                    id,
                    policy,
                    revoked,
                    used_tokens,
                    resets_at,
                })
            })
            .collect()
    }
    pub fn authorize_gateway_key(
        &self,
        secret: &str,
        model: &str,
        now: i64,
    ) -> Result<Grant, GatewayError> {
        let conn = self
            .extension_connection()
            .lock()
            .map_err(|_| GatewayError::Storage)?;
        let row: Option<(String, String, bool)> = conn
            .query_row(
                "SELECT id,policy,revoked FROM gateway_keys WHERE secret_hash=?1",
                [digest(secret)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| GatewayError::Storage)?;
        let (id, raw, revoked) = row.ok_or(GatewayError::Unauthorized)?;
        if revoked {
            return Err(GatewayError::Unauthorized);
        }
        let policy: KeyPolicy = serde_json::from_str(&raw).map_err(|_| GatewayError::Storage)?;
        if model.is_empty()
            || model.len() > 200
            || (!policy.all_models && !policy.models.iter().any(|m| m == model))
        {
            return Err(GatewayError::Forbidden);
        }
        let (window, reset) = policy.period.window(now);
        let tokens: u64 = conn
            .query_row(
                "SELECT tokens FROM gateway_usage WHERE key_id=?1 AND window=?2",
                rusqlite::params![id, window],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| GatewayError::Storage)?
            .unwrap_or(0);
        if policy.token_limit.is_some_and(|limit| tokens >= limit) {
            return Err(GatewayError::Exhausted(reset));
        }
        Ok(Grant {
            id,
            window,
            agent: "external".into(),
            model: model.into(),
            upstream_model: GatewayUpstreamModel::default(),
        })
    }
    pub fn charge_gateway_key(&self, id: &str, window: i64, tokens: u64) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(self.extension_connection());
        conn.execute("INSERT INTO gateway_usage (key_id,window,tokens) VALUES (?1,?2,?3) ON CONFLICT(key_id,window) DO UPDATE SET tokens=MIN(9223372036854775807, tokens+excluded.tokens)", rusqlite::params![id, window, tokens.min(i64::MAX as u64)])?;
        Ok(())
    }
}
fn total_tokens(usage: &TokenUsage, cache_inclusive: bool) -> u64 {
    u64::from(usage.input_tokens)
        + u64::from(usage.output_tokens)
        + u64::from(usage.cache_creation_tokens)
        + if cache_inclusive {
            0
        } else {
            u64::from(usage.cache_read_tokens)
        }
}
#[cfg(test)]
mod tests;
