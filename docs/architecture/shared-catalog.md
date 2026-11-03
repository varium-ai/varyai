# Shared Catalog and Agent Connections

Vary's main application uses a resource-oriented model inspired by
[the reference implementation](../../third_party/routing-reference/NOTICE.md). Providers, model metadata, routing
plans, and Agent connections are separate resources. Adding a provider never
requires choosing an Agent, changing a current provider, or rewriting a client's
configuration.

## Resource flow

1. `routing_upstreams` stores native API providers and Codex account policies with
   revision checks and redacted control-plane responses. The retired provider
   contract is discarded, without translation or retained origins. Shared client
   configurations remain owned by their source applications. Providers enter the
   catalog only through an explicit add or external import; startup and refresh
   never register client configurations automatically.
2. The shared menu publishes direct models from every available provider, plus
   enabled routing groups. A client's current provider can prioritize its starting
   model but cannot hide other providers. Stable aliases disambiguate equal model
   names from different providers.
3. Agent connection adapters project this menu into native tool configuration.
   Claude Code, Claude Desktop, Pi, WorkBuddy, OpenCode, Gemini CLI, Grok Build,
   OpenClaw, Hermes, Qoder CN, Kimi Code, Qwen Code, and ZCode consume the authenticated
   `/connections/{agent}` ingress. Codex retains its field-owned Responses menu
   adapter and native account compatibility protections.
4. The Agent ingress resolves a shared alias directly against an immutable global
   execution snapshot. It does not look up an Agent-owned current provider. Agent
   identity, DLP inspection, optional gateway budget policy, tool observations, and
   asynchronous usage accounting remain independent of the upstream protocol.

## Provider model selection

New API providers start with an optional comma-separated model list. Editors
keep the vendor catalog separate from explicit selections, offer vendor, picked,
best-in-family and usage-only modes, and use one searchable chip list. Fetching
models updates candidates without selecting them or opening a second picker.
Names, reasoning levels, image support and context metadata remain attached to
the exact model ID. Changes are persisted only on Save.

## External provider imports

The add-provider sheet opens a source-tab dialog for CC Switch, Claude Code,
Codex, OpenCode and Provider backup. It scans only on an explicit open. Sources are read
without writes or network calls. Preview payloads contain names, endpoint hosts,
model counts, duplicate status and opaque source fingerprints; credentials and
private headers never enter the renderer. Confirmation re-reads the files and
rejects stale fingerprints before writing a batch.

CC Switch imports read its SQLite database with read-only flags (or its earlier
JSON file). Claude Code reads settings; Codex reads native TOML and API-key auth;
OpenCode reads provider settings and API-key auth. These four sources follow
the reference API-provider import scope: their subscription sign-ins are not copied.
Unsupported or unresolved settings appear as unavailable, with a safe reason.

Provider backup reads `providers.json` and sibling `logins.json` under
the source app's configuration directory (see the
[compatibility notice](../../third_party/routing-reference/NOTICE.md)). A file chooser supports a
portable folder or an exported `providers.json`/`logins.json`. API imports retain
exact model IDs, protocol-specific endpoints, enabled state, supported custom
headers and backup keys. Existing endpoint/credential combinations are skipped;
a matching vendor with a different credential can become another key or a
separate provider. A name collision is renamed or gets a numeric suffix. Backup
keys execute as separate immutable credential snapshots, preserving conversation
binding.

Saved Codex / ChatGPT subscriptions from the source app are also selectable. Their stored
Codex authorization must include a refresh token and an ID-token identity with a
consistent workspace. Tokens stay in the backend and are copied into Vary's
managed Codex account store, never into an API-key field or a native client's
login. The provider retains the saved model policy and binds to that managed
account. Reimport deduplicates by user and workspace without overwriting an
existing account's newer authorization. Import does not refresh credentials or
verify a remote login; first use may require reauthentication. Other source-app
subscription types remain unavailable until Vary has their account lifecycle.
Explicit account selection is independent of the reference implementation's saved `on` rotation flag,
which can be absent for its active native login. Paused accounts and a switched-off
provider still import disabled.

Refresh tokens can rotate. Stop using the imported account in the source app before
using it in Vary; concurrently refreshing the copied authorization can invalidate
either app's session. Import leaves the source app and native client files untouched.
Provider writes are transactional. Account-store persistence is rolled back on a
rejected provider transaction, though these two private stores do not form a
cross-file crash transaction. Treat local databases, account files and portable
exports as sensitive. Advanced source-app routing/plan/body/auth settings without an
execution equivalent are rejected instead of silently discarded.

