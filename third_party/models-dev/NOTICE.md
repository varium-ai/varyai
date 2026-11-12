# Public reasoning capability snapshot

`src-tauri/src/resources/reasoning-capabilities.json` contains a reduced
models.dev public catalog snapshot available locally on November 3, 2026.
The source cache was `models.json`, updated on November 3, 2026, and its
upstream endpoint is [models.dev/api.json](https://models.dev/api.json).

Only 117 maker API entries with explicit effort declarations are retained,
including their canonical model IDs, reasoning switch and effort lists.
No account identities, credentials, endpoints, pricing or private configuration
are included. Selection does not infer levels from model families.

The catalog is distributed under the MIT License, Copyright (c) 2025
models.dev. Its complete notice is retained in [LICENSE](LICENSE).

This offline snapshot supplies missing capability facts for existing selected
models. Refreshed declarations, vendor metadata and explicit user overrides
take precedence. Account visibility and context capacities remain independent.

## Public context capacity snapshot

`src-tauri/src/resources/model-context-windows.json` is a reduced snapshot of
[models.dev/api.json](https://models.dev/api.json), fetched on November 10, 2026.
It retains 2,611 model ID suffixes and integer input capacities only. Each
observation uses the smaller of the declared input and context limits when both
exist. The most frequently reported capacity wins, with the larger capacity
breaking equal counts, matching the existing public context resolver. The
snapshot retains the same MIT license and copyright notice linked above.
It contains no account data, credentials, endpoints, prices or private settings.

The provider editor uses cached public capacities before this fallback.
Vendor metadata and explicit selections take precedence over public facts.
Unknown IDs do not receive inferred capacities or badges. This read-only lookup
does not save a public observation as a user context override.
