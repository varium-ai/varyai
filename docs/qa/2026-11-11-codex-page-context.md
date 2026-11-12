# Codex unsolicited page context conversion — 2026-11-11

The repeated desktop failure is distinct from the preceding tool-search fixture.
Failed-conversation rollout records contain `function_call_output` items in the
`agent` namespace for `codex_apps_open_page` and selected-page parts, without a
`call_id`. These represent automatically supplied page context, rather than the
result of an assistant tool call. No private page content or request input is
included here.

The IR previously required a nonempty call ID for every function result. The
synthetic regression reproduces `Missing protocol string` before this fix. The
preceding tool-search implementation did not address this independently failing
input shape. Its passing tests therefore did not establish acceptance of the
reported desktop conversation.

The parser now preserves these recognized, unsolicited context items as user
context with a descriptive tool-context label. It does not fabricate a call ID,
discard the content or promote it to a system instruction. The exception is
limited to the `agent` namespace, the open-page name and numbered selected-page
parts, and only missing/null call IDs. Empty IDs and ordinary tool results still
fail validation; page items with real IDs retain normal tool-result semantics.
Native Responses forwarding is unchanged. This shared Rust change covers macOS
and Windows.

Unit regressions exercise missing/null IDs and the narrow exception. The existing
twelve-edge HTTP fixture now includes synthetic page context in its Responses
requests, including a security-redaction marker, to check initial and history
requests, preservation and inspection before outbound conversion.

Verification uses macOS 26.6.2 arm64 and Rust/Cargo 1.98.1. Synthetic tests use
temporary storage, in-memory databases and dynamic ports. Logs and final source
inventories are under `/tmp/vary-codex-page-fix/`; pre-fix evidence is in
`/tmp/vary-codex-400-red.log`. A separate synthetic live diagnostic returned 401
before conversion and is not counted as a reproduction or inference acceptance.
The expanded HTTP fixture also reproduced `routing_unknown_continuation` because
conversation binding treated the unsolicited item as an owned tool result.
One common predicate now identifies this narrow shape in IR parsing, continuation
validation and security history classification/extraction. Content inspection,
reference scanning and actual call/result ownership checks remain in place.
The predicate grants no restoration or execution authority. The Chat assertion
was corrected to inspect semantic text parts rather than require a single-string
wire representation.

The renderer checks in `/tmp/vary-codex-400-checks/` pass type checking, formatting,
build and production dependency audit. Helper checks report 107 passed and the
three previously recorded TRAE bridge failures. The full renderer run has
four failures: the three recorded authorized-model-save cases and a
`ProviderAddSheet` focus assertion. An isolated rerun of that unchanged test file
passes all 11 cases (`/tmp/vary-codex-page-fix/provider-focus.log`); the full-run
failure is still disclosed. Final backend checks and deployment outcomes follow.

## Final outcomes

- Formatting, Cargo check and Clippy with warnings denied pass.
- Shared bridge tests: 82 passed. Intent routing tests: 215 passed, 1 existing
  ignored test. Backend source hashes remain identical throughout final checks.
- Full Rust library and no-fail-fast runs retain the recorded TRAE library and
  `profile_bundle` fake-bridge aborts. Other test programs pass; full acceptance
  and remote Windows/Linux/WSL2 CI remain incomplete.
- The development application builds successfully. The old development process
  exits normally, and `pnpm dev --no-watch` starts the updated backend using the
  pinned Node runtime, normal application storage and the existing development
  bundle. Gateway health returns HTTP 200. No installed application is replaced.
- The Codex switch is enabled through VaryAI's existing interface. ChatGPT is not
  manually quit or restarted, and no interface source is changed for this fix.
- A historical display alias is no longer published and returns 404. The final
  probe instead resolves a currently published direct alias for the exact
  historical candidate's upstream execution identity and model. It sends only
  synthetic page context and a short synthetic prompt, using the local route's
  existing credential in memory. That real upstream request returns HTTP 200 and
  a terminal `response.completed` event. No prompt, credential or model response
  content is logged in this report. This establishes live forwarding of the
  previously rejected structural shape, not replay of the private conversation.

Final logs and source inventories are in `/tmp/vary-codex-page-final/`, including
`verified-results.json`, `verified-before.json`, `verified-after.json`,
`build-app.log` and `development-runtime.log`. No commit or push was performed.
