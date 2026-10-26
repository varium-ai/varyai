# Gateway keys, agent policies and usage

Open **Settings → Advanced → Local routing → Gateway keys**. The four tabs separate key management, local agent bindings, usage and connection examples. The service activity cards below this panel describe all local routes, not an individual key's token usage.

## Create and manage a key

The Keys tab lists existing keys. Select **Create key** to open its form. Give the credential a name such as “Desktop testing” or “Project A”. This is a label, not an agent or a running model.

New keys explicitly allow all models by default. To restrict a key, turn that option off and select request model IDs from the configured provider catalog, or enter exact case-sensitive IDs separated by commas. A restricted policy needs at least one ID. Older keys retain their original allowlists; they are never silently widened. Model policy checks the client request before provider model mappings, so Claude Desktop uses its Claude-safe route IDs rather than the mapped upstream model name.

Set an optional token budget and reset period. An empty budget is unlimited; zero blocks immediately. The generated secret appears once and can be copied until hidden. Only its SHA-256 hash is stored in `vary-ai.db`. Existing secrets cannot be recovered. Editing a policy does not replace its credential or clear consumption. Revocation permanently blocks that credential, including agents still bound to it. Unbind the agent or select another key to restore its routed access. Already admitted requests can finish.

Each key card shows its allowlist, usage in the current period, remaining tokens, reset time, bound agents and availability. **View usage** opens the filtered Usage tab.

To permanently remove a key, revoke it first and remove all of its bindings in **Local agents**. Its card then offers **Delete**. The confirmation identifies the key and explains that deletion permanently removes the credential, period counters and all per-model usage records. Other keys are unaffected. Bound keys cannot be deleted, so removing a credential cannot silently remove an agent's policy. Usage arriving from an already admitted request after deletion does not recreate its history.

## Bind a local agent

In **Local agents**, choose a key for the agent. Agents sharing a key share its budget. Client credentials and provider settings remain unchanged: the gateway identifies managed requests by their existing route and applies the binding before forwarding. The secret is needed only by external clients, not by this local binding.

Enable the agent's local route below the key panel and restart the client when its integration requires it. **Start local gateway** starts the listener without rewriting every agent's configuration. The panel distinguishes binding from route activation. Direct traffic bypassing VaryAI is outside the policy. Claude Desktop must have an applied provider in model-mapping/proxy mode; starting the shared listener alone does not connect Desktop.

| Agent | Coverage |
| --- | --- |
| Claude Code | Routed Messages requests |
| Claude Desktop | Dedicated Desktop Messages route; existing Desktop authentication remains required |
| Codex | Routed Chat, Responses and compact requests |
| Gemini CLI | Routed generateContent and streamGenerateContent requests; model ID comes from the URL |
| Grok Build | Dedicated Responses and compact routes |
| Pi | Provider-scoped Chat, Responses, Messages and Gemini routes |
| WorkBuddy | Routed official/custom Chat requests |
| Qoder | Routed custom models and the intercepted official model endpoint |
| Doubao Work, Kimi, Qwen, ZCode | Existing native custom-provider routes |
| TRAE | Custom-provider routes; official traffic observed only through the audit bridge cannot be blocked |
| OpenCode, OpenClaw, Hermes | Current provider configuration is direct. No automatic binding; the UI states this explicitly. A manually configured supported external gateway connection can use a key. |

Budget policies do not turn an observational audit bridge into a forwarding gateway. Non-chat image/search operations on a bound Codex route are refused as `gateway_operation_not_supported` rather than escaping the policy without accounting. Model-list and token-count probes do not consume the generation budget.

## External client connection

The **Connect** tab generates a request example using the chosen key's model policy and selected client protocol. Replace `YOUR_GATEWAY_KEY` with the saved secret and `MODEL_ID` with a permitted request model. It does not change a provider or agent configuration.

The default local origin is `http://127.0.0.1:15721` (use the configured port). Supported POST endpoints:

| Endpoint | Provider namespace |
| --- | --- |
| `/gateway/v1/messages` | Claude |
| `/gateway/v1/chat/completions` | Codex |
| `/gateway/v1/responses` | Codex |
| `/gateway/v1beta/models/<model>:generateContent` | Gemini |
| `/gateway/v1beta/models/<model>:streamGenerateContent?alt=sse` | Gemini |

Use `Authorization: Bearer <key>`, `x-api-key: <key>` or `x-goog-api-key: <key>`. Gateway authentication headers are removed before existing provider routing supplies upstream credentials. No supplier credential is needed in the script. This namespace always requires a gateway key. Existing local client routes retain their native authentication behavior and are limited only when bound to a policy. The listener stays loopback-only.

## Usage and budget semantics

The **Usage** tab filters by key and shows up to 200 recent aggregate rows by key, admission period, agent, client request model and forwarded model: completed request count, reported tokens, requests without usable usage, and last observation time. For a Desktop mapping such as `claude-sonnet-fixture → synthetic-upstream`, the forwarded model is `synthetic-upstream`; the client request model remains the ID checked by the allowlist. Forwarding records the resolved outbound model after mapping and successful provider selection, independently of a supplier's echoed model name. Changing a mapping later creates a separate aggregate rather than relabeling past usage.

Updates poll every five seconds. Missing usage is visible but cannot be charged an accurate token amount. Older totals and detail rows remain intact after migration. Records that did not capture a forwarded model display **Not recorded**; the current configuration is never used to guess a historical model. Only identities, model IDs, counters and timestamps are persisted, not prompts, responses or secrets.

Budgets count input + output + cache creation tokens, plus cache read tokens only when input excludes them (Anthropic Messages). OpenAI prompt/input counts already include cache reads. Reasoning tokens included in output are not added twice. Counters work independently of ordinary proxy logging.

Daily periods reset at 00:00 UTC. Monthly periods reset at 00:00 UTC on the first calendar day. Never-reset counters have no expiry. UI timestamps use the local display timezone. Usage belongs to the period in which a request was admitted. Changing only a limit preserves consumption; changing its period selects that period's counter.

This remains a completed-usage admission limit, not prepaid reservation or a hard cap during generation. In-flight requests, long responses and asynchronous persistence can exceed the budget. Subsequent requests are refused after completed consumption is persisted at or above the limit. Streaming bytes pass through without waiting for accounting. Disconnects queue usage already observed. Responses or SSE lines beyond the observer's 1 MiB bound may be unmetered. Process exit or persistence failure can lose pending observations; accounting failure does not interrupt forwarding. These local token observations are not supplier invoices or monetary spending guarantees.

## Errors

- `401 invalid_gateway_key`: invalid/revoked external credential, or a revoked key still bound to an agent.
- `403 model_not_allowed`: the request model is outside the key's allowlist.
- `403 gateway_operation_not_supported`: the bound policy cannot account for this operation.
- `429 gateway_budget_exhausted`: completed usage reached the limit. `error.resets_at` is a Unix timestamp or null for a never-reset budget.
- `503 gateway_policy_unavailable`: the policy store cannot be read; nothing is forwarded.

Errors contain no credential, request content or supplier configuration.
