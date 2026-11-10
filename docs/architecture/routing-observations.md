# Routing observations

The routing workspace ports the layout and interactions of the reference implementation's routing
plugin at commit `3bce283df43c3ce733e457daa16a69ac38bbb5fd` to React/Tauri.
Attribution is in `third_party/routing-reference/LICENSE`. Vary's branding and existing
application navigation remain in place.

## Window styling

The Agent and routing workspaces share the reference implementation's window palette, 13px native
text stack, fine neutral borders and compact controls. Agent rows use the
upstream 56px connection-row height, unframed 22px tool logos and 32 by 19px
graphite switches. The remaining tools unfold from the list's final row.
Content stays centered within 1200px, plus 16px side gutters.

Routing uses the same palette for the live stage and group editors. The group
section follows the stage directly, without request or account observation lists.
The stage retains a compact horizontal flow below 560px of its own width. Narrow windows retain icon
navigation, and reduced-motion settings suppress control and stage movement.

## Page structure

- Requested models use readable catalog names in the live stage and its header.
  Internal `vary-auto/` and `vary-provider/` execution aliases remain unchanged
  and are available as label tooltips. Recorded direct requests can fall back
  to their upstream model after a provider
  is removed. Routing groups retain their group identity rather than adopting
  the name of a fallback member. Missing metadata is labeled unavailable.
- The header links to groups, opens a new group dialog, and masks account
  identities in the stage and group editor.
- The workspace stage uses a centered compact flow, with Agent icons above names,
  a circular Vary gateway and bounded candidate labels joined by curved paths.
  The gateway exposes its actual listener and strategy in its tooltip. The compact
  tray keeps horizontal client labels and its own gateway dimensions. Live requests
  retain independent packet players, including a response already returning when
  a newer request arrives. Up to four recorded Agents share the stage; identical
  candidate seats share one branch, matched by provider, account, model and effort.
  The overview follows the newest live trace automatically. Its footer retains
  request, fallback and Agent error counts as read-only summaries. Decision
  details, replay, request history, purpose/metric filters, session grouping and
  the accounts-and-keys panel are absent from this workspace. Legacy request-list
  preferences do not affect the live overview, and the page does not fetch history.
  A supplier with one distinct enabled API key displays its provider name.
  With multiple distinct enabled keys, candidates display their configured key
  name or a backend-masked credential label (four leading and four trailing
  characters, or bullets for short keys). Disabled and duplicate keys do not
  make a shared pool. This follows Magpie's `RestKeys`, `weighed` and `who` rules.
  Only configured static API keys supply these labels; OAuth
  tokens and arbitrary authorization headers never do. The original credential
  never enters a trace. Older records without a label retain the provider name.
  Stage status lines use an unnumbered API-key fallback, or "one key" for a
  provider seat. Rest and context restrictions precede answering/completion and
  failure states; quota renewals remain visible alongside those states. Account
  reset countdowns match Magpie's `dur` precision: seconds below a minute,
  minutes below an hour, hours and minutes below ten hours, rounded hours below
  48 hours, then rounded days. Thus 88 hours 46 minutes displays as four days.
  The live stage uses current time for quota countdowns.
  Recorded accounts include a subscription plan when known. Only recognized plan
  types from the bound account's Codex usage response or rate-limit event are
  recorded; keys and unknown plan values never acquire an inferred subscription.
  Connected clients and enabled providers from configuration never supply extra
  nodes or branches. Before any request, the stage shows a generic robot icon, Agent label and
  empty placeholder. A single instruction sits beneath the waiting heading;
  the footer contains only the aligned counters until a request has a status.
  A direct request has only its recorded destination; a group retains every
  recorded candidate, including untried fallbacks. A request that fails before
  recording any candidates shows its failure state and status without borrowing
  providers from configuration or earlier requests.
  The live selected destination receives the Agent's color;
  moving paths and packets follow actual attempts and fallback decisions. A solid
  point travels to the provider and waits for completion; a hollow ring returns
  through Vary to its original Agent. The workspace and compact tray keep only
  small circular packets: no birds, count badges, halo discs, aggregation or
  per-request offsets. Concurrent requests waiting at the same destination
  overlap at the same wire tip, as Magpie's `holdAll` does. Workspace positions
  use Magpie's `min(6, length / 4)` clearance at each end and cosine easing;
  compact tray positions use its wire tips and quadratic easing. The compact
  tray also retains its 3.5px dots, 420/460ms outgoing legs and green/red replies.
  Failed attempts return in red, stopping at
  the hub if another candidate follows; a final failure returns to the Agent.
  Cancellation returns in muted gray, including HTTP 499, and never implies an
  upstream error. The workspace and tray omit the synthetic HTTP 499 status code
  from candidate labels. Recorded traces retain the original status and neutral
  cancellation events for diagnostics.
  Finished players are removed once the return ends and never
  restart on polling. Newly completed long requests use their finish time for
  admission; initial history does not introduce unrelated completed branches.
  Completed requests have stationary paths. Listener and strategy details remain
  on the gateway's tooltip, and branch status appears on hover or keyboard focus.
  Client and provider lists have bounded scrolling, and paths are recalculated
  when either list scrolls or a node resizes. Recorded masked key labels identify
  static credentials; candidates without them retain the safe ordinal fallback.
