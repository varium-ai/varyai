use super::*;
use crate::database::Database;
use rusqlite::params;
use serde_json::{json, Value};

fn filter(value: Value) -> SecurityEventFilter {
    serde_json::from_value(value).unwrap()
}
fn default_filter() -> SecurityEventFilter {
    filter(json!({"view":"security_events"}))
}
fn tool_filter() -> SecurityEventFilter {
    filter(json!({"view":"tool_calls"}))
}

fn dlp(db: &Database, id: i64, ts: i64, action: &str, level: &str) {
    db.extension_connection().lock().unwrap().execute(
        "INSERT INTO security_audit_logs (id,timestamp,app_type,provider_id,rule_id,rule_name,rule_category,risk_level,action_taken,matched_snippet_masked,trigger_count,last_seen_at,session_id) VALUES (?1,?2,'claude','provider','deleted-rule','literal %_\\ needle','pii',?3,?4,'masked only',42,?2+100000,'session%_')",
        params![id, ts, level, action]).unwrap();
}
fn tool(db: &Database, id: i64, ts: i64, verdict: Option<&str>, severity: Option<&str>) {
    db.extension_connection().lock().unwrap().execute(
        "INSERT INTO tool_risk_events (id,timestamp,source,stage,client_category,operation_category,command_summary,detection_scope,integrity_status,verdict,risk_category,severity,policy_version,rule_ids,suppressed_count) VALUES (?1,?2,'claude','response','claude','shell','git status','tool','complete',?3,'pii',?4,999,'[\"deleted-rule\"]',88)",
        params![id, ts, verdict, severity]).unwrap();
}
fn fixture() -> Database {
    let db = Database::memory().unwrap();
    for (id, action, level) in [
        (1, "audit", "HIGH"),
        (2, "redact", "CRITICAL"),
        (3, "block", "MEDIUM"),
        (4, "allow", "LOW"),
        (5, "future-action", "ULTRA"),
    ] {
        dlp(&db, id, 1000, action, level);
    }
    for (id, verdict, severity) in [
        (1, Some("risky"), Some("high")),
        (2, Some("unchecked"), Some("high")),
        (3, Some("uncertain"), Some("medium")),
        (4, Some("low_risk"), Some("none")),
        (5, None, None),
        (6, Some("future"), Some("invalid")),
        (7, Some("risky"), None),
    ] {
        tool(&db, id, 1000, verdict, severity);
    }
    db
}
fn keys(page: &SecurityEventPage) -> Vec<String> {
    page.items.iter().map(|item| item.key.clone()).collect()
}

#[test]
fn mixed_paging_is_stable_and_ids_are_namespaced() {
    let db = fixture();
    let first = db.get_security_events(&default_filter(), 1, 4).unwrap();
    let second = db.get_security_events(&default_filter(), 2, 4).unwrap();
    assert_eq!(first.total, 7);
    assert_eq!(second.total, 7);
    assert_eq!(keys(&first), ["dlp:5", "dlp:4", "dlp:3", "dlp:2"]);
    assert_eq!(keys(&second), ["dlp:1", "tool_risk:7", "tool_risk:1"]);
    assert_eq!(
        db.get_security_events(&default_filter(), 3, 4)
            .unwrap()
            .items
            .len(),
        0
    );
    let value = serde_json::to_value(&second).unwrap();
    assert_eq!(value["pageSize"], 4);
    assert_eq!(value["items"][0]["eventType"], "dlp");
    assert_eq!(value["items"][0]["record"]["triggerCount"], 42);
    assert_eq!(value["items"][1]["triggerCount"], 1);
    assert_eq!(value["items"][1]["lastSeenAt"], 1000);
    assert!(value["items"][1]["sessionId"].is_null());
    assert_eq!(value["items"][1]["record"]["suppressedCount"], 88);
}

#[test]
fn summary_counts_rows_and_preserves_unknown_dlp_values() {
    let db = fixture();
    let summary = db.get_security_events_summary(&default_filter()).unwrap();
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        json!({"total":7,"dlpCount":5,"toolRiskCount":2,"redacted":1,"warnings":3,"protectiveBlocked":1,"other":2})
    );
    let page = db.get_security_events(&default_filter(), 1, 100).unwrap();
    let row = serde_json::to_value(&page.items[0]).unwrap();
    assert_eq!(row["result"], "unknown");
    assert!(row["riskLevel"].is_null());
    assert_eq!(row["record"]["actionTaken"], "future-action");
    assert_eq!(row["record"]["riskLevel"], "ULTRA");
    assert_eq!(
        db.get_security_events_summary(&tool_filter()),
        Err(SecurityEventError::InvalidParameters)
    );
}

