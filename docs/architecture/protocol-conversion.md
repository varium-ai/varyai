# Protocol conversion

Cross-protocol calls use the same architecture as Magpie: a source reader creates
a protocol-neutral request, a target builder emits the selected upstream's wire
format, and reply readers/stream decoders produce neutral replies/events for the
client's renderer. No vendor's request or SSE format is an intermediate protocol.

The behavior reference is `yetone/magpie` revision
`c7bbd3faf6de7c59ff65848450b0640a7ee2ecb1`, particularly
`internal/gateway/{ir,chat,responses,anthropic,gemini,codeassist}.go`. Attribution
and the MIT license are retained in
[the reference notice](../../third_party/routing-reference/NOTICE.md).

## Boundaries

- Same-protocol calls retain their existing native forwarding path. A protocol
  conversion is planned only when the client and selected upstream differ.
- Routing, model selection, account authentication and connection identity stay
  outside the IR. Builders receive the exact selected model; callers never supply
  the upstream credential.
- Security inspection runs on the original client protocol before conversion.
  The IR receives the inspected body. Tool schemas and arguments are structured
  values, not strings searched or rewritten by the protocol adapter.
- Reversible-redaction leases retain their existing fail-closed conversion
  boundary. Conversion does not grant a new authority to restore protected data.
  Prepared Codex Responses-to-Chat/Anthropic operations retain their existing
  provenance-aware transport until the IR can provide equivalent ownership
  evidence; their supported restoration path is not replaced by an unproven one.
- Native transport must not convert an IR-built body a second time. Gemini's
  model and streaming operation are encoded in its endpoint; transport hints do
  not enter the outbound JSON.

## Common representation

`protocol_bridge/ir` separates request parsing/building from reply parsing/rendering
and incremental stream adaptation. Messages retain their roles and ordered text,
media, file, tool-call, tool-result and reasoning parts. Tool IDs, argument objects,
schemas, explicit strict settings, namespaces and freeform custom input have
explicit representations. System/developer turns are folded only for protocols
whose wire schema requires a separate system instruction.

Reasoning effort, explicit off, budget and visible summaries are independent.
Gemini generation-specific levels/budgets are selected at the target builder.
Opaque reasoning and signatures retain their source identity; they are never
relabeled as a different vendor's signed state. Eligible native metadata, output
formats, cache settings and tool declarations are retained by their target reader
and builder.

Readers extract known protocol semantics instead of rejecting every unknown
object key. Client bookkeeping such as WorkBuddy's assistant IDs, model labels,
usage records and text annotations therefore cannot invalidate a supported text
or tool request. Malformed known values and content that cannot be represented
still produce static errors without including the original data.
Unresolved Responses item references still require their specialized continuity
support; the general IR does not invent missing items. Client-executed Responses
tool discovery is represented explicitly: Chat, Anthropic and Gemini builders
expose the existing `tool_search` function contract, retain call/result IDs and
load returned namespace declarations for subsequent turns. The Responses renderer
uses the original tool context to restore tool-search and namespaced calls in
both JSON and SSE replies. Native Responses items retain their original form;
server-executed discovery is not relabeled as a client function.

Codex desktop also injects page context as `function_call_output` items in the
`agent` namespace with a `codex_apps_` name and no `call_id`. These are context
notifications, not replies to model-issued calls. Cross-protocol readers retain
their source label and content as user-level context without inventing a tool
call or elevating the content to system instructions. Ordinary tool results
still require a nonempty call ID. Native Responses forwarding remains unchanged.

## Streaming and verification

Streams pass through semantic events for message start, text, reasoning,
signatures, tool start/arguments/completion, usage and finish. Content may be
emitted before the reply finishes. SSE frames and accumulated replies are bounded;
transport chunk boundaries and fragmented UTF-8 do not define event boundaries.
Truncated streams cannot synthesize a successful completion, and cancellation
must drop the upstream stream.

Converted Responses streams number every emitted event from zero, including
reasoning, text, tool calls, terminal events and sanitized errors. Each stream
owns its counter. Buffered replies use the same event encoder, while native
Responses forwarding preserves the upstream stream. Strict consumers such as
Grok Build require these sequence numbers even for the first reply.

Coverage includes the twelve cross-protocol pairs among Chat Completions,
Responses, Anthropic Messages and native Gemini; native paths remain separately
covered. Synthetic HTTP fixtures inspect actual outbound endpoint, JSON, model
and authentication, while buffered/SSE fixtures cover tool history, reasoning,
media, usage, finish reasons, split events and incomplete streams. WorkBuddy's
captured structural shapes are reproduced with synthetic content and security
inspection enabled. Live checks use ordinary managed client configuration and
do not establish compatibility with every vendor-specific extension or future
client release.

