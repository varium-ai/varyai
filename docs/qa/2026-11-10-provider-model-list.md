# Supplier model list simplification — 2026-11-10

The provider model section no longer offers Vendor list, Picked only, Best in
family or Usage only mode buttons. API providers, Codex and the six supported
subscription providers expose the model list directly. Codex also shows unselected
catalog entries immediately instead of hiding them behind Show others.

Model selection, search, refresh, manual IDs, image-model separation and model
preferences remain available. Saved selections and manual entries remain visible
alongside the supplier catalog. Existing stored publication policies are preserved;
opening or saving a usage-only provider does not silently enable Agent forwarding.
The storage format and backend policy support remain compatible with imports.
These shared React changes apply to macOS and Windows.

## Verification

The test-first run failed eight targeted assertions before implementation. After
removing the mode controls and the Codex selected-only display gate, the three
affected component suites passed all 97 tests. Coverage includes all six
subscription providers, newly fetched Codex models appearing immediately, saved
selections, usage-only policy preservation and cancellation without persistence.

Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, plus the existing shared
working-tree changes. Host: macOS arm64; Node 24.11.0; pnpm 10.12.3; Rust/Cargo
1.98.1 stable. This change modifies the three provider form components and their
three component test files. Other ongoing work in the checkout is preserved.
Logs, the pre-change snapshots and SHA-256 hashes of the six changed files are
under `/tmp/vary-provider-model-list-20261110/`.

`pnpm build:renderer`, `pnpm format:check`, all seven JavaScript helper suites,
the storage recovery helper and `pnpm audit --prod` passed. Prettier also passed
for the three changed component test files after formatting.

The initial full unit attempt exited unsuccessfully during disk exhaustion;
independent DesktopLongestWindow and ProviderKeysField failures were also visible.
Only generated Vitest cache files were removed, and no other task's build process
was stopped. The interrupted backend attempt was waiting on another build's lock.
A retry was started once disk space became available.

The type-check retry reports the concurrently added DesktopLongestWindow test's
missing component and API methods. `cargo check` passed; `cargo test --lib` failed
to compile the concurrent longest-context tests because their new settings fields
and helper functions were not yet implemented. Rust formatting reports changes
in unrelated backend files. These files are outside this change's scope and were
preserved. The shared working tree continued to change during these checks, so
these failures describe the attempted revisions, rather than a stable acceptance
baseline. The full unit retry passed all 3,678 tests across 287 files in 145.90s.

The final frontend recheck passed type checking, renderer build, source formatting
and formatting for the changed tests. The three component suites passed all 99
tests after incorporating other concurrent test additions. The final Rust format
check also passed after the concurrent backend edits were formatted.
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` passed.

After another concurrent context-metadata change in ProviderEditorDialog, the
affected checks were repeated: type checking, renderer build, source formatting
and all 99 component tests passed. The six-file hash manifest matches this final
frontend acceptance tree. The earlier full unit result applies before that
concurrent change; the affected suites validate the updated provider form.

The final `cargo test --manifest-path src-tauri/Cargo.toml` attempt failed during
test compilation: six calls to `normalized_static_provider` in
`src-tauri/src/services/intent_routing/execution.rs` still omitted its newly added
`preserve_key_scope` argument. This task does not modify that backend code or its
tests. Full backend verification remains incomplete; no failed assertion or
compiler check was suppressed.

No installed application was launched, installed or replaced. No commit or push
was requested; remote CI and installer acceptance are outside this verification.

## Bulk model button styling follow-up

Select all and Select none now share the model-footer action rules across API,
Codex and subscription forms. The change is limited to selectors in
`src/components/providers/providers.css` and preserves existing behavior.

An isolated browser fixture reproduced the mismatch using the existing compiled
base CSS, workspace tokens, source provider CSS and the API buttons' actual
utility classes. Before the fix, bulk buttons had a zero-width border, 12px
radius, 11.5px text and a stronger shadow. Footer buttons had a 1px border, 6px
radius and 12px text. Disabled bulk buttons faded the whole control.

After the fix, browser computed styles matched for both rows: 27px height,
1px border, 6px radius, 12px text, 2px/7px padding and a 4% 1px shadow. Enabled
and disabled text colors matched in both light and dark themes; disabled buttons
retained the full-opacity border. Hover and disabled rules are shared. The
synthetic preview screenshot, CSS baseline, final CSS hash and check logs are in
`/tmp/vary-model-bulk-style-20261110/`. The preview tab and server were closed.

Verification used the same base revision and macOS 15.3.1 arm64 toolchain recorded
above, plus the current shared working tree. Type checking, renderer build,
Prettier and Rust formatting passed. The full frontend run passed 3,686 tests and
reported one 5,000ms timeout in the unchanged App project-switcher test. That exact
test passed its isolated rerun in 1.04s; no assertion, timeout or application code
was changed for it. The full-suite attempt remains recorded as a failure.
`cargo check` passed after waiting for another build's lock.

The Rust library attempt reached its 180s command limit after time spent waiting
and compiling. Its already-started test binary completed afterward: 6,273 passed,
19 ignored and one failed Claude Desktop provider/profile synchronization test
(`update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active`).
The Clippy attempt did not complete after waiting on another build's lock and was stopped;
the full Rust integration command was not run in this follow-up. Only this
task's waiting check runner and its child processes were stopped. Backend
verification is incomplete and the unrelated source changes were preserved.
No commit, push or installed-application launch was performed for this CSS change.