#[test]
fn all_tool_calls_keeps_four_states_null_unknown_and_history() {
    let db = fixture();
    let page = db.get_security_events(&tool_filter(), 1, 100).unwrap();
    assert_eq!(page.total, 7);
    let rows: Value = serde_json::to_value(page).unwrap();
    assert_eq!(rows["items"][1]["result"], "unknown");
    assert_eq!(rows["items"][2]["result"], "unknown");
    for row in rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["record"]["verdict"] != "risky")
    {
        assert!(row["riskLevel"].is_null());
    }
    assert_eq!(
        db.get_security_events(
            &filter(json!({"view":"tool_calls","toolVerdict":"uncertain"})),
            1,
            10
        )
        .unwrap()
        .total,
        1
    );
    // No current settings, policy version or rule membership joins.
    assert!(!db.get_tool_risk_settings().unwrap().enabled);
}

#[test]
fn filters_follow_projection_and_source_specific_semantics() {
    let db = fixture();
    for (extra, expected) in [
        (json!({"riskLevel":"HIGH"}), 3),
        (json!({"riskLevel":"CRITICAL"}), 1),
        (json!({"riskLevel":"LOW"}), 1),
        (json!({"riskLevel":"MEDIUM"}), 1),
        (json!({"result":"warning"}), 3),
        (json!({"result":"unknown"}), 1),
        (json!({"eventType":"tool_risk"}), 2),
        (json!({"appType":"claude"}), 7),
        (json!({"appType":"claude-desktop"}), 0),
        (json!({"category":"dlp:pii"}), 5),
        (json!({"category":"tool_risk:pii"}), 2),
        (json!({"category":"tool_risk:pii","eventType":"dlp"}), 0),
        (json!({"sessionId":"  session%_  "}), 5),
        (json!({"sessionId":"session"}), 0),
        (json!({"keyword":"  %_\\  "}), 5),
        (json!({"keyword":"git status"}), 0),
        (json!({"keyword":"%not-a-wildcard"}), 0),
        (json!({"sessionId":"   ","keyword":"  "}), 7),
    ] {
        let mut f = json!({"view":"security_events"});
        f.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let f = filter(f);
        assert_eq!(
            db.get_security_events(&f, 1, 100).unwrap().total,
            expected,
            "{f:?}"
        );
        assert_eq!(
            db.get_security_events_summary(&f).unwrap().total,
            expected,
            "{f:?}"
        );
    }
    assert_eq!(
        db.get_security_events(&filter(json!({"view":"tool_calls","keyword":"git"})), 1, 10)
            .unwrap()
            .total,
        0
    );
}

#[test]
fn timestamp_boundaries_ignore_dlp_last_seen() {
    let db = Database::memory().unwrap();
    for (id, ts) in [(1, 999), (2, 1000), (3, 1999), (4, 2000)] {
        dlp(&db, id, ts, "audit", "HIGH");
        tool(&db, id, ts, Some("risky"), Some("medium"));
    }
    let f = filter(json!({"view":"security_events","startTimestamp":1000,"endTimestamp":1999}));
    assert_eq!(db.get_security_events(&f, 1, 100).unwrap().total, 4);
    assert_eq!(db.get_security_events_summary(&f).unwrap().total, 4);
}

