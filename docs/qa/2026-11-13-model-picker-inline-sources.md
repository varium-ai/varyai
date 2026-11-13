# Inline model source labels

This report covers VaryAI's internal React picker only. Follow-up user evidence
showed that the external DeepSeek Harness menu still lacked source suffixes;
the internal-picker result did not establish completion of the external-Agent
requirement. The follow-up must verify the shared backend catalog and each
Agent's generated configuration, including existing connections.
See [managed Agent source labels](2026-11-13-agent-source-labels.md).

## Scope

The shared picker now renders managed choices as `Model name · Provider name`
in the model-name span, including unique models. Routing groups retain
`· routing group`. Custom names already containing their source avoid another
suffix. Native client labels remain unchanged. The separate trailing provider
note and its unused CSS were removed. Source labels, accessible names and
tooltips respect account masking. Public references, saved selections, request
IDs and the existing same-name identity hints remain intact.

The reference is Magpie's default `Entry.Label` behavior in
`internal/provider/modelprefs.go` at revision
`c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6`, alongside the user's supplied
inline-source screenshot. This change concerns the shared React picker; it does
not add Magpie's optional suffix settings or change the separately edited Rust
catalog publication path. The renderer implementation is shared by macOS and
Windows.

## Provenance

Git blame attributes the original frontend suffix removal to `089af966`
(November 3), and the shared menu's switch to bare direct-model names plus
collision-only Codex suffixes to `4ad92a29` (November 12). Both commits have the
author name `Guitar`. Git author metadata does not identify a coding-agent chat
or establish who personally wrote individual lines. The latter commit's bare
shared labels are the direct change relevant to the reported missing sources.

## Verification context

- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b` with unrelated
  working-tree changes preserved.
- Host: macOS, Apple Silicon.
- Node 24.11.0, pnpm 10.12.3, rustc 1.98.1.
- Checked task-file hashes: `/tmp/vary-model-suffix-source.json`.
- Logs: `/tmp/vary-model-suffix-*.log`.
- Four new inline-source cases failed before the implementation change.
- Final picker, branding and model-visibility suites: 40 tests passed across
  three files, covering all four picker modes, existing suffixed names, custom
  names, native names, masking, filtering, selection and saved ordering.
- TypeScript typecheck, renderer build, frontend format check and diff whitespace
  check passed.
- Development helper tests: 8 passed after allowing temporary loopback listeners;
  the sandboxed attempt failed with `EPERM` when binding those listeners.
- DMG, Trae bridge, model-default, WorkBuddy, Doubao and rewards helper tests
  passed. Qoder codec tests: 2 passed, 1 skipped by its installed-codec gate.
  Shared-v23 recovery tests: 5 passed.
- Production dependency audit passed after enabling registry access; the first
  attempt could not resolve the npm registry.

## Acceptance gaps

The complete frontend suite finished with 310 files passing and one failing:
4,148 tests passed, and six tests in `ProviderForm.protocolFormats.test.tsx`
exceeded the existing five-second timeout. Logs also contain unresolved
`tauri.local` requests; the timeout cause has not been established. These are
outside the changed picker tests. An isolated rerun of that file, without
changing tests, timeout limits or application code, passed all 35 tests in
30.27 seconds. The earlier complete run still must not be described as passed.
After tightening picker assertions to require exact suffixed labels, its three
focused suites passed all 40 tests again and typecheck passed again. Production
renderer files match the hashes used by the complete test run and renderer
build; the task-file snapshot records the final test assertions.

Rust formatting failed on the existing module ordering in
`src-tauri/src/services/mod.rs`, which this change does not edit. Cargo check
waited on another build's lock and was stopped. Clippy, library tests and full
Rust tests were not reached. No backend source was changed by this task, and
these incomplete checks do not validate the independent backend edits already
present in the shared checkout.

No commit, push, remote CI run, installed application launch or replacement was
performed. Live macOS/Windows UI and installer acceptance remain unverified.
