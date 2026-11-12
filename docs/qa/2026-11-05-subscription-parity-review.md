# Subscription parity review — 2026-11-05

Scope: Claude, ChatGPT/Codex, ChatGPT API (Sign in with ChatGPT), GitHub
Copilot, Gemini CLI, and Grok. Antigravity was reviewed immediately before this
pass; this document records the additional subscriptions and shared behavior.

This is an audit, not a repair or a claim of complete parity. No application
source was changed. The existing dirty working tree was preserved. Findings
below distinguish demonstrated local defects from source-confirmed protocol or
feature differences. Real accounts, tokens, vendor endpoints, installed clients,
and the running application were not used for acceptance.

## References and validation

- Vary base: `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`, plus the existing
  working changes identified by fingerprints below.
- Magpie: `465928bccea4fec3537edd6280f7cabb9a0c5bad` in
  `/private/tmp/vary-magpie-audit-20261105`.
- Magpie's Grok entry is plugin-served. Its `migrate_side.go` names
  `@magpie-community/opencode-grok-auth`, minimum version 0.2.3. The additional
  public plugin source was read at commit
  `41e90a18d3affc7487460d032b0cdc5e09b20ebb` in
  `/private/tmp/vary-subscription-plugins-review-20261105/packages/grok`.
  This supplemental checkout is a separately pinned reference, not an assertion
  that every Magpie installation runs that exact plugin revision.
- Host: macOS/Darwin arm64; Rust/Cargo 1.98.1.
- A temporary Rust probe compiled exact DTO/function excerpts extracted from the
  checked source, using the existing compiled serde dependencies. It passed six
  assertions: three Copilot wire incompatibilities, loss of Codex held-account
  information, Claude tool-history flattening, and missing Gemini capabilities.
  Source, build command, binary, and output are in
  `/private/tmp/vary-subscription-audit-20261105/` (`source-probe.rs`,
  `source-probe-command.txt`, `source-probe`, `source-probe.log`).
- The initial ad hoc compile selected incompatible serde artifacts; selecting a
  matching dependency pair resolved that harness-only compilation failure.
- Full application suites were not rerun for this documentation-only audit.
  Prior green tests are not presented as coverage of these newly found cases.

## Findings

### SUB01 — P1 — Grok subscription inference uses the general API contract

Vary binds `xai_oauth` to `https://api.x.ai/v1` in
`src-tauri/src/services/routing_upstreams.rs:156`; model discovery uses that same
host in `src-tauri/src/commands/xai_oauth.rs:118`. Subscription credential
resolution in `src-tauri/src/services/subscription_provider.rs` attaches the token
but adds no Grok CLI request headers.

Magpie's Grok plugin uses `https://cli-chat-proxy.grok.com/v1`, with CLI-specific
client, model, context-window, and loop-check headers. Its README and
`packages/grok/index.mjs` describe the contract. Vary's alternate general API may
have its own acceptance, but it is not the reference subscription transport.
A successful device sign-in therefore does not establish reference-compatible
subscription inference. Validate the model endpoint, inference host, headers,
body shaping, and account refresh together before declaring this fixed.

### SUB02 — P1 — Claude tool history becomes ordinary user text

`src-tauri/src/proxy/native_subscription.rs:334` collects all prior roles and
contents into one user message. `tool_use`, `tool_result`, and thinking blocks
are serialized into text. Each invocation also uses `--no-session-persistence`
at line 431. A multi-turn tool call loses its typed call/result association,
even when the initial reply looked correct. The source-derived probe confirms
that a synthetic assistant tool call and matching result produce only text
blocks in one user message.

Compare Magpie's structured Claude subscription bridge and
`docs/subsystems/claude-subscription-bridge.md`. This was already acknowledged as
part of G07 in the earlier general review; it remains an active gap.

### SUB03 — P1 — A Claude quota read can disable a refreshable account

`native_subscription::resolve` skips token refresh for Claude. The quota branch
at `src-tauri/src/services/native_subscription.rs:1685` reads the access token
from the copied credential file/keychain and calls `/api/oauth/usage` directly.
A 401 reaches `observe_rejection` at line 1712, which persists
`reauth_required=true`. Later `resolve` calls reject that account before its CLI
can renew credentials. An expired access token with a still-valid CLI refresh
credential is sufficient to enter this path.

Quota inspection should not convert a renewable CLI session into terminal
reauthentication without trying the supported CLI renewal/usage path. Magpie's
current Claude usage path and fallback tests cover CLI `/usage` observations.

### SUB04 — P1 — One native account refresh blocks unrelated subscriptions

`src-tauri/src/services/native_subscription.rs:390` acquires the single static
`ACCOUNTS` mutex before reading accounts, then holds it while awaiting the token
request and persistence. The request can take 30 seconds. `resolve` for an
unrelated Google, ChatGPT API, or Claude account needs the same mutex; direct
forwarding calls it too. Account list and edits also wait behind the refresh.

