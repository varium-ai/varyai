# VaryAI independent storage

VaryAI owns its application databases, device state, runtime files and synchronization namespace. A new installation starts with independent empty storage. Upgrading an old VaryAI installation is outside this design: there is no legacy VaryAI discovery wizard, whole-installation transfer or automatic account-file migration.

This document records the shared-file inventory, ownership boundaries and acceptance criteria. The [storage contract](../architecture/storage.md) describes the implementation.

## Scope and assumptions

- Support macOS, Windows and Linux using the existing operating-system home resolution.
- Keep the application identifier, deep-link scheme and historical Rust crate/library names. Display application log targets as `vary_ai_lib` / `vary_ai`.
- Use `~/.varyai`, native files `varyai.db` and `varyai-ext.db`, and local gateway `127.0.0.1:15722`. Development renderer uses port 3001.
- Preserve existing CC Switch files and client ownership while storing VaryAI configuration independently.
- Keep current VaryAI backup/restore and cloud synchronization. Internal schema adapters needed by portable database transport do not establish support for old VaryAI installation upgrades.
- Treat native client configuration as shared external state. Separate manager databases and ports do not coordinate two managers writing the same client.
- Retain the internal staging schema used by portable backups and fixtures; unsupported layouts are rejected before replacement.

## Files and state shared before isolation

In the analysis baseline, the application data accessor resolved to `~/.cc-switch` unless overridden, while several device paths bypassed that override. A separate extension database alone did not isolate the products.

| Resource                                          | Pre-isolation location                                              | Independent behavior                                                                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Main database                                     | `<app_config_dir>/cc-switch.db`                                     | New private `varyai.db` for providers, endpoints, MCP, prompts, Skills, profiles and common fragments.                          |
| VaryAI extension                                  | `<app_config_dir>/vary-ai.db`                                       | New private `varyai-ext.db`; historical extension files and sidecars remain untouched.                                          |
| Device preferences and client-directory overrides | `~/.cc-switch/settings.json`                                        | Fresh VaryAI settings under `~/.varyai`; no source settings copy.                                                               |
| Bootstrap directory pointer                       | Tauri application-data `app_paths.json`                             | Keep native directory selection; discard a historical source pointer without opening or converting its database.                |
| Legacy JSON and Skills registry                   | `<app_config_dir>/config.json`, `skills.json`                       | No startup conversion or archival; configure resources in VaryAI.                                                               |
| Skills content                                    | `<app_config_dir>/skills`                                           | Store new private Skills under the independent data root; historical content remains untouched.                                 |
| Explicit unified Skills storage                   | `~/.agents/skills`                                                  | Remains an intentional user-selected shared directory.                                                                          |
| Skills undo data                                  | `<app_config_dir>/skill-backups`                                    | New undo data stays under the VaryAI root; old uninstall history is not imported.                                               |
| Price overrides                                   | `<app_config_dir>/model-pricing.json`                               | Independent file under the VaryAI data root.                                                                                    |
| Managed accounts                                  | `codex_oauth_auth.json`, `copilot_auth.json`, `xai_oauth_auth.json` | Device files under `~/.varyai`; sign in independently.                                                                          |
| Codex managed-auth ownership                      | `codex_managed_oauth_live_auth.json`                                | New device-local ownership evidence under the VaryAI root.                                                                      |
| Gateway secret copies                             | `gateway-secrets/<UUID>`                                            | Independent device files; source hashes, counters and secret files are not imported.                                            |
| Qoder certificate material                        | `qoder-certificates/<UUID>` and OS trust                            | Independent material and normal explicit trust authorization; no legacy certificate copy.                                       |
| Pi routing recovery                               | `pi-routing.json`                                                   | Independent device state; no source routing intention, local token or restoration transfer.                                     |
| TRAE bridge and launch recovery                   | `~/.cc-switch/trae-bridge`                                          | Generated helpers, sockets and recovery live under `~/.varyai/trae-bridge`.                                                     |
| Logs and crash logs                               | `<app_config_dir>/logs/cc-switch.log` and old panic fallback        | `~/.varyai/logs/varyai.log` and independent crash logging; application module tags use VaryAI branding.                         |
| Environment backups                               | `~/.cc-switch/backups/env-backup-*.json`                            | New VaryAI device backups; source files are not copied.                                                                         |
| Database/recovery backups                         | `<app_config_dir>/backups`                                          | Independent backup directory; old installation archives are not imported.                                                       |
| WebDAV/S3 objects                                 | `cc-switch-sync/v2/db-v6/<profile>`                                 | `varyai-sync` manifest and reserved `<root>/varyai/v2/db-v6/<profile>` namespace. No old credentials or remote-state migration. |
| Frontend paths and export labels                  | Reconstructed shared defaults and CC Switch labels                  | Backend-resolved VaryAI paths and VaryAI export names.                                                                          |

