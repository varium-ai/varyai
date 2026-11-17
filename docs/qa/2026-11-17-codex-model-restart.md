# Codex default-model restart notice

## Report and observation

The user reported that selecting another Codex model in VaryAI did not show a restart button. Authorized native UI inspection used the already running development VaryAI app. Initially Codex routing was off and the native model label was `gpt-6.1-sol`. Selecting a configured shared entry immediately showed configured/new-session guidance without a restart button. A later observation showed the automatic worker waiting for idle and a restart button. Selecting another shared model while that waiting state was present retained the button.

This demonstrates the delayed route-worker path in the running build, not acceptance of the new source repair. The test did not restart any client or send an inference message. Disconnecting restored the native `gpt-6.1-sol` label, and the final accessibility state explicitly showed the Codex switch off. The attachment/browser tracker was not edited.

## Source cause and repair

The semantic route fingerprint intentionally excludes the default model: changing only the default must not be treated as a routing change that automatically reloads an app-server. However, startup freshness also had no independent default-model field. Separately, a current app-server suppressed the desktop-age restart state, although a current route does not prove that the desktop loaded its new default selection.

Startup snapshots now persist a separate SHA-256 effective-model digest and change time. An active profile model overrides the root model. Existing version-1 snapshots remain readable; absent model fields establish a baseline without inventing an upgrade-time model change. The explicit switch path captures the pre-change baseline and retains its existing post-change snapshot. Failure of this auxiliary observation does not prevent the model switch.

Status composition preserves a real desktop default-model restart requirement even while the route worker reports current. After the desktop has loaded the selection, its actual creation-time assessment clears the requirement. Recovery state does not become proof of active ownership or restart readiness. The public status shape and manual restart execution remain unchanged. The same logic is used on macOS and Windows; other platforms retain their native support boundary.

## Verification boundary

New regressions cover an independent default-model change without route/catalog time changes, harmless rewrites, active-profile precedence, old snapshot migration, private model-name exclusion, worker-current restart presentation, post-restart clearing and recovery-state refusal. The test was added before implementation, but its baseline compilation was interrupted before a red result was produced; no completed red run is claimed.

`cargo fmt --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed after the final source edits. The primary agent completed the combined final Rust check, Clippy, formatting and full test run with the WorkBuddy and stopped-Agent restart fixes: 7,821 passed and 21 ignored. Renderer type/build/format checks and 4,257 tests also passed. See [combined verification](2026-11-17-stopped-agent-restart.md#verification) for the checked tree and host. Remote CI, native Windows execution and new-build native acceptance remain unverified. The user subsequently reported that both requested model/restart fixes passed self-testing. No push or remote CI validation is included in this record.
