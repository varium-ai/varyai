//! Private accounting for temporary Chat history plans and complete candidates.
//! Budget handles authorize allocation only, never token restoration.
use super::{
    http_restoration::DlpForwardError,
    payload_budget::{PayloadBudget, Reservation},
    payload_json::{JsonProjection, PayloadJson, ProjectedItem},
    payload_metadata::PayloadVec,
};
use bytes::Bytes;
use serde_json::Value;

/// Immutable input provenance does not authorize restoration or execution.
#[derive(Clone, Copy)]
pub(crate) enum ChatHistoryProvenance {
    StrictResume,
    IndependentReadable,
}

pub(crate) struct ChatEnrichmentBudget(PayloadBudget);

impl ChatEnrichmentBudget {
    pub(super) fn new(budget: PayloadBudget) -> Self {
        Self(budget)
    }

    pub(crate) fn plan<T>(&self) -> Result<EnrichmentPlan<T>, DlpForwardError> {
        PayloadVec::new(self.0.clone())
            .map(EnrichmentPlan)
            .map_err(DlpForwardError::from_token_error)
    }

    pub(crate) fn build(
        &self,
        body: &Value,
        items: &[ProjectedItem<'_>],
    ) -> Result<(Value, ChatEnrichmentOwner), DlpForwardError> {
        let candidate = PayloadJson::try_from_projection(
            JsonProjection::Body { body, items },
            self.0.clone(),
            self.0.limit(),
        )
        .map_err(DlpForwardError::from_token_error)?;
        let (value, reservation) = candidate.into_parts();
        #[cfg(test)]
        let probe = test_support::OwnerProbe::new(reservation.bytes());
        Ok((
            value,
            ChatEnrichmentOwner {
                _reservation: reservation,
                budget: self.0.clone(),
                #[cfg(test)]
                _probe: probe,
            },
        ))
    }
}

pub(crate) struct EnrichmentPlan<T>(PayloadVec<T>);
impl<T> EnrichmentPlan<T> {
    pub(crate) fn push(&mut self, value: T) -> Result<(), DlpForwardError> {
        self.0
            .push(value)
            .map_err(DlpForwardError::from_token_error)
    }
    pub(crate) fn as_slice(&self) -> &[T] {
        self.0.as_slice()
    }
}

// Not Clone: one owner retains the charge for one complete independent tree.
// Declare before that tree so it is dropped after all fields moved from the tree.
pub(crate) struct ChatEnrichmentOwner {
    _reservation: Reservation,
    budget: PayloadBudget,
    #[cfg(test)]
    _probe: Option<test_support::OwnerProbe>,
}
impl ChatEnrichmentOwner {
    #[cfg(test)]
    pub(crate) fn charged_for_test(&self) -> usize {
        self._reservation.bytes()
    }

    pub(crate) fn encode_outbound_body(&self, value: &Value) -> Result<Bytes, DlpForwardError> {
        super::http_restoration::encode_outbound_with_budget(value, self.budget.clone())
    }
}

pub(crate) use super::payload_json::ProjectedItem as EnrichmentItem;

pub(crate) struct EnrichmentIds<'a>(PayloadVec<&'a str>);
impl<'a> EnrichmentIds<'a> {
    pub(crate) fn new(budget: &ChatEnrichmentBudget) -> Result<Self, DlpForwardError> {
        PayloadVec::new(budget.0.clone())
            .map(Self)
            .map_err(DlpForwardError::from_token_error)
    }
    pub(crate) fn insert(&mut self, id: &'a str) -> Result<(), DlpForwardError> {
        if !self.contains(id) {
            self.0.push(id).map_err(DlpForwardError::from_token_error)?;
            self.0.sort_unstable_by(|left, right| left.cmp(right));
        }
        Ok(())
    }
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.0.as_slice().binary_search(&id).is_ok()
    }
}