The historical shared database also contains usage, aggregate/deduplication ledgers, proxy settings, failover state, live backups and pricing. These now belong to VaryAI storage. The native extension holds security, tool-risk, gateway and retained intent-routing state. Managed account stores remain device-local.

## Native directory and format

The default data root is `~/.varyai`. A custom independent data directory moves the database pair, private Skills, prices and database backups. Device settings, managed accounts, gateway secret copies, certificates, Pi state, helpers and logs stay under `~/.varyai`.

| Native resource       | Identity/version                                              |
| --------------------- | ------------------------------------------------------------- |
| Main                  | `varyai.db`, schema 24, product `varyai`, role `main`         |
| Extension             | `varyai-ext.db`, schema 6, product `varyai`, role `extension` |
| Pair manifest         | `storage.json`, layout 1, same storage UUID as both databases |
| SQLite application ID | `0x56525941`                                                  |

Keep the existing two-connection DAO architecture. Validate product, role, UUID and complete supported schema before ordinary writes. Native v24 and historical monolithic v24 are different formats; a version number alone is insufficient.

A fresh installation creates native storage directly and does not scan or copy old VaryAI data. A missing configured native directory opens recovery: reconnect it, choose another native directory, use the default root or explicitly create empty storage. This is recovery for the current format, not an upgrade workflow. Roots inside `.cc-switch`, aliases and shared database hard links are rejected.

Existing historical database filenames in the actual default `~/.varyai` directory are retained and ignored when creating the native pair. They do not turn that product-owned directory into a CC Switch source. Source-directory aliases and custom roots containing CC Switch data remain rejected. Startup preserves specific validation error codes; unexpected lookup failures use a storage-location error without private details.

Native default rows use port 15722. The portable transport adapter converts only the previous default 15721 once and preserves custom ports and later explicit choices. CC Switch files and their historical schema defaults remain untouched.

## Shared native clients and routing

Claude settings and `~/.claude.json`, Codex `config.toml`/`auth.json`, Gemini environment/settings and other client-native directories remain client-owned. Both managers can write them; use one manager per client or separate client directories where supported.

Fresh storage does not infer ownership from placeholders such as `PROXY_MANAGED` or `cc-switch-official`. Automatic client import/cleanup requires local ownership or backup evidence.

New Codex catalog projections use `varyai-model-catalog.json`; the reader recognizes the historical name inside the verified client directory. Preserve the `custom` provider bucket and `cc-switch-official` route ID because session history can refer to them. WorkBuddy ownership files remain beside its catalog, and TRAE ownership also exists in client browser storage.

A routing switch requires usable native configuration. For Claude, Codex, Gemini and Grok Build, missing configuration returns setup guidance before starting the listener or creating a live backup. Other adapters retain their model, installation and authorization checks. See [per-agent prerequisites](../guides/provider-routing.md#routing-prerequisites-on-a-new-installation). Synthetic empty files used in isolated tests are not production client initialization.

## Backup and synchronization

Portable exports combine the pair into the existing monolithic transport; restore validates staging, backs up the destination and retains its native storage UUID. This transport adapter is needed for current backups, not an old VaryAI installation-upgrade promise.

Database exports can contain provider credentials and client recovery data. They exclude device settings, managed accounts, certificates and saved gateway secret copies; Skills also need their file assets. Private filesystem permissions do not encrypt the data.

WebDAV/S3 use manifest `varyai-sync`, default root `varyai-sync` and reserved `<root>/varyai/v2/db-v6/<profile>` paths. Even a manually selected old root cannot collide with the old object layout. Configure cloud accounts independently and explicitly choose the initial synchronization direction. Existing security/tool-risk preservation policies remain unchanged.

## Verification and limits

| Area            | Required evidence                                                                                                              |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Fresh startup   | Native pair created independently; historical pointers do not trigger data/account migration.                                  |
| Native recovery | Missing custom roots remain visible; mismatched pairs, unknown schemas and unsafe paths fail before ordinary writes.           |
| File isolation  | Startup and directory selection leave existing CC Switch and historical VaryAI files untouched.                                |
| Client safety   | Uninitialized clients receive setup guidance without listener startup; recovery does not claim another manager's placeholders. |
| Namespaces      | Independent ports, log paths/tags, device paths, helpers and remote objects.                                                   |
| Backup/restore  | Current portable round trips preserve data and destination identity.                                                           |

Run repository-required typecheck, renderer build, unit tests, formatting, cargo check, Clippy, library and integration tests. Run TRAE helper tests for helper changes. Use temporary directories, synthetic credentials, separate test application identity and dynamic ports. Do not install or replace a running application as routine verification.

Live vendor login, OS trust, installer/signing and notarization acceptance remain separate checks. This design does not require supporting upgrades from a previously released VaryAI installation.