Rotation must remain serialized for the same account, but the network wait
should not serialize every native provider/account. Use per-account refresh
coordination and generation checks around a short store mutation.

### SUB05 — P1 — Copilot rejects supported usage response shapes

`src-tauri/src/proxy/providers/copilot_auth.rs:144` requires
`quota_reset_date`, all three snapshots, integer `entitlement`/`remaining`, and
several other fields. The reference fixtures in
`internal/provider/copilot_usage_test.go` include numeric strings, UTC-only reset
dates, snapshots without `remaining`, and a premium-only snapshot map.

Three source-derived probe cases independently reject an otherwise-valid
response: string entitlement, UTC-only date, and a missing Chat snapshot.
The result is a generic quota failure for legitimate accounts rather than a
usable partial reading. Parse optional snapshots and bounded numeric/string
values without inventing unknown allowance.

### SUB06 — P1 — Exhausted Copilot organization pools disappear

`src/components/providers/SubscriptionAccountQuota.tsx:49` drops every
`unlimited` snapshot. GitHub Enterprise premium pools may report
`unlimited=true` with `has_quota=false` when exhausted. Magpie reads this specific
case as 100% used; it treats ordinary unlimited Chat/Completions separately.
Vary's DTO also drops `has_quota`, so the UI cannot recover the distinction.

Even after SUB05 is fixed, simply accepting more response shapes will not fix
this semantic error. Preserve pool exhaustion and overage/organization policy
through the DTO, quota model, and display.

### SUB07 — P2 — Copilot model refresh can return the same indefinite cache

`fetch_models_for_account` at `copilot_auth.rs:865` returns a cached list without
an age or force-refresh parameter. The editor's refresh calls this method
again. Removal/clear-auth clear the cache, but normal explicit model refresh has
no invalidation path. Newly enabled models can remain absent until the manager
is recreated or the account is removed.

### SUB08 — P2 — Native and managed catalogs discard model capabilities

- Copilot's model DTO (`copilot_auth.rs:192`) retains only ID/name/vendor/picker
  visibility. It discards supported endpoints, context limits, modality,
  reasoning, and policy fields. The registry declares `openai_chat` for the
  provider, so catalog-specific endpoint support cannot be selected from this
  observation. A Responses-only model can be offered under the wrong fixed
  protocol. Validate endpoint selection in the actual shared bridge as well.
- ChatGPT API discovery at `native_subscription.rs:1496` preserves only the model
  ID and display name. Magpie's `parseSIWCModels` retains input modalities,
  reasoning levels, and both context windows.
- Gemini CLI's `gemini_models` at line 1733 emits only `{model}`. The probe
  confirms missing image/context/reasoning metadata. Magpie's offered models
  preserve their declared capabilities.
- Grok's command constructs fetched models from an empty metadata object; the
  subscription editor then narrows them to `{model}`. The plugin additionally
  handles reasoning levels and multiple context windows.

A UI preference editor cannot reconstruct information already discarded before
storage. Preserve upstream capability facts through discovery and publication.

### SUB09 — P2 — Codex quota parsing omits explicit holds and credit metadata

`src-tauri/src/services/subscription.rs:714` represents only
`rate_limit.primary_window` and `secondary_window`. `allowed`, `limit_reached`,
`model_usage`, credits, reset credits, and other quota controls are ignored.
A source-derived case with `allowed=false` and `limit_reached=true` parses
successfully with no representable windows.

The reference has separate account-hold, model-availability, credit-balance, and
credit-spending behavior. Vary must not present successful deserialization as
complete quota parity. Credit-spending choices and account policy are broader
feature gaps; they require an explicit contract, not fabricated percentages.

### SUB10 — P2 — The older Codex quota entry misclassifies transient auth errors

`src-tauri/src/commands/codex_oauth.rs:62` turns every manager token error into
`CredentialStatus::Expired`, including refresh transport/service errors. This
can clear a good tray observation and ask for login during a temporary outage.
The newer editor command in `commands/codex_provider.rs:391` already distinguishes
terminal and transient errors through `quota_auth_error`; both entry points
should share that classification.

### SUB11 — P2 — Gemini experiment discovery sits on every inference request

`gemini_wire_model` at `native_subscription.rs:1768` calls `gemini_flags` each
request. `gemini_flags` performs a network request with the Code Assist timeout
and turns a failure into all flags off. The same account can therefore map a
saved Flash model differently during a transient experiment-service failure,
and a healthy inference waits on an auxiliary lookup. Magpie caches flags by
account and project for an hour. Keep account/project cache boundaries and
avoid a new experiment request on every turn.

### SUB12 — P2 — Gemini quota labels do not use the account's displayed aliases

`parse_google_quota` uses raw bucket `modelId` values for Gemini. The model
picker and wire mapping may display `gemini-3.8-flash` while quota says
`gemini-3-flash`. Magpie applies `geminiFlags.shown` to quota observations so the
bucket, picker model, and rollout are associated. This is an identity/display
mismatch, distinct from SUB11's latency behavior.

