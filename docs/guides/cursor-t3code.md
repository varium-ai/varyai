# Cursor and T3 Code

The Agents page has one **Cursor** entry for its desktop editor and CLI. Presence
is detected independently: installing either is enough to show Cursor. When both
versions can be read, the same entry shows App and CLI versions. The older
`cursor-cli` API identifier remains a compatibility alias for native CLI choices;
it is not a second entry in the Agents list or visibility settings.

| Client | Managed behavior | Platforms |
| --- | --- | --- |
| Standard Cursor desktop | Installation/version discovery and an explicit cloud gateway limitation | macOS, Windows |
| Cursor CLI | Observe its native model and select Cursor Auto inside the Cursor entry | macOS, Windows; native configuration also works on Linux |
| Cursor Private Inference | Connect its local desktop agent to VaryAI's shared model catalog | macOS, Windows |
| T3 Code | Add an independent VaryAI Codex instance and its starting model | macOS, Windows |

## Cursor subscription provider

The separate **Providers → Subscriptions → Cursor** entry uses your Cursor
subscription as an upstream for clients connected to VaryAI. It supports browser
login, account model discovery, caller tools and usage tracking through the
Magpie community plugin protocol. See the [Cursor subscription guide](cursor-subscription.md).
This direction does not require a public VaryAI gateway.

## Standard Cursor and Magpie

The standard desktop editor is now recognized as Cursor. Its built-in Agent
runs through Cursor's backend: a loopback VaryAI endpoint is not reachable from
that backend. Entering a localhost URL in its ordinary API-key settings does not
establish a local connection. Using VaryAI models through that mechanism requires
an authenticated HTTPS gateway reachable by Cursor's service. This adapter does
not create a public tunnel, publish the local gateway, or rewrite the standard
editor's API-key storage.

This boundary is also present in
[Magpie's current implementation](https://github.com/yetone/magpie/tree/b4fc29a6e4e89b128df9254f4e5f5824553eb133/internal/agent):
its `cursor` adapter manages **Cursor CLI**, and its `cursor-local` adapter targets
**Cursor Private Inference**. The author explicitly distinguishes standard
Cursor in [issue #1254](https://github.com/yetone/magpie/issues/1254).
The Cursor popup in Magpie’s plugin catalog instead describes the subscription
provider above; it is a separate integration direction.

The unified entry keeps these capabilities separate. Standard Cursor has no
local gateway switch. If its CLI is installed, its native Auto control is shown;
that control changes only the CLI. A shared model selection is offered only for
a detected Private Inference desktop build. Backend connect and repair operations
also enforce that requirement. Neither the editor nor the CLI is reported as
routed through VaryAI merely because it is detected.

## Cursor CLI

The CLI is detected using `cursor-agent`. Its current model comes from
`model.modelId` in `~/.cursor/cli-config.json`. `CURSOR_CONFIG_DIR` overrides that
directory; on Linux `XDG_CONFIG_HOME` is also honored as documented by Cursor.

Selecting Auto updates the three model display/ID fields and
`hasChangedDefaultModel`. Existing authentication, permissions and other model
parameters are preserved. The CLI remains on Cursor's service and has no local
gateway switch. Other native models can still be selected with Cursor's own
`/model` command. Shared VaryAI models are not advertised to this CLI. If Private
Inference and the CLI are both installed, the same Cursor row provides a separate
CLI Auto action alongside the desktop routing control.

## Cursor Private Inference

Private Inference is a separate Cursor distribution whose Agent runs locally;
it is not the standard editor's privacy-mode setting. Its `product.json` identifies
it as `Cursor Private Inference`. A regular `Cursor.app`, even with the same
bundle identifier, is recognized as an editor without granting local gateway
capability. The product check remains mandatory on both macOS and Windows.

Connecting sets `CURSOR_LOCAL_AGENT_BASE_URL` to the agent-specific local `/v1`
endpoint and `CURSOR_LOCAL_AGENT_API_KEY` to its local connection token. macOS
uses the login session's `launchctl` environment; Windows uses the current
user's `Environment` registry key and notifies Explorer. Fully quit and reopen
Private Inference after connecting or disconnecting.

A gateway saved inside Cursor, or per-model credentials, takes precedence over
these environment defaults. Clear that saved override in Cursor when intending
to use VaryAI. Connected status establishes ownership of the managed environment;
it is not a live inference probe. The model list publishes the Responses API,
context/output limits, streaming, tool, vision and reasoning capabilities. Select
a model in Cursor's own picker; VaryAI does not rewrite its shared editor database.

Disconnect restores the previous values only while the managed variables still
belong to VaryAI. External changes require explicit repair. After a macOS reboot,
VaryAI restores an active connection only if both variables are absent and the
receipt predates that boot. Existing external values are retained. Linux desktop
environment integration and automatic installation of Private Inference remain
outside this adapter.

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
