# First-run connection guide

Vary AI opens a two-step guide when setup has not been acknowledged and the shared provider catalog is empty. Existing configured installations keep their normal workspace. **Maybe later** dismisses the guide and persists the existing `firstRunNoticeConfirmed` setting. After completion or dismissal, the guide does not appear again. Models provides the regular provider addition and import flows without a separate onboarding entry.

Choose one connection method:

- **Paste a URL and Key:** enter the provider's API URL and Key, then choose its API protocol. OpenAI Chat Completions is the default; OpenAI Responses, Anthropic Messages and Gemini API are also supported. **Connect** fetches the model catalog before saving the provider. A failed request or an empty catalog leaves the form editable without saving a provider. Keys remain masked and are cleared from the form after saving. Providers without model discovery can be added manually in Models.
- **Sign in to an account:** choose a supported subscription provider. The guide uses the same browser or device authorization flow as Models, including cancellation when changing providers or returning to the first step. After login, it saves the selected account as a shared provider. ChatGPT sign-in may display Codex in the browser. Available providers follow the application's supported subscription list rather than implying support for every provider's website login.
- **Import from another app:** preview supported sources and select connections to import. This embeds the existing importer, preserving source fingerprints, duplicate detection, name conflict handling and sanitized errors. Source application files are not modified. Import is supported for CC Switch, Claude Code, Codex, OpenCode and Magpie; Alma is not currently supported.

After a successful connection or import, choose **Open Tools** to connect a supported tool. Provider credentials are never placed in the copied setup-help prompt. Completion failures can be retried without repeating a successful provider save or import. The guide stores no credentials in browser storage and does not change the storage schema.

The guide supports English, Simplified Chinese, Traditional Chinese and Japanese, as well as the application's light and dark themes. Its header and footer remain visible while the contents scroll on smaller windows.
