# Local gateway keys

Create a key under Proxy → Gateway keys. Choose exact, case-sensitive client model IDs (comma separated), an optional token limit, and a reset period. Empty model lists are rejected. A zero budget blocks immediately; an empty budget is unlimited. The secret is displayed once; only its SHA-256 hash is stored in `vary-ai.db`. Revocation is permanent for that credential and takes effect on subsequent admissions. Requests already admitted can finish.

Configure scripts with `http://127.0.0.1:15721/gateway/v1` as their base URL (replace the port if customized), and use the generated secret as their API key. Supported POST endpoints are `/messages`, `/responses`, and `/chat/completions`. Use `Authorization: Bearer <key>` or `x-api-key: <key>`. The gateway removes client credential headers before invoking existing provider routing; upstream authentication comes from the configured provider. No supplier credential is needed in the script.

The dedicated `/gateway` namespace always requires a gateway key. Existing local client takeover endpoints keep their existing authentication behavior. These budgets govern gateway-key traffic, rather than serving as an isolation boundary against other processes on the same computer. The listener remains loopback-only. Model policy applies to the client model ID before existing provider model mappings.

## Budget semantics

Budgets count reported input + output + cache creation tokens, plus cache read tokens only when the input count excludes them (Anthropic Messages). OpenAI input/prompt counts already include cache reads. Reasoning tokens already included in output are not added twice. Each key has independent counters, regardless of whether ordinary proxy logging is enabled.

Daily periods reset at 00:00 UTC. Monthly periods reset at 00:00 UTC on the first calendar day. Never-reset counters have no expiry. UI reset timestamps use the local display timezone. Usage belongs to the period in which the request was admitted, including responses that finish after midnight. Changing only the limit preserves consumption; changing the period selects that period's counter and can change the displayed consumption.

This is a completed-usage admission limit, not a prepaid reservation or a hard cap on an active generation. In-flight concurrent requests, a single long response, and asynchronous persistence delay can exceed a budget. After completed usage is persisted, subsequent requests are rejected when consumption is at or above the limit. Streaming responses are observed without delaying their chunks. On disconnect, any usage already observed is queued. Unreported usage cannot be counted. Responses or individual SSE lines above the observer's 1 MiB bound are passed through but may not be metered; a process exit or persistence failure can also lose pending usage. Accounting failures do not interrupt forwarding. Monitor upstream limits for strict spend control.

Budgets use tokens, not money. Token counters are local observations and are not supplier invoices or monetary spending guarantees.

## Errors

- `401 invalid_gateway_key`: missing, invalid or revoked credential.
- `403 model_not_allowed`: authenticated key cannot call the supplied model.
- `429 gateway_budget_exhausted`: completed usage reached the limit; `error.resets_at` is a Unix timestamp or null for never-reset budgets.
- `503 gateway_policy_unavailable`: credential/policy storage could not be read; no request is forwarded.

None of these responses include the credential, request content or supplier configuration.