- Live quota readings and cooldowns match an exact observed credential and model,
  using nonblocking scheduler snapshots. Unknown or expired quota data is not
  shown as zero usage. The stage continues to consume these observations without
  displaying a separate account panel. Imported session usage and provider/model
  summaries remain in Statistics and never supply stage requests.
- Groups retain inline expansion, persistent model selection, order changes,
  strategy selection and publication switches. Templates use available catalog
  models. Import immediately saves a new group and reports missing/substituted
  models and omitted rules. Bulk deletion requires confirmation,
  uses revision checks, and retains unsuccessful selections on partial failure.

### Group form

The group form uses Magpie's framed sheet dimensions: a 48px header and footer,
14px/18px body padding, and a dialog width up to 1040px, bounded by 24px side
gutters. Height is capped at the smaller of 760px and the viewport minus
144px, leaving 72px above and below a full-height dialog. Short windows use
40px vertical gutters. The body scrolls independently. Narrow forms stack labels
above controls. A shared label grid uses a stable 132px label column and the
reference's 10px/14px spacing.
Advanced content and nested fieldsets define their own two-column grids, rather
than inheriting tracks through native disclosure/fieldset rendering boxes. The routing
workspace's page margins must not override dialog centering.

The form has a dedicated stylesheet, separate from the workspace's list styles.
It uses the reference's system font stack, neutral light/dark surfaces, 27px text
inputs with 7px corners, and 22px segmented options on a gray track. Pattern
inputs have no second surrounding frame. Vary's blue remains the form's accent,
including Save, focus rings and interactive highlights. Picker and action menus
inherit the form's typography and palette.

The form places routing directly below the name, followed by models, patterns,
model lists, rate-limit placement, execution settings, rules/classifier and
reasoning levels. Only name, routing and model members are visible initially.
Every other group field is inside the closed Advanced settings section. Its
controls remain mounted, so closing it preserves edited values and template-provided
rules, classifier and execution settings. The duplicate per-member settings list
has been removed; existing effort variants, weights, ordering and disabled seats
still round-trip when membership is unchanged. The name has a localized example
placeholder. Draft strategies use segmented choices; compact group rows retain
their strategy menus. Changes update only the draft until Save succeeds.

Model members appear as wrapping Vary-blue checkbox chips, grouped by provider.
Every provider starts expanded and has an independent disclosure button, a
tri-state selection checkbox and selected/total count. Folding only changes
visibility: selection and bulk actions still work while folded. Provider chevrons
follow the provider name and rotate over 220ms. Provider content animates its grid
height and opacity. A form ResizeObserver drives a separate 220ms dialog-height
transition, including changes from Advanced settings. Fractional heights round
up, and the scrollbar gutter stays stable to prevent sizing feedback. Reduced
motion disables these transitions. New blank groups start with all task-eligible
catalog models selected, up to the existing 256-seat
limit; reaching the limit shows an inline explanation and disables additional
choices. Existing groups and templates keep their own initial membership.
Catalog IDs are paired with provider IDs, so identical model IDs in different
providers remain independent. Unavailable or custom saved seats remain visible;
unavailable providers cannot supply new selections. Chips show model names only,
without context-capacity badges. Global Select all and Select none controls apply
across provider groups.