## Managed official execution

All shared Agent connections use the same IR adapter when their native Chat, Anthropic or Gemini protocol targets a managed ChatGPT/Codex account. No client-name allowlist controls conversion. The existing account executor retains exact destination selection, refresh and authentication; the static API-key executor is not involved. Native Responses requests retain their original wire requirements. Official streaming replies with an omitted SSE media type still pass the strict event decoder, and each client receives its own JSON or SSE format. Private tool reasoning is scoped to the frozen destination and conversation and restored only for matching inspected tool history.

## Gemini search and function combinations

When a converted Gemini request combines native search with function declarations,
the builder enables `toolConfig.includeServerSideToolInvocations`. Automatic or
unspecified function choice uses `VALIDATED`; explicit disabled, required and named
function choices retain their constraints. Function-only and search-only requests
retain their existing configuration and schemas.

Gemini's server-side Google Search `toolCall` and `toolResponse` parts represent
work already executed upstream. The reply reader renders them as completed search
items, never as client function calls. Native parts and their signatures stay in
the existing provider/session-scoped shadow history. Restoration requires an
unambiguous match of the complete visible assistant turn; client-visible metadata
cannot inject native state, and another provider or session cannot borrow it.
Unknown server tool types still fail conversion. Buffered and streamed replies
use the same part reader.

See Google's [tool combination guide](https://ai.google.dev/gemini-api/docs/tool-combination)
and [GenerateContent reference](https://ai.google.dev/api/generate-content), plus
the [Gemini combination verification](../qa/2026-11-06-gemini-tool-combination.md).

## Grok tool-schema compatibility

Cross-protocol requests targeting Grok (including namespaced model IDs on
third-party gateways) escape literal opening brackets and doubled set operators
inside JSON Schema pattern character classes. This preserves ECMAScript character
class meaning when the upstream validates patterns with Rust regex. Only tool
schema keywords are visited; prompts, tool arguments, examples, defaults and
schemas for other models are unchanged. This does not strip unsupported regex
features or discard validation constraints.

Grok tool schemas also omit `required: null`, including nested schema nodes,
because the upstream rejects null instead of treating it as an absent list. Real
required arrays and user data under examples, defaults and const remain intact.

Grok can reject object schemas referenced through local `$ref` entries, including
nullable properties and array items. The adapter expands resolvable local JSON
Pointer references within each tool schema. Reference siblings remain intersected
through `allOf`, preserving their constraints. Unresolved, external, recursive or
scoped references retain the original schema; depth and node budgets also cancel
expansion without dropping constraints. Only schema-bearing keywords are visited,
so examples, defaults and properties named `description` or `title` stay intact.

Hosted search is not a generic Chat Completions capability. Codex can offer a
native `web_search` tool on ordinary turns, but translating that declaration into
`web_search_options: {}` makes regular Chat gateways reject the whole request
with HTTP 400; Grok's retired Chat live-search API returns HTTP 410. The Chat
builder omits these optional native-search declarations while retaining function
tools, schemas, choices, prompts and reasoning. A search-only request with
`tool_choice: "required"` is rejected locally rather than silently running without
its required tool. If no tools remain, optional choices and parallel-tool settings
are omitted as well. Native Chat passthrough and native search on Responses,
Anthropic and Gemini retain their own behavior. This conversion rule does not
implement a replacement search executor; native search needs a supported native
protocol. Magpie's `chat.go` / `gateway.go` likewise avoid injecting search options
into ordinary Chat requests, and handle their search execution separately.

Generic Chat tool conversion also follows Magpie's `buildChat`: function names,
descriptions and complete schemas are portable, while the optional `strict` flag
is omitted. Magpie only opts into emitting that flag in a provider-specific
adapter (`strictSaid`); ordinary conversion does not enable it. Both the IR
builder and prepared Codex/DLP converter apply this rule, including flattened
namespace tools. Schema constraints, tool choices, argument text and DLP
provenance remain intact. This does not provide server-enforced strict outputs
through a generic Chat gateway. Native Chat passthrough and native Responses
keep their existing strict behavior; use those supported native capabilities when
strict enforcement is required.

Intent routing preserves an explicit upstream rejection's HTTP status instead of
labeling it a connection failure. Raw upstream error bodies remain private; only
fixed rejection categories are returned for legacy errors (null required lists,
regex/schema validation and tool-history pairing); no upstream text is included.
Existing sanitized upstream errors retain their safe response path. Transport failures
remain distinct, and ambiguous sends are not replayed at another account.
