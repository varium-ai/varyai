# Magpie comparison review — 2026-11-05

This report supersedes the scope implied by the initial chat findings. The first
pass was a partial review, not an exhaustive comparison. The follow-up found
additional defects in model identity, imported constraints, account capabilities,
concurrent scheduling, and Desktop configuration. It also distinguishes missing
compatibility features from defects and from deliberate Vary safety boundaries.

## Implementation follow-up

The working tree was fast-forwarded to `a30e86d1` from
`origin/codex/intent-recognition`, preserving local work. Four locale conflicts
were resolved by keeping both upstream rewards and local onboarding sections.
The autostash remains available as a recovery snapshot.

Production changes now address A01–A08, P01–P04, and R01–R07. Local verification
passed as recorded below. This is not a claim of complete Magpie parity or live
native-client acceptance.

| Area               | Implemented behavior                                                                                                                                                                                                                                                            |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code        | Shadow cloud switches with receipt restoration, reject conflicting managed cloud policy, publish the shared picker, and adopt top-level model selection.                                                                                                                        |
| Native catalogs    | Preserve image modalities; use the same JSONC-first OpenCode document with comment-preserving writes; accept both OpenClaw model forms and extend its current allow policy.                                                                                                     |
| Request identities | Preserve allocated readable IDs across catalog refresh and suspend/resume. Removed IDs remain reserved and cannot be assigned to another target. This also covers Pi, Hermes and Grok Build added by the remote update.                                                         |
| Desktop paths      | Discover both Windows package families and classic directories, distinguish roaming data from local mode configuration, and project both package and classic sets. Filesystem fixtures run on macOS; installed Windows and DPAPI acceptance remain outstanding.                 |
| Provider snapshots | Deduplicate identical key transports, use enabled classifier transports and custom headers/full URLs, and retain each frozen account's own capabilities.                                                                                                                        |
| Import constraints | Reject unsupported spending, concurrency, account caps/weights, key weights and queue/RPM controls instead of silently dropping them. This does not implement those runtime features.                                                                                           |
| Scheduling         | Preserve strategy order during exhaustion partitioning, distinguish known allowance from unknown paid usage, distinguish weekly low budget from short rate caps, and reconcile Weighted charges per request.                                                                    |
| Fallback           | Advance unbound requests after unavailable credentials and explicit 401/402/403/429/500/502/503/504 rejection. Preserve manual/pinned and ambiguous-network replay boundaries, and preserve 401 when all pre-dispatch credentials fail.                                         |
| Stream commitment  | With another approved candidate, buffer before visible content and allow explicit semantic failure to advance. Partial content and transport ambiguity are never replayed; a single candidate retains immediate streaming headers. Health success is recorded after completion. |

G01–G04 have implementation work: missing Desktop MCP names and locale are
carried without replacing existing values, native edits/removals are preserved,
tool search is seeded once, client tool references survive protocol conversion,
and known output ceilings and complete cost/cache tables reach native catalogs.
Hosted tools still require a representable target protocol.

G05–G07 remain open feature gaps: imported Desktop profile adoption, the broader
provider/group scheduling and runtime limit feature set, and the additional
native profile/WSL/persistent Claude CLI lifecycles have not been implemented by
these bug fixes. They must not be described as completed or silently accepted by
an import. The reference comparison is pinned to the source revision below.

Tests use synthetic fixtures, temporary homes, in-memory databases and dynamic
loopback ports; no application was installed or launched for acceptance.

### Final local verification

- Checked base: `a30e86d1`, including the preserved working changes. No commit or
  push was made. The source/configuration inventory contains 1,347 files and was
  unchanged during the final full Rust run.
- Source/configuration SHA-256: `f6bef45a6e5332f182e6b5b1cb4c36934e77cc88ce899865be80c11d637ee939`.
- Host: macOS Darwin 24.3.0, arm64; Rust/Cargo 1.98.1; Node 24.11.0; pnpm 10.12.3.
- The pinned pnpm was obtained through npm's standard integrity-checked package
  install into a temporary cache. Project dependencies and the global pnpm were
  not changed.

