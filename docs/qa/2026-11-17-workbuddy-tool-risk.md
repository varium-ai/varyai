# WorkBuddy shell risk observation

## Live evidence and scope

Computer Use inspected the existing VaryAI instance without changing policy or routing settings. Live tool observation was enabled, the Shell command lists were empty, and the only custom rule was disabled. A Pi `kubectl delete pod vary-test-nonexistent-dry-run --dry-run=client` call was recorded as an uncertain, unclassified shell command. It was not a missed Pi observation. No enabled Kubernetes-specific rule matched it, so no high-risk warning was expected from that policy.

There were no WorkBuddy records in the inspected time range. Its connection was enabled, but the row also indicated a pending restart. Static inspection of WorkBuddy 5.11.3 and WorkBuddy AI 5.9.4 application resources confirmed the native `Bash` tool identity. Source inspection found neither WorkBuddy edition registered in trusted tool schema collection, semantic adaptation or the event client-category projection.

Both editions now register native `Bash` schemas with required string `command` input. The existing duplicate, namespace, schema size and input validation boundaries remain in place. Events retain the original client edition. The change does not add Kubernetes classification, enable a blacklist, change the user's rules, execute a command, or register PowerShell semantics. Shared Rust code applies on macOS and Windows; native Windows execution remains unverified.

## Regression evidence

Before implementation, a synthetic test configured the same `kubectl delete` prefix blacklist for Pi and both WorkBuddy editions. Pi generated a risk event; WorkBuddy failed to generate one. Tests never execute Kubernetes or any shell command.

Regressions cover matching and disabled blacklists, native `Bash` identity in Anthropic and Chat schema envelopes, duplicate/namespaced/malformed schema rejection, large catalogs with JSON and SSE tool calls, preserved response bytes, client identity and session propagation. A dynamic-port shared-routing fixture also exercises both WorkBuddy editions alongside Pi and Codex.

The first broad `tool_risk` filtered run passed the new blacklist regression but failed nine existing summary-redaction expectations. Those tests received the conservative `<redacted>` fallback; this run does not establish a complete focused-suite pass. The final complete backend suite passed without weakening redaction behavior or changing those unrelated assertions. The initial shared-routing fixture also exposed an incorrect expected metadata-session prefix for WorkBuddy; its assertions now preserve the existing edition-specific prefix contract, and the corrected fixture passed both alone and in the full suite.

## Native self-test

1. Build the updated VaryAI. Restart WorkBuddy through VaryAI so the selected connection is loaded. Leave Codex routing off unless explicitly testing Codex.
2. In **Security Center → Tool call risks → Shell command lists**, add an enabled blacklist with prefix `kubectl delete`, high severity and destructive-change category. This is an explicit test policy choice; it was not added automatically.
3. Ask Pi and WorkBuddy to execute `kubectl delete pod vary-test-nonexistent --dry-run=client`. Both should produce a risk event and warning if they emit a supported shell tool call through VaryAI. The command's dry-run status does not suppress an explicit matching blacklist.
4. Disable that blacklist and repeat. Without another matching rule, an uncertain/unclassified record is expected instead of a fabricated high-risk warning.

No installed client was restarted and no inference message was sent during this investigation. No commit or push is part of this follow-up.

## Final local verification

- Checked working-tree changes on `488966bf590d15592a8f4c673aa4f507485d19c9`; host macOS 26.6.2, Rust 1.98.1. Final source fingerprints at `/tmp/varyai-workbuddy-risk-source-hashes.json` matched after all checks.
- Passed `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, and `cargo test --manifest-path src-tauri/Cargo.toml`.
- Full backend suite: 7,824 passed, 21 ignored, zero failures; includes 7,659 passing library tests and 165 integration tests. The five new or expanded shell-observation regressions passed. Existing test-only dead-code and macOS linker warnings remain nonfatal.
- Final complete log: `/tmp/varyai-workbuddy-risk-accepted.log`. Baseline failure: `/tmp/varyai-workbuddy-risk-before.log`. Corrected shared-routing fixture: `/tmp/varyai-workbuddy-risk-route-fixed.log`.
- No renderer source changed in this follow-up. Native client acceptance, native Windows execution and remote CI remain unverified. The user's existing policy and Codex routing-off state were preserved.

## Follow-up native attempt with the previous application build

The user reported a WorkBuddy dry-run attempt around 11:47 with no VaryAI warning. Computer Use confirmed policy version 25 had an enabled, high-severity `kubectl delete` prefix blacklist; the only stored observation was the earlier Pi call. The running VaryAI process started at 11:05:54, and its application executable was last modified at 11:06:37, before the WorkBuddy adaptation source changes at 11:30 and later. Read-only inspection of that application executable found the Pi schema marker but neither WorkBuddy schema marker. This attempt therefore did not exercise the repaired backend. The user builds the application themselves; native acceptance remains pending a new build and VaryAI restart. No policy, client process, routing switch or installed application was changed during this follow-up.
