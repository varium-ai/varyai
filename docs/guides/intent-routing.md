# Global upstreams and routing groups

## Resource ownership and publication

The routing page header provides a primary **Create group** action and the account visibility toggle. Global routing groups appear below the live overview, without a duplicate creation action in the group toolbar.

Catalog refresh updates available models without inserting per-provider refresh results into the group list. **Create group** opens the template chooser from an empty or populated list. After the catalog loads, selecting a template or **Custom** with fewer than two available task models opens a warning instead of a draft. **Cancel** returns to the chooser, and **Add provider** in the routing workspace opens provider setup. Models from the same provider can count separately; classifier-only models do not count. Template-specific requirements remain visible as guidance, and saving still validates the draft.

Open **Plans / Routing** from the sidebar. Manage independent global connections in **Models / Providers**, then connect clients from **Tools / Agents**. Groups retain stable resource IDs and exact model IDs; no Agent owns a newly added API provider. Existing Agent-owned configurations remain reference-only compatibility sources. The shared native-client catalog and authenticated external gateway access are separate consumers of these resources. See [Shared catalog](../architecture/shared-catalog.md) and [Routing observations](../architecture/routing-observations.md) for the current control plane; [reference parity](../design/reference-parity.md) records remaining differences rather than claiming complete parity.

- Create, edit and delete up to 32 groups, each with up to 256 members and 16 intent rules. Removing a member promotes each rule's next surviving target and drops only rules with no targets.
- Add global providers explicitly in **Models / Providers**, or manually import a supported existing Agent configuration from the add sheet. Startup, catalog reads and proxy refreshes never import saved Agent configurations automatically. Imported references read the source provider's stored configuration; they do not copy managed OAuth tokens or change its live client files. Existing identities, including legacy Codex IDs, remain stable so saved groups and direct-model aliases remain usable. Names or URLs never merge accounts automatically.
- Group members reference global upstream IDs. The persisted JSON key remains `providerId` for compatibility. Older `client:"codex"` documents remain readable; saving normalizes them to `client:"global"`. The field no longer determines ownership.
- Choose the default and explicitly approved fallbacks. The planner deduplicates candidates and retains each exact upstream model ID. Classifiers support API-key upstreams using Chat Completions, OpenAI Responses, Anthropic Messages or Gemini Native. They also support selected models from a bound WorkBuddy China or WorkBuddy AI subscription.
- **Publish to gateway**, followed by **Save**, adds an executable group to connected Agent model pickers and makes it available through authenticated external access. Turning this off withdraws the group from both, including the Codex picker in the ChatGPT app. Existing key allowlists still apply. Automatically discovered groups are published when discovery creates them. Publication leaves disconnected Agents and a stopped listener in their current state.

API-key task upstreams support explicit `openai_responses`, `openai_chat`, `anthropic` and `gemini_native` formats. Native official and managed Codex accounts retain their identity-frozen executor and account discovery; static API upstream creation does not convert them into ordinary keys. Unavailable sources remain visible in the registry but are omitted from executable snapshots. Drafts retain existing unavailable upstream identities so they can be edited without losing references; execution requires repairing or replacing unavailable selected targets. Offline validation remains API-key-only. None of these checks proves tools, images, reasoning capability or inference permission.

The routing provider selector never returns stored API credentials. New API connections are edited inline in **Models / Providers**. Imported rows open the same provider dialog. Supported static connections become independent shared API resources on explicit save, keeping their global IDs and leaving original Agent configurations unchanged. Account authentication and custom transports remain attached to their existing source; their shared names and enabled state can be changed in the same dialog. Stored credentials never enter the renderer. Deleting a source provider makes its targets unavailable instead of redirecting them to another provider. Deleting an imported reference leaves its original source configuration intact and does not restore it automatically. Only a manual reimport restores that reference, keeping its identity and increasing its revision.

## Saving and synchronization

Saving waits for local persistence, global route publication, connected-client
catalog synchronization and any active Codex menu refresh. It does not activate
disconnected clients. These steps remain ordered; a failed publication still
revokes stale targets. Public model facts use immutable lookup indexes.
This reduces repeated preparation without delaying route
activation or moving client synchronization into an unobserved background task.

Runtime logs report only elapsed milliseconds for persistence, provider
preparation, group definitions, executable targets, client synchronization and
Codex refresh. They do not include group names, model IDs or credentials. See
[save performance verification](../qa/2026-11-12-routing-save-performance.md) for
the isolated timing fixtures and verification limits.

TRAE catalog delivery does not prepare native connections or rewrite launch
arguments. Explicit connect/reapply actions prepare them; the existing recovery
loop reconnects previously authorized desktops outside catalog switch locks.
Offline deliveries remain pending until acknowledged. See
[live save diagnosis](../qa/2026-11-12-routing-save-live-followup.md).

## External consumers

Start the loopback gateway explicitly and configure a gateway key under **Settings → Routing → Local routing**. Give restricted keys the group's public alias, not its concrete member model. Authenticated `GET /gateway/v1/models` lists only executable published groups permitted by that key. Use the same alias on `/gateway/v1/responses`, `/gateway/v1/chat/completions`, `/gateway/v1/messages`, or the Gemini generate/streamGenerateContent route. See [gateway keys](gateway-keys.md) for headers, budgets and manual connection setup. This version does not automatically take over every agent.

Protocol selection belongs to the request and chosen upstream, not to the consuming agent. Same-protocol requests preserve native fields and JSON lexemes except supported model/redaction/effort edits. Cross-protocol requests use the explicit bridge; unsupported semantics fail before forwarding instead of being silently discarded. Opaque state is never moved to a different account merely because another member is healthy. Official Codex destinations retain their native Responses constraints.

The stream bridge can reconcile a Responses reasoning item's streamed text with its final `summary` or typed `content`. It checks consistency with text already streamed and requires a valid upstream terminal event before reporting successful completion to the client.

The executable global catalog exists independently of Codex activation and per-agent current-provider selection. Resource changes invalidate its generation immediately; control-plane refresh rebuilds snapshots off the forwarding path. Database restore/import invalidates old snapshots and resets imported publication flags; explicitly publish the intended groups again after reviewing imported resources. A failed refresh revokes stale targets. Group aliases are not passed to ordinary upstream routing when the group is unpublished, unavailable or stale. Account health is shared, while continuation state is scoped by authenticated gateway key and conversation. Native Chat, Messages and Gemini tool/opaque continuations require an observed pinned destination within the same consumer and conversation. The authenticated TRAE Chat adapter also accepts unnamed tool history when every call resolves to one previously observed destination in its Agent and alias scope. When TRAE rewrites a call ID, the assistant function name and JSON arguments must match a complete observed payload by hash; tool results alone cannot establish this mapping. TRAE rewritten-call ownership is never inferred from message text. Responses continuations can locate their consumer-scoped conversation by known response or call IDs without a separate session header. Unknown/conflicting response or tool IDs fail closed. At most 1024 consumer/conversation scopes are retained, with a one-hour inactive lifetime; active streams retain their lease. In-flight requests may finish after an edit or revocation. Eligible normal gateway and Codex-menu conversations can restore exact ownership after restart from the private continuation store described below; this does not reconstruct upstream history or decrypt foreign opaque state.

## Compact list

