# Configuration profiles

Profiles capture selected client configuration so you can restore a known setup across supported agents. A profile can contain credentials, MCP settings, Skills, and instruction files. Keep profile data and exports private.

## Capture and apply

1. Configure each client normally and back up its files.
2. Open the profile controls from a client page and create a named profile. By default, capture only that client. Enable **Multi-agent profile** to preselect the configured, supported agents currently selected in the main window; adjust that selection before saving if needed.
3. Review the selected scope before applying a saved profile. Read any validation errors or warnings instead of assuming every category was restored.
4. After applying, follow the client restart or new-session instructions in the completion notification. A successful apply updates saved configuration; running clients may still need to reload it.
5. Use the explicit refresh/update action to replace a saved snapshot with the current configuration. Renaming or deleting a profile affects the shared profile entity.

Every newly created profile uses a configuration snapshot, including profiles containing only one agent. Multi-agent selection can also contain just one supported agent; an empty selection cannot be saved. Older single-client profiles retain their existing format and behavior.

Refreshing replaces the entire bundle with every configured, supported agent currently selected for management. Newly selected agents are included, and agents no longer selected are removed from the snapshot. The refresh reads current agent availability and captures current provider, model and supported resource selections in one operation; it does not reuse the old bundle's agent scope or require another selection dialog.

Some older profiles contain only a single client group. Provider selection for Claude Code and Claude Desktop is independent. An uncaptured category is different from a captured empty category: the former leaves that category untouched; the latter can clear its enabled resources when applied.

Selecting a single-client profile opens its details, including the currently selected profile. Review its saved provider, MCP, Skills and prompt references, then choose **Apply scheme** to switch. Closing the preview leaves configuration unchanged. These older profiles reference provider configuration rather than freezing model settings, and still save automatically when switching away.

New or refreshed multi-agent profiles capture the current supplier and its saved model configuration for WorkBuddy, Qoder, Kimi, Qwen, ZCode and TRAE. Previously enabled suppliers do not become additional targets. OpenCode, OpenClaw, Hermes and Pi retain their enabled-provider sets. Existing snapshots keep their saved targets until explicitly refreshed. Opening supplier lists after applying a profile does not invalidate Undo; actual external configuration edits still do.

Profile names must be unique after trimming surrounding whitespace. Creation and renaming report a duplicate name without overwriting either profile. Existing duplicate profiles can still refresh their snapshots or be renamed to resolve the duplication.

Opening the profile menu refreshes agent availability so newly configured clients can be selected. Preview/apply and Undo checks ignore JSON object-key ordering, including provider metadata maps. Changes to values, ordered lists or native file contents still invalidate the review and require a fresh preview.

Qwen profile snapshots record upstream model endpoints, so proxy port or route changes do not invalidate model selection. Applying a snapshot resolves the current local route and preserves the model's wire API. Older OpenAI selectors can resolve a Responses-only model when the endpoint and model identify exactly one entry; ambiguous or removed models remain errors. Kimi and ZCode selections use stable model/provider IDs rather than proxy URLs.

If an agent fails during apply, the dialog identifies that agent and the failed provider, model-selection or resource step, then reports whether the previous configuration was restored. Error details use fixed messages instead of raw configuration or credential-bearing upstream errors.

The shared menu displays the last successfully applied multi-agent profile across client pages, including clients not captured in that profile. Uncaptured clients keep their existing configuration and per-client markers. The shared selection survives application restarts; Undo restores the previous selection. Choosing **No profile** clears the selected bundle's markers without rewriting client configuration. Applying a single-client profile returns the menu to its per-client selection behavior.

The implementation validates managed file paths and coordinates native operations. Application and client processes may also update those files, so keep backups and avoid concurrent manual edits during an apply operation. Review recovery errors rather than copying individual database files to force a restore.

## Related integrations

- Pi local routing projects selected provider/model configurations through Vary AI. Review the routing status and reported catalog issues after enabling it. Disabling routing uses recorded ownership and recovery information to restore managed configuration while preserving unrelated edits.
- TRAE provider and profile selections can be saved while the client is closed, including a return to Official. Keep Vary AI running: the latest selection is delivered automatically when the authorized bridge reconnects, withdrawing stale managed entries first. Delivery status does not invalidate profile Undo. Connection setup requires explicit consent and a user-launched supported client. The bridge exposes supported message inspection and managed model configuration; it does not establish coverage of every client action or attachment. Restart the client when requested after first setup. Review the impact of local debugging access before approval.
- See [Doubao Work](doubao-work.md) for its separate connection, coverage, and restoration workflow.

Profile support does not make third-party MCP servers, Skills, commands, endpoints, or imported configuration trustworthy. Review them before activation.
