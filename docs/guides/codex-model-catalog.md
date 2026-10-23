# Codex model catalog import

The frontend can use a provider's returned model list to populate the Codex model mapping/catalog during provider setup. Keep the user's explicit model names, context sizes and reasoning choices; do not silently overwrite custom input during a refresh.

Claude and Codex use the same model-mapping actions: **Fetch Models** loads candidates without changing saved mappings or the default model; **One-click Setup** imports every returned model into a separate row and skips IDs already present. There is no ten-model import limit. One-click setup fetches the list first if no candidates are loaded. Codex also keeps **Add Model** for manual entries and its context-window and reasoning controls. Save the provider and restart Codex to refresh its model menu.

Generated catalog data must continue to match the backend TOML/JSON contract. The live client configuration and catalog file must refer to the same provider and model IDs. A model list response does not prove that every listed model is callable or included in an account's quota.

See the [frontend helper](../../src/components/providers/forms/helpers/codexCatalogAutoImport.ts), its [tests](../../src/components/providers/forms/helpers/codexCatalogAutoImport.test.ts), and [backend configuration](../../src-tauri/src/codex_config.rs). Test with synthetic provider data rather than internal gateway names or credentials.