The group table shows **Group name**, **Models**, **Requests** and **Actions**. The Models column counts all explicit and pattern-matched model members, including separate members on the same upstream and members currently disabled or unavailable. Requests retains its unavailable placeholder until group totals are supported. Actions contains the publication switch and Edit/Delete icons. Open Edit to change the models, order or rules. The import button, per-row strategy selector, More menu and Removed groups entry are absent. Group IDs and public aliases are hidden in list rows; aliases remain searchable and retain their stored values for API access. Enabled row actions show a pointer cursor. Hovering or keyboard-focusing Edit uses the theme accent with a faint accent background. The toolbar status filter remains available. Provider stacks use the same artwork renderer as the provider list, including uploaded images, registered image assets and built-in icons. Providers without artwork use a neutral icon.

Conversation identity and concurrent forwarding follow Magpie revision
`4869d0c9665e1a815fee3a1a4d67a575e0baab93` (`gateway.go`'s `affinityID`,
`conversationID`, `conversationOpening` and `stickKey`, and `affinity.go`'s
short-lived affinity locking). Native session headers take precedence, followed
by `X-Magpie-Session`; without either, the opening is hashed using the reference
normalization. Shared environment/instruction messages include the subsequent
user-owned opening so independent conversations in one directory remain distinct.
Claude subagent IDs separate their affinity from the lead. `prompt_cache_key`
remains an unchanged upstream cache parameter and does not identify a local
conversation. In-memory state is locked only for lookups and updates, never
while awaiting classification, credentials, upstream headers, or stream data.
Concurrent requests, including requests within one named session, can forward
while earlier responses remain open. Known ownership collisions, authenticated
consumer boundaries, native checkpoints, and security inspection remain enforced.

Routing groups use a table with a full group name that wraps when needed, a compact member summary and publication/edit/delete controls in Actions. Separators appear only between rows; a single result has no bottom divider. The table fits the available workspace width without a fixed minimum. The model and request columns are 80 px each, the action column is 108 px, and the name column shrinks with the workspace while long names wrap and member summaries truncate. All four columns remain visible at the application's 900 px minimum window width. The Actions header and cells stay fixed to the right edge. Action controls always stay on one line. The action column stays compact, leaving more space for the group name. Continuous narrow inset shadows, following Ant Design's fixed-column treatment, appear only where more table content can be scrolled into view. The drag grip stays inside its icon button. While dragging, an inert row preview preserves the measured column widths, full row height and fixed-action alignment. The live table keeps its layout; Escape, pointer cancellation and resizing remove the preview and restore the row. Vertical edge scrolling uses the workspace rather than the horizontal table viewport. Rows use a neutral global-resource icon rather than assigning ownership to an agent. Filter locally by group name, exact model ID text or provider name; filtering never changes saved groups. Editing and confirmed deletion remain available per row. Saving edits to an automatic group creates a user-owned group override. Member information checks supported task transport and locally available official/OAuth identity, rather than the older API-key-only offline validator. It does not prove inference permissions or remote availability.

Group readiness is based on whether at least one participating member is locally
available, including members resolved from model patterns. Switched-off members
do not count. Once the provider catalog has loaded, a group with no ready member shows
**No member ready** in the list. A partially available group has no group-level
warning. In the editor, unavailable members remain selected and removable, with
separate explanations for a provider missing from configuration, a provider not
ready for task routing, or a model absent from its provider's current model list.
An omitted model list is not treated as an empty list. These are local readiness
checks, not live probes of credentials, quotas or remote service health. This
presentation does not change forwarding or fallback behavior and is shared by
macOS and Windows.

Click the group name, member summary, blank row space or **Edit** to open its editor. **Create group** opens the template chooser first. The compact list remains in place behind the dialog. Publication toggles, drag handles and delete icons keep their own actions. The dialog is capped at 880 px wide with a scrollable body and persistent Save/Cancel actions. Group deletion is available only through the list's trash icon and its confirmation dialog; the editor footer has no group deletion action. Focus starts in the group name and returns to the row's Edit button or the creation trigger after closing.

Custom drafts start with an editable, distinct group name. The publication
switch also works for automatically discovered groups and retains discovery
as their membership source. The live stage labels the session's name as the
conversation title.

When an Agent sends requests to different routes at the same time, each
route has its own source and attempted upstream cards. The cards show the
requested route during concurrent activity.

Use **Manage** beside **Refresh** only for list selection and bulk removal. It does not reveal templates or settings. Select individual rows or use **Select all** for the current filter, then choose **Remove selected** and confirm. **Cancel** exits management without removing anything. The management button remains visible but disabled when the list is empty and becomes available after a group is added.

Choose one of four templates or **Custom** in **Create group**. Template-created
groups hide **Advanced settings** during both creation and editing, display the
template's **Running rules** directly on the dialog, and keep the template's
preset settings. The
task-classification draft has no separate member or model-order controls: choose
a task model directly for each rule, or choose the default model when there is
no automatic task split. Choose the classifier separately in **Intent told by**.
The **Models** count in the routing group list includes a configured
classifier when its supplier and model combination is absent from the
explicit and pattern-matched members. A classifier shared with a task
member is counted with that member.
For smart groups with rules, the suggested list and saved group contain the
rule targets shown in the editor. Reusing the same supplier and model across
rules counts as one member. The hard-task target becomes the default, and the
remaining targets can serve as fallbacks. Saving a legacy smart group with
rules applies the same member selection. When there is no split, changing
the default keeps it as the only member.
The classifier and rule targets use the same searchable picker layout. Scroll
within the picker to browse models. Escape closes it and returns focus to its
button.
Rule descriptions wrap in full. On narrow forms the model picker sits under
its rule description.
Select the information icon beside the classifier to read about task text and
potential charges. Press Escape to close the explanation.
The other three templates prefill up to three model members. The numbered list
shows their priority during creation and editing, including for capability
presets saved before the template key was stored. Drag the grip at the left
of a row to move it, or focus the grip and press Alt+↑/↓. The move buttons
are also available. Escape
cancels an active drag without closing the editor. **Add model** appends another
eligible model, including another model from the same provider, without
rearranging existing members. The three-model
limit applies only to the initial suggestions. Use **Custom** to edit advanced
settings or run a classification trial. Cancel discards
the draft. The chooser waits for the initial model catalog load before opening
an editor.

Each opening of the template chooser draws new suggestions. Choosing a
template copies the displayed models into the draft. Its default and fallback
order follows the template's ranking. All four templates prefer different
model identities across suppliers.
The comparison normalizes the configured equivalent model ID or, when no
equivalent is configured, the catalog model ID. Each selected route keeps its
exact supplier and model ID. When distinct choices are scarce, another
supplier can serve the same model as a fallback. The speed template still uses
one model per supplier.

- **Strong models for hard tasks, cheaper models for easy tasks.** Confirm
  the suggested primary, fallback and cheaper model. Image requests with
  explicit capability differences and high reasoning use the strong model.
  The intent classifier helps identify simple tasks. Unclear results use the
  primary order. Other members can still serve as backups. If catalog prices
  do not identify a cheaper model, the draft has no automatic task split.
  Automatic suggestions skip the Codex automatic approval review model.
  Set the classification deadline and test the selected intent model in
  the same editor. New drafts use a 10,000 ms deadline.
  The built-in hard and easy rule descriptions follow the interface language.
  The classifier uses the saved rule descriptions, including any custom text.
- **Most capable first.** The initial list follows the template's recommendation.
  Replacing a model keeps its position. Select **Apply recommended order** to
  rank the current members again.
  [Vendor naming rules](routing-model-naming.md) distinguish flagship,
  standard, and efficiency lines. Within a family and tier, known editions
  and numeric versions set the order. Across families, catalog metadata
  sets the order rather than a benchmark score. Unknown names remain neutral,
  and models outside the template catalog go last. Tied models follow the
  catalog order each time the recommendation is applied. The action makes
  no model requests. Adjust the list before saving. The first member becomes the
  default and the rest become fallbacks in order.
