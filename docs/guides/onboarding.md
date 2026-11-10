# First-run connection guide

Vary AI opens a two-step guide when setup has not been acknowledged and the shared provider catalog is empty. Existing configured installations keep their normal workspace. **Maybe later** dismisses the guide and persists the existing `firstRunNoticeConfirmed` setting. After completion or dismissal, the guide does not appear again. Models provides the regular provider addition and import flows without a separate onboarding entry.

The account and import cards preview their supported sources as brand icons.
Hover an icon to see its full name; each icon also has an accessible name.
Import previews use the same icon resources as the regular importer and follow
its Magpie-first order. Icon rows wrap on smaller windows.
Account previews show each brand icon once, using its first method's name.
OpenAI is shown as ChatGPT / Codex. The account chooser still offers each method separately.

Choose one connection method:

- **Paste a URL and Key:** enter the provider's API URL and Key, then choose its API protocol. OpenAI Chat Completions is the default; OpenAI Responses, Anthropic Messages and Gemini API are also supported. **Connect** fetches the model catalog before saving the provider. A failed request or an empty catalog leaves the form editable without saving a provider. Keys remain masked and are cleared from the form after saving. Providers without model discovery can be added manually in Models.
- **Sign in to an account:** choose a supported subscription provider. The guide uses the same browser or device authorization flow as Models, including cancellation when changing providers or returning to the first step. After login, it fetches that account's available models and saves the selected account as a shared provider with all discovered models selected, up to the editor's 256-model selection limit. Model capabilities and display metadata are preserved. A failed or empty model response keeps the guide open and allows retrying without signing in again. Reconnecting an existing account preserves its explicit model policy; an empty default policy is initialized with the discovered selection. ChatGPT sign-in may display Codex in the browser. Available providers follow the application's supported subscription list rather than implying support for every provider's website login.
- **Import from another app:** preview supported sources and select connections to import. This embeds the existing importer, preserving source fingerprints, duplicate detection, name conflict handling and sanitized errors. Its source tabs follow the same order as the Models importer: Magpie, CC Switch, Claude Code, Codex and OpenCode. The guide initially selects the first source in that order with importable connections. Source application files are not modified. Alma is not currently supported.

After an import, API-key connection or account connection is saved successfully, the guide automatically saves its completion state, closes and opens the application's Tools page. There is no success confirmation page or extra button. If saving the completion state fails, **Retry** repeats only that step, preserving the saved connection. Failed connection saves remain in the guide for correction or retry. Provider credentials are never placed in the copied setup-help prompt. The guide stores no credentials in browser storage and does not change the storage schema.

The guide supports English, Simplified Chinese, Traditional Chinese and Japanese, as well as the application's light and dark themes. Its header and footer remain visible while the contents scroll on smaller windows.
