//! Aggregate-only projection for the quick panel. No request bodies or errors cross IPC.
use super::*;
use crate::proxy::security::http_restoration::DlpForwardError;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayPeriod {
    Today,
    Week,
    Month,
    All,
    Activity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayUsageRow {
    pub bucket: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub requests: u64,
    pub failed: u64,
    pub input: u64,
    pub output: u64,
    pub reads: u64,
    pub writes: u64,
    pub cost: f64,
    pub unpriced: u64,
    pub first_ms: u64,
    pub first_count: u64,
    pub timed_output: u64,
    pub generation_ms: u64,
    pub timed_count: u64,
}

impl Database {
    pub fn tray_usage(&self, period: TrayPeriod, now: i64) -> Result<Vec<TrayUsageRow>, AppError> {
        let (days, granularity) = match period {
            TrayPeriod::Today => (0, "%H:00"),
            TrayPeriod::Week => (6, "%Y-%m-%d"),
            TrayPeriod::Month => (29, "%Y-%m-%d"),
            TrayPeriod::All => (0, "%Y-%m-%d"),
            TrayPeriod::Activity => (0, "%Y-%m-%d"),
        };
        let start = if matches!(period, TrayPeriod::Activity) {
            // Monday of the current local week, then back 52 full weeks.
            "date(?1,'unixepoch','localtime','-6 days','weekday 1','-364 days')".to_owned()
        } else {
            format!("date(?1,'unixepoch','localtime','-{days} days')")
        };
        let (detail_since, rollup_since) = if matches!(period, TrayPeriod::All) {
            (String::new(), String::new())
        } else {
            (
                format!("AND l.created_at >= unixepoch({start},'utc')"),
                format!("AND r.date >= {start}"),
            )
        };
        // Rollups represent complete, pruned days. Never assign them an invented hour.
        let rollup_until = if matches!(period, TrayPeriod::Today) {
            "0"
        } else {
            "r.date <= date(?1,'unixepoch','localtime')"
        };
        let grouping_model = |alias| {
            if matches!(period, TrayPeriod::Activity) {
                "''".to_owned()
            } else {
                effective_model_sql(alias)
            }
        };
        let model = grouping_model("l");
        let rollup_model = grouping_model("r");
        let fresh = fresh_input_sql("l");
        let rollup_fresh = fresh_input_sql("r");
        let provider = provider_name_coalesce("l", "p", Some("l.data_source"));
        let rollup_provider = provider_name_coalesce("r", "p", None);
        let detail_join = providers_join("l", "p");
        let rollup_join = providers_join("r", "p");
        let effective = effective_usage_log_filter("l");
        let generation = "COALESCE(NULLIF(l.duration_ms,0),l.latency_ms-l.first_token_ms)";
        let timed = format!(
            "l.output_tokens > 0 AND {generation} > 0 AND l.status_code BETWEEN 200 AND 299"
        );
        let sql = format!(
            "SELECT strftime('{granularity}',l.created_at,'unixepoch','localtime'),
                l.provider_id, {provider}, {model},
                COUNT(*), SUM(l.status_code NOT BETWEEN 200 AND 299),
                SUM({fresh}), SUM(l.output_tokens), SUM(l.cache_read_tokens),
                SUM(l.cache_creation_tokens), SUM(CAST(l.total_cost_usd AS REAL)),
                SUM(l.status_code BETWEEN 200 AND 299
                    AND l.input_tokens+l.output_tokens+l.cache_read_tokens+l.cache_creation_tokens > 0
                    AND CAST(l.total_cost_usd AS REAL) = 0
                    AND CAST(COALESCE(l.cost_multiplier,'1') AS REAL) != 0),
                SUM(CASE WHEN l.first_token_ms > 0 THEN l.first_token_ms ELSE 0 END),
                COUNT(CASE WHEN l.first_token_ms > 0 THEN 1 END),
                SUM(CASE WHEN {timed} THEN l.output_tokens ELSE 0 END),
                SUM(CASE WHEN {timed} THEN {generation} ELSE 0 END),
                COUNT(CASE WHEN {timed} THEN 1 END), p.name IS NULL
             FROM proxy_request_logs l {detail_join}
             WHERE l.created_at <= ?1 {detail_since} AND {effective}
                AND COALESCE(l.error_message,'') != ?2
             GROUP BY 1,2,3,4,18
             UNION ALL
             SELECT r.date, r.provider_id, {rollup_provider}, {rollup_model},
                SUM(r.request_count), SUM(r.request_count-r.success_count),
                SUM({rollup_fresh}), SUM(r.output_tokens), SUM(r.cache_read_tokens),
                SUM(r.cache_creation_tokens), SUM(CAST(r.total_cost_usd AS REAL)), 0,0,0,0,0,0,
                p.name IS NULL
             FROM usage_daily_rollups r {rollup_join}
             WHERE {rollup_until} {rollup_since}
             GROUP BY 1,2,3,4,18"
        );
        // This optional lookup releases the extension lock before the ledger read.
        let routing_names = self.routing_usage_names().unwrap_or_default();
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![now, DlpForwardError::Blocked.client_message()],
            |row| {
                let provider_id: String = row.get(1)?;
                let mut provider_name: String = row.get(2)?;
                let unresolved = row.get::<_, bool>(17)? && provider_name == provider_id;
                if let Some(name) = routing_names.get(&provider_id).filter(|_| unresolved) {
                    provider_name.clone_from(name);
                }
                Ok(TrayUsageRow {
                    bucket: row.get(0)?,
                    provider_id,
                    provider_name,
                    model: row.get(3)?,
                    requests: row.get(4)?,
                    failed: row.get(5)?,
                    input: row.get(6)?,
                    output: row.get(7)?,
                    reads: row.get(8)?,
                    writes: row.get(9)?,
                    cost: row.get(10)?,
                    unpriced: row.get(11)?,
                    first_ms: row.get(12)?,
                    first_count: row.get(13)?,
                    timed_output: row.get(14)?,
                    generation_ms: row.get(15)?,
                    timed_count: row.get(16)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
