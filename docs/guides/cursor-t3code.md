# Cursor and T3 Code

The Agents page distinguishes Cursor's two integration paths. This follows the
separate CLI and Private Inference adapters in
[Magpie](https://github.com/yetone/magpie/tree/745d209dcf5f4d9b7feee416e1848e33502834e6/internal/agent).

| Agent | Managed behavior | Platforms |
| --- | --- | --- |
| Cursor CLI | Observe its native model and select Cursor Auto | macOS, Windows; native configuration also works on Linux |
| Cursor Private Inference | Connect the local desktop agent to VaryAI's shared model catalog | macOS, Windows |
| T3 Code | Add an independent VaryAI Codex instance and its starting model | macOS, Windows |

## Cursor CLI

The CLI entry is detected independently from the editor, using `cursor-agent`.
Its current model comes from `model.modelId` in `~/.cursor/cli-config.json`.
`CURSOR_CONFIG_DIR` overrides that directory; on Linux `XDG_CONFIG_HOME` is also
honored as documented by Cursor.

Selecting Auto updates the three model display/ID fields and
`hasChangedDefaultModel`. Existing authentication, permissions and other model
parameters are preserved. The CLI remains on Cursor's service and has no local
gateway switch. Other native models can still be selected with Cursor's own
`/model` command. Shared VaryAI models are not advertised to this CLI.

## Cursor Private Inference

This is the desktop distribution whose `product.json` identifies it as
`Cursor Private Inference`. A regular `Cursor.app`, even with the same bundle
identifier, is not counted as this distribution. The regular editor's API-key
override still sends requests through Cursor's servers and cannot reach a
loopback gateway from there.

Connecting sets `CURSOR_LOCAL_AGENT_BASE_URL` to the agent-specific local `/v1`
endpoint and `CURSOR_LOCAL_AGENT_API_KEY` to its local connection token. macOS
uses the login session's `launchctl` environment; Windows uses the current
user's `Environment` registry key and notifies Explorer. Fully quit and reopen
Private Inference after connecting or disconnecting.

A gateway saved inside Cursor, or per-model credentials, takes precedence over
these environment defaults. Clear that saved override in Cursor when intending
to use VaryAI. The connected status establishes ownership of the managed
environment; it is not a live inference probe or proof that Cursor has discarded
an already saved override.

The model list publishes the Responses API, context/output limits, streaming,
tool, vision and reasoning capabilities. Select a model in Cursor's own picker;
VaryAI does not rewrite Cursor's model preference or its shared editor database.

Disconnect restores the previous values only while the managed variables still
belong to VaryAI. External changes require explicit repair. After a macOS reboot,
VaryAI restores an active connection only if both variables are absent and the
receipt predates that boot. Existing external values are retained.

Linux desktop environment integration, automatic installation of the Private
Inference distribution, and regular Cursor cloud bridging are outside this
adapter. The download action opens Cursor's website; it does not replace an
existing installation.

## T3 Code

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

These entries do not implement VaryAI's legacy per-provider configuration,
MCP/Skills synchronization or profile bundles. Installation detection and
version inspection do not launch or install any application.
