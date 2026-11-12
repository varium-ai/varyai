# Subscription providers

The global provider picker offers Claude, ChatGPT / Codex, ChatGPT API,
GitHub Copilot, Gemini CLI, Antigravity, and Grok. Subscription accounts are
separate from API-key vendors. Each saved provider binds an explicit account;
changing another application's default account does not change that binding.

## Account setup

Clicking any of the seven subscription tiles starts sign-in directly below
the subscription grid. The tile stays selected while its panel shows progress,
the authorization link when available, copy/reopen and cancellation controls.
Codex can switch between browser authorization, device codes and file import;
Google sign-ins and ChatGPT API accept a pasted callback URL for the current
attempt. Supported file imports are offered alongside the pending flow.
Selecting another subscription, an API vendor, or leaving the add page cancels
the pending flow. A completed account opens its provider editor with that account
selected. An existing subscription tile adds an account to its existing provider;
it does not open the editor before authentication or create another provider.

- **Claude** uses an installed `claude` executable. Browser sign-in runs
  `claude auth login --claudeai` in an isolated configuration directory. Local
  credential import copies the original sign-in without editing its source.
  Claude Code may rotate the copied credentials when it refreshes them. Its
  cached account model options and entitlements supplement the CLI aliases
  Sonnet, Opus, and Haiku.
- **ChatGPT / Codex** defaults to browser authorization with PKCE, matching the
  account panel in Magpie. The panel shows the authorization URL, copy/reopen,
  the selected sign-in proxy, a device-code alternative, file import and a
  callback URL field. An automatic loopback callback or a pasted callback can
  complete the same flow once. The browser returns to `localhost:1455`; if that
  port is occupied, the flow remains usable by pasting the final callback URL.
  Tests bind dynamic ports. State, exact callback origin/path and duplicate
  parameters are validated before the fixed token endpoint is contacted.
  Browser and device login share managed-account identity checks and targeted
  reauthentication. Cancellation or a superseded login prevents a late token
  response from saving credentials. The chosen proxy applies to that sign-in's
  token requests without changing the global proxy configuration.
- **ChatGPT API** uses the independent public-client Sign in with ChatGPT flow.
  It registers Vary AI, validates state, PKCE, ID-token signature, issuer,
  audience, expiry, nonce and granted plan-usage scope, and uses the issued
  account-specific client ID for refresh. Inference goes to the OpenAI API.
  A Codex token is never reused for this entry.
- **GitHub Copilot** and **Grok** keep their existing device authorization flows.
- **Gemini CLI** uses Google authorization or imports `.gemini/oauth_creds.json`.
  Code Assist selects the project, model availability and backend model IDs.
  Standard or Enterprise accounts can supply a Google Cloud project explicitly.
- **Antigravity** uses its own Google OAuth client and Code Assist endpoint.
  Its public installed-app OAuth parameters are bundled, matching Magpie, so
  adding an account opens Google sign-in without environment setup. Both
  `VARYAI_ANTIGRAVITY_CLIENT_ID` and `VARYAI_ANTIGRAVITY_CLIENT_SECRET` can
  optionally override that client; a partial override is rejected.
  Model discovery uses `fetchAvailableModels` for the selected account. Account
  JSON imports accept Google OAuth fields, optionally nested under `token`.

The four additional account types keep credentials under
`~/.varyai/subscription-accounts`, outside portable routing data. JSON writes
are atomic and owner-only on Unix. The renderer receives account summaries;
the routing registry stores account IDs and model preferences, never tokens.
Removing a Claude account also removes its isolated credential directory and
corresponding macOS keychain item. Removing other accounts forgets the local
credentials; it does not revoke the remote authorization.

The browser login panel retains the authorization URL, with copy and reopen
controls. If the operating system cannot open the browser, the pending flow
remains available and the panel explains how to open the URL manually. Login
failures are reported in the account panel rather than as catalog failures.

The provider list displays subscription allowance beneath each provider name,
including the remaining percentage for each reported window. Selected Codex
accounts are shown separately; API-key providers keep their endpoint and model
count subtitle alongside supported relay balances. Account editors retain their
full allowance meters, reset times and refresh controls. Both views share the same per-account quota cache, including
safe failure and stale-reading states. Providers without an allowance endpoint
show that limitation instead of an estimated balance.

Provider rows and account editors share subscription-plan labels. Codex native
sign-ins, managed accounts and ChatGPT API read the same plan claim, including
Prolite, Promax, Go, Team and education/workspace tiers. New bounded plan
identifiers retain their upstream names rather than falling through a Plus/Pro
allowlist. Existing ChatGPT API accounts recover the label from their stored
ID token without another login. Google prefers the upstream paid-tier name;
Claude reads local subscription metadata from its status or credential file.
Copilot retains both known marketing labels and new upstream plan identifiers.
Missing plan metadata never implies Plus: a known-valid account shows its
vendor, and missing or rejected credentials still take precedence over a cached
plan. These display labels do not grant model access or change forwarding.

Waiting for browser or device authorization is local to the account panel.
Model choices and the editor's Save/Cancel actions remain available. Closing
the editor stops login polling; Codex device flows and native browser flows
are cancelled in the backend as well, including a start response that arrives
after closing. The pending panel replaces Add/Import while that login
is waiting. Successful account addition selects the new account in the current
provider draft without creating another provider.

