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
