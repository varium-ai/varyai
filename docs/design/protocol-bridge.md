# Four-protocol routing

## Configuration

The provider's upstream format describes the protocol accepted by that provider. Claude, Codex / Grok Build, and Gemini forms use the same four names: Anthropic Messages, OpenAI Chat Completions, OpenAI Responses API, and Gemini Native generateContent. Selecting a format does not change the client's native protocol. Enable local route takeover when the protocols differ. Codex / Grok Build clients continue to use Responses. Official OAuth presets retain their authentication and protocol restrictions; conversion cannot expand account permissions.

Configure the real provider URL, key, and model. Do not use the local proxy as its own upstream. Gemini official OAuth does not expose custom format selection; third-party providers can select any supported format.

## Conversion matrix

| Client protocol | Anthropic upstream | Chat upstream | Responses upstream | Gemini upstream |
| --- | --- | --- | --- | --- |
| Anthropic Messages | Native | Existing conversion | Existing conversion | Existing conversion |
| Chat Completions | Added conversion | Native | Added conversion | Added conversion |
| Responses | Existing conversion | Existing conversion | Native | Added conversion |
| Gemini generateContent | Added conversion | Added conversion | Added conversion | Native |

Seven missing directions are composed from the existing Anthropic adapters. Provider selection, model mapping, retries, and usage attribution remain in the client's existing forwarding chain. Historical Chat clients without an explicit upstream override retain native forwarding. Chat Completions does not include the legacy `/v1/completions` endpoint.

New directions support JSON and SSE, text, images, parallel function calls, argument deltas, call IDs, and referenced tool results. Unsupported semantics return explicit errors instead of silently losing behavior: multiple choices/candidates, unsupported structured outputs, strict tool constraints, explicit image-detail settings, unknown tool-history extensions, opaque Gemini thoughtSignature history that cannot be represented, and unadapted native tools. Same-protocol routes retain existing behavior.

Claude Chat/Responses key authentication uses Bearer; Anthropic retains explicit API-key/Bearer selection. Gemini conversion requires a real finish reason and rejects malformed events, upstream errors, and multiple candidates. Usage-only tail frames update final accounting. Rebuilt responses remove invalid entity headers; converted tool observations use ConvertedUnknown quality. New Chat/Gemini streams cap each event and tool arguments at 2 MiB and concurrent active tools at 256. Interrupted streams never synthesize successful completion. Conversion errors do not echo request bodies, keys, or opaque history.

## Automated coverage and limits

| Area | Coverage |
| --- | --- |
| Basic text | 16 directions × JSON/SSE: 32 local HTTP cases check paths, instructions, user/assistant history, and output |
| Tools and images | 32 additional cases check base64 PNGs, two calls, IDs, arguments, historical calls, and results |
| Models and usage | Actual mapped upstream model, URI-based Gemini model selection, input/output counts, cache split, and Gemini usage tails |
| Authentication and redaction | Upstream-specific synthetic credentials, no client credential forwarding, synthetic GitHub-token redaction before upstream delivery, and explicit Anthropic Bearer/API-key regressions |
| HTTP errors | 16 directions × JSON/SSE × 400/401/429/503: 128 cases preserve status and readable reasons |
| Stream errors and cancellation | 16 error and 16 cancellation cases reject false success and release upstream connections |
| Unrepresentable inputs | Explicit rejection of multiple candidates, strict tools, image detail, unknown tool history, and incompatible signatures |

The matrix contains 224 local HTTP scenarios. Unit tests additionally cover interleaved arguments, byte-by-byte UTF-8, event-size boundaries, truncation, namespaces, refusals, output limits, and usage-only interruption. Tests use synthetic fixtures and dynamic ports; they do not switch real providers or edit client settings. These contracts do not establish compatibility with every provider, OAuth service, audio/video input, or hosted tool.

Generate fixture-only evidence with:

```sh
VARYAI_PROTOCOL_MATRIX_REPORT=/tmp/varyai-protocol-matrix-evidence.json \
  cargo test --lib --manifest-path src-tauri/Cargo.toml \
  proxy::protocol_matrix_tests
```

The report includes upstream paths and request JSON, client status/body, and failure/cancellation categories. It contains no real provider credentials.

Gateway model allowlists/budgets and reversible placeholder restoration are separate work. This change validates existing DLP forwarding and routing; it does not claim their combined acceptance. Legacy gateway work remains preserved in the original repository's stash.

## Manual checks

1. Edit a third-party provider and select its actual upstream format, URL, key, and model. Do not infer Responses support merely because the client is Codex.
2. Enable route takeover and start a new client conversation; restart desktop clients when prompted.
3. Check plain text, tool calls, consecutive tool results, and images. A successful minimal probe alone does not prove full client compatibility.
4. Save and reopen the provider. Codex Gemini format must remain visible; Gemini persists its choice in meta.apiFormat, and switching presets restores native defaults.
5. Switch back to a matching-protocol provider and verify native forwarding.

A recorded 2026-10-21 case had a successful minimal Grok-Codex Responses probe but HTTP 400 on the full Codex request. Selecting Chat Completions worked. This preserves the existing Responses-to-Chat route without claiming the provider never supports Responses.

## Provenance and verification

The legacy implementation first reproduced Chat-to-Anthropic requesting the Chat endpoint, then passed the matrix after restoration. Follow-up failing tests covered false Gemini completion, ignored Chat errors, authentication, usage tails, candidate truncation, signatures, and tool-result images.

Legacy checks on 2026-10-22 passed typecheck, renderer build, 2168 frontend tests, 3801 backend library tests (12 ignored), and 28 Profile integration tests. Magpie follow-up passed 3803 library tests, including the updated matrix. These are historical results; migration validation is reported separately.

Gemini tool-result role correction and image support adapt [Magpie's implementation](protocol-magpie-comparison.md), with its MIT notice retained.

Protocol references: [Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create), [Gemini generateContent](https://ai.google.dev/api/generate-content), and [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming).
