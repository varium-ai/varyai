# Codex model catalog import

The frontend can use a provider's returned model list to populate the Codex model mapping/catalog during provider setup. Keep the user's explicit model names, context sizes and reasoning choices; do not silently overwrite custom input during a refresh.

Claude and Codex use the same model-mapping actions: **Fetch Models** loads candidates without changing saved mappings or the default model; **One-click Setup** imports every returned model into a separate row and skips IDs already present. There is no ten-model import limit. One-click setup fetches the list first if no candidates are loaded. Codex also keeps **Add Model** for manual entries and its context-window and reasoning controls. Save the provider and restart Codex to refresh its model menu.

Generated catalog data must continue to match the backend TOML/JSON contract. The live client configuration and catalog file must refer to the same provider and model IDs. A model list response does not prove that every listed model is callable or included in an account's quota.

Generated model labels use the model name and supplier name without a VaryAI
suffix. Routing-group labels keep the group name and group designation. Codex
wire identities follow `provider-id/model-id` for direct entries and
`group/group-id` for saved groups. ASCII identities remain unchanged. If the
identity contains non-ASCII characters, publication uses a deterministic ASCII
`vary-provider/alias-<sha256>` identity instead. The picker retains its full
Unicode display label, and the receipt maps the published identity back to the
same stable internal target. The actual upstream model ID is not rewritten by
this compatibility encoding.

Codex places the selected identity in `x-codex-routing-hint`, whose string
conversion rejects non-ASCII text. Previously published identities remain hidden
compatibility entries and are never reassigned to another target. Their model
capabilities are retained. Refresh or reapply the Codex connection after updating,
restart Codex, then reselect the model inside an existing conversation. A session
that still uses an old Unicode identity can fail before its request reaches
VaryAI; restarting alone does not prove its saved selection was replaced.
Internal routing and continuation checks continue to use the stable target ID.