## Configuration ownership

Connection receipts are device-local private files under `agent-connections/`.
They contain sensitive recovery material and must not be logged, exported as
portable configuration, or committed. The control plane returns status and model
identities, never these receipts or client credentials.

The adapter journals before writes. Restore compares owned fields against the
installed, original, or interrupted-refresh generations, preserves unrelated
fields, and refuses conflicting external changes. No-clobber file replacement
uses staging and quarantine files. A partial recovery can be retried without
requiring already restored fields to match the installed generation.

Claude Desktop uses a profile-aware adapter, following the reference implementation's recovery model.
Desktop 4.4212 seals gateway credentials at rest as `__sealed__:v1:` values.
Windows opens these with the current user's DPAPI and Desktop's purpose entropy;
macOS treats an opaque seal as unknown, not evidence of a changed credential.
This exception applies only to Desktop's managed profile while its gateway
address and provider still match. Missing or different plaintext credentials,
foreign gateway addresses, and malformed documents remain conflicts. Sync keeps
sealed credentials and existing policy/user settings instead of rewriting them.
Receipts created before this compatibility fix are recognized in memory without
discarding their original recovery baseline.

On disconnect, only a still-managed `3p` deployment mode is restored. The adapter
merges its own metadata entry back to the baseline, keeps other profiles and an
externally selected profile, and falls back to a remaining profile if the saved
selection disappeared. Existing legacy Vary profiles are restored rather than
blindly deleted; newly created managed profiles are removed. Non-connection
profile settings are privately retained for reconnect, without caching the
retired local key or model list. All documents validate before restoration, and
each write retains the no-clobber checks. Suspension uses the same recovery path.

Catalog changes synchronize active adapters. If a projection cannot be refreshed,
its local authorization is revoked rather than continuing to serve stale targets.
On startup, intact device-local connections can resume when the gateway starts.
Turning off an Agent does not delete a provider or disable global security.

Connection status distinguishes externally changed fields from an inactive
runtime or a missing recovery record. A saved connection intent alone is not
reported as a configuration edit. A pending connection shows the reference implementation's **Apply
again** action beside the tool name. This explicit action checks the shared
catalog before changing any connection fields, restores fields still owned by
Vary, and preserves external edits as the next connection's recovery baseline.
It then installs the current shared catalog without editing login credentials.
Background refresh continues to refuse conflicting edits. An unreadable or
malformed configuration, or externally changed owned arrays, fails without
guessing its contents or element ownership. Successful
reapplication establishes configuration and local authorization, not proof of
a real upstream response; open a new tool session to verify.

Gateway shutdown suspends managed native projections and restores their owned
fields while keeping connection intent. A later start or explicit reconnect
rebuilds the projections from the shared catalog. Explicit stop-and-restore
removes that intent. Codex stores its desired connection separately from its
field-owned receipt, so clean-exit recovery does not silently disconnect it on
the next launch. Explicit model selections override the previous pick; ordinary
catalog refreshes preserve it. WorkBuddy and Desktop expose a native picker,
not a fictitious application-wide default model setting. Qoder CN and TRAE CN also
use their native model pickers.

Starting-model selection and Desktop visibility use the same `SharedModelPicker`
panel and `ModelPickerPopoverContent` shell, not separately styled lists. Search,
provider navigation, grouping, capability badges, keyboard navigation, geometry,
and close behavior are shared. Single selection switches the starting model;
only visibility mode exposes show/hide, future-model policy, and ordering.
Disabled native selections remain read-only and opening a picker never connects
an Agent. The embedded setup picker still requires explicit confirmation.

The shared popover follows the reference implementation's compact 380px layout (580px with
more than two provider groups on desktop). Desktop uses immediate, serialized saves
instead of a draft Save/Cancel form. Search spans all providers, including when
a provider is selected in the rail. All/Shown/Order views share grouped counts;
newly hidden rows remain in Shown until the view changes. Bulk show/hide across
every provider requires a second click within four seconds. The footer controls
whether future models are automatically shown, preserving the current subset
when the policy changes. Failures restore the last confirmed snapshot and show
only a fixed localized error.

Model order is device-local per Agent, validated and persisted alongside
visibility. Dragging or Alt+Up/Down reorders shown models; unlisted/new models
follow the saved order. Default order clears the preference. Both the Desktop
profile's `inferenceModels` and authenticated model-list response use this
order. Hidden aliases remain routable for existing conversations. Editing the
disconnected model list does not connect the Agent or rewrite native settings.

