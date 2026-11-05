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
Unresolved Responses item references and tool-search history require their
specialized continuity support; the general IR does not invent missing items.

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
