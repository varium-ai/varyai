# Cursor subscription provider

Open **Providers → Add → Subscriptions → Cursor**. Browser sign-in opens
Cursor's PKCE authorization page; VaryAI polls the corresponding authorization
attempt locally and verifies the returned account with Cursor before saving it.
Closing or replacing the flow cancels it. Reauthentication must match the
existing account identity. There is no callback URL to paste.

The account editor also offers **Import local sign-in** and **Import account
JSON**. Local import reads the Cursor CLI macOS keychain entry or its native
`auth.json` on macOS, Windows or Linux. It copies credentials once: changing the
CLI's account later does not change a saved VaryAI binding. JSON imports accept
Cursor CLI `{ "accessToken": "…" }`, plugin OAuth auth records, or plugin API-key
records `{ "type": "api", "key": "…" }`. Identity is checked remotely for all
imports. Files and native credentials are never edited. A plugin record using the
mutable `refresh: "cursor-agent"` marker is rejected; use local import instead.
Expired OAuth sign-ins require a new login, as Cursor does not issue a usable
refresh grant for this flow. API keys are exchanged by the protocol core.

## Runtime and storage

Install **Node.js 20.19 or later** and make it available to VaryAI's installed-tool
search before signing in. Missing or outdated runtimes produce a localized error;
VaryAI does not install Node or launch Cursor. The protocol worker is started
without a shell and hidden on Windows. The same implementation runs on macOS,
Windows and Linux.

Account credentials stay in device-local `subscription-accounts/accounts.json`.
The routing registry and renderer receive only account IDs and summaries. The
embedded protocol files are extracted to `subscription-accounts/cursor-runtime`;
Unix files are owner-only and the runtime directory is mode 0700. A private
random bearer is handed to the worker over stdin, never process arguments. The
worker listens only on an ephemeral IPv4 loopback port, rejects unauthenticated
requests, and exits when VaryAI closes its stdin. Runtime files include the
protocol license and source notice.

Cursor network calls use the Node worker's direct HTTPS/HTTP2 transport. VaryAI's
HTTP proxy setting and per-sign-in custom proxies do not apply to it; unsupported
custom proxy arguments are rejected. The worker restricts remote destinations to
Cursor HTTPS hosts and refuses redirects. No public gateway or tunnel is needed.
The integration does not change the ordinary Cursor IDE's model endpoint.

## Models, tools and usage

Discover models from the selected account. Picker IDs, context-size suffixes and
reasoning variants are preserved; unavailable catalogs remain errors. Inference
uses Cursor's Connect `agent.v1.AgentService/Run` API, with the pinned core's
HTTP1 `RunSSE`/`BidiAppend` fallback. VaryAI converts Anthropic Messages, OpenAI
Chat, OpenAI Responses and Gemini through its shared protocol representation.

Only the caller's registered tools are exposed. Dynamic calls return to that
client for execution, and linked tool results continue a retained native Run
when its owner is still available. Account, session, history, tool and execution
identity checks guard continuation. Missing owners rebuild from supplied history;
uncertain native result submissions are not replayed. Cursor-native shell/file
operations are not executed by the worker. Existing VaryAI DLP and tool-risk
checks remain on the shared forwarding path.

Allowance refresh reads the account's actual subscription windows, including
weekly/merged pools, enterprise and available credit windows. A failed read is
not converted to zero usage. Per-request token accounting preserves cache-read,
cache-write and output counts. Native tool handoffs can initially report pending
usage; later replies settle earlier steps via a separate `varyai_deferred` numeric
usage extension. VaryAI adds that extension only to persisted usage, leaving the
current reply's ordinary context-token counts unchanged. Gemini replies also
carry numeric `varyai_cache_write_tokens` when needed to preserve a cache-write
bucket that its native usage schema cannot represent. If Cursor has not
published a measured count, the pinned core can estimate it; this is not an
invoice or proof of exact billing.

## Protocol provenance and validation

The MIT protocol core is pinned to
[`@magpie-community/opencode-cursor-auth` 0.2.44](https://github.com/magpie-community/plugins/tree/401b549fb3aa56e3e8c5dfb7805a89e7e9b3158a/packages/cursor).
The vendored core is unchanged; VaryAI owns the worker transport, account adapter
and protocol conversion. See `third_party/cursor-subscription/NOTICE.md`.

`pnpm test:cursor-subscription` uses synthetic wire fixtures and private dynamic
ports. It covers native tool continuation, cross-account refusal, error/image
results, repeated exec identity, shell refusal, worker authentication, model
metadata, login cancellation, stream cancellation and worker lifecycle. Rust
checks cover account isolation and usage across all four client protocols, both
streamed and collected. CI runs the Node fixtures on Linux, Windows and macOS.
No automated check signs into a real subscription or consumes paid inference.
Live account/model entitlement acceptance remains a separate manual check.