### Additional native adapters

Installed tools belong in the main list regardless of their delivery mechanism.
Only missing tools remain in the installation fold. Gemini projects a URL-safe
model alias and uses the native Gemini wire protocol, including authenticated
model listing, generation, SSE generation, and local token estimation. Query
credentials are rejected. Grok and Kimi use TOML, Hermes uses YAML, Gemini uses
dotenv plus JSON settings, and OpenClaw uses JSON5. Private receipts carry the
format with a JSON default for existing receipts. TOML formatting, unrelated YAML
sections, dotenv lines, and top-level JSON5 comments are retained. Arrays are
conservative ownership units: conflicting external edits require intervention.
Unknown context capacity is not replaced with a guessed 200k limit.

TRAE uses a separate private delivery outbox, not Agent-owned provider records.
Explicit consent prepares its local bridge without launching the client or
enabling official-session audit hooks. The user opens/restarts TRAE and selects
a delivered model there. An acknowledgement is required before configuration is
reported as delivered. Offline cleanup remains pending; only this connection's
model namespace is removed. Authorization is revoked on failed refresh or stop.
An unchanged catalog is checked against native model ownership, endpoint and
credential inside the bridge; only a boolean returns. External native edits
revoke authorization and require explicit reapplication instead of silent writes.
These adapters are tested with synthetic documents and bridge callbacks, not by
changing a real installation or account.

Doubao Work has no supported third-party model injection API. Its main row is
explicitly audit-only, delegates to the existing consent-protected official
session inspection, and offers no misleading shared-model picker. Audit waiting
and active states do not imply that its model or account was replaced.

The reference was checked against the reference implementation commit
`3bce283df43c3ce733e457daa16a69ac38bbb5fd`. This is an architectural adaptation
to Vary's Rust/Tauri runtime, not a claim of matching every reference adapter,
authentication integration, or native-account relay.

## Tool versions and explicit updates

The Tools page reads CLI versions asynchronously, independently of connection
state and model catalogs. Supported installed CLIs use the existing allowlisted
lifecycle commands. Codex's desktop App version is read separately from bundle
metadata on macOS, without launching the application; ChatGPT is not reported as
the Codex App. Its version label does not offer a CLI updater for the desktop App.
Other platforms omit desktop metadata that cannot be read safely.

Update buttons are explicit user actions, never a consequence of version probing
or connecting an Agent. Two or more available CLI updates expose an Update all
bar. A synchronous UI lock covers installation preflight, confirmation and the
entire serial batch; workspace navigation is blocked while it runs. Multiple or
unanchored installations require confirmation. Failed preflight does not start
an update. The backend generates commands from its allowlist, not renderer input.

Each update is followed by a fresh version probe, including after command
failure. Success requires a readable installed version newer than the original;
an unchanged or unreadable version is explicitly unverified. Batches continue
after individual failures and retain a result summary with verified version
transitions. Raw process output, installation paths and commands are not surfaced
in the Tools page. Synthetic acceptance tests never execute an actual updater.

Missing tools expose an Install action directly in the expanded More tools list.
The nine allowlisted CLIs reuse the existing lifecycle installer after explicit
consent; the UI never accepts an arbitrary command. Installation rechecks presence
before running, does not replace an installation discovered during confirmation,
and shares the update lock for the full operation. Fresh presence and a readable,
working CLI version are required before reporting success. Installation never
connects an Agent, imports its provider configuration, or writes connection fields.
Presence-only scans cover OpenCode, Pi, OpenClaw and Hermes without executing them.

Desktop-only or otherwise unsupported installers use a separate consent message
and open the tool's official installation page. This is not reported as a completed
installation; the user installs it there and explicitly rescans afterward. Failed
or unknown presence checks never offer a potentially destructive reinstall.

## UI and verification boundaries

The main sidebar separates Tools, Routing, Models,
Statistics, and Security Center. The former provider-oriented application shell
is retained only as a test fixture for legacy configuration compatibility; it is
not bundled as an alternative production entry point.

A configured connection is not proof that the client restarted, sent a request,
or successfully called an upstream. The UI says so explicitly. The adapters never
launch or restart user applications merely to verify configuration.

