# Codex readable aliases and discovery recovery

## Scope

Base revision: `c7f0ce6ac8639c3ad969cd85f3e9bc22ad562ce9`, with the existing staged
work and the additional working-tree changes in this session. Verification runs
use this checkout, not a different branch. Host: macOS Darwin 25.6.0 arm64;
Rust 1.98.1, Node 20.20.2, pnpm 10.12.3. Cargo checks use `CARGO_INCREMENTAL=0`.

The Codex connection now publishes readable ASCII wire names and keeps full
Unicode picker labels. The receipt retains name-to-stable-ID mappings across
refreshes and renames. Duplicate names receive stable numeric suffixes; removed
names cannot silently move to another destination. Old hash IDs retain hidden
capability metadata. Both native and standalone ingress normalize aliases before
routing and continuation checks, editing only the request's model string.

## Reproduced failures and evidence

- The CLI model-change notice formats the wire model ID, independent of the
  catalog display name. Initial synthetic picker tests reproduced a readable
  picker label followed by a hash in the change notice.
- Publishing Unicode wire names fixed that notice but introduced a separate
  client failure: Codex also writes the ID into `x-codex-routing-hint`. Its header
  conversion rejected the middle-dot separator and Chinese text before the
  request reached VaryAI. A regression test failed with the Unicode projection
  and passed after ASCII normalization. Five isolated alias tests pass, covering
  metadata retention, duplicate names, rename/removal, reserved IDs and Unicode.
- The installed CLI was 0.166.0 while its shared daemon was 0.167.0. Authenticated
  loopback model discovery returned 200 for 0.166 and 400
  `routing_native_client_unsupported` for 0.167. The native version guard now
  includes 0.167 while still rejecting unreviewed versions.
- The daemon also held older credentials for the same account and subject. Its
  cached token received 401 while the current file credential received 200.
  The daemon's guarded `getAuthStatus` refresh reloaded the current credential;
  the login file remained byte-for-byte unchanged and the daemon was not restarted.
  Its visible model list changed from nine bundled models with zero custom entries
  to 38 entries including 30 custom models.
- Ordinary CLI startup through the shared daemon displayed the custom models.
  Session-only model selection showed a readable change notice. After the ASCII
  correction, a normal CLI chat using an ASCII Grok alias completed with `OK`.
  All 38 visible IDs in the refreshed catalog were ASCII. Full Unicode names remain
  display labels only; a conversation still selecting a previous Unicode ID must
  reselect its model because the client can fail before contacting the gateway.

## Checks

Frontend typecheck, renderer build, format check and all 3,475 unit tests in 279
files passed. Development launcher, Trae bridge, WorkBuddy authentication,
Doubao check-in, agent rewards, Qoder codec and shared-storage recovery helper
tests passed. The production dependency audit passed.

Final `cargo check`, `cargo fmt --check`, Clippy with `-D warnings`, and
`git diff --check` passed. The completed full Rust run used `cargo test
--no-fail-fast`: 6,047 library tests passed, 18 were ignored, and one existing test
failed: `database::routing_upstreams_compat_tests::provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`
with `routingUpstreams.invalid`. All 163 integration tests passed. This is not a
fully green backend acceptance result. The earlier standalone `cargo test --lib`
attempt failed during linking when concurrent development builds removed dependency
files; the subsequent full run compiled and executed that same library test suite.

Earlier runs found three assertions that still expected hash IDs in generated
configuration; they now assert the readable selection and retained canonical ID.
No source files changed after the completed full run and the final check retry.
The known provider-notes compatibility failure predates this work and remains
outside this change. No commit or push was made.

## Operational limits

No installer, release, signing or cross-platform CI acceptance is established.
The existing `pnpm dev` watcher automatically rebuilt and reloaded the running
application when source files changed; this was not an explicit install or
manual restart. Some concurrent Cargo attempts lost fingerprint or dependency
files during those rebuilds and were retried after source changes stopped.

## Verified source fingerprints

SHA-256 values of the final source files checked in this working tree:

```json
{
  "src-tauri/src/services/intent_routing/client/aliases.rs": "919cb0c290dd3379ce9a91702faa1367981da5d0d41f2421f192ca00d00414d2",
  "src-tauri/src/services/intent_routing/client/alias_tests.rs": "393ae2eaa5d1df9ec0f0857ea80b7e381a6614682bc630beb410f158871ac974",
  "src-tauri/src/services/intent_routing/client/mod.rs": "1d8ddae7ef6194fb234f4b62a3ee1e91886360e0686042c329d68241fb4bd3ff",
  "src-tauri/src/services/intent_routing/client/tests.rs": "3757fdfc564af9e26b33150eead6a98577caa9efb34515c35388cd868f673a3e",
  "src-tauri/src/services/intent_routing/menu.rs": "37a4ca7884aa31069a474314af74738a6dc43254205d202066fc3f1e7b8ce8ea",
  "src-tauri/src/services/proxy.rs": "7e7967c07272b7f80521a59390f2db9e7eb6aba56ebc16c618129fd952f100dc",
  "src-tauri/src/proxy/intent/mod.rs": "1f683131ccab21816a2cd02b11d8e6bf0f288a98afdb4173ccbb63644972c6b3",
  "src-tauri/src/proxy/intent/native.rs": "dc3f3585017780e03f08d49b45c0a568be647e8a82d2211e9b2fa62ac4d95016"
}
```
