# Concurrent conversations and Magpie affinity

For acceptance of the combined submission, see the
[consolidated pre-submit record](2026-11-10-pre-submit-verification.md).
The results below describe this task's intermediate working trees.

## Reference and reproduced failure

Reference revision:
[`4869d0c9665e1a815fee3a1a4d67a575e0baab93`](https://github.com/yetone/magpie/tree/4869d0c9665e1a815fee3a1a4d67a575e0baab93).
The reviewed functions are `affinityID`, `conversationID`, `conversationOpening`,
`writeOpening`, `oneTextPart`, `userOpens` and `subagentOf` in
`internal/gateway/gateway.go`, `userWords` in `prompt.go`, and `stickKey`,
`affine`, `answeredStanding` and `opened` in `affinity.go`. The existing MIT
license and [attribution](../../third_party/routing-reference/NOTICE.md) apply.

An isolated export of base revision
`82204c3fca2ed229f40fd5bfda5c4daf33967d4f`, with the new concurrent HTTP
regression added, reproduced the reported failure: while the first SSE response
remained open, request 1 returned HTTP 409 where HTTP 200 was required. This was
an expected failing regression run, not verification of the final source tree.

The previous code selected `prompt_cache_key` before native session headers and
held an owned conversation mutex guard through upstream I/O and response EOF.
An unnamed continuation scan also rejected a request whenever any candidate
conversation was locked, without establishing that it owned the continuation.

## Implemented behavior

- Session-header precedence, `X-Magpie-Session` fallback, opening hashes and
  normalization, and Claude subagent separation follow the pinned Go functions.
  Shared environment/instruction-only openings include the following user-owned
  message. A cache key remains an upstream parameter, not local session identity.
- A conversation mutex protects synchronous in-memory lookups and updates. It
  is released before classification, credential preparation, upstream waits,
  response yields, and stream cancellation. An Arc retains the conversation
  through the response lifetime.
- Independent and same-session requests may forward concurrently. No busy
  rejection, request queue, polling retry, or application restart is added.
- Existing authenticated-consumer and frozen-destination boundaries, DLP, and
  observed ownership collisions remain enforced. An opening-derived affinity
  cannot authorize unknown or mixed tool-call ownership. Existing observed
  continuation discovery still supports clients that omit their original opening.

This is a shared Rust implementation for macOS and Windows, without a new
platform-specific execution branch. It does not change the continuation store
format or clear existing state.

## Regression evidence

`testdata/magpie_identity.json` contains 38 synthetic expected identity values
emitted by the original pinned Go function bodies in a temporary standard-library
harness. JSON helper adapters read valid synthetic fixtures; this is not a build
or live-account acceptance test of the complete Magpie application. The Rust
golden test also passed in a standalone harness before application compilation.

The vectors cover all supported session headers and their precedence, whitespace,
`X-Magpie-Session`, cache-key independence, subagents, raw string inputs, ordinary
and shared-environment openings, later turns, intervening system messages,
images, tool-result boundaries, numeric lexemes, and cache-mark normalization.

The HTTP regression keeps five upstream streams open concurrently, including two
requests in one native session, another session sharing its cache key, and two
unnamed conversations with the same environment preamble. It checks that both
overlapping response owners remain observed, distinct native sessions retain
separate state, and a sixth request forwards after cancellation. The gateway
regression exercises concurrent same-session requests and separate consumers.

Rotation and deterministic-rule fixtures now explicitly identify independent
conversations rather than reusing one identical unnamed opening. Their existing
route/model/effort assertions are retained. Unknown/mixed/colliding tool-owner
refusal assertions remain enforced.

## Verification environment

Host: macOS Darwin 24.3.0 arm64; rustc/Cargo 1.98.1, Node 24.11.0,
pnpm 10.12.3. The checked source is the base revision above plus working-tree
changes. Other concurrent work in the checkout includes native Codex admission,
official tool bridging, model-picker metadata, authentication helpers, version
metadata and macOS packaging; those changes are not attributed to this fix.

Logs, command outcomes and per-command Rust source manifests are under
`/tmp/varyai-magpie-concurrency-checks-20261110/`. Final backend commands use cached
offline dependencies, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, and
`CARGO_TARGET_DIR=/tmp/vary-magpie-parity-target`. Test processes use a dedicated
temporary `CC_SWITCH_TEST_HOME` and append loopback addresses only to their own
`NO_PROXY`/`no_proxy`. Tests use synthetic bodies/credentials, in-memory databases
and dynamic listener ports.

Two intermediate backend builds failed because the disk filled during code
generation. Task-owned cloned build caches were removed, and verification
returned to the shared cache with serial commands. Those attempts are not
acceptance evidence. The first focused routing run passed the new concurrency
and identity tests, but exposed outdated busy/independent-session fixtures and
the opening-derived tool-ownership admission issue; these were corrected before
final verification. It also exposed an independently changing native-admission
fixture, which is reported with the final results rather than silently skipped.

No application launch, installation, live-account request, commit, push, or remote
CI acceptance is performed by this task. Local macOS results do not establish
Windows, Linux, WSL2, installer, signing, notarization, or live-client acceptance.