Adding a provider unfolds a searchable vendor/relay sheet at the bottom of the
provider list. Choosing a preset opens its editor; closing the editor returns to
the sheet. The provider page follows the reference implementation's compact rows, host/model summaries,
separate switched-off fold, unframed vendor tiles, and a centered provider dialog.
Both new and existing providers use the same framed editor.
Search includes names, safe endpoint hosts, and model IDs; matching switched-off
providers appear without changing the user's fold preference. Credential status
indicates whether a key is saved, without retrieving or displaying that key.
The dialog header and footer stay fixed while its fields scroll. The background
list remains in place and cannot be filtered or folded while editing.
Focus returns to the provider row or add sheet when editing ends. Selected model
chips can be removed without dropping metadata from the remaining models.
The catalog view retains deselected models and their metadata for reselection.
Protocol detection and selected-model tests require an explicit click; opening
the form never sends inference requests. Saved-key probes resolve the exact
stored revision privately, and changed draft endpoints require a newly entered
key. Showing a key reveals only newly entered text.
Duplicate stages a separate provider and uses a revision-checked private key
copy on Save. Cancelling creates no resource. Unsaved edits require confirmation
before opening a copy of the saved provider.
An existing provider's header switch applies immediately, retaining unsaved form
fields and advancing the draft's revision. A new provider stages its enabled
state until Save. Selected-model protocol tests are limited to 30 models and
run in batches of three.
The inline Names & levels list stages display names, offered reasoning levels
and image support alongside the provider draft. Save applies them to the shared
menu and the actual request capability fitting; Cancel discards them. Clearing
all reasoning levels suppresses the wire reasoning field, including in groups.
Restore default removes only these overrides, preserving vendor facts, exact
model IDs and capacity metadata. Overrides have a bounded validated schema;
unknown capabilities remain unknown rather than being inferred from a name.
Account catalog refresh keeps overrides only for models still visible in that
account. Display-name changes do not change opaque continuation identity;
reasoning/image policy changes still invalidate the affected destination.
Deletion requires confirmation and leaves the editor open on failure.
Deleting an imported provider records a durable exclusion: catalog refreshes,
proxy startup, source edits, and restoration of a backup made after deletion
preserve that deletion. The original configuration remains available as an import source.
Only an explicit import restores the deleted resource, retaining its identity
with a newer revision so stale edits and deletions still fail.
Preset identity and regional choice survive redacted saves and reopening the
editor. A saved preset retains its protocol-specific regional endpoints while
preserving an explicitly customized endpoint for its active protocol.
API keys and model selection are primary fields, while authentication
overrides and raw model metadata are advanced settings. First-save discovery
preserves vendor context metadata. Large catalogs require an explicit selection
of at most 256 models. Importing an existing configuration does not connect an
Agent. Provider enablement defaults to true for older records; disabling keeps
the identity and credential but withdraws its executable models.

Each Agent chooses independently from the shared model picker. A missing catalog
links directly to the provider-add flow. Routing groups open inline, start with
no members, and use a persistent multi-picker grouped by provider. Selected
members remain checked until the picker closes. Moving a member changes the
fallback order while preserving the previous default as a fallback. Group
publication, context settings, classification and diagnostics remain available
in folded sections. Switching editors or navigating away from unsaved changes
requires explicit discard. Saving is blocked while an operation is pending.

Connected tools expose their starting-model picker directly in the row. Selecting
a model updates the actual connection; details expand inline rather than opening
a second setup dialog. A configuration awaiting recovery cannot reconnect through
that picker. Choosing a starting model while off connects directly; switching
off asks for confirmation before restoring the tool's owned configuration.
Pi's model entries preserve image support and reasoning choices instead of
declaring every shared model text-only and reasoning-disabled.

Supported preset protocols use their own endpoint rather than reusing the last
protocol's path. Paired Global/China sites retain the entered, unsaved key while
changing the endpoint, clear stale discovery and reset selected models. Protocol
changes clear stale discovery but preserve explicit model selections. These
controls also distinguish Z.ai and Zhipu pay-as-you-go and Coding Plan endpoints,
including the plan's own Responses URL, and persist the plan for later edits.
Changing the plan preserves a user-renamed provider rather than overwriting it.
The key is retained locally, not sent while choosing a region or plan. The
controls currently store one active protocol and endpoint, not the reference implementation's complete
independent multi-protocol resource schema or account/key-pool lifecycle.

