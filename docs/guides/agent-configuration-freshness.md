# Agent configuration freshness

VaryAI distinguishes saved connection configuration, configuration delivery,
native model selection and the configuration a running client loaded. A successful
request through one instance does not establish that every other window or session
has loaded the current configuration.

## When a change needs to be loaded

The managed Agent adapters preserve their durable application timestamp when only
unrelated preferences, receipt bookkeeping, recognized catalog labels, descriptions,
icons or catalog ordering change. Endpoint, credential, model membership and model
behavior changes still advance that timestamp. Unknown or ambiguous arrays retain
exact comparison; ordered non-catalog arrays keep their ordering semantics.

A route created or edited inside VaryAI does not itself trigger a restart warning.
A warning can appear if that change alters a model catalog or connection setting
read by an already running client. Changing an upstream behind an unchanged
client-facing model does not require restarting the client unless its generated
startup configuration also changes.

| Client | Freshness and recovery behavior |
| --- | --- |
| Claude Desktop, WorkBuddy editions, Qoder, Kimi, ZCode, DeepSeek Harness | Compare verified application startup with the time of the last meaningful managed configuration change. Reopen is an explicit user action. |
| TRAE China and International | Persist a digest of the bridge startup argument. Unrelated `argv.json` edits and delivery retries do not advance launch freshness. Configuration delivery and native model-selection acknowledgements remain independent. |
| Doubao Work | An active inspection connection takes precedence over historical application age. A session still waiting for inspection remains waiting. Platform-specific inspection availability is unchanged. |
| Claude Code, Pi, OpenCode, Gemini, Grok Build, OpenClaw, Hermes, Qwen, T3 Code | Preserve meaningful configuration change times and use session-specific guidance. They are not given a desktop restart action merely because a configuration file changed. No per-process CLI/IDE session inventory is claimed. |
| Codex | Separate persisted route/catalog semantics and verified app-server identity; see [Codex model catalog](codex-model-catalog.md). |

Both macOS and Windows desktop comparisons allow one second for startup clock
precision, matching the reference's boundary rule. A process with an unknown start
time remains unknown. The process identity, ownership and configuration rechecks
used by explicit restart actions remain in place.

Disconnecting records a new restoration time only if owned startup semantics
actually change. Repeat disconnections do not create new timestamps. Existing
receipts remain compatible; previously conservative timestamps cannot reconstruct
what a client loaded before the feature was introduced. TRAE's first snapshot uses
the launch file's modification time, then preserves it across harmless rewrites and
VaryAI restarts. A malformed snapshot is not overwritten to manufacture a fresh
baseline.

## Reference and scope

The comparison principle follows Magpie commit
`6d15f63d52146fcadccef30c9777e45051bcd11c`, primarily
`internal/agent/connectinfo.go` (`startsWith`, `changedAt`, `StaleCopies`). That
reference specializes startup-content observation for Claude Code and Codex; it
does not supply a universal restart adapter for every Agent. VaryAI applies the
same principle to its additional adapters while preserving their native delivery,
model-control and inspection requirements. Catalog behavior parameters remain
significant because these adapters serialize those parameters for their clients.
