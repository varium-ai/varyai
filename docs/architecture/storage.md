# Shared and extension database storage

Provider, MCP, prompt, skill, proxy configuration and usage data remain in `cc-switch.db`. VaryAI security audit, DLP policy, tool-risk rules, lists, events and configuration history use `vary-ai.db` in the same configuration directory.

## Version boundaries

The shared database follows the upstream schema chain. The current shared version is 20; the extension version is independently 4. Version 2 added hashed local gateway credentials and per-key token counters (`gateway_keys`, `gateway_usage`). Version 3 added local agent policy bindings and usage aggregates by agent/model (`gateway_agent_bindings`, `gateway_usage_details`). Version 4 adds `gateway_usage_models` to distinguish the original client model from the resolved outbound model at request time. New observations use that table; reads include legacy detail rows with an unknown outbound model. Existing credentials, counters and historical aggregates remain intact. Historical single-database version 24 is a compatibility/export format, not the version to apply to the current shared file. Inspect schema structure as well as version because historical VaryAI databases reused some upstream version numbers.

Before changing shared storage, check the corresponding upstream structure. Increment the extension version for VaryAI-only schema changes.

## Migration and recovery

On startup, unsupported future versions stop initialization and preserve the files. Recognized older formats are backed up consistently under `backups/storage-split/` before migration; backup failure stops migration. Existing extension data that would conflict with an automatic move also stops the migration rather than overwriting records.

The split uses SQLite ATTACH and a single transaction, DELETE rollback journaling and FULL synchronization. IDs, sequence values, settings and policy versions must survive the split. Unknown formats must not be repaired by lowering `user_version` manually. Another application's incompatible database lock can cause migration to fail; exit that application and retry.

## Locking, backup and sync

Public DAOs use the shared connection and security/tool-risk and gateway-key DAOs use the extension connection. Gateway admission reads run on blocking workers; gateway usage persistence runs through an asynchronous queue. Operations needing both always lock shared before extension. Audit persistence remains asynchronous and must not hold database locks in request forwarding.

Gateway authentication still stores only SHA-256 hashes in the extension database. Newly created gateway secrets also have device-local copies under `gateway-secrets/<key UUID>` in the application configuration directory, outside both databases and their export/sync payloads. Private atomic writes use `0600` on Unix; these files are not encrypted. The separate reveal command validates a saved copy against an active key's hash before returning it. Lists contain only an availability flag. Missing copies leave authentication and local agent policies intact. Old hash-only credentials cannot be reconstructed. Failed secret-file writes roll back key creation; deleting a revoked, unbound key removes its local copy.

Native-client model ownership is separate from provider storage. WorkBuddy keeps model IDs and SHA-256 ownership fingerprints in a private `models.json.varyai-owned` file beside its native catalog; it does not store plaintext keys. Model changes roll back that file together with the native catalog when their database callback fails. Multi-client profile undo includes the ownership file. TRAE keeps native/request model IDs in its own local browser storage, without credentials. Deleting `cc-switch.db` does not delete either native model catalog or its separate ownership data.

Portable SQL/binary exports combine the two files into the compatibility format. Import validates and migrates staged data, creates current safety backups, then writes the two current files transactionally. Preserve local tool-risk settings, rules, lists, events and change history according to the existing synchronization rules.

See [storage implementation](../../src-tauri/src/database/storage.rs), [schema](../../src-tauri/src/database/schema.rs), and [file-level regression tests](../../src-tauri/src/database/split_storage_tests.rs). Unit tests do not prove compatibility with every future upstream schema.
