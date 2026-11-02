# Independent VaryAI storage

VaryAI owns a native database pair, device state and synchronization namespace. New installations start independently under `~/.varyai`, or an independent data directory selected in Settings. Old VaryAI installation upgrades are not supported; startup does not discover, transfer or archive their databases, settings or account files. See the [design and shared-file inventory](../design/varyai-storage-isolation.md).

## Paths and ownership

| Resource                                                                                               | Location                                                                |
| ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| Main: providers, endpoints, MCP, prompts, Skills metadata, profiles, usage and current client recovery | `<data root>/varyai.db`                                                 |
| Extension: security, tool risk, gateway keys/counters and retained intent-routing tables               | `<data root>/varyai-ext.db`                                             |
| Pair identity                                                                                          | `<data root>/storage.json` and identity rows in both databases          |
| Private Skills, price overrides and database backups                                                   | `<data root>/skills`, `model-pricing.json`, `backups`, `skill-backups`  |
| Device preferences, client directory overrides and cloud credentials                                   | `~/.varyai/settings.json`                                               |
| Managed accounts, gateway secret copies, certificates and routing recovery                             | `~/.varyai/`                                                            |
| Helpers and logs                                                                                       | `~/.varyai/trae-bridge`, `~/.varyai/logs`; crash log in the device root |
| Bootstrap data-directory pointer                                                                       | VaryAI's Tauri application-data directory                               |
| Explicit unified Skills choice                                                                         | `~/.agents/skills`, intentionally shared                                |

A data-directory override relocates data assets, not device files. A new pointer takes effect after restart; current connections and writers retain their root for the running process. A missing configured native directory opens recovery instead of silently selecting another database. Recovery can choose another native root or explicitly create empty storage. Historical pointers into CC Switch storage are discarded without converting source files. Destinations inside `.cc-switch`, aliases and shared database hard links are rejected.

Historical files named `cc-switch.db` and `vary-ai.db` may already exist in VaryAI's own default `~/.varyai` directory. Their presence does not block startup or trigger conversion. VaryAI creates/opens its separate native filenames and leaves the historical files untouched. This exception requires the actual product-owned default directory; a symlink or junction from that directory into a CC Switch source remains rejected.

The serialized Skills option `cc_switch` still means private application storage and now follows the VaryAI root. Client configuration remains in native client directories. Independent storage does not coordinate two managers concurrently controlling the same client.

## Format boundaries

| Format                      | Main                         | Extension             | Use                                             |
| --------------------------- | ---------------------------- | --------------------- | ----------------------------------------------- |
| Native VaryAI               | 24                           | 6                     | Live independent storage                        |
| Internal staging layout     | 23                           | 5                     | Backup normalization and compatibility fixtures |
| Portable database transport | Combined monolithic snapshot | Included in transport | Current VaryAI database backup/restore          |

Native files have SQLite `application_id = 0x56525941`. Identity rows contain product `varyai`, roles `main` / `extension` and the same storage UUID. `storage.json` records that UUID and layout version 1. A mismatched pair, unsupported version or malformed schema fails before ordinary writes. Native v24 differs from historical monolithic v24; a version number alone does not identify a format.

Internal adapters normalize portable backups on private staging copies. They do not provide a full old VaryAI installation-upgrade path. Unsupported or incomplete layouts are rejected rather than modified in place.

## Client and routing boundary

The default gateway is `127.0.0.1:15722`; the development renderer uses port 3001. Native creation and portable restore normalize the previous default 15721 once, preserving other custom ports and later explicit choices through the `varyai_proxy_port_v1` receipt. CC Switch schema defaults and source files remain unchanged.

Fresh storage does not infer ownership from shared placeholders such as `cc-switch-official`. Automatic client import, common-fragment extraction and takeover cleanup require local ownership/backup evidence.

A fresh VaryAI data root still reads existing native client configuration from its original default or explicitly selected client directory. Changing the application data directory does not move or reset those client directories. An empty test home redirects them too, so it must not be mistaken for an existing-client installation.

VaryAI does not initialize an absent client or grant connection/certificate authorization. Missing Claude/Codex/Gemini/Grok configuration returns setup guidance before listener startup; the other adapters enforce their supported models and prerequisites. See [routing prerequisites](../guides/provider-routing.md#routing-prerequisites-on-a-new-installation).

Claude Desktop uses the same per-agent routing control as other clients. Its switch applies or restores its own native profile; disabling it rejects new requests to the cached gateway address while other agents remain connected. The shared gateway stop guard covers every supported active or waiting route.

New Codex catalog files are `varyai-model-catalog.json`. The historical catalog name remains readable inside the verified Codex directory. The `custom` provider bucket and `cc-switch-official` route ID remain stable because session history can refer to them.

WorkBuddy ownership and native backups remain beside its catalog. TRAE model ownership also exists in client browser storage. Removing a VaryAI database does not clear those native catalogs.

## Transactions, backups and synchronization

Connections use separate mutexes; lock main before extension. Audit, usage and observation persistence is asynchronous. Request forwarding must not execute blocking transactions or wait for auxiliary persistence.

Cross-file replacement uses SQLite ATTACH, DELETE journaling and FULL synchronization. Native rollback recovery validates journal ownership and rejects foreign super-journal paths.

Portable SQL/binary exports combine both databases into the existing monolithic transport without native identity metadata. Restore validates staging, creates a safety backup, replaces both live databases transactionally and retains the destination UUID. Exports can contain provider credentials and client recovery data, but exclude device settings, account/certificate files and saved gateway secret copies. Skills require their file assets too.

Gateway authentication stores SHA-256 hashes in the extension. Device-local `gateway-secrets/<key UUID>` files can contain plaintext copies. Reveal validates a copy against its hash; a missing copy does not invalidate the key and cannot be reconstructed. Unix private files use `0600` and directories `0700`; these permissions are not encryption.

WebDAV/S3 use manifest `varyai-sync` and reserved `<remote root>/varyai/v2/db-v6/<profile>/...` paths. The default root is `varyai-sync`. The reserved segment prevents collision even with an old custom root. New installations configure accounts and initial sync direction independently; no old credentials, remote state or automatic upgrade are transferred. Existing security/tool-risk preservation rules still apply.

Implementation: [native identity](../../src-tauri/src/database/native.rs), [startup/recovery](../../src-tauri/src/storage_setup.rs), [portable storage](../../src-tauri/src/database/storage.rs), [native tests](../../src-tauri/src/database/native_tests.rs). Synthetic tests do not establish installer/signing or every vendor client's live acceptance.