Routing, model-list visibility and rate-limit help describe the selected option.
Timeout, retry and context help also follow their active settings. Segmented
controls support arrow keys, Home/End and one tab stop per group. A non-preset
inherited value remains unchanged until explicitly edited. Saving trims the name
and clears `sink` for rotation, weighted and manual strategies, where the setting
does not apply. Duplicate submission and dismissal remain blocked while saving;
failed persistence keeps the draft and reports a sanitized error.

## Recording and retention

`proxy::intent::Runtime` owns an in-memory ring of 60 requests. Decisions are
recorded at their execution sites, after actual scheduling and capacity ordering.
It covers shared group/direct-model execution and temporary routing sessions.
Legacy forwarding outside this execution path has usage summaries only and
does not appear on the routing page. Stopping the gateway clears the live view;
persisted dates remain available and replay their recorded decisions.

The recorder uses nonblocking `try_read`/`try_write` operations. Contended or
poisoned observation locks discard observations and do not change forwarding.
Each request retains at most 256 candidates and 1024 steps, reserving the final
step for completion or cancellation. A request guard moves into the
response stream, so dropping a response before polling or before EOF records
cancellation. Successful completion is recorded at EOF, not at response headers.
Embedded stream errors are recorded as failures.

The trace never retains request or response bodies, headers, vendor error text,
credentials, filesystem paths, raw session IDs, or gateway keys. Session
grouping uses a digest scoped to a random runtime salt and the authenticated
consumer. Only configured model/provider identities, fixed decision categories,
HTTP status, relative times and numeric usage are exposed to the renderer.

Each candidate captures a safe account/key observation and a runtime-salted
credential identity. Static identities include the configured provider namespace,
endpoint and authentication, but never the model or fixed reasoning level.
Native account identities use frozen sign-in provenance, surviving access-token
refresh while separating account switches. Only the salted digest leaves the
runtime, together with a masked display label for configured static keys; no full
key, endpoint or workspace is exposed. Configured provider display
names are retained for native accounts as well as static providers, avoiding
internal-ID fallbacks. Weak scheduler references let live readings update only
observed candidates, without retaining credentials or enumerating the catalog.
Evicting a request from the ring also removes its otherwise-unreferenced readings.

Purpose observation reads `x-openai-subagent`, falling back to the explicit
`thread_source` in `x-codex-turn-metadata` (at most 8 KiB). Native compaction
operations record `compact`. Ordinary `user` and generic `subagent` sources
remain unmarked. Only fixed catalog tokens are retained; an unrecognized
explicit marker records Unknown purpose rather than exposing arbitrary header
text. This intentionally differs from the reference implementation's literal unknown-kind labels to
preserve Vary's metadata-redaction boundary. Reading purposes never edits the
headers, request body, selected model or routing behavior. Observation lock
contention and malformed/oversized metadata cannot interrupt forwarding.

Completed gateway traces are copied to a bounded 128-entry nonblocking queue.
A dedicated worker writes private daily JSONL files under the application data
folder's `routing-history` directory. Yesterday's files are compressed, history
older than 30 calendar days is removed, and the oldest prior-day archives are
pruned to a 200 MiB budget. Today's file is exempt from byte pruning. Reads are
bounded and return at most the latest 2,000 records per day; incomplete trailing
appends and malformed records are ignored. The completion guard submits once,
including cancellations. Worker startup, queue saturation, observation contention
and disk failures discard observations without interrupting forwarding. An
unavailable history reader reports a generic error without filesystem paths.
Missing history directories are empty only when their nearest existing ancestor
is a directory. A regular-file ancestor remains unavailable on every platform,
including Windows where that filesystem failure is reported as `NotFound`.
No native session files are read by this recorder or by the routing page.

Files contain the same safe metadata as the live trace, including candidate
observations, purpose, scoped session digest and relative decision timestamps.
Scheduler weak references and all credentials are excluded from serialization.
Stored history never borrows current credential identities or current quota readings.
Unknown timing, throughput and prices are omitted from the recorded metadata.
The existing usage queue and native session imports
continue to feed Statistics independently. Trace history begins with this version;
older usage summaries are not converted into invented gateway traces.

