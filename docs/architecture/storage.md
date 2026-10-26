# Shared and extension database storage

Provider, MCP, prompt, skill, proxy configuration and usage data remain in `cc-switch.db`. VaryAI security audit, DLP policy, tool-risk rules, lists, events and configuration history use `vary-ai.db` in the same configuration directory.

## Version boundaries

The shared database follows the upstream schema chain. The current shared version is 20; the extension version is independently 3. Version 2 added hashed local gateway credentials and per-key token counters (`gateway_keys`, `gateway_usage`). Version 3 adds local agent policy bindings and usage aggregates by agent/model (`gateway_agent_bindings`, `gateway_usage_details`), without changing existing credentials or counters. Historical single-database version 24 is a compatibility/export format, not the version to apply to the current shared file. Inspect schema structure as well as version because historical VaryAI databases reused some upstream version numbers.

Before changing shared storage, check the corresponding upstream structure. Increment the extension version for VaryAI-only schema changes.

## Migration and recovery

On startup, unsupported future versions stop initialization and preserve the files. Recognized older formats are backed up consistently under `backups/storage-split/` before migration; backup failure stops migration. Existing extension data that would conflict with an automatic move also stops the migration rather than overwriting records.

The split uses SQLite ATTACH and a single transaction, DELETE rollback journaling and FULL synchronization. IDs, sequence values, settings and policy versions must survive the split. Unknown formats must not be repaired by lowering `user_version` manually. Another application's incompatible database lock can cause migration to fail; exit that application and retry.

## Locking, backup and sync

Public DAOs use the shared connection and security/tool-risk and gateway-key DAOs use the extension connection. Gateway admission reads run on blocking workers; gateway usage persistence runs through an asynchronous queue. Operations needing both always lock shared before extension. Audit persistence remains asynchronous and must not hold database locks in request forwarding.

Portable SQL/binary exports combine the two files into the compatibility format. Import validates and migrates staged data, creates current safety backups, then writes the two current files transactionally. Preserve local tool-risk settings, rules, lists, events and change history according to the existing synchronization rules.

See [storage implementation](../../src-tauri/src/database/storage.rs), [schema](../../src-tauri/src/database/schema.rs), and [file-level regression tests](../../src-tauri/src/database/split_storage_tests.rs). Unit tests do not prove compatibility with every future upstream schema.