The shared Codex connection follows the provider identity behavior reviewed at
[Magpie revision `465928b`](https://github.com/yetone/magpie/tree/465928bccea4fec3537edd6280f7cabb9a0c5bad).
With a supported file-backed ChatGPT sign-in and an owned native gateway URL,
VaryAI keeps the built-in `openai` provider and uses the gateway's model catalog.
It removes the local catalog override from that projection so it cannot hide the
remote list. An API key alone, `forced_login_method = "api"`, or an existing
custom provider uses the standalone `varyai` provider and local catalog.
A temporarily incomplete login read preserves an established mode. Authentication
files are not rewritten by this connection adapter.

The native catalog combines the signed-in account's official models with VaryAI's
published aliases. Discovery failure leaves the VaryAI entries available. Official
model Responses and compaction requests retain their native payload and stream
through the fixed ChatGPT Codex endpoint; unknown VaryAI aliases remain local
errors. Native authentication must still match the active account and workspace,
and revoking that connection cancels its requests. Provider refresh also retains
existing capability declarations and custom headers, including table-form headers.

Codex records conversation provider identity according to its history mode.
For `paginated` history, SQLite is authoritative: a standalone connection updates
the provider row without reading, rewriting, or backing up rollout contents.
Those contents may contain only a suffix, be packed, or have fixed byte offsets;
their original provider text does not need to change. Legacy history still needs
both SQLite and rollout metadata updated, with private recovery records and backups.
Both paths move only the previous provider's quiet conversations. New standalone
connections use `model_provider = "varyai"` and `[model_providers.varyai]`.
Other providers' conversations remain separate. Disconnect restores recorded
conversations and moves chats created during the connection back to the original
provider. Active files and conflicts remain pending rather than being overwritten;
the running service retries pending work after 65 seconds, including recovery
resumed after startup. Recovery is separate
from request forwarding and does not require a running Codex application.

**Apply again** updates the Codex projection directly rather than disconnecting
and reconnecting it. A busy conversation does not block that update. If the
configuration has changed or an earlier disconnect left a pending recovery
record, explicit reapplication archives that record privately and uses the
preserved external settings as the next restoration baseline. Existing history
and recovery backups are retained. Automatic refresh still requires ownership
of the current projection.

If the model menu does not change, check the complete user-level configuration:
`model_provider = "varyai"` requires a matching
`[model_providers.varyai]` definition. An unrelated `custom` provider
cannot supply its endpoint or authentication. Missing definitions or dangling
VaryAI references require connection recovery; use **Apply again** when a recovery
record exists, or select a shared model to reconnect when the record is missing.
Restart the client after the complete configuration has been restored, then send
a new message and check VaryAI's request statistics.

Codex also keeps a model list in `models_cache.json` in its configuration
directory. That cache can still contain another manager's routing groups after
the endpoint changes. After a successful connection, explicit reapplication or
published catalog change, VaryAI expires the cache's `fetched_at` timestamp so
the next Codex load requests the current list. Disconnecting an owned connection
expires it as well. Model metadata and other cache fields are retained; an
unchanged background refresh does not repeatedly expire a fresh cache. Missing,
malformed or inaccessible caches do not block connection changes. Recovery after
another manager has taken over leaves that manager's cache untouched.
Use **Apply again**, then reload Codex through its existing restart action if
an older installation still shows foreign routing groups. An already open model
menu may need to be closed and reopened. This refresh does not import or delete
the other manager's saved routing groups.

Recent Codex CLI versions can connect to a shared app-server daemon whose version
differs from the CLI executable. Check `codex app-server daemon version` when
`/model` shows only bundled official models even though the gateway publishes
custom models. The native adapter supports 0.114, 0.165, 0.166 and 0.167; an
unsupported daemon receives `routing_native_client_unsupported`. On CLI 0.166,
`codex --no-daemon` uses a fresh embedded server without restarting the shared
daemon. Open `/model` and **All models** when the quick auto-model menu appears.
A daemon retaining older login credentials can also receive HTTP 401 after a
credential change. For the same account, its `getAuthStatus` RPC with
`refreshToken: true` performs a guarded reload before requesting a token refresh.
A successful reload can restore model discovery without restarting the daemon or
rewriting login files. Confirm the cached and stored account agree first; do not
bypass the native account check or replace credentials from a different account.
Close and reopen an already displayed model picker after discovery recovers. See the
[readable-alias verification report](../qa/2026-11-06-codex-readable-aliases.md)
for the tested CLI/daemon versions and remaining acceptance gaps.

Exiting another gateway such as Magpie does not prove that its configuration was
removed. A preexisting `openai_base_url` can remain after the gateway exits.
VaryAI preserves an unowned value and can publish its custom model menu while
native OpenAI conversation compatibility remains unavailable. Tools / Agents
shows this compatibility state separately from menu configuration. Restore the
previous gateway's connection settings before reconnecting native compatibility;
do not substitute its upstream API key for VaryAI's generated local credential.

Conversation provider migration runs in the background, as in Magpie. Provider
IDs determine the transition even when displayed names match, and a saved model
being absent from the menu does not prevent the provider change. Migration
preserves saved model identities; select an available model afterward if needed.
A busy conversation is deferred and retried without adding a connection warning
or disabling the model picker. Route changes automatically reload the desktop
background service when idle. Disconnect retains its provider definition for
historical conversations without reporting an inactive table as a connection.
If Codex itself still reports a provider mismatch, close that conversation and
allow the background retry to finish. Fully quit and reopen Codex only if its
background reload fails. Do not delete history databases, rollout files, or
VaryAI recovery records.

See the [frontend helper](../../src/components/providers/forms/helpers/codexCatalogAutoImport.ts), its [tests](../../src/components/providers/forms/helpers/codexCatalogAutoImport.test.ts), and [backend configuration](../../src-tauri/src/codex_config.rs). Test with synthetic provider data rather than internal gateway names or credentials.

For ordinary generated third-party catalogs, the configured default model has the highest catalog priority without changing row order, model IDs or reasoning capabilities. An active TOML profile's model takes precedence over the top-level model. An absent or unlisted default leaves catalog ordering unchanged. Official vendor catalog priorities remain authoritative.

The desktop client can issue independent background requests for titles and summaries. When its preferred background model is unavailable, it may select the catalog default or another model with the required capabilities. These requests are real usage and remain attributed to their requested and forwarded model IDs, even when the visible chat uses a different model. The gateway usage panel explains this distinction.

When a full inference URL is configured, model discovery removes the supported operation suffix (such as `/chat/completions` or `/responses`) and retains the provider's path/version prefix and query parameters. HTTP 429, upstream HTTP 5xx and connection failures have distinct sanitized feedback; no upstream response body or credential is displayed. An unexplained failure recording still requires the actual endpoint and failure category before its historical cause can be confirmed.

## Fast mode and service tiers

Fast is a service tier, not a reasoning-effort level. VaryAI carries the supported
`service_tiers` fields through model discovery, account-scoped caching, direct
model aliases, and the Codex model catalog. The generated local catalog and the
native gateway's VaryAI entries use the same tier declarations. Known speed tiers
also populate `additional_speed_tiers` for clients that read that field.

Explicit model declarations take precedence, including an empty tier list that
disables the GPT fallback. As in the pinned Magpie reference, a GPT or o-series
model served under a provider prefix can offer `priority` / Fast when the upstream
has not supplied a tier declaration. A published VaryAI alias is never used to
infer the upstream model family. Unrelated template models cannot donate their
speed tiers to a different model. Non-GPT models require an explicit declaration.

Routing groups advertise only tiers common to all selectable member/account
models, excluding disabled members. This is deliberately more conservative than
Magpie's any-fast-member publication: VaryAI does not yet select group members by
requested service tier. It must not promise Fast while it can select an upstream
that does not offer it.

Publishing a tier does not enable it by default, grant account access, or prove
that a relay honors it. The caller's `service_tier` remains unchanged through
native Responses forwarding and supported Responses/Chat protocol conversion.
Actual acceleration and higher usage depend on the selected upstream and account.
Reapply the Codex connection and fully restart Codex after updating so it reloads
its generated catalog; existing ChatGPT-native mode keeps official model metadata.

## Personal-account model labels

Codex model-menu labels keep readable source names separate from request
identities. Every direct model published by VaryAI includes the same source
suffix, whether or not another source offers a model with the same name. For
example, `GPT Synthetic Sol · Team Relay` and
`GPT Synthetic Terra · Team Relay` remain consistent. Custom model names that
already contain their source name are not suffixed twice.

Automatically named personal-account sources use a short service label rather
than account details: `Codex · person@example.test` publishes models with a
`· Codex` suffix. Custom source names are retained. When visible sources share
the same name, stable source-ID ordering assigns readable source numbers such
as `Codex (1)` and `Codex (2)` consistently across their models. Hidden sources
do not force visible sources to grow a number. Remaining identical custom model
or group labels use readable numbers, never internal UUIDs or request aliases.

In native ChatGPT mode, the official account's backend list retains its original
labels and slugs. VaryAI does not append another copy of an official direct model
already present there; models absent from that list and non-official sources
remain available. This presentation rule does not change account records,
authentication bindings, upstream model parameters, request aliases or retained
aliases used by existing chats. The implementation is shared by macOS and
Windows. Refresh the Codex connection's catalog and restart Codex when requested
to reload updated labels; rebuilding source alone does not update a running app.

## External connection ownership

Connection switches follow the client's current routing configuration. If
another manager, such as Magpie, replaces VaryAI's owned endpoint, provider or
credentials, the switch turns off on the next status refresh. A saved connection
intent or recovery receipt does not keep it on. Status reads preserve the
external configuration and the recovery receipt. A stopped local gateway is
reported separately while the client's configuration still belongs to VaryAI.

Turning the switch on after external takeover is an explicit reapplication. It
uses the externally edited configuration as the new restoration baseline, so a
later disconnect can restore that configuration. It does not modify the other
manager's settings. The existing reapply action remains available for recovery.

## Codex refresh hints

A refresh hint describes configuration freshness, not whether inference works.
Following Magpie's semantic startup snapshot, VaryAI records route/account identity
and the model menu separately. Reapplying identical settings, changing the
selected model or effort, refreshing credentials for the same account, and
rewriting catalog descriptions, priorities or ordering must not advance the
recorded change times. Menu identity consists of model slugs and visibility.
Only digests and timestamps are persisted; configuration and credentials are not
copied into the freshness record. The record survives a VaryAI restart. On the first observation without a semantic
record, file modification times provide a conservative baseline; a running
client may still receive a one-time manual refresh hint. Unknown startup state
never triggers an automatic restart.

The desktop app-server's creation identity, rather than the desktop window's age,
determines whether it has reloaded. A current replacement clears the desktop
refresh hint even if the window stayed open. A stale menu keeps its specific
"model list updated" explanation; it does not imply that current conversations
cannot send requests. Refreshing the menu is manual. After confirmation, the
worker waits for idle, rechecks configuration and activity, and restarts only the
verified desktop app-server. Windows may reopen the application only if the
ended service has no replacement. Whole-application restart remains a fallback
when service recovery is unavailable. No successful request alone proves that
all clients have loaded the current menu.

This freshness check covers the verified Codex desktop app-server on macOS and
Windows. It does not claim to detect every terminal CLI, editor extension or
independent managed daemon; those clients may need their own new session or
refresh.