#[test]
fn validates_pagination_times_strings_and_combinations() {
    let db = fixture();
    for (page, size) in [(0, 10), (1, 0), (1, 101), (u64::MAX, 100), (u64::MAX, 1)] {
        assert_eq!(
            db.get_security_events(&default_filter(), page, size)
                .unwrap_err(),
            SecurityEventError::InvalidParameters
        );
    }
    for value in [
        json!({"view":"security_events","startTimestamp":-1}),
        json!({"view":"security_events","endTimestamp":9007199254740992_i64}),
        json!({"view":"security_events","startTimestamp":2,"endTimestamp":1}),
        json!({"view":"security_events","toolVerdict":"risky"}),
        json!({"view":"tool_calls","eventType":"dlp"}),
        json!({"view":"security_events","keyword":"界".repeat(513)}),
        json!({"view":"security_events","sessionId":"界".repeat(513)}),
        json!({"view":"security_events","category":"missing-prefix"}),
    ] {
        let f = filter(value);
        assert_eq!(
            db.get_security_events(&f, 1, 10).unwrap_err(),
            SecurityEventError::InvalidParameters
        );
        assert_eq!(
            db.clear_security_events(&f).unwrap_err(),
            SecurityEventError::InvalidParameters
        );
    }
    for value in [
        json!({"view":"bad"}),
        json!({"view":"security_events","riskLevel":"ULTRA"}),
        json!({"view":"security_events","result":"future"}),
        json!({"view":"tool_calls","toolVerdict":"future"}),
    ] {
        assert!(serde_json::from_value::<SecurityEventFilter>(value).is_err());
    }
    let unicode = filter(json!({"view":"security_events","keyword":"界".repeat(512)}));
    assert!(db.get_security_events(&unicode, 1, 1).is_ok());
}

#[test]
fn tool_hydration_reuses_existing_privacy_projection() {
    let db = fixture();
    db.extension_connection().lock().unwrap().execute("UPDATE tool_risk_events SET command_summary=?1 WHERE id=1",["git status /workspace/project --token=super-private-token --email=alice@example.com"]).unwrap();
    let old = db.get_tool_risk_events_filtered(100, 0, None).unwrap();
    let page = db.get_security_events(&tool_filter(), 1, 100).unwrap();
    let row = page.items.iter().find(|r| r.key == "tool_risk:1").unwrap();
    let record = serde_json::to_value(row).unwrap()["record"].clone();
    assert_eq!(
        record,
        serde_json::to_value(old.iter().find(|r| r.id == 1).unwrap()).unwrap()
    );
    let text = record["commandSummary"].as_str().unwrap();
    assert!(text.contains("git status /workspace/project"));
    assert!(!text.contains("super-private-token"));
    assert!(!text.contains("alice@example.com"));
}

#[test]
fn clear_is_filtered_atomic_and_preserves_configuration() {
    let db = fixture();
    let before = db.get_tool_risk_settings().unwrap();
    let receipt = db.clear_security_events(&default_filter()).unwrap();
    assert_eq!(
        serde_json::to_value(receipt).unwrap(),
        json!({"deletedTotal":7,"deletedDlp":5,"deletedToolRisk":2})
    );
    assert_eq!(
        db.get_security_events(&tool_filter(), 1, 100)
            .unwrap()
            .total,
        5
    );
    assert_eq!(db.get_tool_risk_settings().unwrap(), before);
    let f = filter(json!({"view":"tool_calls","toolVerdict":"uncertain"}));
    assert_eq!(db.clear_security_events(&f).unwrap().deleted_tool_risk, 1);
    assert_eq!(
        db.clear_security_events(&tool_filter())
            .unwrap()
            .deleted_tool_risk,
        4
    );

    let db = fixture();
    db.extension_connection().lock().unwrap().execute_batch("CREATE TRIGGER deny_tool_delete BEFORE DELETE ON tool_risk_events BEGIN SELECT RAISE(ABORT, 'fixture-secret-do-not-echo'); END;").unwrap();
    let error = db.clear_security_events(&default_filter()).unwrap_err();
    assert_eq!(error, SecurityEventError::Database);
    assert!(!error.to_string().contains("fixture-secret"));
    assert_eq!(
        db.get_security_events(&default_filter(), 1, 100)
            .unwrap()
            .total,
        7
    );
}

#[test]
fn sql_failures_propagate_and_split_storage_never_locks_public_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::init_at(&dir.path().join("cc-switch.db")).unwrap();
    dlp(&db, 1, 1, "audit", "HIGH");
    tool(&db, 1, 1, Some("risky"), None);
    let public = db.conn.lock().unwrap();
    assert!(!Database::table_exists(&public, "security_audit_logs").unwrap());
    assert_eq!(
        db.get_security_events(&default_filter(), 1, 10)
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        db.get_security_events_summary(&default_filter())
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        db.clear_security_events(&default_filter())
            .unwrap()
            .deleted_total,
        2
    );
    drop(public);
    db.extension_connection()
        .lock()
        .unwrap()
        .execute_batch("DROP TABLE security_audit_logs")
        .unwrap();
    assert_eq!(
        db.get_security_events(&default_filter(), 1, 10)
            .unwrap_err(),
        SecurityEventError::Database
    );
    assert_eq!(
        db.get_security_events_summary(&default_filter())
            .unwrap_err(),
        SecurityEventError::Database
    );
}

