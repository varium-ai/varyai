# TRAE edition connection recovery

Base revision: `e2e37b2d` on `codex/intent-recognition`. Host: macOS arm64. Toolchain: Node 22.22.0, pnpm 10.12.3, Rust 1.98.1. Results apply to the source changes in this commit.

## Observed failures

- Two CN application copies existed: the standard Applications copy and a compatibility-directory copy. Both reported CN 3.4.17 despite the compatibility folder's historical 3.4.4 name. The open native window belonged to the compatibility copy, while the bridge runtime came from the standard copy. Its exact resource-root allowlist therefore found no eligible native window. The random endpoint itself was reachable and matched the saved endpoint.
- International 3.6.13 had a ready bridge, but native model delivery returned duplicate-add code 4024. Shared ownership recovery allowed only the CN loopback path and could not recover an international partial delivery.
- The helper's default eight-second evaluation timer and twenty-second socket deadline were inappropriate for large catalog mutations. Model mutations could continue after the caller retried. Full native UI refreshes were also awaited repeatedly inside those mutations.
- CN also had a persisted legacy provider-selection retry. The shared synchronizer ran that replacement before adding its own catalog, so an unrelated legacy recovery failure could block shared delivery indefinitely.
- International Agent-mode model-change notifications return before the native refresh finishes. Recording the protected-credential fingerprint at that point captured an intermediate value: immediate readback matched, but subsequent native refreshes invalidated all thirty saved fingerprints. Awaiting the native forced model refresh before recording proofs resolved this without relaxing credential comparisons.
- A starting-model adapter that was not initialized in a particular chat window prevented a delivered catalog from becoming an active connection.

Diagnostics retained only capability flags, counts, stages and recognized numeric errors. Native credentials, account IDs, model catalogs and conversation text were not exported to diagnostics.

## Changes

Bridge startup selects the verified running application copy on macOS and Windows. Ambiguous identities fail closed. Each edition retains its own paths, consent, credentials and outbox. Shared synchronization no longer executes a legacy provider replacement first; legacy configuration and retry metadata are retained, and the existing legacy worker remains excluded while a shared outbox owns the client. Startup is serialized per edition, and the merged helper revision is 18.

Native edits now send the mutable fields used by the installed vendor form, rather than resubmitting immutable provider/model identity fields. Diagnostics preserve only recognized stage identifiers and numeric native errors. Shared ownership recovery recognizes both exact edition routes while retaining model, endpoint, protocol, label and uniqueness checks. Native mutations share a window-local queue, use a five-minute operation deadline and a longer transport deadline, and wait for a compatible old bridge to finish on upgrades. Native UI refresh barriers are retained before authoritative native API readback; an asynchronous refresh without that barrier was rejected during diagnosis because it could capture stale protected credential representations.

Catalog delivery and default selection have separate outcomes. A pending default no longer revokes a delivered catalog or rewrites it repeatedly. Native model rejection has its own status. Waiting text names TRAE CN or TRAE International explicitly in all four runtime languages.

## Verification

`pnpm typecheck` and `node --check` for the changed bridge implementation passed. `pnpm dev --no-watch` compiled and launched the final helper revision 18 with the changed Rust source (dev build completed in 20.94 seconds). A separate `cargo check` was interrupted to release the build lock for that development rebuild. Source and staged whitespace were inspected.

The two native clients were normally quit and reopened without a save/discard prompt to release previously hung evaluations. No replacement application, downgrade, extra compatibility copy or new control grant was installed. Existing saved model intentions were reapplied. Both CN 3.4.17 and International 3.6.13 showed the configured/new-session status on the homepage. Native readback matched all thirty desired owned rows and credential proofs for each edition; a subsequent independent read retained all thirty matches. No conversation was sent, so these observations establish catalog delivery and readback, not successful forwarding of a new conversation.

The renderer build, test suites, dependency audit, standalone Cargo checks, formatting/Clippy CI checks and remote CI acceptance were not run at the user's explicit request. Regression fixtures were updated but not executed. Windows runtime and cross-platform acceptance remain unverified. These observations do not establish compatibility with every future vendor release or successful conversation forwarding.
