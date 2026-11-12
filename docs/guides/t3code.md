# T3 Code

The adapter targets the `providerInstances` settings schema in
[official T3 Code](https://github.com/pingdotgg/t3code/tree/b5256ad09c0553347a6ddfd2d30de9b9c12c79d5).
The default file is `~/.t3/userdata/settings.json`, or
`$T3CODE_HOME/userdata/settings.json`.

Connecting creates a `varyai` provider instance using the `codex` driver, with
its own `userdata/varyai-codex/config.toml` and `models.json`. Codex uses the
Responses endpoint and a local bearer token without requiring an OpenAI login.
T3 Code must be able to locate a compatible Codex executable. Existing provider
instances, logins and the user's regular Codex configuration are preserved.

The selected model becomes T3 Code's default model selection. Choose the VaryAI
instance for new threads; existing threads keep their original provider. The
adapter preserves unrelated settings, rejects an existing unowned `varyai`
instance, and restores only owned fields when disconnected. Older `providers`
schemas require migration in T3 Code first. This version adds the Codex harness;
it does not add a second Claude harness instance.

This integration does not implement VaryAI's legacy per-provider configuration,
MCP/Skills synchronization or profile bundles. Installation detection and
version inspection do not launch or install any application.

## Manual acceptance

1. Quit T3 Code before changing its settings. To test an uninstalled VaryAI source
   change, quit the installed VaryAI instance and run `pnpm dev` from the repository.
2. Save at least one working provider/model in VaryAI. Open **Tools → T3 Code**,
   choose that shared model and enable the connection. If the application is
   undetected, install its desktop bundle in the platform's standard application
   location and rescan. T3 Code must be able to find the Codex executable.
3. Start T3 Code. Choose the **VaryAI** provider instance and create a new thread;
   existing threads retain their original provider. Ask for a short fixed reply.
4. In the same thread, request two sequential terminal-tool calls that each print
   a synthetic marker, with the second call made after receiving the first result.
   Confirm that both tool results are followed by the model's final answer.
5. Inspect VaryAI's request/token statistics to confirm that inference went through
   its gateway. A configured connection or a populated model menu alone is not a
   successful inference test.
6. Disconnect through VaryAI and confirm that the previous instance/default model
   is restored. Unrelated T3 Code settings and its original provider instances
   should remain intact.

Live inference consumes the chosen upstream's quota. Automated tests use temporary
settings, synthetic credentials and dynamic ports; they do not launch T3 Code or
establish live acceptance with the installed client. Older `providers` settings
must be migrated in T3 Code before this adapter can connect.
