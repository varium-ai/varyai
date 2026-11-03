# Reference Parity Worklist

Reference: the upstream routing reference commit
`3bce283df43c3ce733e457daa16a69ac38bbb5fd`.

Visual similarity is not behavioral parity. An item is complete only when its
storage, control-plane API, runtime behavior, UI, and synthetic acceptance tests
agree. Preserve Vary's security inspection, field-owned recovery, historical
storage names, and credential redaction throughout this migration.

## Tools and Agents

- [x] Separate global providers, shared model menus and per-tool connections.
- [x] Use compact reference rows, inline details and starting-model selection.
- [x] Preserve owned-field recovery and block model changes during conflicts.
- [x] Publish shared reasoning choices to Codex, Pi and OpenCode.
- [x] Connect directly from a switch and confirm disconnect-and-restore.
- [x] Connect directly when choosing a starting model while disconnected.
- [x] Distinguish inactive connections from configuration drift and offer explicit Apply again.
- [x] Load CLI versions independently, distinguish Codex App metadata, and offer verified single/serial CLI updates.
- [x] Offer confirmed CLI installation in More tools, verify completion, and distinguish official-page desktop installation.
- [ ] Port the reference Agent registry and all configuration adapters.
- [x] Add shared native projections for Gemini, Grok, OpenClaw, Hermes, Qoder, Kimi, Qwen and ZCode.
- [x] Deliver TRAE shared models through a private pending outbox; identify Doubao as audit-only.
- [ ] Match connection, verification, bypass, recovery, and process states.
- [ ] Match explicit restart confirmations without restarting during tests.
- [ ] Match default-model, reasoning, and per-tool configuration interactions.
- [ ] Verify every rendered state against reference fixtures.

## Models and Providers

- [x] Port compact rows, switched-off fold, searchable add sheet and editor frame.
- [x] Add API vendors and relays without selecting an Agent.
- [x] Pair supported regional sites and protocol-specific preset endpoints.
- [x] Separate Z.ai/Zhipu Coding Plan and pay-as-you-go endpoints with saved plans.
- [x] Preserve stored-key redaction, revision checks and model context metadata.
- [x] Edit in a framed modal with focus return, draft protection and saved preset regions.
- [x] Use fixed header/footer, protocol tabs, explicit probes, model chips and private duplication.
- [x] Stage model names, reasoning levels and image overrides with default restoration.
- [x] Replace the imported Codex placeholder with account selection/order, explicit login/import/quota/discovery/probes and saved model modes.
- [x] Execute same-model account pools without moving a bound response/tool continuation to another account.
- [ ] Match separate image-generation model selection and all Codex advanced account controls.
- [x] Replace retired provider references with independent native provider records; discard the old contract.
- [x] Add explicit source-tab imports from CC Switch, Claude Code, Codex, OpenCode and Provider backup, with safe previews and stale-source checks.
- [x] Import saved Codex subscriptions from the source app into managed accounts without changing source applications.
- [ ] Match subscription and plugin authentication lifecycle.
- [ ] Match multiple keys, accounts, quotas, concurrency, and routing controls.
- [ ] Match all built-in vendor presets, discovery, model overrides, and import.
- [ ] Verify editor, list, folded states, keyboard and focus interactions.

## Routing

- [x] Port stage, decision log, request/account columns and inline group list.
- [x] Match conditional multi-select call-purpose filtering; keep usage-only history unmarked.
- [x] Fold observed candidates by credential, with provider headings, attempt tallies, answered models and reference account/key folding thresholds.
- [x] Execute nine strategies and record bounded real forwarding decisions.
- [x] Resolve dynamic catalog globs/regex, including unsaved editor previews.
- [x] Keep disabled members and hidden direct models without deleting resources.
- [x] Enforce first-content deadlines and bounded transient-failure patience.
- [x] Offer custom reasoning levels and preserve destination capability fitting.
- [x] Distinguish per-member effort variants and reject identical seats.
- [x] Match deterministic AND conditions, zoned hours, folders and pause rules.
- [x] Prioritize ordered rule fallback targets and fit output reasoning.
- [x] Persist group order and expose manual radio choices and handle actions.
- [x] Stack vendor logos, drag group/model handles, and support row context menus.
- [x] Share the reference envelope without machine-local folder rules.
- [x] Import immediately with missing-model, substitution and omitted-rule results.
- [x] Keep rules visible in the inline label/content editor; fold diagnostics.
- [x] Edit rules as condition/model sentences with ordered targets and handle actions.
- [x] Select an intent classifier from the shared picker; fold legacy manual entry.
- [x] Generate smart/thrifty/steady templates from catalog facts, not generic presets.
- [x] Add a template immediately or edit first; explain unavailable templates.
- [ ] Match nested groups with independent child routing semantics.
- [ ] Match automatic cache-TTL affinity and all sink/rest classifications.
- [ ] Match durable within-turn rule holding and every classifier decision state.
- [ ] Match classifier groups, automatic effort, effort overrides and fast mode.
- [x] Discover equivalent models across registered providers by default; retain disabled seats.
- [x] Hide/restore discovered groups, take ownership on edit, and reseat Agents when discovery is off.
- [x] Stage explicit Same-as model identities without changing upstream wire IDs.
- [x] Load missing API/account catalogs for discovery; distinguish unavailable logins and empty catalogs.
- [x] Exclude providers from automatic groups without deleting direct models or user-owned members.
- [x] Move shared Long conversations to relevant tool details, with a Settings fallback and segmented controls.
- [ ] Match every family/unlisting rule and all Same-as capability/price inheritance.
- [ ] Match persistence, transfer, replay and account-refresh semantics.
- [ ] Verify streaming cancellation and continuation invariants at dynamic ports.

## Acceptance

Use temporary configuration and synthetic accounts. Do not read or rewrite live
client configurations, initiate logins, restart user applications, or send paid
requests merely to demonstrate parity. Unit/build checks do not establish live
account, native-client, packaging, or release acceptance.