### SUB13 — P2 — Google accounts with an existing project can keep no plan label

`google_project` returns immediately for a nonempty saved project at line 1329.
Quota reads then skip `loadCodeAssist` and plan observation. Model discovery has
an extra best-effort plan lookup, so a card can stay without its plan until a
manual model fetch. This affects Gemini CLI as well as Antigravity. Plan
observation should be independent of whether a project was already known.

### SUB14 — P2 — Non-Codex model discovery is not automatic after account selection

`src/components/providers/SubscriptionProviderFields.tsx:166` only invokes
`discover` from the refresh action and selected reauthentication paths. Native
and managed account selection clear the previous catalog without starting the
new account's discovery. Opening an empty saved catalog or completing a new
account can leave the model area empty despite successful authorization.
The subscription editor should observe the selected account explicitly, retain
per-account good results, and reject stale responses after a binding changes.

### SUB15 — Protocol gap — Grok quota uses a different service from the plugin

`src-tauri/src/services/subscription_grok.rs:23` reads the older gRPC-web
`grok.com/.../GetGrokCreditsConfig` endpoint with a protobuf heuristic.
The plugin reads `cli-chat-proxy.grok.com/v1/billing?format=credits`, shapes its
credits/windows, and retries a rejected token through the CLI. The two are not
the same quota contract. This review does not assert that the old endpoint is
universally unavailable; it establishes that the reference's current quota and
renewal behavior is not implemented by this code.

## Cross-cutting feature gaps

Non-Codex subscription sources currently bind one `accountId`; Codex has a
separate multi-account settings path. Magpie's per-account enablement, preferred
account, ordered exhaustion switching, and per-account model/allowance behavior
cannot be represented by replacing a radio button with a checkbox. Preserve
explicit binding and add a reviewed account-pool representation and dispatch
policy if parity is required. This is a feature gap, not an allegation that
explicit single-account binding is intrinsically wrong.

The generic subscription editor also lacks the dedicated model-test and image
model configuration paths visible in the reference. Some provider restrictions
are intentional: ChatGPT API reports no separate allowance endpoint, uses
stateless streamed Responses, and rejects unsupported hosted tools. Those
restrictions agree with the reference's stated SIWC boundary and are not quota
bugs by themselves. The reference also contains compatibility filtering and
prompt-cache key derivation that are not fully reproduced here.

## Priority and acceptance

Start with SUB01–SUB06, then complete account-specific model and allowance
contracts. Use vendor-shaped synthetic fixtures and fake CLI multi-turn tests.
Do not infer production acceptance from the six local source probes, and do not
weaken explicit account bindings, credential redaction, or replay/stream safety
to imitate a UI. No live login, paid request, or user-account modification was
performed in this audit.

## Source fingerprints

| File | SHA-256 |
| --- | --- |
| `src-tauri/src/services/native_subscription.rs` | `0c43acb703504d79d5df42ad9634b79dfd4f9987beb239bb4f7fef6cf6cad2c0` |
| `src-tauri/src/proxy/native_subscription.rs` | `bdf611b9c49eaec82e90fa98fddd7dff31453133b029d4392d1641e60008fc33` |
| `src-tauri/src/proxy/providers/copilot_auth.rs` | `8c386ce5a00a6ead9b8fca29ee82cb26c24eb518830de08af44bfdf80124ed63` |
| `src-tauri/src/commands/codex_oauth.rs` | `02159addb69b8f6f92f36f1067110fc9b1ed75b70fc916303dfc9882c22112d6` |
| `src-tauri/src/services/subscription.rs` | `aef2bb61b9213b0a3cd8822e909327510a491db7a8081c38a9c5ff09cc8af8c3` |
| `src-tauri/src/services/subscription_grok.rs` | `faa9ca370a1b9fea4cd4187c3ea506a3c7ab9d0520e1b3eb28341d7c6e8bee15` |
| `src-tauri/src/services/routing_upstreams.rs` | `707ed22f0ab8921070430c9c09e103b1db8e81ea238cac040290a149eac80203` |
| `src-tauri/src/commands/xai_oauth.rs` | `fc4dacf46350ceb2b63ba3afcc768325b92215373686fdcd80562979c01a4881` |
| `src/components/providers/SubscriptionProviderFields.tsx` | `596859b2472fa62b3a6d5c8236f4c8d0fcb86a910dbe92a0eba387f0b29721d6` |
| `src/components/providers/SubscriptionAccountQuota.tsx` | `ca5bddf901554ddb846eb8e8d53de736c3b4f8112654b732a4b36bffd56482b9` |
| `src/components/providers/NativeSubscriptionAccounts.tsx` | `84ed7acff838a6f1acb30ee81fc195d47202f818a17227ab7e79beed677e910f` |