## Group transfer

Copy to share and export use the version 1
[portable reference envelope](../../third_party/routing-reference/NOTICE.md) with provider/model references rather than internal member identities.
Rules containing working-folder inclusions or exclusions never leave the
machine. Disabled rules and derived/private publication state are excluded.
Legacy Vary version 1 transfers remain readable. Import generates new
group/member/rule identities and resets revisions so it cannot overwrite an
existing definition; duplicate names are numbered case-insensitively.

Shared envelopes may be pasted with surrounding text or a code fence. Members
not available under the original provider use a locally available model with
the reference model-name normalization, preserving the actual local wire ID and
fixed effort. Missing members and rules with no remaining target are omitted;
folder-scoped rules from another machine are always omitted. Results report
missing models, substitutions and omitted rule counts. A share with no usable
member or pattern fails without saving. Unsupported nonempty semantics still
fail explicitly rather than pretending they were reproduced.

Reference JSON with explicit `provider/model` members, built-in strategy names,
weights, manual pick, context capacity and disabled state can be imported.
Member `off`, explicit conversation affinity, `firstToken`, and `patience`
settings also round trip. A first-content deadline covers response headers and
the buffered pre-content stream; no slow member bytes reach the Agent before
fallback. The final candidate is not subject to the first-content deadline.
Only received transient failures are eligible for patience retries; ambiguous
network failures are not replayed. Old Vary definitions default to no additional
retries, while new groups and reference imports default to 60 seconds.
Opaque continuations retain their destination even with affinity off, and a
continuation targeting an explicitly disabled member is rejected safely.

Dynamic `match` membership accepts anchored, case-insensitive globs and `re:`
regular expressions over the complete provider/model identity. Explicit members
stay first. Catalog-derived members have stable identities but are never stored
as explicit members; disabled derived identities survive catalog disappearance.
Editing patterns requests a read-only backend resolution and updates the draft's
matched-model list without saving or probing an upstream. A pattern-only group
can be saved before any model matches. Definitions are validated before immutable
publication; request planning does not compile catalog expressions.

`unlists` removes the models in an active group from published menus, without
revoking direct-alias execution. Switching the group off restores those entries.
`sink` moves a rate-limited account behind accounts not rate limited since; this
order survives success and cooldown but is memory-only. A current, exhausted
quota window does not sink. Round-robin, weighted and manual routing ignore sink.
Recorded account metadata distinguishes an expired rest from a retained sink position.
Vendor-specific failure classification and manual rest lifting are not yet the
reference's complete implementation.

Named `levels` override a group's shared reasoning menu without changing its
models' declared capabilities. Codex menus, gateway model lists, Pi's explicit
thinking-level map and OpenCode variants expose the same choices. Explicit
Responses, Chat, Anthropic output-effort and Gemini level fields are fitted to
the destination's supported levels while opaque JSON siblings retain their
lexemes. Native budget-only thinking requests are not reinterpreted as levels.

Members can name the same provider/model at distinct fixed reasoning levels.
Identical seats are rejected. Fixed reasoning and rule output reasoning are
fitted to known destination capabilities; unknown capabilities preserve the
Agent's wire request. Fixed seats have separate continuation identities while
sharing account quota observations.

Rules combine token length, images, reasoning, Agent identity, compacting,
intent, time windows, and working folders with AND semantics. An overnight
window belongs to the day it begins, in its recorded IANA zone. Unknown folders
match no inclusion and are outside all exclusions. Days-only conditions cover
the whole local day without adding invented hours; partial clocks are invalid.
Folder matching is lexical,
including Windows drives and WSL paths; declarations in assistant/tool output
are ignored. Paths remain ephemeral and never enter the trace. A pause rule
excludes its member on every request and only accepts clock, Agent and folder
conditions. Opaque continuations to paused members fail safely rather than being
replayed elsewhere. All-paused groups return a safe 503 without forwarding.
Matching rules' ordered primary/fallback seats precede the strategy tail.
Classification is not requested for intent rules after an already-matching
deterministic rule. Within-turn rule holding remains in memory.

