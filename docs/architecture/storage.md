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

Native extension version 7 introduces independent global providers in
`routing_upstreams`. Provider imports create private copies with stable identities
and revision checks. Catalog reads and startup never import client configurations
automatically. The retired registry contract is discarded on staging upgrades;
shared client settings, security policy and observations remain intact.

The serialized Skills option `cc_switch` still means private application storage and now follows the VaryAI root. Client configuration remains in native client directories. Independent storage does not coordinate two managers concurrently controlling the same client.

## Format boundaries

| Format                      | Main                         | Extension             | Use                                             |
| --------------------------- | ---------------------------- | --------------------- | ----------------------------------------------- |
| Native VaryAI               | 24                           | 7                     | Live independent storage                        |
| Internal staging layout     | 23                           | 7                     | Backup normalization and compatibility fixtures |
| Portable database transport | Combined monolithic snapshot | Included in transport | Current VaryAI database backup/restore          |

Native extension v6 pairs created by the independent-storage implementation are
validated and backed up before adding the global-provider table in one attached
transaction. Their private settings and pair identity remain intact. This narrow
schema upgrade does not import an old installation or shared CC Switch storage.

Native files have SQLite `application_id = 0x56525941`. Identity rows contain product `varyai`, roles `main` / `extension` and the same storage UUID. `storage.json` records that UUID and layout version 1. A mismatched pair, unsupported version or malformed schema fails before ordinary writes. Native v24 differs from historical monolithic v24; a version number alone does not identify a format.

Internal adapters normalize portable backups on private staging copies. They do not provide a full old VaryAI installation-upgrade path. Unsupported or incomplete layouts are rejected rather than modified in place.

## Client and routing boundary

The default gateway is `127.0.0.1:15722`; the development renderer uses port 3001. Native creation and portable restore normalize the previous default 15721 once, preserving other custom ports and later explicit choices through the `varyai_proxy_port_v1` receipt. CC Switch schema defaults and source files remain unchanged.

Fresh storage does not infer ownership from shared placeholders such as `cc-switch-official`. Automatic client import, common-fragment extraction and takeover cleanup require local ownership/backup evidence.

A fresh VaryAI data root still reads existing native client configuration from its original default or explicitly selected client directory. Changing the application data directory does not move or reset those client directories. An empty test home redirects them too, so it must not be mistaken for an existing-client installation.

Public provider DAOs use the shared connection; routing-registry, security/tool-risk and gateway-key DAOs use the extension connection. Routing resolution reads shared provider snapshots before locking the extension connection and runs on the control plane, not during forwarding. Successful routing/source mutations and full imports/restores invalidate the in-process routing snapshot generation; the runtime must publish a refreshed snapshot before using changed resources. Gateway admission reads run on blocking workers; gateway usage persistence runs through an asynchronous queue. Operations needing both always lock shared before extension. Audit persistence remains asynchronous and must not hold database locks in request forwarding.