- **Fastest first.** Choose models whose outputs you can use. Recent time to
  first content and output speed drive new selections. Without measurements,
  the model order provides the starting point.
- **Expiring allowance first.** Try ready Sub2API accounts with a confirmed
  positive allowance in earliest expiry order. Next try ready Codex and Claude
  accounts in descending order of weekly allowance percentage left per hour
  until reset. When there is no weekly allowance, a confirmed 5-hour-only
  account uses the same calculation for its short window. A full 5-hour
  window makes the account unavailable for new selections. Select **Apply
  recommended order** to restore the template's catalog-based fallback
  order for the models already selected. Live quota still determines
  request priority.
- **Custom.** Use the regular group editor to select models and set a default
  member or rules in Advanced settings.

When a WorkBuddy subscription account explicitly reports that it lacks access
to a selected model, an unpinned request can try the next eligible group
member. Requests pinned to a destination retain the subscription error.

In **Advanced settings → Rules**, select the information icon to see how match
conditions work. Select **Add rule** beside it to start editing a rule. Each
rule shows its preferred member and the fallback models in the order they will
be tried. A pause rule shows the member it pauses. Select a model or
**Add fallback** to open the same searchable picker used by the templates.
Search by model name, model ID or supplier, and scroll within the list.
Drag a rule by its grip or press Alt+↑/↓ while the grip has focus to
change rule priority. The rule's action menu also provides move controls.

The editor and tray no longer expose a scheduling selector. Previously stored
retired strategy values remain readable and normalize to **In order**.
The request graph shows candidates without strategy labels. Sharing an expiry
group uses the VaryAI format so the expiry preference survives import.

Row operations track pending state by group ID. Publishing or deleting a group disables only that group's conflicting controls. Other rows, templates and the toolbar remain available during background synchronization. Saving closes the editor after persistence and keeps only the saved row busy until synchronization finishes. Completion or failure of one operation never releases another operation's controls. Membership and ordering writes are serialized against the latest catalog so concurrent removals and reordering retain all current group IDs. Sorting temporarily disables drag handles while other row actions remain available.

Reload only shows a spinning icon on its own button and prevents repeated clicks
until catalog and list refreshes finish. Management, filtering and group
editing remain available. An open draft keeps its unsaved edits, and completing
a reload does not unlock a separate save or deletion that is still running.
While loading, the refresh arrows become a circular progress indicator with a
faint full-circle track. This keeps the visible outline centered throughout
rotation, rather than rotating the arrow tips beyond the circular outline.
The indicator uses the same fixed square as the idle icon, including the compact
toolbar where button labels are hidden. The arrows return when loading ends.

While removing selected groups, the confirmation stays open and its removal button shows a loading indicator. Confirmation, cancellation, Escape and backdrop dismissal are disabled until removal and the list refresh finish. Successful removal closes the dialog. A failure keeps it open with a sanitized error and allows retrying only the remaining groups.

Save closes the dialog after persistence succeeds. Failed saves retain the draft and show a sanitized error. Cancel, Escape or clicking outside discards unsaved changes; saving blocks dismissal. Delete opens a separate confirmation dialog. Closing unmounts the editor and revokes its temporary execution sessions. While editing, list shortcuts and automatic-discovery toggles remain disabled to prevent conflicting configuration changes.

## Automatic group discovery

The provider-exclusion control opens a compact checklist below the discovery description. Select multiple providers without reopening it; changes save immediately. The panel scrolls when needed and stays within the window. Long provider names are truncated with the full name available on hover. Escape closes the panel and returns focus to the control.

The automatic-discovery switch and provider-exclusion controls are temporarily hidden from the routing page. Their saved preferences remain independent of proxy activation and default to off when no preference exists. Opening or managing the list does not change an explicitly saved on or off preference. When enabled, the list derives enabled groups from explicitly registered global providers using their saved `modelCatalog.models[].model` entries and account-scoped official Codex model discovery caches. **Refresh** reloads the saved list and confirms completion. **Sync models** fetches upstream catalogs and remains available with automatic creation off. Neither action runs inference.

Registered executable upstreams participate, including available official and OAuth Codex accounts. Imported static Codex connections also contribute their explicitly selected TOML model without rewriting provider storage. An empty API model selection follows its cached vendor list; a nonempty saved selection remains authoritative. With automatic discovery on, entering Routing loads missing model lists off the forwarding path, with four concurrent catalog preparations and a five-minute UI freshness interval. Official accounts fetch their own visible models from the fixed ChatGPT Codex models endpoint, never from a public model list or another account. Opening a signed-in official provider with no cached models also loads the selected account's catalog. Missing login, fetch failure, and successful empty results have separate safe messages. These are model-list reads, not inference probes or automatic logins.

Discovery normalizes vendor spelling following the reference implementation's `sameModel`: lowercase, remove vendor path prefixes, collapse spaces/underscores/dashes, equate digit-separated dots or `p` with dashes, and remove compact six/eight-digit snapshot suffixes. Dashed OpenAI dates, four-digit releases and variants such as `:batch`, `-thinking` and `-flash` remain distinct. This is spelling normalization, not inferred semantic equivalence. In the provider editor, **Names & levels → Same as** explicitly supplies a different model identity for grouping when vendor IDs cannot be normalized together. This also fills missing public catalog facts and context capacity, without replacing declared limits or image/reasoning overrides. Every member retains its exact original model ID for forwarding. These model preferences are staged until the provider is saved, and can be restored to vendor defaults.

Duplicate rows from one provider count once. At least two distinct providers are required. Groups and members follow provider registration order; duplicate model rows within one provider count only once. Temporarily disabled providers retain their members in the derived group but are skipped by execution. Storage IDs remain hashes; public automatic aliases are readable, for example `auto/gpt-5.6-sol`. Catalog display names are preferred over raw IDs, including names retained in account-scoped official caches. Existing saved groups without a public alias retain `vary-auto/<group-id>`. Legacy exact-ID manual overrides and deletion records suppress corresponding normalized auto groups, avoiding duplicate or resurrected entries.

Automatic groups are not persisted as separate configuration rows. Editing and saving one creates a user-owned override that survives switching discovery off. Deleting an automatic group or its saved override records a hidden ID, so refreshes and off/on cycles do not recreate it. Deletion records and archived definitions remain in storage even though the routing page no longer exposes a Removed groups or Restore control. Switching an untouched automatic group off asks to remove it; saving any edit creates a user-owned override. **Leave out a provider** excludes selected providers from discovered groups only, preserving direct models and user-owned group members. If exclusions remove an automatic group, an unlisted single-provider stand-in keeps its old alias usable, preferring a provider not excluded. Catalog changes are reflected when the list reloads; use **Reload** to refresh API and official account model lists. Existing user overrides are never regenerated from the catalog. Having several providers does not guarantee an automatic group: at least two must actually offer the same model.

The combined list shows at most 32 groups, with saved groups taking priority. Groups exceeding the validated 256-member limit are omitted instead of silently discarding members. The interface reports the number omitted. Import/restore turns automatic discovery off while preserving hidden IDs and saved drafts. Discovery never starts the listener or connects an Agent. Enabled discovered groups enter the global execution snapshot alongside direct provider models, independently of Codex activation. Switching discovery off keeps user-owned groups, removes discovered groups from pickers, and reseats connected Agents to the corresponding model from the first available provider. Requests still naming an automatic alias resolve to that one provider, even when only one provider now remains. Opaque continuations are never remapped across changed destinations; an incompatible continuation still fails closed.

