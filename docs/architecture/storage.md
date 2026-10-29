# Shared and extension database storage

Provider, MCP, prompt, skill, proxy configuration and usage data remain in `cc-switch.db`. VaryAI security audit, DLP policy, tool-risk rules, lists, events and configuration history use `vary-ai.db` in the same configuration directory.

## Version boundaries

The shared database follows the **released** upstream schema chain. The current shared version is 23, matching CC Switch **v4.0.21**; the extension version is independently 5. Never advance the shared file to an unreleased migration found on upstream `main`: installed CC Switch releases reject a higher version even if the new columns appear harmless. Version 2 added hashed local gateway credentials and per-key token counters (`gateway_keys`, `gateway_usage`). The gateway branch used version 3 for local agent policy bindings and usage aggregates by agent/model (`gateway_agent_bindings`, `gateway_usage_details`) and version 4 for `gateway_usage_models` to distinguish the original client model from the resolved outbound model at request time. New observations use that table; reads include legacy detail rows with an unknown outbound model. Existing credentials, counters and historical aggregates remain intact. Historical VaryAI single-database version 24 remains the compatibility/export format, not the live shared version. VaryAI security and tool-risk tables identify that historical format. Inspect schema structure as well as version because historical VaryAI databases reused some upstream version numbers.

Extension version 5 reconciles two development branches that independently used versions 3 and 4. The intent-routing branch used those versions for `intent_routing_groups` and `intent_routing_preferences` rather than the gateway additions. Startup recognizes the complete known table/column layouts, backs up both files before upgrading, and adds the missing tables transactionally. Routing rows, gateway credentials/counters, and security/tool-risk state remain intact through startup and full backup/restore. Keeping the intent-routing tables does not enable that branch's runtime or UI. Malformed, partial, and unknown layouts still stop before schema writes; do not delete the extension database or lower its version to bypass validation. Older branches that only support extension v4 cannot reopen the upgraded v5 file.

The supported upstream additions are:

| Shared version | CC Switch addition | Preserved columns                         |
| -------------- | ------------------ | ----------------------------------------- |
| 19             | MCode              | `enabled_mcode` on MCP servers and skills |
| 20             | Pi                 | `enabled_pi` on MCP servers only          |
| 21             | Antigravity CLI    | `enabled_agy` on MCP servers and skills   |
| 22             | Kimi Code          | `enabled_kimi` on MCP servers and skills  |
| 23             | Oh My Pi           | `enabled_omp` on MCP servers only         |

The schema and migrations were checked against the [CC Switch v4.0.21 release](https://github.com/farion1231/cc-switch/releases/tag/v4.0.21) and its [tagged schema source](https://github.com/farion1231/cc-switch/blob/v4.0.21/src-tauri/src/database/schema.rs). Opening an upstream v21-v22 database upgrades it transactionally to v23 after a consistent recovery backup. Opening v23 leaves its version at v23, including repeated startup, extension upgrades, SQL export/import, and binary backup/restore. Existing flags survive VaryAI MCP/skill edits. These storage columns preserve CC Switch-owned state; they do not add the corresponding upstream clients to the VaryAI interface. Shared versions above v23 are rejected without modification, even when upstream `main` contains matching code. This does not affect recognized historical VaryAI monolithic v24 backups.

Opening an older supported shared database also migrates it to v23. A CC Switch installation whose supported schema is below v23 will then reject that file and require an application upgrade. Compatibility with the released v23 schema does not guarantee that every older CC Switch binary can reopen the migrated database.

Before changing shared storage, check the official release tag, its schema, and compatibility with that release's version check. Development branches and prereleases are not the compatibility baseline. Increment the extension version for VaryAI-only schema changes.

## Migration and recovery

On startup, unsupported future versions stop initialization and preserve the files. Recognized older formats are backed up consistently under `backups/storage-split/` before migration; backup failure stops migration. Existing extension data that would conflict with an automatic move also stops the migration rather than overwriting records.

The split uses SQLite ATTACH and a single transaction, DELETE rollback journaling and FULL synchronization. IDs, sequence values, settings and policy versions must survive the split. Unknown formats must not be repaired by lowering `user_version` manually. Another application's incompatible database lock can cause migration to fail; exit that application and retry.

An earlier compatibility development build incorrectly advanced released shared v23 databases to unreleased v24. The explicit [recovery helper](../../scripts/recover-shared-v23.py) is only for that known mistake, not general database downgrades. With both applications closed, it requires the affected current file and a verified pre-upgrade v23 backup. Its default is read-only. Before `--apply` writes anything, it validates that removing the two default-valued `enabled_dsh` columns produces exactly the reference schema, creates a private durable backup of the **current** file, checks for concurrent changes, and reverses the schema/version in one transaction. Current data rows are retained rather than replaced with old backup rows. Any v24-specific values, unrelated schema changes, missing reference, or failed backup stop recovery. See [troubleshooting](../troubleshooting.md#cc-switch-rejects-a-shared-database-as-too-new).

## Locking, backup and sync

Public DAOs use the shared connection and security/tool-risk and gateway-key DAOs use the extension connection. Gateway admission reads run on blocking workers; gateway usage persistence runs through an asynchronous queue. Operations needing both always lock shared before extension. Audit persistence remains asynchronous and must not hold database locks in request forwarding.

Gateway authentication still stores only SHA-256 hashes in the extension database. Newly created gateway secrets also have device-local copies under `gateway-secrets/<key UUID>` in the application configuration directory, outside both databases and their export/sync payloads. Private atomic writes use `0600` on Unix; these files are not encrypted. The separate reveal command validates a saved copy against an active key's hash before returning it. Lists contain only an availability flag. Missing copies leave authentication and local agent policies intact. Old hash-only credentials cannot be reconstructed. Failed secret-file writes roll back key creation; deleting a revoked, unbound key removes its local copy.

Portable SQL/binary exports combine the two files into the compatibility format. Import validates and migrates staged data, creates current safety backups, then writes the two current files transactionally. Preserve local tool-risk settings, rules, lists, events and change history according to the existing synchronization rules.

See [storage implementation](../../src-tauri/src/database/storage.rs), [schema](../../src-tauri/src/database/schema.rs), [file-level regression tests](../../src-tauri/src/database/split_storage_tests.rs), and [upstream compatibility tests](../../src-tauri/src/database/upstream_compat_tests.rs). Unit tests do not prove compatibility with every future upstream schema.
