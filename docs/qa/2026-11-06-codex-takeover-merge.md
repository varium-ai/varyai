# Codex takeover integration before push

## Integrated changes

The user requested committing and pushing the completed takeover and historical-model fixes. Local checkpoint `caeb9d6c` preserves the previously verified working tree. Fetching the branch found six newer remote commits, ending at `d85f2bb25141c4d5190cd32b7c3aa71ee7a0b770`, including provider onboarding, protocol conversion, readable Codex model aliases, and the provider-notes fixture fix. These updates are merged without overwriting remote history. The existing pull request is [#44](https://github.com/varium-ai/varyai/pull/44).

The client configuration conflict retains both the strict external-takeover classifier and readable-name orphan cleanup backed by an owned generated catalog. The integration also preserves desktop restart/reload behavior, proxy diagnostics, original-history model checks, protocol fixes, four runtime languages, and provider metadata changes.

## Alias compatibility

A clean textual merge was insufficient for model identity. Runtime menus retain canonical identities while Codex receives readable names. Inbound history migration now accepts canonical identities and retained readable names only when their exact canonical destination remains executable. Removed names stay unavailable, including when their old label is reused for another destination. Offline recovery validates the owned catalog's complete visible/hidden projection against the receipt; it does not infer ownership from display names or accept every retained mapping.

Missing-catalog repair reconstructs the same readable/hidden projection using the recorded alias map. It requires matching ownership and local token, and refuses to allocate new aliases or change the recorded publication through a repair. Normal refresh remains responsible for a changed menu.

New integration regressions first failed for valid history rejection, incomplete hidden catalogs, raw-only catalog repair, and stale-token repair. The corrected cases cover runtime and offline legacy/paginated histories, readable and renamed aliases, canonical compatibility, removed targets and label reuse, tampered publication metadata, legacy receipts and token boundaries. History contents and model choices remain unchanged.

## Verification and acceptance

Final local checks passed on Darwin arm64 with Rust/Cargo 1.98.1, Node 22.22.2 and pnpm 10.12.3. Node differs from the repository's exact 22.12.0 pin.

| Check                           | Result                                              |
| ------------------------------- | --------------------------------------------------- |
| `cargo fmt --check`             | Passed                                              |
| `cargo check`                   | Passed                                              |
| `cargo clippy -- -D warnings`   | Passed                                              |
| `cargo test --lib`              | 6,149 passed; 18 existing ignored                   |
| `cargo test --no-fail-fast`     | 16 test binaries; 6,312 passed; 18 existing ignored |
| `pnpm typecheck`                | Passed                                              |
| `pnpm build:renderer`           | Passed                                              |
| `pnpm test:unit --maxWorkers=1` | 280 files; 3,511 tests passed                       |
| `pnpm format:check`             | Passed                                              |
| `pnpm audit --prod`             | Passed; no known vulnerabilities                    |

Backend checks used offline cached dependencies, the verified temporary Cargo target, incremental compilation disabled and package debug info disabled. Test processes used a temporary home and process-local loopback proxy exclusions. The existing nonfatal macOS compact-unwind linker warning remained. Source and configuration inventories match before and after every backend check; frontend sources also match across all their checks. The canonical backend inventory SHA-256 is `4fd9b5788ea11b29db6517025495c5d59d79c0db31c35b3616415b5e1fbc7cc0`. Exact commands, results and inventories are retained under `/tmp/vary-push-20261106/`. The unchanged helper sources match the earlier verified inventory, retaining 94 passing Node helper tests with one existing skip and five passing Python recovery tests.

Earlier reports in this directory describe their own checked trees and are not evidence for this integration. Remote CI must be inspected for the exact pushed revision; local success does not establish Linux, Windows, macOS, Windows-to-WSL2 or installed-client acceptance. No installed application was launched or replaced for these checks. This report was completed after the source checks without application-source changes.