## Group scheduling

The experimental four-policy UI and the seven retired legacy schedulers (`smart`,
`round_robin`, `least_used`, `pace`, `free_first`, `weighted`, `manual`) have
been removed. Previously saved experimental `policy`, member `weight`, and
`manualMemberId` metadata are accepted only for reading and omitted from saved
or exported configuration; retired strategy values (`expiring`, `smart`,
`round_robin`, `least_used`, `pace`, `free_first`, `weighted`, `manual`) are
read as **In order**. Original rules, members and execution settings remain
intact.

Automatic groups and saved groups without a scheduler value use **In order**.
The runtime supports two scheduling modes plus optional expiry sorting:

- **In order:** try the configured default and approved fallbacks in order (used by **Strong models for hard tasks, cheaper models for easy tasks**, **Most capable first**, **Expiring allowance first**, and **Custom**).
- **Speed first (`fastest`):** compare streamed first-content latency plus the estimated time to write 200 tokens (used by **Fastest first**). Output rate excludes hidden reasoning. Buffered bursts and non-streaming replies provide no output rate. Ten-percent bands retain configured order. Every fifth eligible selection rotates a route lacking a first-content measurement, or lacking an output-rate measurement when another candidate has one, to the front. These probes use ordinary user requests. Measurements belong to the exact upstream and model. A test of one account supplies measurements for that account.

Both modes apply the ready/resting partition after rule, affinity, and cost priorities. Only approved candidates participate. An intent rule's selected ready member takes priority over other ready members. Continuation/account binding overrides scheduling. Ambiguous failures and bound requests are never replayed at another account.

The live routing graph explains when a request must continue at the same
upstream, or when conversation affinity gives the previous upstream priority.
Its candidate count describes the current request.

Expiry sorting is optional group configuration. It applies when the gateway
can select a new destination. A fresh, positive Sub2API subscription allowance
with a confirmed expiry ranks by date. Codex and Claude accounts with known
remaining allowance then rank by percentage left divided by hours until reset,
with a one-hour floor. The tightest applicable weekly window wins when Claude
reports both account-wide and model-specific allowances. A 5-hour window
ranks by pace only when the account's usage response confirms there is no
weekly allowance. An exhausted applicable window is excluded for a new
selection, even if an editable rule prefers it. Exact continuations retain
their existing ownership. Unknown or stale data falls back to the configured
model order. Ties retain that order.

