# Routing strategy simplification

Status: proposal; no runtime behavior changed.
Date: 2026-11-12.
Reviewed base: `919a3b9d9`; routing files were also inspected in the original
working tree. This document was restored to the original checkout on
`codex/intent-recognition` after the separate proposal worktree was removed.
Its implementation findings describe the earlier review, not a fresh audit of
subsequent code changes.

## Current implementation

The interface combines three separate concepts:

1. A routing group contains exact provider/model members, a default member and
   fallback members. Discovery can group equivalent models across providers.
2. Nine scheduling algorithms order approved candidates using configuration and
   in-memory observations.
3. Optional deterministic conditions and model-based intent classification can
   prefer members ahead of the scheduler's remaining candidates. Three templates
   generate group members, rules and classifier settings.

These concepts are not interchangeable. In particular, the scheduling strategy
named `smart` does not assess task difficulty.

| Existing strategy | Implementation                                                                                                                                                                                |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `smart`           | Quota-aware ordering, with 90%/98% utilization tiers, reset-time comparisons and usable-credit handling. Unknown official-account allowance can receive learning priority.                    |
| `ordered`         | Preserve the configured default/fallback order.                                                                                                                                               |
| `round_robin`     | Rotate the first candidate on unbound selections.                                                                                                                                             |
| `least_used`      | Compare quota utilization, then recent routed tokens with a one-hour half-life; usable credits help distinguish spent accounts.                                                               |
| `pace`            | Prefer a higher remaining-allowance percentage per hour until reset; use day-or-longer windows when available and the tightest applicable budget. This is not simply earliest deadline first. |
| `free_first`      | Prefer explicitly free members. This does not rank paid models by price.                                                                                                                      |
| `fastest`         | Estimate first-content latency plus time to produce 200 tokens, retain configured order within 10% bands, and periodically explore an unmeasured available route.                             |
| `weighted`        | Smooth weighted rotation using member weights from 1 to 1000. Charge the destination that actually answers.                                                                                   |
| `manual`          | Use the selected member; pause intent/rule selection and cross-member fallback.                                                                                                               |

All scheduling remains subordinate to continuation ownership, supported request
capabilities, context capacity and availability handling. A rule's preferred
members can precede the scheduler's order. Optional rate-limit sinking and
account-pool handling also affect execution; the selected strategy is not the
only source of the final order.

Supported rule conditions include estimated tokens, images, requested reasoning
effort, agent, compaction, time and folders. Intent classification sends only
supported user text and rule descriptions through independent privacy inspection
to an explicitly configured eligible classifier. Classification is bounded and
falls back to the default plan when unavailable or invalid. Hard-bound
continuations do not become opportunities to switch accounts or reclassify.

The existing templates are:

- **Hard tasks to stronger models, easy tasks to cheaper models:** an ordered
  group with strong/cheap members, a classifier, hard/easy intent descriptions
  and special compaction/image rules. The strong member is the default.
- **Cheaper first:** cheap members precede a strong fallback; high requested
  reasoning effort and relevant image support can prefer the strong member.
- **Keep going:** select up to three providers' strongest candidates and order
  them as fallbacks; this is a template, not a tenth scheduler.

Template strength is heuristic: maker metadata, output-price thresholds,
small-model name/size markers and release dates. Missing output prices become
zero in template ranking. This is neither a validated ability score nor reliable
proof of low cost. Actual free-use scheduling separately preserves unknown cost.

The scheduling quota reader consumes official Codex allowance, supported headers
and rate-limit events. Its windows record utilization, duration and reset time,
not generic prepaid-credit expiry. Provider quota cards have separate allowance
readers; displaying a balance does not automatically make it scheduler telemetry.

## Product model: four choices

Expose four task-oriented policies instead of the combination of templates,
nine algorithms and rule configuration. Select models first, then choose one
policy. Show a plain-language preview of the resulting preference order before
saving. Prefer simple controls over a general rule editor in the default flow.

| Product policy                                                    | Reuse                                                                         | Required change                                                                                                                                                               |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Adaptive: hard tasks to strong models, easy tasks to cheap models | Existing hard/easy template, classifier privacy checks and fallback execution | Give the policy durable identity, expose strong/economical roles and generate its rules internally. Do not make users configure rule IDs or a separate classifier by default. |
| Capability first                                                  | Ordered execution and fallback machinery                                      | Add explicit, editable capability ordering. Existing template heuristics can suggest an order but must not present price or model names as measured ability.                  |
| Speed first                                                       | Existing `fastest` scheduler and exact-destination measurements               | Retain measured first-content/output speed and bounded exploration. Explain when there are insufficient observations; use a stable fallback order.                            |
| Expiring allowance first                                          | Existing quota observations and cooldown handling                             | Normalize usable expiring allowance and order by its actual deadline. Do not relabel `pace` as equivalent behavior.                                                           |