| Check                                                                                          | Result                                                                                                                                  |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                               | Passed.                                                                                                                                 |
| `pnpm build:renderer`                                                                          | Passed.                                                                                                                                 |
| `pnpm test:unit`                                                                               | Passed: 278 files, 3,434 tests.                                                                                                         |
| `pnpm format:check`                                                                            | Passed.                                                                                                                                 |
| `pnpm audit --prod`                                                                            | Passed: 164 production dependencies, zero advisories.                                                                                   |
| CI Node helpers: dev, TRAE bridge, WorkBuddy auth, Doubao check-in, Agent rewards, Qoder codec | Passed: 94 tests; one existing skip. Executed together with `node --test`.                                                              |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                             | Passed.                                                                                                                                 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                             | Passed.                                                                                                                                 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                       | Passed.                                                                                                                                 |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                              | Passed: 5,981 library tests and 163 integration tests; 18 existing ignored tests. This full run includes the 19 new parity regressions. |
| `git diff --check` and unresolved merge entries                                                | Passed; no unresolved entries.                                                                                                          |

Rust commands used `--offline`, an isolated target directory, disabled incremental
compilation and a package-only debug-info override to fit available disk space:
`--target-dir /private/tmp/vary-magpie-parity-target --config build.incremental=false --config profile.dev.package.varyai.debug=0`.
The Mac linker emitted its compact-unwind size warning; tests completed.

Logs and the source inventory are local temporary evidence in
`/private/tmp/vary-magpie-verification/`. The final Rust log is
`rust-final-tree.log`; `tree-current.json` records the checked source files.
Earlier failed runs are retained separately and do not establish final acceptance.
Linux/Windows/WSL2 CI, DPAPI, native client versions, live vendor sign-in,
installation, signing and release acceptance were not run. No remote CI result
is claimed.

## Sources and evidence