Reads are asynchronous and bounded. Codex uses observations for the frozen
account and refreshes through its existing ChatGPT usage endpoint. Claude
uses the bound account's structured usage response so a CLI percentage
without a reset time cannot become a pace score. Sub2API observations are
scoped by relay origin and API key. Wallet balances and credential expiry
carry no priority. [Sub2API usage implementation](https://github.com/Wei-Shaw/sub2api/blob/main/backend/internal/handler/gateway_handler.go)

Other quota data comes from the official fixed `/backend-api/wham/usage` endpoint, response headers and supported rate-limit events. Samples expire after five minutes or their reset. Exact-account/model speed averages are in-memory, bounded by published sessions, and reset on restart. Speed samples use a twenty-minute half-life and expire after an hour. Preparation and telemetry failures never interrupt forwarding; no database access or usage-history scan is added to selection. Direct entries and groups share telemetry. Compatible refreshes preserve bindings and telemetry across strategy and cost changes; destination/account changes do not inherit them.

## Official account discovery and cache

- An unbound **OpenAI Official** card follows the native Codex `auth.json` ChatGPT access token and workspace; it does not silently use the managed OAuth default account. This first implementation supports file-based credentials only. Keyring, auto/keyring-first, ephemeral credentials and official API-key login are not supported by this discovery path. An unavailable status asks the user to check login/storage/network; native token refresh remains Codex's responsibility.
- The provider editor marks the first checked ChatGPT account as **Preferred** and displays that account's catalog. **Set as preferred** moves an account to the top and checks it in the draft. Switching accounts preserves saved model selections. Refreshing an account catalog updates the shared Agent model menu. For a provider with several selected accounts, each published model uses the accounts whose catalogs list it. Deleting a ChatGPT provider leaves its saved logins available for reuse. With multiple accounts in the editor, use **Remove account** to delete a saved login and detach it from every ChatGPT provider using it.
- A card explicitly bound to a managed Codex OAuth account uses that account. An unbound `codex_oauth` provider uses the manager's default. Missing or reauthentication-required accounts do not fall back to another account. The existing managed OAuth token manager handles necessary token renewal.
- Fetches use the fixed official endpoint, disable redirects, have bounded concurrency (four providers), a 30-second per-provider operation deadline and a 2 MiB response limit. Errors expose no upstream response bodies or credentials. Refresh results include updated counts, valid-cache fallback or unavailable status for each account provider.
- Discovery and official forwarding share a Codex client-version resolver, with a `0.165.0` compatibility floor. Like the reference implementation, it selects newer local version evidence instead of always advertising an older hardcoded client. Passive preparation reads only the `client_version` field of Codex's bounded cache and the version of installed `@openai/codex` packages; it never executes a CLI. Authenticated routing requests from a Codex user agent may raise the in-memory version. File probing is cached for ten minutes and runs off the forwarding path. No models or account data are imported from Codex's cache. After updating the Rust backend, select **Reload** to replace an existing account catalog fetched under the old identity; merely refreshing the page reuses the current account-scoped cache. Model availability still comes from the account's actual upstream response, not a hardcoded model list.
- Successful catalogs are cached for up to 24 hours in extension preferences (`model_cache:<identity hash>`). Native cache identity includes a token fingerprint and workspace; managed identity includes local account ID, authentication generation and workspace. A native token rotation therefore requires another reload. Cache entries contain only model IDs, timestamps and hashed identity, never credentials. The history is bounded to 64 entries and import/restore clears these caches.
- Failed fetches preserve a still-valid cache for the same identity, not a different account. Explicit user-saved catalogs remain separate and available. Codex's `models_cache.json` is not imported because its account provenance is not reliable. Provider settings and Codex client files are not updated with discovered models.
- Official discovery respects model-picker visibility: entries with an explicit `visibility` must use `list`; `hide`, `none` and unknown values are omitted. Explicit `hidden:true` or `show_in_picker:false` also omits an entry. Legacy entries without visibility metadata remain supported, and `supported_in_api:false` does not exclude ChatGPT/OAuth models. Filtering preserves exact upstream IDs and applies to discovered names and capabilities, not user-saved catalogs, configured defaults or saved groups. A successful empty visible catalog replaces previous discovery results. Discovery caches from before this visibility policy are not reused; select **Reload** after updating the Rust backend to populate the account-scoped visible catalog.
- Refreshing a catalog does not validate inference access, start routing or switch providers. It synchronizes an already authorized model menu. The running application's Rust backend must include this change; frontend hot reload alone cannot add the new command.

## Offline plan validation

In **Advanced settings** for a Custom draft or saved group, enter a synthetic
JSON classification result:

```json
{ "intentId": null }
```

Replace `null` with an enabled rule's stable ID, shown in the editor, to inspect its preferred member followed by approved fallbacks and the default member. The backend rejects unknown or disabled IDs, missing or duplicate fields, extra fields, explanatory prose and responses over 4 KiB.

This operation makes no network calls. It does not classify arbitrary user messages or verify real traffic. Editing the configuration or test input invalidates the result; late replies cannot replace the newer editor state.

## Network classification trial

Open **Live classification trial** under the intent model in the hard/easy template, or under **Advanced settings** in a Custom group. Select an API-key classifier or a model from a bound WorkBuddy account, enter synthetic task text, review the displayed provider/model and check outbound consent before running. The call may incur charges. Saving never sends text. Editing group fields or test text revokes consent and invalidates late results.

The trial and live routing use the group's **Classification budget (ms)**. New hard/easy drafts start at 10,000 ms and new Custom drafts start at 2,000 ms. Saved groups keep their configured deadline. If a trial times out, raise the deadline up to 15,000 ms or select a faster classifier. An invalid deadline displays the allowed range beside the input and disables the trial until corrected. Chat classifiers request reasoning off when the configured provider supports that option. Gemini 3 classifiers use their lowest declared native thinking level to reduce classification latency. During live routing, the stage displays whether classification matched, timed out, or produced another outcome. The routing trace records the fixed outcome without storing task text or model output.

Only test text and enabled rule descriptions are sent with the classifier's own credentials. The trial never reads conversation history, activates routing, switches accounts, changes client files or forwards tasks. Official/OAuth task members may appear in the proposed plan: this is not transport readiness validation. The offline validator remains unchanged.

Independent privacy inspection runs even if business auditing is disabled. Enabled audit/allow rules become redaction rules; block rules remain blocking. Both text and descriptions are inspected. Redaction precedes Unicode-safe truncation to 4,000 scalar values (first 3,000 and last 1,000). Limits: 32 KiB payload, 64 KiB response, 4 KiB classification content, four concurrent trials and the configured 500–15,000 ms deadline including preparation. No redirects or retries. Detection cannot guarantee finding every secret; use synthetic examples.

The network classifier accepts a single `{ "intentId": "enabled-rule-id" }` or `{ "intentId": null }` object, with optional surrounding Markdown code fences. Unknown IDs, extra fields, explanatory prose, tool calls, refusals and truncated responses are rejected. Errors, timeout and privacy rejection return the default candidate order and a fixed reason, never raw responses. VaryAI does not persist trial text or results.

For eligible new turns, live classification sends the latest user text and
the eligible rule descriptions. Completed, visible Responses reasoning
summaries from prior turns are omitted from the classifier input. Opaque
continuations still use their existing ownership. The classifier response
may include null tool fields or an empty tool-call array.

The Responses extractor excludes instructions, attachments, tool payloads and unknown continuations. Temporary execution uses it only for unbound initial classification; bound continuations do not trigger classification.

## Storage and safety

Configuration is stored in `intent_routing_groups` in the private extension database `varyai-ext.db`, schema version 7. Global providers use `routing_upstreams`; the retired origin/reference contract is discarded on staging upgrades. The independently identified main database is `varyai.db`, native schema version 24. Internal portable staging retains released CC Switch schema version 23 without opening or modifying CC Switch's live files. Native extension v6 receives the global-provider table after validation and backup. Members and ordered rules are stored inside one versioned JSON document per group, making each update atomic without duplicating credentials. Updates and deletes require the current revision to prevent lost edits. See [storage boundaries](../architecture/storage.md).

Existing database export, sync and backup mechanisms include draft configuration. Import/restore validates group structure and disables group activation and intent recognition. Provider existence is rechecked on draft save; plan validation additionally checks task transport configuration. Stale references are not evidence of readiness. No prompts, classification history or decision records are stored by this phase.

## Temporary Codex execution (experimental)

Open a valid group editor and expand **Temporary Codex execution**. Enable the local proxy first, confirm outbound consent, then create a session. Run the displayed macOS/Linux command in a new Codex CLI conversation. It selects the group's public alias (`auto/<model>` or the existing `vary-auto/<group-id>`) with per-process `-c` overrides and an environment credential. The command also sets `model_catalog_json` to a private temporary file containing only this session's group, with the same capability-derived reasoning levels and image policy used by the full model menu. This makes `/model` display the selected group instead of the built-in provider list. It does not publish every saved group or provider model; use the Codex routing switch for that combined menu. No global `config.toml`, `auth.json`, persistent model catalog or current-provider setting is changed. Keep the editor open: editing/closing it revokes the session. The token otherwise expires after one hour, or when VaryAI exits. Temporary catalogs are private (0600 on Unix), contain no token or upstream credentials, and are removed on explicit revocation, expiry or normal runtime shutdown. Failed session installation removes its catalog too. Expiry timers are canceled when the session is removed and do not keep the runtime alive. An in-flight request may finish after revocation.

- Separate `/intent/v1/responses` and `/intent/v1/models` endpoints accept only the random local session token; browser-origin requests are rejected. No gateway-key budget is implied by this credential.
- Preparation reads providers off the forwarding path and freezes account credentials, exact model IDs, endpoint and classifier configuration in memory. Native official accounts use file credentials; managed accounts resolve their explicit/default account once. A later account/default change never redirects an existing session. Tokens are not persisted; managed credentials can refresh only for the frozen account identity. Native login changes and session expiration require a new session. Startup currently caps sessions at 16, each for one hour.
- API-key members must explicitly declare one of the four supported upstream protocols; cross-protocol execution is validated by the bridge. Official/OAuth members use the fixed ChatGPT Codex endpoint with their own bearer token, workspace and client headers. Native official execution requires the caller to send `stream:true` and `store:false`; unsupported semantics are rejected, not silently rewritten. Inference permissions still require live validation.
- Initial requests may classify sanitized supported user text, then apply the configured strategy within the approved plan, keeping an intent rule's selected member first. A received 429/502/503/504 may try the next candidate before any output is delivered. Ambiguous connection failures, other errors and any bound continuation are not replayed at another account. Error responses are fixed codes without upstream bodies.
- The first implementation conservatively pins the entire client conversation, not just one tool turn. Response IDs and function-call IDs also pin the exact candidate. Unknown/conflicting continuations or opaque state without a known binding are rejected. Overlapping requests in the same identified conversation receive 409 rather than racing; independent conversations do not share the stream-long guard. Bindings are memory-only and bounded to 4096 entries; restart/expiry does not silently reconstruct them.
- JSON and SSE bytes pass through unchanged. A bounded observer records response/tool IDs; duplicate IDs from different targets become unusable rather than overwriting ownership. Request edits preserve untouched JSON lexemes. Caller credentials/cookies/workspace headers never become upstream authentication. Business security inspection, tool observations and asynchronous completed-usage accounting remain connected.
- The local test suite covers model-specific fallback, credential stripping, JSON/SSE tool continuations, unknown/colliding IDs, account snapshots and revocation using synthetic accounts and dynamic ports. No real paid inference or installed-app acceptance is implied.

## Codex model-menu takeover

In **Settings → Routing → Local routing → Codex**, enable the single Codex switch and confirm configuration changes, forwarding and classification costs once per connection. The local gateway must already be running. There is no Current provider / Routing groups mode picker, group-publication checklist or separate connect button. The confirmation also authorizes automatic menu updates for subsequent group additions, edits, deletions and discovery changes while routing remains on. Saving configuration with routing off never starts forwarding.

One dedicated `varyai` provider and one external Codex-native catalog expose both kinds of entries:

- `vary-provider/model-<hash>`: **exact model ID · provider name**. The hash is derived from the exact provider ID and model ID. Each entry has one isolated target and never runs intent classification or another provider's fallback.
- `auto/<model>` for newly discovered groups, or the existing `vary-auto/<group-id>` alias for older/manual groups: **group name · routing group**. Selecting it uses that group's strategy, approved candidates and optional intent classification. A saved group remains a routing group even when it has only one member.

Hover over a VaryAI entry in the Codex desktop picker to read its complete
label, including the provider or routing suffix and any disambiguation number.

The menu combines **available enabled global provider models and executable routing groups**. Direct entries retain each exact upstream identity; selecting one never silently substitutes a different provider. Group-only model unlisting withdraws a direct entry without removing the group target. Unsupported transports, unavailable credentials and non-executable entries are omitted, with a count in the UI; discovery is not evidence of inference permission. At most 256 entries are published, reserving room for the routing groups. Agents with a starting-model control choose their own default from this shared menu; native-picker applications choose the chat model inside the application. Background catalog refresh preserves the starting choice while explicit selection changes it; provider creation and editing do not select an Agent or change another Agent's default. Existing Codex conversations may retain their earlier native configuration.

The shared VaryAI model picker uses readable model names for both rows and hover tooltips. Internal routing aliases remain selection values and are not shown in these tooltips.

Managed Gemini CLI connections use the full catalog name as both the model ID
and the model label. The local gateway resolves this name to the selected
provider route and still accepts earlier model aliases. VaryAI sets a compatible
User-Agent for Unicode model IDs through Gemini CLI's custom header setting.
Gemini CLI 0.67.0 supports this configuration. Start a new CLI session after the
gateway has synchronized the model list.

When models in the same provider group have identical displayed names, the
picker follows Magpie's `modelTells`: it removes common leading and trailing
path segments from their public references and displays the remaining identity
in muted text beside the name. For example, `gs/vendor/glm` and
`gs-2/vendor/glm` show `gs` and `gs-2`. Unique names need no hint. Search includes
these references, and selecting a row still sends its original routing alias.

Model rows and the provider rail use the saved provider icon, including built-in
icons, uploaded images and letter icons. Saving an icon change refreshes the
shared catalog and updates these views. Native model choices keep their explicit
application logo; routing groups keep their routing mark. An unreadable uploaded
image falls back to the standard provider icon.

Claude Desktop has a separate **model list** control on its connected Agent row, following the reference implementation's distinction between model visibility and a chat's selected model. Search and check several models, show or hide all, then save. By default, newly added shared models appear automatically; **Only models I pick** keeps new models hidden until explicitly selected. These preferences survive disconnect, reapply, gateway suspension and catalog refresh, and never hide a model from another Agent or disable its global provider. The native gateway profile and authenticated Desktop model-list endpoint publish only the visible subset, including an explicitly empty list. Visibility is not an access-control policy and does not invalidate an existing conversation's model identity. Quit and reopen Claude Desktop to load the updated profile, then choose the model inside Desktop. Configuration conflicts keep the recovery record rather than overwriting externally changed fields. This covers model visibility, not the reference implementation's separate per-Agent model ordering controls.

Disconnected Agents with a starting-model control show their persisted native model selection. **Default** means that no explicit model is stored and the tool chooses its own default; it is not a guessed model ID, a shared-catalog default, or proof of a Vary connection. The current native value is read-only in this picker. Selecting a shared model explicitly connects the Agent to Vary; **Disconnect and restore** restores the saved pre-connection configuration. The status observation reads only supported selection fields from bounded configuration files, never imports providers, starts a CLI, reads authentication files, or changes configuration. Read errors show an unavailable state instead of claiming Default. Runtime-only choices, command-line overrides and inherited shell environments are not inferred. Native-model editing and the reference implementation's distinct factory-default reset action remain outside this control.

Reload Codex configuration after enabling routing. Changing a conversation's model alone does not change its stored provider. A standalone connection migrates the previous provider's quiet conversations to `varyai` only when their saved model is executable in the current menu: paginated history changes only its SQLite provider row, while legacy history also updates rollout metadata. Unavailable, inconsistent or unreadable saved models stay with their original provider; model names are never guessed or replaced with a default. Already migrated conversations with an unavailable historical model require explicit model reselection, or a new conversation if automatic compaction blocks that choice. Busy compatible conversations wait for writer locks to be released and retry in the background. Native compatibility instead handles eligible built-in OpenAI conversations without changing their provider. Profiles, environment/CLI overrides, cached configuration and unknown historical opaque state remain separate constraints. Actual desktop acceptance must be verified with the user's client version.

On macOS and Windows, the desktop app-server watcher follows observed route changes automatically after configuration settles and tasks finish. It waits for a replacement process instead of treating a termination request as a successful reload. Catalog-only updates do not automatically restart Codex: **Tools / Agents** reports the outdated model list and offers a confirmed **Restart** action, which also waits for idle. Reload status describes process recovery, not successful authentication or inference. Other platforms require reloading Codex manually.

The local credential, catalog and recovery receipt are private (0600 on Unix). The catalog contains no upstream credentials; the recovery receipt includes the original configuration, which may contain credentials and must be kept private. Publication does not write `auth.json`. A legacy ordinary takeover is restored before the unified owner is installed, under the same Codex switch lock; failed activation attempts restore the previous owner without stopping the listener. Changes are limited to the selected model, model provider, catalog pointer, web-search setting, one owned provider table and, when native compatibility is available, the explicitly owned `openai_base_url`. The catalog uses the native Responses template, explicit context metadata, capability-derived reasoning levels and image policy, and shell-based edits. No capability is inferred from a name. Standalone search and WebSockets are disabled for this HTTP/SSE-only path.

Configuration writes journal a durable receipt before replacing `config.toml`. A no-clobber install rejects concurrent replacements. Restore operates field by field, including owned model aliases selected in Codex, and preserves unrelated/newer edits and unowned native URLs even when the rest of the document matches an installed projection. Catalogs have unique names and are retained until disconnect; unchanged refreshes reuse the existing file. At most 64 distinct catalog versions are retained per connection. Interrupted updates record the previous projection. Symlinked/oversized files and overlapping ownership are refused; external edits are never force-overwritten.

Turning the Codex switch off, stopping the gateway or normal VaryAI shutdown restores the original configuration. Startup attempts recovery before importing live configuration after a crash. Failed recovery keeps its receipt and exposes a recovery action. Reconnect explicitly after restart; existing Codex processes may retain an old local token. Native requests are cancelled when their ingress authorization is revoked; already submitted custom-provider requests may finish after disconnect.

If the recovery receipt is missing but the reserved `varyai` provider section remains, including after another application's configuration restore, the Codex switch stays available. Explicitly enabling it saves a private full-config backup named `vary-routing-orphan-backup-<id>.toml` next to the normal receipt and replaces the reserved provider and top-level model-menu settings. Active, profile-referenced or modified reserved sections do not block this explicit reconnect. The new receipt uses the current configuration as its baseline: disconnect restores that pre-connection configuration, including any previous reserved provider, not an unavailable older version. Other providers, profiles, unrelated settings, unowned native URLs and `auth.json` are preserved. Automatic menu refresh cannot acquire receiptless configuration; fresh consent is required. The optional **Clean up leftover configuration** action still requires confirmation and is offered only for an inactive, exactly recognized VaryAI section with no remaining provider, model-alias or catalog references, including profiles. It backs up the configuration and removes only that section. Both operations use no-clobber write protection and stop if backup fails. Backups may contain credentials and must be kept private. Status reads and ordinary restore/shutdown never delete orphaned configuration automatically.

Every provider entry and routing group has an independent runtime snapshot and continuation-binding store, with no one-hour menu expiry. Group save/delete, discovery toggles and explicit catalog reload automatically rebuild the authorized menu. The local token and continuation bindings survive refresh only for unchanged group configuration and identical account/model destinations. Changed destinations cannot inherit old response/tool bindings. Failed synchronization leaves the saved configuration intact but revokes the stale runtime and exposes recovery rather than continuing to forward to deleted/stale targets. Disconnect/recover and enable again after resolving the problem.

Managed identities are frozen at connection time; refreshed credentials must still match that identity. Native login changes fail closed. Provider editing/switching and listener-address changes remain blocked while this configuration is owned; disable Codex routing first. Provider models and routing groups coexist in Codex's menu without allowing two independent writers to own its configuration.

Tests use temporary config directories and synthetic accounts, covering menu aliases, exact JSON/SSE forwarding, cross-group continuation rejection, explicit consent, private-file modes, crash recovery, newer external edits and gateway shutdown restoration. The implementation follows the documented `model_catalog_json`, `model_providers`, bearer-token and native Responses configuration fields: https://developers.openai.com/codex/config-reference/ . No real login file was changed during verification.

### Context capacity and compaction metadata

Capacity publication follows [the reference implementation's model resolution](../../third_party/routing-reference/NOTICE.md) and [group policy](../../third_party/routing-reference/NOTICE.md). A model-specific saved override wins, followed by the provider's positive top-level `model_context_window`, the vendor's own catalog, and public models.dev facts. Provider-wide overrides are bounded by explicit vendor capacity, never by another vendor's public estimate; a model-specific override is authoritative. Imported capacity carries `contextSource: "vendor"`; editing the capacity marks it as a user override. Older unmarked saved capacities remain explicit overrides for compatibility.

**Reload** refreshes supported metadata from configured API model-list endpoints and account-scoped official catalogs, and refreshes `https://models.dev/api.json` at most daily. Public facts are cached privately in `routing-model-context.json`; vendor facts use the extension database's bounded, identity-keyed model cache. Public requests contain no account credentials or model names. Fetch/cache failures retain available facts without breaking forwarding. Discovery, database access and file I/O run outside request forwarding. ChatGPT account models never borrow larger API/public catalog windows. Refreshing metadata does not modify saved provider catalogs or resurrect hidden account models.

Supported fields include `contextWindow` / `context_window` / `context_length` / `inputTokenLimit`, `maxContextWindow` / `max_context_window`, `effectiveContextWindowPercent` / `effective_context_window_percent`, and `autoCompactTokenLimit` / `auto_compact_token_limit`. Positive integers and numeric strings are accepted within safe integer bounds; capacity below 2,048 tokens is unknown and percentages must be 1-100. Public lookup supports vendor prefixes, normalized spelling and known-model window/effort suffixes without changing upstream model IDs. An unknown model remains unknown: the generated entry omits its capacity instead of inventing 32,768 tokens (the former approximately 31k effective window).

Groups default to the **largest known member window**. The editor also supports the smallest known member (`contextWindow: -1`) and an explicit token count. Manual groups publish their selected member's capacity, not inactive alternatives. Effective percentage defaults to 95; multi-member groups retain the most conservative declared percentage. A contradictory maximum below the stated capacity is raised, not used to shrink the window. Disk, native and temporary catalogs use the same publication policy.

Actual capacity and early compaction are independent, matching [the reference implementation's Codex publication](../../third_party/routing-reference/NOTICE.md). A generated 1M Codex entry advertises `context_window: 1000000`, with a separate default `auto_compact_token_limit: 272000`. Open **Tools > Codex / Claude Code / Grok Build > Details > Long conversations** to choose **Compact at** or **Full window**. These are shared preferences, not independent per-tool policies. If none of these tools is installed, the control appears in General Settings instead. The segmented control saves immediately; the optional token input accepts `272k`, `1m`, or a positive integer and saves on blur or Enter. Empty uses the default. No long-conversation controls remain on Routing.

**Full window** disables the global threshold, or a custom global threshold can replace 272k. Explicit model/provider thresholds still win; a user-typed window suppresses the implicit default but not an explicit global threshold. Thresholds at or above capacity are omitted. Claude Code's owned context environment value and Grok Build's native profile use the effective working window; the shared model catalog retains the true capacity. Subscription-native models retain their vendor policy. Changes invalidate the runtime catalog and refresh already-authorized tool projections; disconnected tools are not connected implicitly. Reload the tool configuration to observe changes. Existing supported vendor thresholds remain separate. Original global Codex overrides are untouched and can still take precedence in the client.

Before dispatch, supported text and tool schemas are estimated at four UTF-8 bytes per token. Roomy or unknown-capacity members precede known-too-small members, without rewriting the request. If every roomy member is temporarily unavailable, a small last resort can return an overflow so the client knows to compact. A confirmed context overflow advances only to remaining members not known to be too small; a 413 byte-limit refusal does not impose a token estimate on other vendors. Exhaustion returns a sanitized context error, never upstream text. Ordinary rate/availability errors are not mislabeled as context overflow.

An observed, successfully completed plain-history turn has soft affinity: as its replayable history grows it may move to a larger member. Capability groups with per-turn affinity use the current priority on the next replayable turn, including after a member is re-enabled or reordered. Requests with response references, tool state, encrypted reasoning or checkpoints remain bound to their producer; ambiguous submission, interrupted output and pending tool calls do not become portable. A restart conservatively restores hard affinity until another complete plain turn is observed. Codex Responses can recover from an explicit upstream encrypted-state refusal using the bounded same-destination cleanup described below; initial requests retain their state. Capacity-only refreshes preserve unchanged static transport identity. Existing Codex conversations can retain their initial metadata; reload/reconnect and start a new conversation to observe the new catalog. Synthetic tests do not establish real-account or installed-client acceptance.

Managed WorkBuddy and DeepSeek Harness requests with self-contained visible history can continue in the same conversation when group members are removed or restored. Remaining destinations retain their exact ownership. Per-turn affinity can reselect the preferred member after an observed completed turn. Requests that depend on upstream response IDs, tool state or encrypted reasoning still require their original binding.

Managed Harness Chat also resumes a saved conversation with a completed tool transcript after a membership change when the gateway can match the calls to their original, still available destination. This check also applies to observations restored from disk after a gateway restart.

In a per-turn group, a Harness tool result awaiting its assistant reply stays
with its original model. Once the transcript includes that reply, the next
user message follows the current group order. The gateway checks the recorded
tool calls and forwards the complete visible transcript. A completed
transcript can follow a surviving group member after the last responder or a
historical tool owner is disabled. Saved evidence of a completed call does
not grant authority to send a live tool result to a different model.

## Continuation recovery after restart

Normal Codex model-menu/native and authenticated external gateway requests persist **observed ownership**, not conversation text. A new runtime or menu reconnect can restore response IDs, tool-call IDs, item references, opaque-content hashes and conversation pins for the same authenticated consumer and alias. Codex Responses follows Magpie's upstream-validation approach: still-available exact destinations retain their ownership when other group members change, and unknown state is forwarded intact for upstream validation. Strict recovery requires the complete destination set to match. Target reordering remaps owners by fingerprint rather than array position. Native ingress scopes recovery by the request workspace, falling back to the authorization header, following Magpie's account-scoping convention. This namespace is separate from upstream authentication; there is no local authentication lease. Temporary editor execution remains memory-only.

WorkBuddy and WorkBuddy AI can continue a self-contained text conversation
after a member is removed from a saved group. An exact surviving destination
keeps its recorded bindings. If the previous destination was removed and the
saved scope has no tool-call or opaque state, the remaining members can be
considered for a fresh route. Requests carrying sealed or incomplete history
still require the original continuation checks.

Stable conversation identifiers include `prompt_cache_key`, `session_id`, `session-id`, `thread-id`, `thread_id` and `x-conversation-id`, in that priority order. Without a name, Responses requests can locate a saved conversation from known response/tool/item/opaque references; conflicting matches remain unusable. Destination fingerprints include the exact provider/model, endpoint, static authentication/configuration and verified official account provenance. Static API-key changes invalidate recovery. Ordinary official access-token rotation can preserve durable provenance when the same verified identity is freshly prepared; replacing or reauthenticating a managed account changes its authentication generation. Missing provenance disables durable recovery rather than weakening the checks.

The private local store contains only domain-separated identifiers and fingerprints inside authenticated-encrypted records, including authenticated timestamps. It retains at most 512 conversations for 24 hours, within a 16 MiB file budget. Individual snapshots are bounded to 128 KiB and 4096 mappings; an oversized update invalidates the prior snapshot instead of persisting only part of its ownership. Duplicate IDs retain collision tombstones. Queue/coalescing and file I/O failures do not fail an otherwise valid live forwarding operation; future recovery still requires a valid saved record. No SQLite locks, disk reads or writes are added to forwarding.

Response observations are queued as complete SSE events arrive, before their bytes are yielded downstream; cancelling before EOF retains already observed bindings. JSON ownership is observed when the complete response is available. Normal disconnect/stop flushes queued generations, not an unlimited drain of every active stream. An abrupt crash before a queued write completes can lose recent observations or invalidations. Persistence is not a zero-loss journal or rollback-resistant history service. Expired, evicted, corrupt or missing-key records cannot restore ownership. Conflicting known ownership remains an error. Strict clients reject unknown or destination-changed state locally; Codex Responses lets the selected upstream validate unobserved state. Lost ownership cannot be reconstructed retroactively.

Custom-provider clients must still reconnect/reload configuration to obtain the current local credential; an old menu token is not revived. Native Codex requests instead use the credentials supplied on each request. Restoring ownership does not recreate remote Responses state, decrypt foreign reasoning/compaction, or persist hidden cross-protocol adapter histories. It does not make a `store:false` upstream response available by ID: the client must still provide the history its upstream protocol requires. For direct Codex Responses forwarding, a concrete encrypted-state refusal can trigger at most two recovery mutations on the same destination before output is released, following Magpie's `foreignReasoning`, `withoutSeals` and `withoutCompaction` behavior. First, reasoning and nonempty encrypted copies of ordinary items are removed while visible messages and tool pairs remain. If that is refused again, a non-native request may remove sealed compaction history, losing its earlier context. Native compaction/checkpoints retain that history. Ordinary conflicts and ambiguous network failures do not trigger cleanup. Untouched fields retain their original numeric lexemes; no raw upstream refusal or sealed content is logged.

## Native OpenAI conversation compatibility

Explicit Codex takeover can also install `openai_base_url = "http://127.0.0.1:<actual-port>/backend-api/codex"`. This affects all built-in `openai` requests, not only routing aliases. Following Magpie `4869d0c`, built-in models are relayed to the official backend using the request's current authorization and workspace headers. API-key requests use the official OpenAI API. Names containing `/` remain managed names and cannot fall through to an official model. Existing `/intent/v1` bearer authentication is unchanged.

Native loopback forwarding does not require a credential snapshot, token fingerprint, ownership receipt, supported-version allowlist or periodically renewed lease. Browser-origin and nonloopback requests remain rejected before gateway policy/body work. An upstream generation HTTP 401 reaches Codex, which owns refreshing its native sign-in. Each subsequent request carries the refreshed token or selected account. Configuration discovery treats an incomplete authentication-file read as unknown and retains an established native connection; it does not revoke forwarding.

Native model discovery shares an account-scoped official catalog for one minute. Stale data returns immediately while one background fetch refreshes it. Cold callers wait at most three seconds; the shared fetch continues for up to thirty seconds. Ordinary upstream HTTP 401/403 responses reach Codex for renewal. An explicitly revoked sign-in falls back to the available catalog so managed models remain visible, and the next discovery can retry with a new sign-in. Unknown client versions are forwarded using their original metadata. WebSocket handshakes currently receive HTTP 426 for the existing HTTP fallback; there is no general WebSocket bridge or unknown-path passthrough.

Compaction is independent of transport fallback. Legacy `/responses/compact` and Responses-v2 terminal `compaction_trigger` have separate validated envelopes. Native compaction stays on a compatible official target; opaque output is pinned to its producer. Other targets summarize through the existing selected-target execution, inspection and usage path with tools disabled. A compaction rule can select a different summary model after a completed turn with self-contained replayable history. For an existing conversation, the checkpoint keeps the original answer destination. A successful bounded summary becomes a `varyai1:` authenticated-encrypted checkpoint, bound to the native request account scope, alias and exact destination. The private checkpoint key remains in VaryAI's configuration directory on ordinary disconnect so an eligible checkpoint can resume after restart. Only this gateway decodes the marker into conversation context; it is never sent upstream as OpenAI ciphertext. Malformed, wrong-identity, wrong-destination or unavailable-key checkpoints fail locally. These checkpoints are not portable to the official backend when takeover is disabled. No conversation database, rollout or auth file is rewritten.

Unknown pre-takeover response/tool IDs, encrypted reasoning and foreign compaction state are initially forwarded intact to the selected upstream. Empty reasoning summaries and null optional encryption fields do not require a local ownership record, including in brand-new conversations. An explicit upstream encrypted-state refusal can trigger the bounded same-destination recovery above; native compaction/checkpoints remain protected. Conflicting known ownership still fails locally. Once a request is ambiguously submitted or output starts, it is not replayed at another destination.

Receipt v2 records `openai_base_url` ownership separately. A preexisting unowned URL conflicts even if it matches localhost; the custom menu can remain connected while native compatibility is unavailable. Refresh/restore changes only a still-owned value and preserves newer user edits. Legacy receipts never gain native ownership implicitly, including whole-document and missing-file recovery. Configuration conflicts retain recovery evidence. A loopback URL without a receipt is not proof of configuration ownership, but configuration ownership is not an extra authentication gate in front of native forwarding.

## Remaining before live acceptance

Lossless migration of opaque state, native image/search endpoint routing and real Codex desktop/official-account acceptance remain outside the verified scope. Plain-history length-aware reselection is implemented; opaque continuation affinity remains strict. Generation responses preserve a bounded allowlist of content type, cache/retry metadata, request IDs and supported rate-limit headers; credentials, cookies and hop-by-hop headers are never propagated. Global gateway conversations have consumer-scoped identities; clients without a stable identity cannot assume opaque continuation recovery. Model-menu publication/restoration is implemented; managed account tokens can refresh without following a changed default. Inbound native forwarding uses each request's credentials across token rotation and account changes; frozen outbound managed-account snapshots can still require a menu reload after token rotation. The stored `enabled` flag controls explicit authenticated gateway publication only. Codex projection and temporary execution retain their separate consent and recovery paths.
