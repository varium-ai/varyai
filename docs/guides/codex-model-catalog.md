# Codex model catalog import

The frontend can use a provider's returned model list to populate the Codex model mapping/catalog during provider setup. Keep the user's explicit model names, context sizes and reasoning choices; do not silently overwrite custom input during a refresh.

Claude and Codex use the same model-mapping actions: **Fetch Models** loads candidates without changing saved mappings or the default model; **One-click Setup** imports every returned model into a separate row and skips IDs already present. There is no ten-model import limit. One-click setup fetches the list first if no candidates are loaded. Codex also keeps **Add Model** for manual entries and its context-window and reasoning controls. Save the provider and restart Codex to refresh its model menu.

Generated catalog data must continue to match the backend TOML/JSON contract. The live client configuration and catalog file must refer to the same provider and model IDs. A model list response does not prove that every listed model is callable or included in an account's quota.

See the [frontend helper](../../src/components/providers/forms/helpers/codexCatalogAutoImport.ts), its [tests](../../src/components/providers/forms/helpers/codexCatalogAutoImport.test.ts), and [backend configuration](../../src-tauri/src/codex_config.rs). Test with synthetic provider data rather than internal gateway names or credentials.

For ordinary generated third-party catalogs, the configured default model has the highest catalog priority without changing row order, model IDs or reasoning capabilities. An active TOML profile's model takes precedence over the top-level model. An absent or unlisted default leaves catalog ordering unchanged. Official vendor catalog priorities remain authoritative.

The desktop client can issue independent background requests for titles and summaries. When its preferred background model is unavailable, it may select the catalog default or another model with the required capabilities. These requests are real usage and remain attributed to their requested and forwarded model IDs, even when the visible chat uses a different model. The gateway usage panel explains this distinction.

When a full inference URL is configured, model discovery removes the supported operation suffix (such as `/chat/completions` or `/responses`) and retains the provider's path/version prefix and query parameters. HTTP 429, upstream HTTP 5xx and connection failures have distinct sanitized feedback; no upstream response body or credential is displayed. An unexplained failure recording still requires the actual endpoint and failure category before its historical cause can be confirmed.