Completing sign-in from the Codex subscription tile prepares an unsaved editor
observation; it does not insert a routing provider. Model discovery and model probes accept
this draft without inserting a provider. Save persists it once, using the
draft's stable ID so a repeated save cannot create a second record. Cancel or
restart before saving leaves no newly created provider. The account credentials
are independent of the provider draft and remain available after successful sign-in.

## Execution

Google subscription requests use the Code Assist request envelope, not the
public Gemini API-key endpoint. SSE responses pass incrementally through the
shared protocol bridge. Provider- and conversation-scoped continuity retains
opaque tool signatures for subsequent turns.

Claude requests run through Claude Code with built-in local tools disabled.
An ephemeral loopback MCP endpoint lists only the caller's tools. Tool names
are mapped back to caller names; the CLI stops after the model's message and
the caller performs tool execution. Subsequent requests carry the supplied
conversation and tool results. The implementation currently starts a fresh
CLI process for each request rather than reusing Magpie's persistent runs.
Claude Code owns its system prompt and may normalize generation options.
Subscription token-counting endpoints are not supported.

Automated checks use synthetic credentials and dynamic loopback ports. They
do not establish live account entitlement, OAuth approval, or compatibility
with every installed Claude Code version. Real sign-in and inference require
validation with the user's own authorized account.

## References

- [OpenAI public-client registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [OpenAI ChatGPT plan models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- Magpie revision `1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`:
  `internal/provider/google.go`, `chatgpt_api.go`, `claude_signin.go`,
  `claude_dirs.go`, `claude_code_models.go`, and
  `internal/gateway/claude_subscription.go` / `codeassist.go`.

## Relay key allowances

Saved custom API providers and relay presets also show allowance in the list
and provider form. Like Magpie, discovery tries the configured origin's
`/v1/usage` (sub2api), then `/api/usage/token` (new-api/one-api), using the first
enabled saved key. It never uses a panel login token or asks an account profile
endpoint. Sub2api wallet amounts and 5-hour, daily and weekly limits remain
separate; a response containing only windows does not imply a zero wallet.
New-api's reported quota units are converted at 500,000 units per USD. Unknown
or unlimited balances are not estimated. The form retains window meters, reset
times and a manual refresh button.

Both list and form reuse per-provider, per-revision query results. Allowances
are fresh for 60 seconds; re-entering after that reads them again. Unsupported
relay queries are cached for six hours. Staying on the list does not poll quota
endpoints periodically. Manual refresh in the form bypasses the freshness cache.
Transient failures preserve the last successful observation with a stale status;
an explicit key rejection clears amounts. Vendor response bodies, request URLs
and credentials never appear in allowance errors. Reads reject redirects, bound
response sizes and run outside forwarding and database locks. Platform-neutral
Rust and React implementations provide the same behavior on macOS and Windows.

Reference: Magpie revision `a79d16c6843051ee75088949d6f8f611ea868dd8`,
`internal/provider/balance_probe.go`, `balance.go` and `sub2api_usage.go`.
The endpoint contracts were reimplemented for Vary AI; arbitrary balance field
expressions and panel JWT configuration are outside this change.

## Account observations and model discovery

Selecting a subscription account starts model discovery for that binding. Results
are cached per service and account; explicit refresh requests bypass the editor's
freshness cache and Copilot's model cache. A response from an account selected
previously cannot overwrite the current binding. Refreshed vendor capabilities
remain attached to picked models, while explicit model preferences are retained.

Copilot accepts partial quota snapshots and finite numeric strings. Missing
allowance remains unknown. An exhausted Enterprise premium pool stays visible
even when GitHub labels the pool unlimited. Codex quota observations retain
account restrictions, per-model availability, credit balances, spending controls
and reset-credit counts separately from percentage windows. Reading these fields
does not change spending policy. A temporary token-refresh failure retains the
last successful quota reading instead of requesting a new login.

Native account refreshes coordinate by account. Network waits do not hold the
shared account-store lock, and credential generations prevent a late refresh
from replacing a removed or reauthenticated account. Claude usage first asks the
isolated CLI for `/usage`; an expired access token does not disable a session
that still has a renewal credential. Claude inference replays typed conversation
history through a private, temporary CLI transcript, preserving tool-call IDs,
results, images and thinking blocks. The temporary replay is deleted when the
request finishes or is cancelled.

Gemini experiment observations are cached by account and project. Inference
reads that cache without an experiment-service request; a failed observation
retains the last known rollout. Quota labels use the same displayed model aliases.
Quota inspection also observes Google plan labels when a project was already
saved. Unknown model limits remain unknown.

Antigravity merges `fetchAvailableModels` model allowances with
`retrieveUserQuotaSummary` shared windows. A shared window replaces a model's
reading only when it describes the same pool and period; a weekly-only group
keeps the model's separate short window. Versioned groups such as Gemini 4 Argon
remain independent of the broader Gemini family, and a model whose remaining
share or reset differs from the shared short window stays visible separately.
Repeated readings for a shared model family are consolidated without hiding
different readings. An explicit Antigravity quota object with no remaining
fraction means exhausted; a model with no quota object remains unknown. Disabled
models and windows are excluded. The list and account form share compact pool
labels and remaining percentages rounded to one decimal place.

Pool-merging reference: Magpie `a45dd9c6573ab19cf19afd621501c9aef2e3f462`,
`internal/provider/google.go` (`quota`, `pools`, `PooledWindows`).

Managed Grok subscriptions use the CLI proxy for model discovery, Responses
inference and credit billing, including CLI request headers and context-window
aliases. A rejected token can be renewed once before retrying. The general xAI
API and the legacy native-CLI billing reader remain separate integrations.
