//! Worker-owned aggregation; candidate state becomes visible only after commit.
use super::identity::ContentFingerprint;
use crate::database::{AuditLogEvent, AuditRowTarget, AuditWrite, AuditWriteReceipt};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const IDLE_WINDOW: Duration = Duration::from_secs(60);
#[derive(Clone)]
pub struct PendingAuditEvent {
    pub log: AuditLogEvent,
    pub identity: Option<ContentFingerprint>,
    pub detected_at: Instant,
    pub risk_epoch: u64,
    pub config_revision: u64,
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct BaseKey {
    app: String,
    rule: String,
    content: ContentFingerprint,
    action: String,
}
impl PendingAuditEvent {
    fn key(&self) -> Option<BaseKey> {
        Some(BaseKey {
            app: self.log.app_type.clone(),
            rule: self.log.rule_id.clone(),
            content: self.identity?,
            action: self.log.action_taken.clone(),
        })
    }
}
#[derive(Clone)]
struct Segment {
    row: AuditRowTarget,
    last_hit: Instant,
    touched: u64,
    trigger_count: i64,
    last_seen_at: i64,
}
#[derive(Clone, Default)]
struct Versions {
    watermark: u64,
    segments: HashMap<(u64, String), Segment>,
}
pub struct AggregateState {
    entries: HashMap<BaseKey, Versions>,
    capacity: usize,
    clock: u64,
}
pub struct AggregatePlan {
    pub writes: Vec<AuditWrite>,
    drafts: HashMap<BaseKey, Versions>,
    clock: u64,
    last_hit: Option<Instant>,
}
impl AggregateState {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
            clock: 0,
        }
    }
    pub fn plan(&self, events: &[PendingAuditEvent]) -> AggregatePlan {
        let mut plan = AggregatePlan {
            writes: Vec::with_capacity(events.len()),
            drafts: HashMap::new(),
            clock: self.clock,
            last_hit: None,
        };
        for event in events {
            plan.clock = plan.clock.saturating_add(1);
            plan.last_hit = Some(
                plan.last_hit
                    .map_or(event.detected_at, |at| at.max(event.detected_at)),
            );
            let mut target = AuditRowTarget::New;
            if let Some(key) = event.key() {
                let versions = plan
                    .drafts
                    .entry(key.clone())
                    .or_insert_with(|| self.entries.get(&key).cloned().unwrap_or_default());
                // Keep older epochs separately: their arrival never changes a newer watermark.
                versions.watermark = versions.watermark.max(event.risk_epoch);
                let version = (event.risk_epoch, event.log.risk_level.clone());
                let previous = versions.segments.get(&version).filter(|segment| {
                    event
                        .detected_at
                        .checked_duration_since(segment.last_hit)
                        .is_none_or(|idle| idle < IDLE_WINDOW)
                });
                let last_hit =
                    previous.map_or(event.detected_at, |s| s.last_hit.max(event.detected_at));
                if let Some(previous) = previous {
                    target = previous.row;
                }
                versions.segments.insert(
                    version,
                    Segment {
                        row: AuditRowTarget::Earlier(plan.writes.len()),
                        last_hit,
                        touched: plan.clock,
                        trigger_count: 0,
                        last_seen_at: event.log.timestamp,
                    },
                );
            }
            plan.writes.push(AuditWrite {
                target,
                log: event.log.clone(),
            });
        }
        plan
    }
    pub fn commit(&mut self, mut plan: AggregatePlan, receipts: &[AuditWriteReceipt]) {
        for versions in plan.drafts.values_mut() {
            for segment in versions.segments.values_mut() {
                if let AuditRowTarget::Earlier(index) = segment.row {
                    if let Some(receipt) = receipts.get(index) {
                        segment.row = AuditRowTarget::Existing(receipt.id);
                        segment.trigger_count = receipt.trigger_count;
                        segment.last_seen_at = receipt.last_seen_at;
                    }
                }
            }
        }
        self.entries.extend(plan.drafts);
        self.clock = plan.clock;
        if let Some(at) = plan.last_hit {
            for versions in self.entries.values_mut() {
                versions.segments.retain(|(epoch, _), segment| {
                    *epoch == versions.watermark
                        || at
                            .checked_duration_since(segment.last_hit)
                            .is_none_or(|idle| idle < IDLE_WINDOW)
                });
            }
        }
        self.entries
            .retain(|_, versions| !versions.segments.is_empty());
        while self.segment_count() > self.capacity {
            let oldest = self
                .entries
                .iter()
                .flat_map(|(key, versions)| {
                    versions.segments.iter().map(move |(version, segment)| {
                        (key, version, segment.last_hit, segment.touched)
                    })
                })
                .min_by_key(|(_, _, at, touched)| (*at, *touched))
                .map(|(key, version, _, _)| (key.clone(), version.clone()));
            let Some((key, version)) = oldest else {
                break;
            };
            if let Some(versions) = self.entries.get_mut(&key) {
                versions.segments.remove(&version);
                if versions.segments.is_empty() {
                    self.entries.remove(&key);
                }
            }
        }
    }
    pub fn invalidate(&mut self, events: &[PendingAuditEvent]) {
        for event in events {
            if let Some(key) = event.key() {
                self.entries.remove(&key);
            }
        }
    }
    pub fn segment_count(&self) -> usize {
        self.entries.values().map(|v| v.segments.len()).sum()
    }
}