### Adaptive policy

The essential configuration is a strong role and an economical role, with
optional alternatives within each role. Suggest role assignments, but allow
correction. Use known provider pricing when available; unknown pricing must
remain unknown rather than being classified as free or cheaper.

Choose an eligible economical classifier automatically when one is available and
show its destination and possible extra charge in the policy preview. If no
classifier is usable, explain the requirement and permit an explicit choice or
another policy; do not silently claim that difficulty detection works.

Keep the existing bounded classification and privacy behavior. Failed, ambiguous
or unavailable classification uses the strong role. Prefer alternatives in the
same role during failover. A cross-role downgrade for a hard task must be a
visible policy decision, not a hidden consequence of a generic fallback list.
Image support and context capacity remain independent eligibility checks.

### Capability-first policy

Use an ordered model list that the user can correct. It is sufficient for the
first version; a new numeric scoring or benchmark service is unnecessary.
Ordering suggestions must identify themselves as suggestions. Cost, release
date, context length and reasoning-level names do not independently establish
general ability. Fail over to the next eligible member when safe.

### Speed-first policy

Reuse real inference observations rather than a network ping. Retain bounded
freshness, 10% bands and the existing exploration behavior. Exploration uses a
normal task request and may select an unmeasured route; explain this in policy
help. Do not add background billable inference probes. Show measurements only
for the exact account/model destination that produced them.

### Expiring-allowance-first policy

Working interpretation: use remaining allowance that will be lost soonest. A
periodic reset counts only when unused allowance is known not to carry over.
A credential's expiry is not allowance expiry, and an exhausted window is not
usable allowance. Subscription renewal alone is not evidence that wallet funds
expire. Supporting prepaid balances requires their actual expiry metadata.

Among otherwise eligible candidates, prefer a usable known deadline over an
unknown deadline, then the earliest deadline. Use configured capability order
to break ties. Unknown data remains eligible as a fallback and never receives
an invented expiry. Expired, depleted and temporarily unavailable allowance must
not win because its deadline is earlier. Observation failures must not interrupt
normal forwarding.

The existing `pace` algorithm optimizes consumption rate, not strict deadline
order. If rate-based consumption is desired later, it needs a separately stated
product rule; it must not silently override this policy's meaning.

## Compatibility and configuration

- Keep the legacy scheduler values readable for stored and imported groups.
  A legacy group must retain its current behavior until the user explicitly
  converts it. Hiding a scheduler does not authorize changing its meaning.
- Preserve legacy rules in compatibility editing. Do not let invisible old rules
  override a newly selected simple policy. Conversion must make the replacement
  of custom rules and manual/no-fallback semantics explicit and reviewable.
- Existing `smart` groups do not automatically become adaptive groups. Existing
  `pace` groups do not automatically become strict expiry-first groups.
- Preserve group IDs, public aliases, exact model/account identities, publication
  consent and ownership receipts. A strategy change cannot transfer opaque
  continuation state between destinations.
- Keep manual provider/model selection available through direct model entries;
  it need not be a fifth intelligent policy.
- Apply shared behavior on macOS and Windows and update all four runtime
  languages. New documentation and code comments remain in English.

## Implementation sequence and acceptance

1. Add a distinct product-policy configuration with legacy round-trip tests;
   verify old groups and imports keep their exact semantics.
2. Implement editable capability ordering and role assignment with synthetic
   tests; verify missing prices never prove cheapness or ability.
3. Implement deadline metadata and selection tests, including multiple windows,
   unknown/stale data, carryover, depleted allowance and cooldown. Keep fetches
   and persistence outside the forwarding hot path.
4. Replace the new-group flow with four choices, generated defaults and a
   decision preview. Test each policy and explicit legacy conversion.
5. Exercise classification fallback, protocol/context eligibility, retry safety
   and continuation ownership across every policy. Run the repository's required
   frontend/backend checks and applicable platform CI before release acceptance.

This change is a design-only checkpoint. No application was launched, no client
configuration was changed, and no runtime or cross-platform acceptance is
claimed.

## Implementation references

- [Strategy configuration](../../src-tauri/src/services/intent_routing/mod.rs)
- [Scheduling and telemetry](../../src-tauri/src/proxy/intent/scheduling.rs)
- [Runtime planning and forwarding](../../src-tauri/src/proxy/intent/mod.rs)
- [Intent classification](../../src-tauri/src/services/intent_routing/classifier.rs)
- [Deterministic rules](../../src-tauri/src/services/intent_routing/rules.rs)
- [Templates and import/export](../../src/lib/routingGroupTransfer.ts)
- [Group editor](../../src/components/routing/IntentRoutingPanel.tsx)
- [Provider quota observations](../../src-tauri/src/services/routing_provider_quota.rs)
- [Magpie source attribution](../../third_party/routing-reference/NOTICE.md)
