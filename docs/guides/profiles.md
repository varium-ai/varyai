# Configuration profiles

Profiles capture selected client configuration so you can restore a known setup across supported agents. A profile can contain credentials, MCP settings, Skills, and instruction files. Keep profile data and exports private.

## Capture and apply

1. Configure each client normally and back up its files.
2. Open the profile controls, create a named profile, and select the supported clients and resource categories to capture.
3. Review the selected scope before applying a saved profile. Read any validation errors or warnings instead of assuming every category was restored.
4. After applying, follow the client restart or new-session instructions in the completion notification. A successful apply updates saved configuration; running clients may still need to reload it.
5. Use the explicit refresh/update action to replace a saved multi-agent snapshot with the current configuration. Renaming or deleting a profile affects the shared profile entity.

Some older profiles contain only a single client group. Provider selection for Claude Code and Claude Desktop is independent. An uncaptured category is different from a captured empty category: the former leaves that category untouched; the latter can clear its enabled resources when applied.

The implementation validates managed file paths and coordinates native operations. Application and client processes may also update those files, so keep backups and avoid concurrent manual edits during an apply operation. Review recovery errors rather than copying individual database files to force a restore.

## Related integrations

- Pi local routing projects selected provider/model configurations through Vary AI. Review the routing status and reported catalog issues after enabling it. Disabling routing uses recorded ownership and recovery information to restore managed configuration while preserving unrelated edits.
- TRAE connection setup requires explicit consent and a user-launched supported client. The bridge exposes supported message inspection and managed model configuration; it does not establish coverage of every client action or attachment. Restart the client when requested after first setup. Review the impact of local debugging access before approval.
- See [Doubao Work](doubao-work.md) for its separate connection, coverage, and restoration workflow.

Profile support does not make third-party MCP servers, Skills, commands, endpoints, or imported configuration trustworthy. Review them before activation.
