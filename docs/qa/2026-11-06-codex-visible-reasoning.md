# Codex visible reasoning after a model switch

## Evidence and change

The reported conversation completed a GLM turn before switching to Gemini and
receiving `409 routing_unknown_continuation`. Its stored reasoning item contained
a visible summary, `content: null`, `encrypted_content: null`, and Codex turn
metadata. Only field names, types and event order were inspected for the report;
no conversation text or credentials are included here.

Synthetic requests with this same item shape reproduced 409 on the running
gateway for both Gemini and Grok. The rejection occurred before upstream model
execution. Codex 0.167's
[reasoning item serialization](https://github.com/openai/codex/blob/rust-v0.167.0/codex-rs/protocol/src/models.rs)
supports these null optional fields and harness metadata.

The router's visible-summary predicate previously allowed only `type`, `id`,
`status` and `summary`, so the extra fields incorrectly made plain text depend on
an existing continuation owner. It now also accepts null content/encryption and
null or object-shaped harness metadata. A nonempty, valid visible summary is
still required. This is classification only: the request retains its fields.
Encrypted content, unknown semantic fields, incomplete summaries, response/item
references and unknown tool outputs retain their existing ownership checks.

## Verification

Base revision: `c7f0ce6ac8639c3ad969cd85f3e9bc22ad562ce9`, with the existing staged
and unstaged changes preserved. Host: macOS Darwin 25.6.0 arm64; Rust 1.98.1,
Node 20.20.2, pnpm 10.12.3. Cargo uses `CARGO_INCREMENTAL=0`.

Before the implementation change, the new unit regression failed at visible
summary classification. The native HTTP regression also failed with 409 instead
of 200. Both fixtures use synthetic data; the HTTP fixture uses an in-memory
database and dynamic loopback ports. It covers named and unnamed conversations,
unchanged forwarded input, and rejection of unknown encrypted state.

Final checks on this working tree:

| Command                                                            | Result                                                |
| ------------------------------------------------------------------ | ----------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                |
| `pnpm build:renderer`                                              | Passed                                                |
| `pnpm test:unit`                                                   | 3,475 tests passed in 279 files                       |
| `pnpm format:check`                                                | Passed                                                |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,048 passed, 1 existing failure, 18 ignored          |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | Same library result; all 163 integration tests passed |
| `git diff --check`                                                 | Passed                                                |

The remaining full-suite failure is the previously recorded
`database::routing_upstreams_compat_tests::provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`
with `routingUpstreams.invalid`. Backend acceptance is therefore not entirely
green. An earlier filtered `proxy::intent::` run passed the new regressions but
failed four capacity fixtures (503 or missing response model); their standalone
filter also failed. All four passed in both complete library runs. These fixtures
do not initialize the shared security runtime before forwarding, which is
consistent with a subset initialization dependency; no assertions or production
security checks were weakened. The macOS linker also emitted its existing large
unwind-section warning. No commit, push or cross-platform CI run was performed.

SHA-256 fingerprints of the checked source files:

```json
{
  "src-tauri/src/proxy/intent/length.rs": "13369ca5bb8eb927b2a875e3b3b2da77088a08d90759dc392e26f5e96f3b3dae",
  "src-tauri/src/proxy/intent/tests.rs": "13ead34d82b916435557b163ee3dbd9e39d31a515c02a5f6993dec242f2314fa",
  "src-tauri/src/proxy/intent/native.rs": "e7c3a2a62e2b4b6a9c51b8a0014f9b851a1386acc2e84cc2f5b2d8752d1baab0"
}
```

## Runtime boundary

`cargo build --manifest-path src-tauri/Cargo.toml --bin varyai` passed and produced
the updated development binary without launching it.

The active development launcher uses `--no-watch`. Source edits and verification
do not reload that gateway. No running application or Codex daemon was restarted
for the reproduction or regression checks. Live acceptance of the corrected
running gateway remains pending until an authorized reload.