The routing page follows the reference implementation's routing plugin layout: live source/gateway/
candidate stage, the selected request's ordered decision story, request and
account columns, day/session/metric filters, recorded-decision replay, and the
inline group list below. See [Routing observations](routing-observations.md) for
retention, privacy, import compatibility, and verification boundaries.

Only implemented adapters are offered as connectable. Additional tools may link
to their official installation instructions; that is not a claim of complete
reference feature or Agent coverage. JSON-only adapters reject unsupported JSONC,
JSON5, malformed, or externally changed configuration rather than rewriting it.
WorkBuddy's managed model array is owned conservatively: external edits require
recovery instead of automatic merging. Desktop's metadata has explicit profile
identities, allowing its adapter to merge only its managed entry on disconnect
without replacing other entries.

## Automatically discovered groups

Group discovery reads only explicitly registered global providers. It never imports
Agent configurations. Two distinct providers serving the same normalized model
identity produce an enabled, derived group; an explicit per-model `sameAs` override
can supply that identity without changing the wire model ID. Provider registration
order defines group/member order. Disabled providers remain in derived membership
and are marked off in executable snapshots.

The discovery preference defaults to enabled when absent; explicit off values are
retained. Saving a derived group makes it user-owned. Deletion stores a durable
hidden record with safe group name/ID metadata, and restoration removes that
record. Legacy boolean tombstones remain supported. Removed records stay visible
even when their provider models disappear.

Global refresh includes derived groups, not only persisted group rows. When
discovery is off, unlisted compatibility aliases reuse one existing direct model
snapshot and do not consume picker capacity. Shared-Agent and Codex projections
reseat selected automatic aliases to that exact direct model. Continuation state
is not transferred to a different upstream destination. This control-plane work
does not launch a listener, acquire an unowned client configuration, or introduce
request-time database transactions.

## Imported provider editing

Every provider row opens the shared provider dialog, including imported resources.
The production shell does not dispatch imported rows to the historical Agent
provider form. Import already creates an independent native resource; opening an
API connection resolves a redacted snapshot at the requested registry revision.
Opening and canceling do not change source files or credentials.

Saving retains the global resource ID and existing route references. The private
key stays in backend storage. Reusing it requires an unchanged trusted endpoint,
protocol and authentication mode; an imported protocol endpoint may be selected
when the retained key is unrestricted. Changing the destination requires an
explicitly entered replacement key, and retained private headers or backup keys
cannot silently move to another destination. Retired origin records are not
retained. Shared names, including account names, belong to the native resource
rather than following source-name changes. Reimport compares current endpoint,
credential and account identities, not historical ownership records.

Official Codex/OAuth imports now use a subscription editor rather than the
name-and-ID placeholder. Opening reads safe local account summaries and the
identity-scoped model cache; it does not renew tokens, query allowance or send an
inference request. Login, bounded Codex auth-JSON import, allowance refresh, model
discovery and model tests are separate explicit actions. Inference tests require
a completed Responses stream with the expected text, not merely HTTP success;
only the result and elapsed time return to the renderer.

Account selection/order, model mode, exact-model preferences and groups-only are
revisioned extension policy on the existing global resource. Saving does not
convert OAuth to an API key, copy credentials or overwrite the source provider.
Vendor and best-family modes publish at most 24 models; picked mode publishes
the exact selection. Usage-only and an empty account selection publish no routes.
Groups-only removes direct listings while permitting explicit group members.
Known per-account catalogs constrain that account's targets. Catalog discovery
is account-scoped; changing the first account never relabels the previous
account's discovered list. Explicitly typed model IDs remain possible when no
authoritative catalog is available.

Selected accounts expand into immutable targets of the same provider/model.
New requests prefer configured account order, moving known exhausted accounts
behind the remaining accounts. Transient failures can try another account before
any reply is delivered. A response or tool continuation retains its exact
answering account and never silently follows a changed default login. Official
subscription traffic retains its Responses-only, stream/store requirements.

Other custom transports retain their original authentication and connection
settings in the shared frame. Imported connections must be saved before they can
be duplicated as independent API resources. Editor failures expose only sanitized
codes; no source configuration, credential, response text or vendor error body
returns to the renderer.

This does not establish complete subscription parity with the reference. Separate image-generation model
selection, account caps/concurrency, reset-credit actions, per-account model
restrictions and the full advanced account controls remain unimplemented. Native
Codex keyring/ephemeral stores are not treated as readable file credentials.
Synthetic acceptance uses temporary stores and dynamic-port inference fixtures;
it does not sign in, consume a real subscription or restart an installed app.