#[test]
fn clear_respects_every_filter_and_keeps_rules_lists_policy_and_settings() {
    let db = fixture();
    {
        let conn = db.extension_connection().lock().unwrap();
        conn.execute_batch("INSERT INTO tool_risk_settings VALUES ('enabled','false'),('policy_version','37');
            INSERT INTO security_settings VALUES ('enabled','false');
            INSERT INTO security_custom_rules VALUES ('keep','Kept','keyword','safe','HIGH','audit',0,NULL,1);
            INSERT INTO tool_risk_custom_rules VALUES ('keep',1,'Kept','{}',0,1);
            INSERT INTO tool_risk_builtin_overrides VALUES ('keep',0,1);").unwrap();
    }
    let tables = [
        "security_custom_rules",
        "security_settings",
        "tool_risk_settings",
        "tool_risk_custom_rules",
        "tool_risk_builtin_overrides",
        "tool_risk_command_lists",
        "tool_risk_policy_changes",
    ];
    let snapshot = || {
        let conn = db.extension_connection().lock().unwrap();
        tables
            .iter()
            .map(|table| {
                let mut statement = conn
                    .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                    .unwrap();
                let width = statement.column_count();
                statement
                    .query_map([], |row| {
                        (0..width)
                            .map(|i| row.get::<_, rusqlite::types::Value>(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let before = snapshot();
    let f = filter(
        json!({"view":"security_events","eventType":"dlp","startTimestamp":1000,"endTimestamp":1000,"appType":"claude","riskLevel":"HIGH","result":"warning","category":"dlp:pii","sessionId":"session%_","keyword":"%_\\"}),
    );
    assert_eq!(db.clear_security_events(&f).unwrap().deleted_total, 1);
    assert_eq!(
        db.get_security_events_summary(&default_filter())
            .unwrap()
            .total,
        6
    );
    assert_eq!(before, snapshot());
    assert_eq!(db.get_tool_risk_settings().unwrap().policy_version, 37);
    let f = filter(json!({"view":"security_events","startTimestamp":1001}));
    assert_eq!(db.clear_security_events(&f).unwrap().deleted_total, 0);
    assert_eq!(db.clear_security_events(&filter(json!({"view":"tool_calls","category":"tool_risk:pii","toolVerdict":"risky","riskLevel":"HIGH"}))).unwrap().deleted_tool_risk,1);
    assert_eq!(before, snapshot());
}

#[test]
fn missing_or_unrecognized_tool_metadata_is_never_inferred() {
    let db = fixture();
    db.extension_connection().lock().unwrap().execute_batch("UPDATE tool_risk_events SET client_category=NULL,risk_category=NULL,severity='none' WHERE id=7;
        UPDATE tool_risk_events SET severity='invalid' WHERE id=1").unwrap();
    let page = db.get_security_events(&default_filter(), 1, 100).unwrap();
    let row =
        serde_json::to_value(page.items.iter().find(|r| r.key == "tool_risk:7").unwrap()).unwrap();
    assert!(row["appType"].is_null());
    assert!(row["sessionId"].is_null());
    assert!(row["riskLevel"].is_null());
    assert_eq!(row["result"], "warning");
    assert_eq!(
        db.get_security_events(
            &filter(json!({"view":"security_events","eventType":"tool_risk","riskLevel":"LOW"})),
            1,
            100
        )
        .unwrap()
        .total,
        0
    );
    assert_eq!(
        db.get_security_events(
            &filter(json!({"view":"security_events","eventType":"tool_risk","riskLevel":"HIGH"})),
            1,
            100
        )
        .unwrap()
        .total,
        0
    );
    assert_eq!(
        db.get_security_events(
            &filter(json!({"view":"security_events","eventType":"tool_risk","appType":"claude"})),
            1,
            100
        )
        .unwrap()
        .total,
        1
    );
}

// Test-only instrumentation measures from successful mutex acquisition until
// just before release; excludes waiting and never runs in production builds.
thread_local! { static LAST_LOCK_DURATION: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) }; }
pub(super) struct LockTimer(std::time::Instant);
impl LockTimer {
    pub(super) fn start() -> Self {
        Self(std::time::Instant::now())
    }
}
impl Drop for LockTimer {
    fn drop(&mut self) {
        LAST_LOCK_DURATION.with(|cell| cell.set(self.0.elapsed()));
    }
}

#[test]
#[ignore = "100k rows/table; run release explicitly with --ignored --nocapture --test-threads=1"]
fn performance_100k_per_table() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let db = Database::init_at(&dir.path().join("cc-switch.db")).unwrap();
    // Deterministic 60-day fixture, 1% risky, 10 timestamps/second at the newest
    // boundary. Every row is synthetic; no production database/files are read.
    let today = 1_790_035_200_000_i64;
    let day = 86_400_000_i64;
    {
        let mut conn = db.extension_connection().lock().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute_batch(&format!("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100000)
            INSERT INTO security_audit_logs(timestamp,app_type,provider_id,rule_id,rule_name,rule_category,risk_level,action_taken,matched_snippet_masked,last_seen_at,session_id)
            SELECT {today}-(i%60)*{day}+(i%10)*100,'claude','synthetic','fixture',CASE WHEN i%100=0 THEN 'literal %_needle' ELSE 'fixture' END,'pii',CASE WHEN i%3=0 THEN 'CRITICAL' ELSE 'HIGH' END,CASE i%5 WHEN 0 THEN 'allow' WHEN 1 THEN 'audit' WHEN 2 THEN 'redact' WHEN 3 THEN 'block' ELSE 'unknown' END,'***',{today},'synthetic-session' FROM n;
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100000)
            INSERT INTO tool_risk_events(timestamp,source,stage,client_category,command_summary,detection_scope,integrity_status,verdict,risk_category,severity,policy_version,rule_ids)
            SELECT {today}-(i%60)*{day}+(i%10)*100,'claude','response',CASE WHEN i%2=0 THEN 'openai' ELSE 'claude' END,'git status','tool','complete',CASE WHEN i%100=0 THEN 'risky' WHEN i%4=0 THEN 'unchecked' WHEN i%4=1 THEN 'uncertain' WHEN i%4=2 THEN 'low_risk' END,'pii','high',1,'[]' FROM n;")).unwrap();
        tx.commit().unwrap();
    }
    let cases = [
        ("default", default_filter(), 1),
        (
            "today",
            filter(json!({"view":"security_events","startTimestamp":today})),
            1,
        ),
        (
            "30_days",
            filter(json!({"view":"security_events","startTimestamp":today-29*day})),
            1,
        ),
        ("all_history", default_filter(), 1),
        (
            "application",
            filter(json!({"view":"security_events","appType":"claude"})),
            1,
        ),
        (
            "high_level",
            filter(json!({"view":"security_events","riskLevel":"HIGH"})),
            1,
        ),
        (
            "same_timestamp",
            filter(json!({"view":"security_events","startTimestamp":today,"endTimestamp":today})),
            1,
        ),
        ("deep_page", default_filter(), 1800),
        (
            "literal_keyword",
            filter(json!({"view":"security_events","keyword":"%_needle"})),
            1,
        ),
    ];
    fn stats(samples: &mut [Duration]) -> (f64, f64, f64) {
        samples.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        (
            ms(samples[samples.len() / 2]),
            ms(samples[(samples.len() * 95).div_ceil(100) - 1]),
            ms(*samples.last().unwrap()),
        )
    }
    fn measure(mut query: impl FnMut(), capture_lock: bool) -> (f64, f64, f64, f64, f64, f64) {
        for _ in 0..5 {
            query();
        }
        let mut elapsed = Vec::new();
        let mut held = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            query();
            let time = start.elapsed();
            elapsed.push(time);
            held.push(if capture_lock {
                LAST_LOCK_DURATION.with(|cell| cell.get())
            } else {
                time
            });
        }
        let (median, p95, max) = stats(&mut elapsed);
        let (lm, lp, lx) = stats(&mut held);
        (median, p95, max, lm, lp, lx)
    }
    println!("PERF fixture=100000/table risky=1000 arch={} os={} debug_assertions={} temp_file_db=SQLite warmup=5 samples=30 serial=true lock=exact_for_new/elapsed_upper_bound_for_old",std::env::consts::ARCH,std::env::consts::OS,cfg!(debug_assertions));
    println!("BASELINE NOTE: old APIs are informational, not equivalent: tool API cannot apply common filters; old HIGH excludes CRITICAL; old keyword uses LIKE wildcards; old cards count DLP actions only. No relative speedup/non-regression claim is made.");
    let mut violations = Vec::new();
    for (name, f, page) in cases {
        let normalized = f.normalized().unwrap();
        let queries = Queries::new(&normalized);
        {
            let conn = db.extension_connection().lock().unwrap();
            for (kind, sql, mut values) in [
                ("count", queries.count_sql(), queries.values()),
                (
                    "page",
                    format!(
                        "{} ORDER BY timestamp DESC,event_type ASC,id DESC LIMIT ? OFFSET ?",
                        queries.index_sql()
                    ),
                    queries.values(),
                ),
                ("summary", queries.summary_sql(), queries.values()),
            ] {
                if kind == "page" {
                    values.extend([
                        rusqlite::types::Value::Integer(50),
                        rusqlite::types::Value::Integer((page - 1) * 50),
                    ]);
                }
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
                let plan = stmt
                    .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                println!("PLAN {name}/{kind}: {}", plan.join(" | "));
            }
        }
        let list = measure(
            || {
                std::hint::black_box(db.get_security_events(&f, page as u64, 50).unwrap());
            },
            true,
        );
        let summary = measure(
            || {
                std::hint::black_box(db.get_security_events_summary(&f).unwrap());
            },
            true,
        );
        let old_filter = crate::database::AuditLogFilter {
            app_type: f.app_type.clone(),
            start_timestamp: f.start_timestamp,
            end_timestamp: f.end_timestamp,
            risk_level: f.risk_level.map(|l| l.as_str().to_owned()),
            keyword: f.keyword.clone(),
            ..Default::default()
        };
        let old_list = measure(
            || {
                std::hint::black_box(
                    db.get_security_audit_logs(&old_filter, page as u32, 50)
                        .unwrap(),
                );
                std::hint::black_box(
                    db.get_tool_risk_events_filtered(
                        50,
                        ((page - 1) * 50) as u32,
                        Some(crate::services::tool_risk::engine::Verdict::Risky),
                    )
                    .unwrap(),
                );
            },
            false,
        );
        let old_summary = measure(
            || {
                for action in ["redact", "audit", "block"] {
                    let mut filter = old_filter.clone();
                    filter.action = Some(action.into());
                    std::hint::black_box(db.get_security_audit_logs(&filter, 1, 1).unwrap());
                }
            },
            false,
        );
        println!("PERF {name}: list median/p95/max={:.3}/{:.3}/{:.3}ms lock median/p95/max={:.3}/{:.3}/{:.3}ms; summary={:.3}/{:.3}/{:.3}ms lock={:.3}/{:.3}/{:.3}ms; old_dlp+tool_list={:.3}/{:.3}/{:.3}ms old_3_dlp_cards={:.3}/{:.3}/{:.3}ms",list.0,list.1,list.2,list.3,list.4,list.5,summary.0,summary.1,summary.2,summary.3,summary.4,summary.5,old_list.0,old_list.1,old_list.2,old_summary.0,old_summary.1,old_summary.2);
        let budget = if matches!(name, "default" | "today") {
            250.
        } else {
            500.
        };
        if list.1 > budget || summary.1 > budget || list.5 > 1000. || summary.5 > 1000. {
            violations.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "performance budget exceeded: {violations:?}"
    );
}

#[test]
fn redact_restore_is_request_redaction_without_losing_raw_action() {
    let db = Database::memory().unwrap();
    dlp(&db, 1, 1000, "redact_restore", "HIGH");
    let query = filter(json!({"view":"security_events","result":"redacted"}));
    let page = db.get_security_events(&query, 1, 100).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].result, SecurityEventResult::Redacted);
    let row = serde_json::to_value(&page.items[0]).unwrap();
    assert_eq!(row["record"]["actionTaken"], "redact_restore");
    let summary = db.get_security_events_summary(&default_filter()).unwrap();
    assert_eq!(summary.redacted, 1);
    assert_eq!(summary.other, 0);
}
