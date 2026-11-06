# Codex historical model identity after provider takeover

## Report and diagnosis

An existing Codex conversation continued to receive `404 routing_model_unavailable` at `/intent/v1/responses` after switching from Magpie to VaryAI. The current Codex default selection and the gateway's authenticated model list agreed; the failing historical conversation retained a model identity from its previous provider. Reassigning the conversation's provider bucket did not change its saved model identity.

The read-only investigation compared membership and ownership booleans, never printed credentials, configuration documents or conversation content, and did not rewrite live client files. A recent thread had a VaryAI database provider while its last saved model was absent from the published catalog. This explains why restarting or retrying the same conversation can repeat the failure even when the current default selection is valid. Synthetic examples below use `legacy-provider/legacy-model`; they do not identify a private provider or conversation.

## Recovery for an already migrated conversation

Select a model from the current VaryAI model list in Codex. If the conversation remains blocked on automatic compaction before the selection takes effect, start a new conversation and select the model there. Keep the historical conversation for reference. Reconnecting the provider or restarting Codex alone does not rewrite every historical turn's model choice.

This change does not automatically replace historical model choices with a default, infer a provider from a display name, or modify live conversation files. Existing recovery journals continue to govern explicit disconnect and restoration.

## Source change and regression contract

Inbound history migration must verify that a saved model is executable in the current published menu before moving the conversation into the VaryAI provider bucket. An unavailable, ambiguous or unreadable model stays with its original provider. This preserves the original model and history; it can also keep that conversation outside the VaryAI provider's history view until the user explicitly selects a supported route. Stable incompatible histories do not create an endless background retry. A known incompatible database model is excluded before reading large legacy history files or charging the scan budget; pending journaled changes still retain their recovery checks.

Legacy history checks both the database model and the last saved turn model. Paginated history relies on its authoritative database model and preserves suffix files and byte offsets. Database updates retain conditional checks so a concurrently changed model cannot be silently rebound. Outbound restoration remains separate from inbound model eligibility. Incomplete journaled operations retain their recovery safeguards.

The gateway continues to reject an unknown model with the same HTTP status and `routing_model_unavailable` code. Its fixed error message directs the user to select a model from the current list or start a new conversation. Authentication remains checked first. Error output does not include the unavailable model, request contents or credentials, and unknown model names never fall back to an unrelated provider.

Regression fixtures cover legacy and paginated histories, inconsistent database/turn models, missing model metadata, compatible aliases, repeated synchronization, model changes during conditional updates, and the existing authenticated HTTP boundary. They use temporary homes, in-memory or temporary databases, synthetic model identifiers and dynamic loopback ports.

## Verification

The two inbound migration regressions reproduced the original bug before implementation: unsupported historical models were reassigned from the synthetic source provider to VaryAI. An independent authenticated HTTP regression reached the original 404 and failed because the fixed model-reselection guidance was absent. The initial isolated run of an older forwarding fixture stopped earlier on a closed, uninitialized DLP policy gate; that fixture now explicitly publishes the enabled default test policy and restores its previous configuration using the existing test guard pattern. Production policy admission and forwarding assertions remain unchanged.

A stale Tauri dependency cache initially referred to permission files in an earlier target location. Rebuilding the verified temporary target's generated Tauri cache resolved that build prerequisite. It was not a migration assertion failure.

Base revision: `417def030f4c23ec6a4577f3492ab8409468ecde`, including the existing uncommitted working tree. Host: Darwin arm64; Rust/Cargo 1.98.1; Node 22.22.2 and pnpm 10.12.3. Node differs from the repository's exact 22.12.0 pin. No commit or push was requested.

| Final local check                    | Result                                              |
| ------------------------------------ | --------------------------------------------------- |
| `cargo fmt --check`                  | Passed                                              |
| `cargo check`                        | Passed                                              |
| `cargo clippy -- -D warnings`        | Passed                                              |
| `cargo test --lib`                   | 6,115 passed; 18 existing ignored                   |
| `cargo test --no-fail-fast`          | 16 test binaries; 6,278 passed; 18 existing ignored |
| `pnpm typecheck`                     | Passed                                              |
| `pnpm build:renderer`                | Passed                                              |
| `pnpm test:unit --maxWorkers=1`      | 280 files; 3,506 tests passed                       |
| `pnpm format:check`                  | Passed                                              |
| `pnpm audit --prod`                  | Passed; no known vulnerabilities                    |
| Node development/client helper tests | 94 passed; 1 existing skip                          |
| Python storage-recovery helper tests | 5 passed                                            |

Backend commands used cached offline dependencies, `/tmp/vary-magpie-parity-target`, incremental compilation disabled, and package debug info disabled. Test processes used an explicit temporary `CC_SWITCH_TEST_HOME` and appended only `127.0.0.1,localhost,::1` to `NO_PROXY` and `no_proxy`. Application and system proxy settings were unchanged. The existing nonfatal macOS debug linker compact-unwind warning remains.

The before/after source and configuration inventories match across every final backend command. Their canonical sorted inventory SHA-256 is `5b4199ae7d24c4348fdd90d81623fd421010cfbabf569f012f1a387dbbfb3c17`. Frontend sources also remained unchanged during their checks. Commands, results, source inventories and logs are retained under `/tmp/vary-codex-404-20261106-1500/`; final focused regressions are retained under `/tmp/vary-thread-model-final-55l_ey_f/`. This report was completed afterward without application-source changes.

The implementation is shared by macOS and Windows. Local macOS tests do not establish Linux/Windows/macOS remote CI acceptance, Windows-to-WSL2 acceptance, or a successful real historical conversation. No installed client was launched, terminated, installed or replaced for verification. The live inspection was read-only; no live conversation file, database row or configuration was rewritten. Loading the updated native backend is required to use the migration guard and improved error message. Already migrated incompatible conversations still require explicit model selection or a new conversation; foreign opaque or compacted state is not converted by this repair.
