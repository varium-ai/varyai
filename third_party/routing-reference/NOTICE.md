# Routing Reference Attribution

Vary AI adapts selected routing, catalog, gateway, and interface behavior from
[yetone/magpie](https://github.com/yetone/magpie), Copyright (c) 2026 yetone,
under the MIT License. The complete copyright and permission notice is retained
unchanged in [LICENSE](LICENSE). Renamed local assets and modules retain this
attribution.

## Pinned source revisions

- `1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`: routing reset countdown
  precision reviewed in `internal/gui/assets/plugins/routing/client.js` (`dur`).
  Vary applies the same second/minute/hour/day thresholds and rounding in its
  shared React stage, with translations for its four runtime languages.
  Packet geometry and motion additionally follow `holdAll`, `pose`, `frame`,
  `pLayout`, `pTick` and `pPlay`: independent dots at the same waiting endpoint,
  original endpoint clearances, easing and workspace/tray timings. Vary keeps
  only the small circular packets; the bird artwork is not copied.
- `465928bccea4fec3537edd6280f7cabb9a0c5bad`: Codex provider identity,
  ChatGPT sign-in coexistence, model catalog publication, and recoverable thread
  provider transitions reviewed in `internal/agent/codex.go`,
  `internal/sessions/codex_provider.go`, `internal/sessions/codex_portable.go`,
  and the corresponding
  `codex_threads_over`, `codex_threads_back`, and `codex_beside_threads` tests.
  Vary implements these behaviors in Rust using its existing configuration
  receipts, native authentication leases, and isolated recovery records.
- `e17b6cefc7b25c19d1e522f9e8a9626bf68912d9`: account-visibility behavior
  reviewed in `internal/gui/assets/app.js` (`hideAccounts`, `names` and
  `dots`). Vary uses React-rendered stand-ins for account labels and sanitized
  tooltips, while preserving provider names and logos. No DOM walker or source
  implementation is copied.
- `magpie-community/plugins@f7e87877e0692b669a05125d21348178c40d0240`:
  reviewed `packages/trae/index.mjs` (`conversationID`, `sessionOf` and
  `traeMessages`) and `conversation.test.mjs` for stable conversation identity
  and preservation of native tool-call/result pairs. That plugin sends requests
  to TRAE's service; Vary receives TRAE client requests. Vary's independent
  implementation resumes unnamed Chat tool history through existing observed
  call ownership instead of deriving a session from the first user message.
  No plugin source is copied.
- `6d1e3f1e29ebe7d862cda8fa4fa842842f3acb49`: explicit Codex disconnect
  behavior reviewed in `internal/agent/codex.go` and `internal/agent/applied.go`.
  Vary uses its own private backup, recognition checks and atomic writes when
  disconnecting a configuration whose recovery receipt is missing. Background
  recovery remains separate from that explicit action. Agent model-picker
  geometry, search, icon navigation, source badges, and row styling additionally
  follow `internal/gui/assets/app.js` and `internal/gui/assets/app.css` at this
  revision; Vary retains its existing connection and disconnect confirmation APIs.
- `c7bbd3faf6de7c59ff65848450b0640a7ee2ecb1`: protocol-neutral request,
  reply and streaming-event architecture, typed wire readers and target builders
  under `internal/gateway/{ir,chat,responses,anthropic,gemini,codeassist}.go`.
  The Rust implementation keeps Vary AI's routing, authentication and security
  boundaries; see `docs/architecture/protocol-conversion.md` for verification.

- `3bce283df43c3ce733e457daa16a69ac38bbb5fd`: resource-oriented provider and
  Agent management, the routing workspace, and provider/import artwork under
  `internal/gui/assets/icons/`. Original vendor files are `groq.svg`,
  `together-color.svg`, `fireworks-color.svg`, `ai302-color.svg`, and `zai.svg`.
  Original import files are `app-cc-switch.png`, `claudecode-color.svg`,
  `codex-color.svg`, and `magpie.svg`.
  Routing motion additionally follows `plugins/routing/client.js` (`play`,
  `home`, `staged`, `playing` and `flying`): independent concurrent request
  lifetimes, provider waits, fallback returns and the final response journey.
  Vary uses its React packet players and recorded trace metadata; no bird artwork
  is copied. Cancellation has a separate neutral presentation.
- `c4551dfcacf67536180291da402ded1d1316a2d8`: protocol review and Gemini
  tool-result role/image adaptation. Reviewed sources under `internal/gateway/`
  are `ir.go`, `gateway.go`, `gemini.go`, `translate_severed_test.go`, and
  `gemini_function_parts_test.go`.
- `e5dd86965b56a8a176790e2c1ade09f9ef554208`: the original intent-routing
  proposal, referencing `internal/gateway/rules.go`, `classify.go`, `decide.go`,
  and `internal/provider/grouprule.go`.
- `508911e431f752052d1c8374f89de7ec75f291ac`: routing-row motion from
  `internal/gui/assets/app.js` and `plugins/routing/client.js`; pool strategies
  from `internal/routes/strategies.go`, `fastest.go`, `round.go`, and
  `internal/gateway/speed.go`.
- `0414bbeddbfc2028508b610fd1e7c194143454a0`: capacity and compaction
  publication from `internal/provider/models.go`, `group.go`, and
  `internal/codexcat/codexcat.go`.
- `84da86ebd5aecf3ef4656c2315442c26b624933d`: reasoning capability lookup
  and native Agent publication, reviewed in `internal/provider/models.go`,
  `internal/catalog/catalog.go`, `internal/codexcat/codexcat.go`, and the
  Claude, Pi, WorkBuddy, Grok, OpenClaw, Qoder, Kimi and ZCode adapters under
  `internal/agent/`. Import also follows per-model preference semantics from
  `internal/provider/modelprefs.go` and the corresponding settings fields in
  `internal/settings/settings.go`.
- `869c4d112d00d0c204cb346c49371a40a58d2a08`: Claude Desktop effort-aware
  gateway identities and old-session alias compatibility, reviewed in
  `internal/gateway/desktop.go` and `desktop_effort_test.go`. Profile effort caps
  additionally follow the installed Desktop 4.4212 native configuration schema.
  Desktop auxiliary routing also follows `desktopTurn`, `desktopChosen`,
  `desktopTitle`, `desktopProbe` and `servedBeside`, with behavior checked against
  `desktop_turn_test.go`, `desktop_probe_test.go` and
  `desktop_tier_provider_test.go`. Vary additionally keeps bounded in-memory
  bindings when requests include a session identifier and queues persistence
  outside forwarding.

Other adapted catalog helpers include `internal/catalog/family.go`,
`catalog.SameModel`, `internal/provider/grouptemplate.go`, and the usage-purpose
catalog. The behavioral limits remain documented in Vary's architecture and
design guides; an attribution does not establish complete compatibility.

Claude Desktop credential synchronization additionally references
`internal/agent/claudedesktop.go` (`desktopKeyApplied`, `desktopKeyTaken`) and
`internal/agent/desktopseal.go` at revision
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`. Vary repairs readable credentials
only within its journaled gateway profile. It retains independent random
connection tokens, so an opaque seal is preserved only when refreshing the
existing token; a new connection writes its new token. No fixed Magpie token or
cross-agent credential acceptance is adopted.

## Compatibility literals

Daily reward protocol behavior was reviewed in `magpie-community/plugins`
`packages/qoder/index.mjs` (Git blob `5785a7c0c8f68493d16d315c1063d1514da09a76`),
`packages/trae/index.mjs` (blob `15d2389db8ebb07fcb30b46fd5f0c65f4a11da5f`),
and `packages/zcode/index.mjs` (blob `13cbbe330f3896621e492b4f69f621ed16664f84`).
VaryAI independently implements campaign-window accounting, official daily-credit
IPC and explicit gift-plan actions. Native interfaces were additionally checked
against the installed Qoder CN 0.4.8/0.5.3, TRAE CN 3.4.14/3.4.15 and ZCode 3.16.3 clients and
ZCode's public `docs/marketing-touch-client.md` contract. No plugin implementation
is copied. Qoder account reuse additionally follows the installed client's
`product.json`, encrypted auth schema and Electron/Chromium's macOS `v10` format.
OS-approved native account access is preferred, with an optional independent
browser grant. Native refresh tokens are not rotated, and CAPTCHA remains in the
official client.

Additional subscription integrations reference `internal/provider/google.go`,
`chatgpt_api.go`, `claude_signin.go`, `claude_dirs.go`, `claude_code_models.go`,
and `internal/gateway/claude_subscription.go` / `codeassist.go` at revision
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`. The Gemini CLI and Antigravity
SVGs come from that revision's `internal/gui/assets/icons` (Lobe Icons,
MIT licensed). Vary's Rust implementation uses its own account storage and
shared protocol bridge; it does not implement Magpie's persistent Claude runs.

