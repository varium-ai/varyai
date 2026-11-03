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

Routing uses the same palette for the stage, selected requests, account
observations and inline group editors. Request rows adapt to their own column
width; accounts move beside requests only when the content has at least 1150px.
The stage folds below 560px of its own width. Narrow windows retain icon
navigation, and reduced-motion settings suppress control and stage movement.

## Page structure

- Requested models use readable catalog names in the stage, selected-request
  header, and request list. Internal `vary-auto/` and `vary-provider/` execution
  aliases remain unchanged and are available as label tooltips. Historical
  direct requests can fall back to their recorded upstream model after a provider
  is removed. Routing groups retain their group identity rather than adopting
  the name of a fallback member. Missing metadata is labeled unavailable.
- The header links to groups, opens a new inline group, and masks provider names
  in the stage, request list, account observations, and group editor.
- The stage shows the selected request's Agent, gateway, and candidate order.
  Moving paths indicate an active request or playback of recorded decisions.
  A completed request has stationary paths.
- Selecting a request shows the decisions recorded while forwarding it:
  strategy order, continuation pinning, conversation affinity, intent preference,
  attempts, context/image exclusions, rejection status, acceptance, and completion
  or cancellation. Replay uses those events' relative timestamps.
- Request controls select dates, group recorded sessions, filter call purposes,
  and choose visible metrics. The purpose menu follows the reference implementation: it appears only
  for multiple distinct purposes or an active filter, supports several checked
  purposes without closing, and offers All purposes and a clear button. Agent
  names, final errors and fallback attempts are not purpose-menu options.
  Equivalent call-kind spellings share a purpose; an absent kind is Unmarked,
  not an inferred conversation turn. Session grouping and metric
  preferences are stored locally. History dates come only from persisted gateway
  traces; the last 2,000 requests per day remain replayable, with a truncation notice.
- Accounts and keys are folded from the candidates of the listed requests,
  including untried standby routes. The full model catalog never supplies rows.
  A provider heads its observed accounts/keys; models answered by one credential
  appear below it. Completed attempts, answers, bounded failure categories and
  the last answer time are counted over those requests. Cancelled attempts are
  excluded. Imported session usage and provider/model summaries never supply
  routing requests or account observations; they remain in Statistics. With no
  gateway requests, the account column is hidden rather than filled from a catalog.
- Three or more keys start folded. Three to five subscription accounts start
  expanded; six or more start folded, matching the reference implementation. Explicit folding choices
  persist locally. Model cooldowns name the affected models and do not mark an
  entire account unavailable. A green verification dot is not inferred from
  absence of a cooldown. API keys do not display subscription-quota placeholders.
  The list has a bounded scroll area at every window width.
- Live quota readings and cooldowns match an exact observed credential and model,
  using nonblocking scheduler snapshots. Historical views do not use today's
  quotas. Unknown or expired quota data is not shown as zero usage. Refresh is
  offered only for live subscription observations and rereads telemetry; it does
  not issue a vendor quota request or health probe.
- Groups retain inline expansion, persistent model selection, order changes,
  strategy selection and publication switches. Templates use available catalog
  models. Import immediately saves a new group and reports missing/substituted
  models and omitted rules. Bulk deletion requires confirmation,
  uses revision checks, and retains unsuccessful selections on partial failure.

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
runtime; no key, endpoint or workspace is exposed. Configured provider display
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
No native session files are read by this recorder or by the routing page.

Files contain the same safe metadata as the live trace, including candidate
observations, purpose, scoped session digest and relative decision timestamps.
Scheduler weak references and all credentials are excluded from serialization.
History never borrows current credential identities or current quota readings.
Unknown timing, throughput and prices are omitted; missing values are not shown
as zero latency or zero cost. The existing usage queue and native session imports
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
The account panel distinguishes an expired rest from a retained sink position.
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
focus the input on selection and block saving invalid sizes. Model rows expose
their fixed reasoning menu without opening legacy member settings; identical
provider/model/effort seats cannot be selected twice.
Removing a member promotes each rule's next surviving target and drops only
rules with no targets, instead of blocking removal or leaving dangling seats.

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
explains its missing resource rather than inventing a model. Add persists it
directly; Edit first opens an unsaved draft. No template action sends a model
request. Public facts are projected through an explicit safe-field API and
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

Synthetic component tests cover request/stage selection, masking, grouping,
metric filters, trace failure, immediate import, safe sharing, local duplication,
pointer cancellation, direct Agent model selection and confirmed bulk removal.
Rust tests cover bounded metadata, scoped session digests, lock contention,
response-drop cancellation, first content detection for all four wire protocols,
and a dynamic-port HTTP fixture that records a real 429 fallback and usage while
preserving the response's extension fields. The fixture also distinguishes a
completed stream from a clean EOF without a terminal event.

Visual checks use an isolated browser preview and synthetic catalogs. They do
not launch or replace the installed application, modify a real Agent's
configuration, or establish acceptance against a live vendor.