VaryAI does not initialize an absent client or grant connection/certificate authorization. Missing Claude/Codex/Gemini/Grok configuration returns setup guidance before listener startup; the other adapters enforce their supported models and prerequisites. See [routing prerequisites](../guides/provider-routing.md#routing-prerequisites-on-a-new-installation).

Claude Desktop uses the same per-agent routing control as other clients. Its switch applies or restores its own native profile; disabling it rejects new requests to the cached gateway address while other agents remain connected. The shared gateway stop guard covers every supported active or waiting route.

New Codex catalog files are `varyai-model-catalog.json`. The historical catalog name remains readable inside the verified Codex directory. The `custom` provider bucket and `cc-switch-official` route ID remain stable because session history can refer to them.

Native API providers store their primary key, backup keys and private headers
in the extension record. The registry does not encrypt these values. List,
editor, save and import responses redact credentials and headers. Omitted keys
retain stored values only at their original trusted destinations; private
backup keys are matched by stable IDs. Import previews expose only safe metadata
and source fingerprints, then re-read the source for explicit confirmation.
Imported Codex subscriptions use the separate managed OAuth account file and native
Codex provider policies. No OAuth token is stored in an API-key field. Source
applications and native client sign-ins are not rewritten. Treat both database
files, private account files and portable exports as sensitive. See
[external imports](shared-catalog.md#external-provider-imports).

Portable SQL/binary exports combine the two files into the compatibility format, including routing records, origin tombstones and API keys. Import validates record sizes, revisions, protocols and origin consistency in staging, normalizes registry status fields, and disables imported routing groups and automatic group creation. It then creates current safety backups and writes the two current files transactionally. Older exports without a registry remain importable. Preserve local tool-risk settings, rules, lists, events and change history according to the existing synchronization rules.

## Local continuation ownership

Normal Codex-menu/native and authenticated gateway routing can save observed continuation ownership in `intent-routing-continuations.json` beside the existing native checkpoint key. Its separate `intent-routing-continuations.key` authenticates and encrypts each record, including its timestamp, with the existing checkpoint codec. Only domain-separated hashes of consumers, aliases, conversation/response/tool/item/opaque identifiers and exact destination fingerprints are persisted. Prompts, raw opaque content, tool arguments, tokens, endpoint plaintext and provider configuration are not written to this store. Unix files are private (`0600`); unsafe, malformed or oversized files are refused, not repaired or replaced on load.

The store is separate from both databases and their portable export/sync payloads; no schema version changes. Queued aggregate snapshots use a blocking worker for encryption and atomic file replacement, never a forwarding-path transaction or file write. Normal disconnect/stop flushes observed generations; abrupt termination can lose queued observations. The store retains at most 512 conversations for 24 hours, with 128 KiB per encrypted plaintext snapshot, 4096 identifier mappings per snapshot and a 16 MiB file budget. Overflow invalidates the affected snapshot rather than saving a partial owner map. Eviction, corruption, missing keys and destination changes make recovery unavailable instead of guessing another account. See [routing recovery and limits](../guides/intent-routing.md#continuation-recovery-after-restart).

WorkBuddy ownership and native backups remain beside its catalog. TRAE model ownership also exists in client browser storage. Removing a VaryAI database does not clear those native catalogs.

## Transactions, backups and synchronization

Connections use separate mutexes; lock main before extension. Audit, usage and observation persistence is asynchronous. Request forwarding must not execute blocking transactions or wait for auxiliary persistence.

Cross-file replacement uses SQLite ATTACH, DELETE journaling and FULL synchronization. Native rollback recovery validates journal ownership and rejects foreign super-journal paths.

Portable SQL/binary exports combine both databases into the existing monolithic transport without native identity metadata. Restore validates staging, creates a safety backup, replaces both live databases transactionally and retains the destination UUID. Exports can contain provider credentials and client recovery data, but exclude device settings, account/certificate files and saved gateway secret copies. Skills require their file assets too.

Gateway authentication stores SHA-256 hashes in the extension. Device-local `gateway-secrets/<key UUID>` files can contain plaintext copies. Reveal validates a copy against its hash; a missing copy does not invalidate the key and cannot be reconstructed. Unix private files use `0600` and directories `0700`; these permissions are not encryption.

WebDAV/S3 use manifest `varyai-sync` and reserved `<remote root>/varyai/v2/db-v6/<profile>/...` paths. The default root is `varyai-sync`. The reserved segment prevents collision even with an old custom root. New installations configure accounts and initial sync direction independently; no old credentials, remote state or automatic upgrade are transferred. Existing security/tool-risk preservation rules still apply.

Implementation: [native identity](../../src-tauri/src/database/native.rs), [startup/recovery](../../src-tauri/src/storage_setup.rs), [portable storage](../../src-tauri/src/database/storage.rs), [native tests](../../src-tauri/src/database/native_tests.rs). Synthetic tests do not establish installer/signing or every vendor client's live acceptance.