Routing candidate names additionally follow `internal/provider/keys.go`
(`RestKeys` and `severalOn`), `internal/gateway/trace.go` (`weighed`), and the
routing plugin's `who` / `makeRow` at Magpie revision
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`: one enabled key uses the supplier
name, while a shared pool uses key names with masked-key fallbacks.

Internal selectors, variable names, assets, and design filenames use Vary or
descriptive names. A few external format identifiers remain literal so existing
files and integrations continue to work:

- Provider imports retain the source ID `magpie`, the optional IPC argument
  `magpiePath`, the default `~/.config/magpie` / `$XDG_CONFIG_HOME/magpie`
  locations, and recognition of `magpie-models.json` and the `magpie` provider
  identity when avoiding imports of a client's own generated gateway entries.
- Portable routing-group JSON retains `magpie: "routing-group"`, version 1,
  and accepts the `magpie/` model-reference prefix.
- Compatibility tests retain those exact synthetic identifiers. They must not
  be replaced with escaped or dynamically assembled spellings to conceal them.

The renderer labels the portable source **Magpie** and places it first. Import reads
the original source files, keeps credentials in the backend, and does not modify
the source application.

- `c3593dc4944b91819ed12c601b0f2e844b804b5e`: desktop discovery behavior
  reviewed in `internal/agent/desktopapp.go`, `workbuddy.go` and
  `workbuddy_app_test.go`: application identities, both WorkBuddy builds,
  application subfolders and Spotlight fallback. Vary uses its own Rust
  discovery and React window-activation refresh implementation.

## Codex service-tier publication

At `465928bccea4fec3537edd6280f7cabb9a0c5bad`, the `ownTiers`, `namedTiers`, and
entry-tier publication behavior in `internal/codexcat/codexcat.go`, together with
`internal/agent/codex_catalog_tiers_test.go`, inform VaryAI's independent Rust
service-tier adapter. VaryAI normalizes supported public fields, keeps local and
served catalogs consistent, and uses common tiers for routing groups rather than
Magpie's any-fast-member rule because group routing is not tier-aware. No upstream
source implementation is copied.

At the same revision, `internal/provider/modelprefs.go` (`Label`, `Labels`) and
`internal/provider/codex_models.go` (`codexApart`) inform the separation of short
picker labels from stable source/model identity and visible-list disambiguation.
VaryAI independently abbreviates automatically named personal-account sources,
keeps explicit custom names, and numbers colliding visible sources without adding
account email addresses to model labels.

## Subscription protocol repairs

The managed Grok subscription transport independently follows
[magpie-community/plugins](https://github.com/magpie-community/plugins),
`packages/grok/index.mjs` at
`41e90a18d3affc7487460d032b0cdc5e09b20ebb`: CLI proxy endpoints, client headers,
model/context-window identities, credit billing and rejected-token renewal.
Copyright (c) 2026 magpie-community contributors; MIT License. The complete
permission notice is retained in [PLUGIN-LICENSE](PLUGIN-LICENSE).

At Magpie revision `465928bccea4fec3537edd6280f7cabb9a0c5bad`,
`internal/gateway/claude_replay.go`, `internal/provider/claude_usage_cli.go`,
`internal/provider/copilot_usage_test.go` and the Google/Codex subscription
adapters inform VaryAI's typed temporary replay, quota compatibility and
account/project observations. VaryAI uses its own Rust account coordination,
configuration receipts and shared protocol bridge.

Codex paginated-provider migration was compared with
`internal/sessions/codex_provider.go` (`RetagPagedCodexThreads`) and
`internal/agent/codex.go` at Magpie revision
`8c733bb36232e04d74a47ff2b3e05f71c3c6fa7b`. VaryAI independently implements
the SQLite-only transition using its private recovery journal, writer-lock
coordination, and checked provider/history-mode updates. Paginated rollout
contents and byte offsets remain unchanged. The Magpie MIT notice and license
listed above apply to this behavior reference.
