# Magpie protocol implementation comparison

Reviewed on 2026-10-22 against Magpie commit `c4551dfcacf67536180291da402ded1d1316a2d8`. The review did not run Magpie or contact real providers.

## Architecture and sources

Magpie parses four protocols into shared Request/Message/Part objects and builds upstream requests with protocol-specific builders. Decoders map upstream streams to shared Events; encoders produce the client protocol. Text, reasoning, images, files, calls, and results have separate representations. Matching protocols usually pass through, with compatibility exceptions.

Pinned sources:

- [ir.go: requests, parts, events, and results](https://github.com/yetone/magpie/blob/c4551dfcacf67536180291da402ded1d1316a2d8/internal/gateway/ir.go)
- [gateway.go: routing, conversion, completion, and errors](https://github.com/yetone/magpie/blob/c4551dfcacf67536180291da402ded1d1316a2d8/internal/gateway/gateway.go)
- [gemini.go: messages, tool results, and images](https://github.com/yetone/magpie/blob/c4551dfcacf67536180291da402ded1d1316a2d8/internal/gateway/gemini.go)
- [translate_severed_test.go: interrupted streams](https://github.com/yetone/magpie/blob/c4551dfcacf67536180291da402ded1d1316a2d8/internal/gateway/translate_severed_test.go)
- [gemini_function_parts_test.go: tool images and roles](https://github.com/yetone/magpie/blob/c4551dfcacf67536180291da402ded1d1316a2d8/internal/gateway/gemini_function_parts_test.go)

## Adapted behavior and exclusions

| Area | Magpie | Vary AI |
| --- | --- | --- |
| Conversion architecture | Shared request/part/event model | Compose existing Anthropic adapters without replacing gateway or storage |
| Gemini result roles | functionResponse belongs to user; split mixed content in order | Adapted: accept model/assistant labels, move results to user, preserve surrounding text and call IDs |
| Tool-result images | Read functionResponse.parts | Adapted: inlineData and fileData image URLs reuse existing conversion; reject other media and unknown extensions |
| Interrupted streams | Track actual completion, errors, and cancellation | Existing coverage retained across 224 HTTP cases |
| Protocol choice | Provider/model-specific choices and endpoint fallback | Keep explicit user configuration; do not retry every HTTP 400 as Chat |
| Gemini snake_case | Normalize protocol keys while preserving business objects | Deferred: normalization must be validated with original-field inspection and redaction writeback |
| Opaque signatures | Signature state and call-ID references | Retain existing Gemini shadow; reject history that cannot be represented faithfully |
| Provider compatibility | Vendor-specific reasoning, pairing, and empty-message repairs | Keep existing adapters; add repairs only with reproducible failures |

The full Go gateway is not a drop-in Rust dependency. This work adapts mapping semantics and regression cases, without introducing a Go service or replacing authentication, DLP, routing, or persistence.

Two failing tests first reproduced assistant-labeled tool results and rejected functionResponse.parts. Subsequent tests covered model/user roles, surrounding text, two image sources, and three cross-protocol upstreams. The HTTP matrix includes tool-image and model-role fixtures. See [four-protocol routing](protocol-bridge.md) for capability and acceptance limits.

## License

Gemini tool-result role and image handling are adapted from Magpie, Copyright (c) 2026 yetone, under the MIT License. The complete notice is retained in [third_party/magpie/LICENSE](../../third_party/magpie/LICENSE).