#[cfg(test)]
pub(in super::super) mod test_support {
    use std::{
        cell::RefCell,
        sync::{Arc, Mutex},
    };
    #[derive(Clone, Copy, Default, Debug)]
    pub(in super::super) struct Snapshot {
        pub created: usize,
        pub live_bytes: usize,
        pub released: usize,
    }
    thread_local! {
        static OBSERVER: RefCell<Option<Arc<Mutex<Snapshot>>>> = const { RefCell::new(None) };
    }
    pub(in super::super) struct Guard(Option<Arc<Mutex<Snapshot>>>);
    pub(in super::super) fn install() -> (Guard, Arc<Mutex<Snapshot>>) {
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let previous = OBSERVER.with(|slot| slot.replace(Some(state.clone())));
        (Guard(previous), state)
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            OBSERVER.with(|slot| slot.replace(self.0.take()));
        }
    }
    pub(in super::super) struct OwnerProbe {
        state: Arc<Mutex<Snapshot>>,
        bytes: usize,
    }
    impl OwnerProbe {
        pub(in super::super) fn new(bytes: usize) -> Option<Self> {
            let state = OBSERVER.with(|slot| slot.borrow().clone())?;
            if let Ok(mut snapshot) = state.lock() {
                snapshot.created += 1;
                snapshot.live_bytes += bytes;
            }
            Some(Self { state, bytes })
        }
    }
    impl Drop for OwnerProbe {
        fn drop(&mut self) {
            if let Ok(mut snapshot) = self.state.lock() {
                snapshot.live_bytes -= self.bytes;
                snapshot.released += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{payload_json, payload_metadata};
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn chat_enrichment_plan_growth_precharges_old_and_new_backing() -> Result<(), DlpForwardError> {
        let raw_budget = PayloadBudget::with_limit(4096);
        let budget = ChatEnrichmentBudget::new(raw_budget.clone());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let observed = seen.clone();
        let _hook = payload_metadata::test_support::install(move |event| {
            if event.phase == payload_metadata::test_support::Phase::BeforeAllocation {
                observed.borrow_mut().push((event.bytes, event.used));
            }
            payload_metadata::test_support::Action::Continue
        });
        let mut plan = budget.plan::<u64>()?;
        for value in 0..5 {
            plan.push(value)?;
        }
        assert_eq!(seen.borrow().as_slice(), &[(32, 32), (64, 96)]);
        assert_eq!(raw_budget.used(), 64);
        drop(plan);
        assert_eq!(raw_budget.used(), 0);
        Ok(())
    }

    #[test]
    fn chat_enrichment_plan_failed_growth_keeps_old_values_and_charge(
    ) -> Result<(), DlpForwardError> {
        let raw_budget = PayloadBudget::with_limit(95);
        let budget = ChatEnrichmentBudget::new(raw_budget.clone());
        let mut plan = budget.plan::<u64>()?;
        for value in 0..4 {
            plan.push(value)?;
        }
        assert!(matches!(
            plan.push(4),
            Err(DlpForwardError::CapacityExceeded)
        ));
        assert_eq!(plan.as_slice(), &[0, 1, 2, 3]);
        assert_eq!(raw_budget.used(), 32);
        drop(plan);
        assert_eq!(raw_budget.used(), 0);
        Ok(())
    }

    #[test]
    fn chat_enrichment_candidate_each_build_is_precharged_and_releases_on_failure() {
        let body = json!({"input": [], "model": "synthetic-model"});
        let item = json!({"type":"function_call", "call_id":"synthetic-call", "arguments":"{}"});
        let projected = [EnrichmentItem {
            value: &item,
            cached: None,
            missing: 0,
        }];
        let raw_budget = PayloadBudget::with_limit(64 * 1024);
        let budget = ChatEnrichmentBudget::new(raw_budget.clone());
        let baseline = raw_budget.reserve(31).expect("synthetic baseline");
        let observations = Rc::new(RefCell::new(Vec::new()));
        let seen = observations.clone();
        let _hook = payload_json::test_support::install(move |event| {
            seen.borrow_mut().push(event);
            if event.phase == payload_json::test_support::Phase::BeforeAllocation {
                assert!(event.charged > 0);
                assert_eq!(event.used, 31 + event.charged);
                payload_json::test_support::Action::Fail
            } else {
                payload_json::test_support::Action::Continue
            }
        });
        assert!(matches!(
            budget.build(&body, &projected),
            Err(DlpForwardError::Unavailable)
        ));
        assert!(observations
            .borrow()
            .iter()
            .any(|event| event.phase == payload_json::test_support::Phase::BeforeBuild));
        assert!(observations
            .borrow()
            .iter()
            .any(|event| event.phase == payload_json::test_support::Phase::BeforeAllocation));
        assert_eq!(raw_budget.used(), 31);
        drop(baseline);
        assert_eq!(raw_budget.used(), 0);
    }

    #[test]
    fn chat_enrichment_candidate_projection_preserves_keys_order_and_value_types() {
        let body = json!({"model":"synthetic", "input": [], "temperature":0.25});
        let item = json!({"type":"function_call", "call_id":"synthetic", "name":"kept", "arguments":"", "status":null});
        let cached = json!({"type":"function_call", "call_id":"synthetic", "name":"ignored", "arguments":"{}", "reasoning":{"n":1}, "status":"completed"});
        let projected = [EnrichmentItem {
            value: &item,
            cached: Some(&cached),
            missing: (1 << 2) | (1 << 4) | (1 << 7),
        }];
        let raw = PayloadBudget::with_limit(64 * 1024);
        let budget = ChatEnrichmentBudget::new(raw.clone());
        let (candidate, owner) = budget
            .build(&body, &projected)
            .unwrap_or_else(|_| panic!("synthetic candidate"));
        assert_eq!(
            candidate["input"][0],
            json!({"type":"function_call", "call_id":"synthetic", "name":"kept", "arguments":"{}", "status":"completed", "reasoning":{"n":1}})
        );
        assert_eq!(candidate["temperature"], body["temperature"]);
        assert_eq!(
            candidate
                .as_object()
                .expect("synthetic body")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["model", "input", "temperature"]
        );
        assert_eq!(
            candidate["input"][0]
                .as_object()
                .expect("synthetic item")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "type",
                "call_id",
                "name",
                "arguments",
                "status",
                "reasoning"
            ]
        );
        assert_eq!(
            serde_json::to_string(&candidate["input"][0]).expect("synthetic serialization"),
            r#"{"type":"function_call","call_id":"synthetic","name":"kept","arguments":"{}","status":"completed","reasoning":{"n":1}}"#
        );
        let expected = PayloadJson::try_from_value(&candidate, raw.clone(), raw.limit())
            .unwrap_or_else(|_| panic!("synthetic copy"));
        let (copy, copy_charge) = expected.into_parts();
        assert_eq!(owner.charged_for_test(), copy_charge.bytes());
        drop(copy);
        drop(copy_charge);
        drop(candidate);
        drop(owner);
        assert_eq!(raw.used(), 0);
    }
}