- Reference: [yetone/magpie at 465928bccea4fec3537edd6280f7cabb9a0c5bad](https://github.com/yetone/magpie/tree/465928bccea4fec3537edd6280f7cabb9a0c5bad).
- Vary base commit: `25d6b901658d4fd96a7d783542670ab7c53667d7`, with the
  existing staged/unstaged/untracked working files included in the review.
- Host: macOS, Darwin 24.3.0, arm64. Rust: `rustc 1.98.1 (48a229cea 2026-09-01)`.
- No production client configuration, account, credential, running application,
  or vendor inference endpoint was used for the initial audit reproduction.
  Implementation changes are described above; no commit, push, application
  restart, or installation was performed.
- `M` below means a minimal source-extraction reproduction. Actual implementation
  functions or blocks were compiled with synthetic boundary types and temporary
  files. This proves the named local behavior; it does not establish a full
  application or native-client integration failure.
- `S` marks the original static call-chain finding with a concrete trigger.
  The implementation follow-up adds integrated regressions for these paths.
- `G` means a reference compatibility gap. A gap is not automatically a defect
  in Vary's stated support contract.

Temporary reproduction sources, outputs, and a 67-file SHA-256 inventory are in
`/private/tmp/vary-magpie-audit-20261105/repro/`. The machine-readable result is
`results.json`. These are temporary local evidence, not committed test fixtures.

Five inventory files changed during the follow-up: `proxy/intent/native.rs`,
and `services/intent_routing/client/{health,mod,mode_tests,threads}.rs`.
At that stage, the evolving Codex client implementation had no fresh acceptance
result. The final shared working tree is covered by the local checks above; live
native-client acceptance remains separate. None of the extracted reproductions
below depends on those five files. The historical findings and line locations
below describe the audited pre-fix behavior, not the current implementation.

## Agent findings

### A01 — P1 — Claude Code cloud switches survive gateway projection (M) — fixed locally

Location: `src-tauri/src/services/agent_connections.rs:854`.

The projection writes the Anthropic gateway fields without taking an enabled
`CLAUDE_CODE_USE_BEDROCK`, Vertex, Foundry, or another cloud-provider switch out
of the same settings environment. The client can continue selecting its cloud
transport rather than Vary, or reject Vary's model alias there. Receipt integrity
does not inspect these competing switches.

Reference: `internal/agent/claude.go` and `claude_cloud_test.go`. Magpie temporarily
removes owned conflicting switches, restores them on disconnect, and reports
switches in settings it cannot safely change. Administrator policy must not be
silently overridden when addressing this finding.

### A02 — P2 — Claude Code's model picker and persisted model choice diverge (M) — fixed locally

Location: `src-tauri/src/services/agent_connections.rs:855` and
`services/agent_connections/selection.rs:7`.

The projection publishes no `modelPicker`, sets `ANTHROPIC_MODEL`, and adopts
changes to that environment field rather than Claude Code's top-level `model`.
Shared models/groups are not populated into the native picker. A native choice
saved in top-level `model` can be outranked by the environment on a later session.
Two minimal assertions cover the missing picker and retained environment override.

Reference: `internal/agent/claude.go`, `claude_pick_test.go`,
`claude_picker_group_test.go`. Native picker support remains client-version dependent.

### A03 — P2 — OpenCode loses image capability (M) — fixed locally

Location: `src-tauri/src/services/agent_connections.rs:145`.

`opencode_model()` ignores `Model.images`. A known image-capable model is
published without `modalities.input`, so native capability checks cannot see
image support. Magpie emits input/output modalities and tests them in
`internal/agent/image_input_test.go`.

### A04 — P2 — OpenCode discovery and projection disagree on JSONC (S) — fixed locally

Locations: `services/agent_connections/native.rs:63`,
`services/agent_connections.rs:915`, and `opencode_config.rs:58`.

Native selection can read `opencode.jsonc` and parses OpenCode using JSON5.
Connection projection instead targets `opencode.json` using strict JSON.
Comments in an existing JSON-named file reject projection; a JSONC-only
installation gets a second file instead of an edit to the observed file.
The final native precedence should be tested against supported OpenCode versions.

Reference: the JSONC-first file selection in `internal/agent/agents.go`.

### A05 — P2 — OpenClaw's string model form rejects connection (M) — fixed locally

Location: `services/agent_connections/projections.rs:161`.

For valid native `agents.defaults.model: "native/model"`, writing its `primary`
child fails because `put()` requires an object at every intermediate node.
Magpie's `openClawPrimary` and `wire` handle both a string and an object without
discarding object-form fallback settings.

### A06 — P2 — OpenClaw's current allow policy is not extended (S) — fixed locally

Location: `services/agent_connections/projections.rs:166`.

The projection extends the older `agents.defaults.models` map, but does not
handle a restrictive `agents.defaults.modelPolicy.allow`. On client versions
using that policy, the new provider/model remains outside the allowed set.
Magpie checks and extends the current policy in `internal/agent/openclaw.go`.

### A07 — P1 — Display labels become mutable request identities (M, new) — fixed locally

Locations: `services/agent_connections/projections.rs:25` and the WorkBuddy
projection at `services/agent_connections.rs:879`.

Kimi, Qwen and ZCode replace stable catalog IDs with display labels, and replace
the runtime name-to-alias map on refresh. WorkBuddy has a similar label map.
Renaming the provider/model changes a client's request ID even though the real
destination did not change. More seriously, with two identically labelled models,
the first gets `Label` and the second `Label (2)`. Removing the first assigns
`Label` to the second: an old request ID now resolves to a different destination.

The two minimal assertions reproduce both rename instability and label reuse.
They do not claim that opaque continuation validation is bypassed; that validation
can still reject an old tool/response continuation. A plain old-model request can
resolve to the replacement alias before those checks.

Reference: `internal/provider/modelprefs.go` keeps names separate from IDs;
`internal/agent/qwen.go`, `kimi.go`, and `zcode.go` project stable model IDs.
Use stable wire IDs and retain any necessary old aliases when migrating.

### A08 — P1 — Windows packaged Desktop configuration is not discovered (M, new) — fixed locally

Location: `claude_desktop_config.rs:1364` and `:1373`.

The resolver scans immediate `%LOCALAPPDATA%` children. It never discovers
`Packages/Claude_<publisher>/LocalCache/...` or the Store package family.
A valid package-only fixture returns no path. Windows builds also differ in
whether their 3p data remains packaged or moves to the real local directory.
Writing only the classic paths can leave the application reading another copy.

Reference: `internal/desktopdir/desktopdir.go` finds both package families,
distinguishes roaming data from local mode files, and exposes both directory sets.
The reproduction exercises filesystem selection on macOS; Windows acceptance
for actual installed builds is still required.

## Provider findings

### P01 — P1 — Imported duplicate keys become duplicate destinations (M) — fixed locally

Locations: `services/routing_upstreams.rs:492` and `proxy/intent/scopes.rs:139`.

The key set in `variants()` only influences labels. It does not deduplicate
execution variants. With the primary key repeated as an identical backup, two
identical targets are created. When continuation persistence is available,
`acquire_durable_with_history()` rejects duplicate destination identities before
dispatch, including for a new request. Magpie's `KeysOn()` explicitly deduplicates
this source-file case in `internal/provider/keys.go`.

### P02 — P2 — Classifier transport bypasses enabled-key and header selection (S) — fixed locally

Locations: `services/intent_routing/menu.rs:311` and `classifier.rs:274`.

The global menu clones the unexpanded classifier provider. The classifier then
reads its primary credential directly and appends `/chat/completions`, rather
than using the enabled immutable transport variant. Turning the primary key off
while leaving a backup on does not stop this path from using the primary key.
Custom headers and full-URL transport semantics are also omitted from its sender.
Classification errors degrade to the default route and can conceal the problem.

The separate preview preparation already calls `first_enabled()` in
`execution.rs`; its success does not validate the global menu's classifier path.

### P03 — P1 — Import accepts providers while dropping spending constraints (M, new) — fixed locally

Location: `services/provider_import/readers.rs:679`.

The unsupported-setting predicate rejects several transport/routing features,
but not `keySpendCaps`, `maxConcurrency`, `accountConcurrency`, `maxRPM`, or queue
limits. These fields are not preserved in the imported executable record.
A capped source provider can therefore appear importable while its Vary copy
has none of those protections.

The minimal test executes the actual unsupported-setting predicate with synthetic
non-default constraints and demonstrates that it does not reject them. This is
not a live spending test. An unsupported control should be surfaced in preview
or rejected; silently dropping it is distinct from merely lacking the feature.

Reference: `internal/provider/provider.go`, `keys.go`, and `key_spend.go`.

### P04 — P1 — Account-specific model capabilities are replaced by union facts (S, new) — fixed locally

Locations: `services/intent_routing/catalog.rs:390` and `menu.rs:236`.

`prepare_editor_pool()` retains per-account catalogs, but its display union takes
the first row for each model ID. Menu construction later clones each account's
target and overwrites its capability with `declared_capability(provider, model)`
from that union. The per-account identity is retained, but its image support,
context limit and reasoning facts can now be those of another account.

Trigger: two accounts advertise the same ID with different capabilities. A target
can advertise or be selected for images/context it cannot serve, or unnecessarily
lose capabilities. `serves_model()` checks model membership, not capability
equivalence. An integrated two-account fixture is required before fixing this.

## Routing findings

### R01 — P1 — Codex account post-sorting undoes the strategy (M) — fixed locally

Location: `proxy/intent/mod.rs:1727`.

After arranging candidates, the account-pool pass sorts them by exhaustion and
original index. Equal-health accounts lose the order selected by rotation,
weight, usage, pace, speed, or cooldown ordering. The minimal two-account
rotation returns `[0, 0, 0, 0]` instead of `[0, 1, 0, 1]`. An exhaustion partition
should not reintroduce the original-index tie breaker over strategy output.

### R02 — P1 — Explicitly rejected or unavailable credentials terminate fallback (S) — fixed locally

Locations: `proxy/intent/mod.rs:1997`, `:2049`, and `:2346`.

For an unbound request, a subscription credential-resolution failure returns
immediately. Codex refresh failure continues only when a different official
account has the same provider/model/fixed effort; a configured relay fallback
does not satisfy that condition. The HTTP retry branch only accepts
`429/502/503/504`, so a primary's explicit `401`, credit rejection `402`, or `500`
can end a group before its healthy backup is tried.

This does not justify replaying an ambiguous network failure or moving a pinned
opaque continuation. Tests must separate those safety cases from a new request
with a clear pre-execution rejection.

### R03 — P2 — LeastUsed treats an unknown share as zero (M) — fixed locally

Location: `proxy/intent/scheduling.rs:583`.

An unobserved paid key has `used = 0`. It outranks a subscription whose known
allowance is only 20% used, potentially spending paid balance first. Magpie's
current strategy distinguishes known allowance from shareless/unknown seats.

### R04 — P2 — Smart/Pace omit the weekly-only low-budget distinction (M) — fixed locally

Locations: `proxy/intent/scheduling.rs:482`, `:522`, and `:594`.

The largest usage percentage across all windows determines the low tier. A week
95% used and renewing in an hour is held behind a week 20% used and renewing
tomorrow, even with a healthy short-term rate window. This differs from Magpie's
`WeekLow` behavior, which spends remaining expiring budget while distinguishing
it from a nearly exhausted short-term cap. The Smart difference is reproduced;
this is a reference strategy mismatch, not evidence of unsafe forwarding.

### R05 — P2 — Concurrent Weighted completions charge another request's pick (M, new) — fixed locally

Locations: `proxy/intent/scheduling.rs:422` and `proxy/intent/mod.rs:2554`.

`Pool.charged` is one slot shared by all in-flight requests. Request B overwrites
it after request A chooses a seat. When A finishes, `reassign(A)` consumes B's
charge and moves it to A, even if neither request used affinity or failover.
B's completion then has no charge to reconcile. Two equal-weight requests
served once by each seat leave the next request biased to the wrong seat.

The minimal regression interleaves two selections and their normal completions.
The reference `MoveRound(pool, picked, kept)` checks the selected pick identity;
Vary's completion API does not carry that request's original pick at all.

### R06 — P1 — Disabling one provider withdraws an entire saved group (S, new) — fixed locally

Locations: `services/intent_routing/menu.rs:306` and
`database/dao/intent_routing.rs:204`.

For a saved non-manual group with two approved providers, disabling one removes
its provider from execution preparation. The group's `ready` predicate requires
every enabled member to have a target, so it omits the whole group instead of
using the remaining healthy member. The catalog automatically derives disabled
members into `off` for newly discovered groups, but does not do that for an
already saved group skipped by discovery. Those two cases must not be conflated.

Magpie excludes disabled resources from a group's executable members. Vary's
guide also describes disabled providers as retained in discovery but skipped in
execution. Test both saved groups and discovered groups, plus pinned requests
whose disabled destination must still fail closed.

### R07 — P2 — HTTP 200 stream failures do not enter pre-content fallback (S, new) — fixed locally

Locations: `proxy/intent/mod.rs:2531` and `:2551`.

The first-content buffer can detect `probe.failed`, but then breaks out, calls
`scheduling.succeeded()`, and exposes the failed stream. It does not try the next
approved candidate even when only a terminal `response.failed` was buffered and
no content was sent to the client. With first-content buffering disabled, status
success is likewise treated as transport success before semantic events arrive.
The final observer records failure, but the cooldown was already cleared.

Magpie distinguishes failed/empty semantic completions from HTTP success before
committing a reply. A regression must cover an explicit overloaded failure before
content, a normal empty completion, and a failure after content. Only the first
safe cases should be considered for fallback; partial replies must not be replayed.

## Compatibility gaps requiring separate scope decisions

| ID  | Gap and consequence                                                                                                                                                                                                             | Evidence / scope                                                                                                                                                                                                                                                                                      |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G01 | Desktop's local MCP servers are not carried from normal mode into Claude-3p. Entering gateway mode can leave the connector list missing the user's locally configured servers.                                                  | M: the actual shared profile builder only changes `deploymentMode` in those two configuration files. Magpie's `desktopCarryMCP` merges missing names, preserves 3p conflicts, and removes only unchanged copied entries on disconnect. Account-hosted claude.ai connectors are a different feature.   |
| G02 | Desktop's `toolSearchEnabled` policy is not seeded. Supported Desktop/embedded-Claude versions can send every MCP tool schema instead of using deferred tool discovery.                                                         | M: the generated profile has no policy. Reference: `desktopSeedToolSearch` and `claudedesktop_test.go`. Enabling the field alone is insufficient; deferred-tool conversion must also be accepted by the intended upstream path.                                                                       |
| G03 | Desktop's normal-mode locale is not carried into its separate 3p userData. First entry can fall back to another language.                                                                                                       | M: the builder emits no locale edit. Reference: `desktopCarryLocale`. Use a supported locale fixture; Magpie itself documents that Desktop builds lacking a Chinese catalog can reset Chinese independently of the gateway.                                                                           |
| G04 | Known output-token ceilings and price/cache metadata are not carried through the shared `Model` type into several native projections. Native defaults can cap output below a known upstream limit and omit cost/cache behavior. | S: compare `agent_connections::Model`, `pi_model`, `opencode_model`, and WorkBuddy projection with Magpie's `piModelJSON`/`buddyModel` output fields. This is independent of the image-capability bug.                                                                                                |
| G05 | A Desktop-imported gateway profile with a new UUID is not recognized as the active compatible gateway profile.                                                                                                                  | S: Vary requires fixed `PROFILE_ID`; Magpie's `desktopAppliedGateway` and `desktopKeyApplied` can retain the user's applied profile and repair a readable rejected key in place. Do not overwrite arbitrary user profiles to close this gap.                                                          |
| G06 | Provider-level routing/fallback, nested groups, Drain, FreeExpiring, Cheapest, wallets, price-aware selection, per-account/key weights and runtime concurrency/RPM queues are not at reference parity.                          | These are feature gaps. P03 separately identifies the defect of accepting an import while silently dropping non-default source limits.                                                                                                                                                                |
| G07 | Broader native lifecycle support is incomplete: Hermes active/multiple bot profiles, legacy Kimi paths, both ZCode generations and moved data roots, WSL variants, and persistent Claude subscription CLI replay.               | Reference adapters explicitly handle these branches. Vary's current adapters have narrower paths/ownership. The Claude subscription's textual replay is not equivalent to a persistent structured tool conversation; a fake-CLI multi-turn test and a client-version support matrix remain necessary. |

## Reviewed behavior that must not be reported as missing

1. Desktop gateway address and a local connection token are already projected.
   Upstream provider credentials remain behind the local ingress.
2. Desktop sealed-key compatibility exists. Non-Windows builds preserve an
   unknown seal instead of comparing ciphertext to a plaintext key; Windows
   has DPAPI handling. The minimal seal control passes. This establishes
   configuration compatibility, not successful authentication of the secret
   inside an unreadable seal.
3. The exact `"."`, `max_tokens: 1` Desktop probe is recognized and patched to
   16, with unrelated raw fields retained. That control passes. Routing also
   excludes that purpose from paid intent classification.
4. Rejecting unknown tool ownership, ambiguous network replay, or an identity
   change is an intentional safety boundary. It is not automatically a routing
   parity bug.
5. A conflict after a user changes an owned readable key is not, by itself, a
   defect: Vary deliberately preserves external edits. The previous chat's
   suggestion that ordinary refresh must overwrite such a key was too broad.
   G05 is the narrower compatible-profile scenario to evaluate.
6. Native provider schemas can differ across versions. In particular, Hermes'
   older `custom_providers` compatibility is not proved broken merely because
   Magpie writes the newer `providers` dictionary.

## Coverage ledger

This is an inventory of compared surfaces, not a claim that every line and
interleaving of both projects has been verified.

| Surface                | Compared implementation and result                                                                                                                                                | Remaining acceptance                                                                                                                            |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Shared Agent ownership | Journal-before-write, field diffs, checked replacement, refresh, suspend, restore, explicit reapply, native selection adoption                                                    | Fault injection at each multi-file write and process-crash boundary                                                                             |
| Claude Code            | Gateway auth, competing cloud switches, roles, model picker, effort/context fields, persisted selection                                                                           | A01/A02 regressions through a real supported CLI; managed policy and login coexistence                                                          |
| Claude Desktop         | Profile publication, sealed keys, exact probe, titles, model aliases, visibility, settings restoration, fixed/imported profiles, MCP/tool-search/locale, classic/MSIX directories | Actual Windows builds, DPAPI on Windows, supported native UI versions; A08 and G01–G05                                                          |
| Codex                  | Native/custom provider separation, auth-store constraints, profiles, catalog ownership, thread transition structure and recovery                                                  | Concurrently edited native/thread files have no new acceptance result; keyring, running app-server and WSL paths need isolated integrated tests |
| Pi                     | Provider/model/default projection, scope append, reasoning maps, images, restoration                                                                                              | Native API preference, output limits/cache, external edits to scoped arrays                                                                     |
| OpenCode               | Provider block, variants/context, native JSONC read, strict write path, images                                                                                                    | A03/A04, native provider filters and supported OpenCode versions                                                                                |
| WorkBuddy              | Named catalog, availableModels, reasoning/image/context fields, array-form configuration                                                                                          | Stable wire identity, output ceilings, both installed builds and cached native menus                                                            |
| Gemini CLI             | Env/settings two-file projection, auth selection, hashed IDs, native model read                                                                                                   | Env-file grammar variants and cross-file choice consistency after native edits                                                                  |
| Grok Build             | Model table, protocol, context/effort fields, default selection, ownership                                                                                                        | Native client protocol defaults/output limits and official-login coexistence                                                                    |
| OpenClaw               | JSON5 projection, provider merge, native model forms, legacy/current allowlists                                                                                                   | A05/A06, named profiles and existing session model overrides                                                                                    |
| Hermes                 | YAML projection, custom provider compatibility, selection and restoration                                                                                                         | Active/bot profiles, Windows paths, native profile merge rules                                                                                  |
| Qoder                  | Shared provider projection, capability fields, existing native adapter handoff                                                                                                    | Supported client versions, certificates and native takeover acceptance remain separate                                                          |
| Kimi / Qwen / ZCode    | Shared model mapping, native document adapters, effort/image/context fields                                                                                                       | A07, legacy/current schemas, moved homes and per-task native choices                                                                            |
| DeepSeek Harness       | Cordis projection, field ownership, selection, reasoning map enum                                                                                                                 | Current native schema and inherited user overrides                                                                                              |
| TRAE                   | Durable outbox, delivery/selection distinction, reconnect state, shared ingress identity                                                                                          | Receiver-side adapter differs from Magpie's provider plugin; native bridge/live-client acceptance remains separate                              |
| Provider registry      | Revision checks, redacted DTOs, explicit secret retrieval, endpoint/credential restoration on save/copy, reference checks on delete                                               | Mutation races across profile restore/import and UI operations                                                                                  |
| API keys/imports       | Credential variants, protocol endpoints, custom headers, disabled/preferred keys, import fingerprints and unsupported guards                                                      | P01–P03; full import-and-dispatch fixtures for source-file edge cases                                                                           |
| Subscriptions          | Explicit account binding, dispatch resolution, Codex identity freezing, native OAuth/CLI transport structure, plan/quota display boundaries                                       | P04; vendor acceptance, refresh cancellation/rotation and concurrent accounts                                                                   |
| Catalog/discovery      | Union/per-account lists, selected/vendor/best/usage modes, groups-only, exact preferences, normalized aliases, public facts                                                       | P04, output metadata; no universal per-vendor inference acceptance                                                                              |
| Group definition       | Validation, ordered rules, patterns, manual/disabled members, saved/discovered differences, publication                                                                           | R06; unsupported nested/advanced strategy behavior is G06                                                                                       |
| Scheduler              | Ordering, rotation, weighting, usage decay, quota windows, pace, speed, sinking/cooldown, affinity reconciliation                                                                 | R01/R03/R04/R05; concurrent stress and reset-boundary tests                                                                                     |
| Execution              | Pre-dispatch credentials, approved candidates, headers, context/image eligibility, patience, first-content timeout, semantic stream observations                                  | R02/R07; integrated fault matrix across native and converted protocols                                                                          |
| Continuations          | Consumer scopes, account identity, response/call/item ownership, persistence boundaries, capacity and target-set checks                                                           | Crash/restart and replay integration; intentional fail-closed behavior must remain                                                              |
| Renderer controls      | Provider save/duplicate/toggle/discovery, subscription selection/login callbacks, Agent catalog/connection controls, portable groups                                              | Full browser interaction, stale async replies and complete native schema acceptance not established                                             |

## Verification results and limits

The source-extraction suites compiled with `rustc --edition=2021 --test` using
the checkout's cached dependency artifacts where needed. All fixtures are
synthetic. The output files retain assertion failures rather than hiding them.

| Suite           | Assertions | Passed | Failed | What the evidence establishes                                                                        |
| --------------- | ---------: | -----: | -----: | ---------------------------------------------------------------------------------------------------- |
| `agents.rs`     |          5 |      0 |      5 | Existing projection omissions: cloud switch, picker, model override, OpenCode image, OpenClaw string |
| `providers.rs`  |          1 |      0 |      1 | Duplicate execution variants from a repeated key                                                     |
| `scheduling.rs` |          4 |      0 |      4 | Rotation post-sort, unknown usage, weekly-only tier, overlapping weighted completions                |
| `followup.rs`   |          4 |      0 |      4 | Mutable/colliding request labels, unsupported import guard, package path discovery                   |
| `desktop.rs`    |          5 |      2 |      3 | Sealed-key and exact-probe controls pass; MCP, tool-search and locale projection parity fails        |
| **Total**       |     **19** |  **2** | **17** | **Expected failing review assertions, not a passing application test suite**                         |

The first attempt to run the checkout's focused Cargo suite waited for another
build's target-directory lock and was interrupted. pnpm's project-version
bootstrap could not complete registry/signature verification in the available
network environment. These failures do not prove a source defect and no package
security checks were disabled. The full contributor/CI command set has not been
accepted for this tree, and no remote CI result is claimed.

The direct command `node node_modules/prettier/bin/prettier.cjs --check
docs/qa/2026-11-05-magpie-parity-review.md` passed. The extraction
tests are evidence for review, not substitutes for the repository regressions,
Linux/Windows/macOS jobs, native client tests, or release acceptance.

## Suggested implementation order

1. Preserve stable identities and imported limits first: A07, P01, P03, P04.
2. Correct routing selection and failure boundaries: R01, R02, R05, R06, R07,
   retaining the pinned/opaque safeguards.
3. Repair native connection compatibility: A01, A03–A06, A08, and Desktop
   G01/G05 before treating a successful configuration write as connectivity.
4. Add native menu/metadata and strategy parity with version-scoped acceptance:
   A02, R03/R04, G02–G04 and the explicitly chosen parts of G06/G07.

Each implementation item needs an integrated regression before the fix, then
the applicable final-tree contributor checks. No finding in this report is
marked fixed.