The group logo stack is its drag handle; clicking or right-clicking the row
opens the same move/duplicate/share menu. Model numbers support pointer dragging
and Alt+Arrow keys. The lifted row follows the pointer on animation frames;
neighbors move aside and the scroll container advances at its edges. Escape,
lost capture, blur, resize and unmount cancel without persisting; drag-ending
clicks do not open a menu. Removal lives in the inline editor, not a permanent row action.
The editor uses the reference implementation's label/content columns and keeps rules visible, with
Vary-specific trial/diagnostic controls folded. Explicit classifier selection
activates its intent rules, without sending a trial request on save.
Rules use the reference condition-chip sentence, ordered model chips and a
numbered drag handle. Adding a condition focuses its input and empty edited
chips remain until explicitly removed. Reordering preserves rule identities,
legacy names and keyboard focus. The action menu controls pause/send, output
reasoning and removal. New names are internal labels, not required user input.
The classifier uses the shared grouped model picker; legacy manual entries and
the timeout remain under Advanced classifier settings. No selection sends a
classifier request.
First-content timeout and patience use the reference segmented choices, with
custom minutes converted to integer seconds. Existing non-preset values remain
unchanged until edited; raw seconds are available in Advanced execution settings.
Context uses the reference largest/smallest/custom segments beside those
execution settings. Custom sizes accept decimal `k`/`m` suffixes and underscores,
focus the input on selection and block saving invalid sizes. Saved fixed-effort
variants remain distinct seats behind one provider/model chip. Deselecting a chip
removes all of its effort variants. Bulk deselection promotes each rule's next
surviving target and drops only rules with no targets. Adding catalog members
preserves existing seat identities, settings and rule references. Dynamic pattern
targets remain intact when explicit membership changes.

The group handle menu moves, duplicates and copies a group. Local duplication
retains folder rules and unavailable seats, turns the copy on and inserts it
after the source. Order is stored
separately from group definitions, validates current membership atomically and
propagates to native/shared model menus without revising each group. Manual
routing offers the model choices as inline radio controls.

Templates use the reference implementation's smart, thrifty and steady ranking instead of generic
fallback, rotation or context presets. Public catalog maker prices take
precedence over a relay's tariff; plan/free-tier prices do not impersonate a
maker price. Ranking considers known makers, flagship prices, size words,
release dates and output prices, excluding non-chat and excessively expensive
models. Smart generates compaction, image and hard/easy intent rules; thrifty
adds the high-reasoning rule and cross-provider cheap fallback; steady takes
the strongest model from each of up to three providers. An unavailable template
explains its missing resource rather than inventing a model. Each available
card has one Add action, which opens a dialog populated with the template's
name, models, strategy, rules and classifier. The user reviews or changes these
fields and saves to create the group; canceling creates nothing. Template menu
entries use the same flow, with a fresh group identity and a unique suggested
name. Opening a template never persists it or sends a model request. Public facts are projected through an explicit safe-field API and
never include arbitrary catalog fields or credentials.
Publication-only model facts are stripped from frozen transport snapshots, so
a public price or release-date refresh does not revoke an unchanged destination
or split its account observations. Actual endpoint and credential changes still
change destination identity.

Unsupported options are rejected rather than silently removed. Nested groups,
classifier groups, automatic reasoning and fast mode still require execution
semantics beyond the current schema. Transfer of those unsupported semantics,
account lifecycle and the trace event schema do not reproduce the complete reference behavior; this port
does not establish full engine parity.

## Verification

Synthetic component tests cover the simplified page structure, automatic live
updates, summary counts, masking, trace failure, immediate import, safe sharing,
local duplication,
pointer cancellation, direct Agent model selection and confirmed bulk removal.
Rust tests cover bounded metadata, scoped session digests, lock contention,
response-drop cancellation, first content detection for all four wire protocols,
and a dynamic-port HTTP fixture that records a real 429 fallback and usage while
preserving the response's extension fields. The fixture also distinguishes a
completed stream from a clean EOF without a terminal event.

Visual checks use an isolated browser preview and synthetic catalogs. They do
not launch or replace the installed application, modify a real Agent's
configuration, or establish acceptance against a live vendor.
