# Provider setup and local routing

Select the target client before adding or editing a provider. Provider credentials, endpoint formats and model catalogs belong to that client configuration; a preset does not establish account access or model availability.

## Configure a provider

1. Install and initialize the client separately. Back up existing client configuration.
2. Add a provider from an appropriate preset, or enter a custom endpoint and your own credentials. Use the provider's actual API documentation for account requirements and supported models.
3. Select the upstream API format supported by that endpoint. A model name does not determine the transport protocol.
4. Fetch models when that provider supports discovery, review the resulting catalog and mappings, then save. Existing custom model settings may take precedence over imported defaults.
5. Select or enable the provider according to the client integration. Reload the client when it must reread configuration.

See [Codex model catalogs](codex-model-catalog.md) for import behavior. Do not use an internal gateway address or a usable key in a shared example.

## Reuse a named connection

Every agent's provider form includes **Connection preset**. Open **Manage presets** to show a separate dialog above the provider form, enter a name, the supplier's API base URL and an API key, choose its upstream format, and save. The preset table shows the saved upstream format. Leave the format unspecified to preserve each agent's current choice, including for existing presets. Closing the dialog or pressing Escape returns to the provider form with its unsaved values intact. Use the API endpoint supplied for client configuration, not the supplier's website homepage. Presets can be edited or deleted from the same dialog; keys are masked in the editor.

To check protocol support, enter a detection model and click **Detect configuration**. Detection reuses the standard four-protocol JSON/SSE probes and can send up to eight small inference requests that may incur charges. Choose a verified format with **Use this format**, then save. Results apply only to that URL, key and model; failures and temporary unavailability do not prove permanent incompatibility. The detection model is not copied into agent configurations.

Choose a saved connection when adding or editing a custom provider to fill its name, API URL, key and selected upstream format. Save the provider normally to persist those values for that agent. This copies the values once: the agent remains independently editable, and later preset edits or deletion do not change previously configured providers. Model mappings and other agent-specific settings remain unchanged. Presets with an explicit protocol unavailable to the target client cannot be applied; use a compatible protocol or an unspecified-format preset instead. Selecting a format does not enable route takeover. Official account and managed OAuth configurations retain their own sign-in flow.

Connection presets differ from Universal Providers, whose application-specific configurations can be synchronized across clients. Saving a connection preset alone does not configure an agent or activate local routing.

## Claude Desktop menu mappings

In model-mapping mode, the editor offers Sonnet, Opus, Fable and Haiku role fields. Only roles with an actual request model are saved to the Desktop menu. Clear a role's actual request model and save to remove that entry; an old display label or checked 1M declaration does not keep it published. Reopen Desktop after applying the provider to reload its menu. Three configured roles with 1M declared can appear as six options in Desktop: regular and 1M context for each role. The declaration does not grant additional upstream capacity.

An omitted Haiku role remains available for background sub-agent calls through an internal fallback to Sonnet or the first configured route, without an extra menu entry. Explicit Haiku mappings take precedence. Gateway allowlists still check the incoming client route ID before this mapping.

When routing Claude Desktop to a third-party endpoint, Vary AI omits Desktop-specific `anthropic-client-*` metadata headers, including `anthropic-client-version` and `anthropic-client-platform`, because some compatible gateways reject them. Anthropic Messages requests retain the protocol's `anthropic-version` header. Requests to `api.anthropic.com` and Claude Code header handling keep their existing behavior.

## Import all Claude Code models

In the Claude Code provider's model mapping section, **Quick Set** imports every model returned by discovery as a separate editable row. If no catalog has been fetched, the button fetches it first using the current endpoint/key or selected managed account. Empty role fields do not disable import. Failed or superseded requests do not change the rows.

Import appends missing model IDs without duplicating existing IDs or replacing customized labels. Save the provider to persist the rows in `modelPicker.options`; reopening the provider restores them. Rows can be renamed, edited, or removed. The Sonnet, Opus, Fable, Haiku and Subagent defaults remain separately configurable. With local routing enabled, an explicitly listed model takes precedence over role and fallback mapping, so selecting it preserves its request ID.

Claude Code's [`modelPicker` setting](https://code.claude.com/docs/en/settings-reference#modelpicker) requires version 2.1.242 or later. Restart Claude Code after saving to load the menu. Older clients can still use their role mappings or select an explicit model with `/model <model-id>`; importing rows does not upgrade the client. A discovery result lists advertised IDs and does not verify inference access for every model.

## Choose the correct protocol

The format controls conversion when the supported client is routed through Vary AI:

| Client request       | Upstream selection                    | Behavior                                                                                                                    |
| -------------------- | ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Codex Responses      | Responses                             | Native upstream format; conversion is unnecessary. Routing can still be used for supported inspection and routing features. |
| Codex Responses      | Chat Completions                      | Requires local routing to convert requests and supported JSON/SSE responses.                                                |
| Codex Responses      | Anthropic Messages                    | Requires local routing to convert supported Messages payloads and responses.                                                |
| Claude Code Messages | Anthropic Messages                    | Native upstream format.                                                                                                     |
| Claude Code Messages | Chat Completions, Responses or Gemini | Requires the corresponding local conversion path. Endpoint capabilities and conversion limits still apply.                  |

The provider form exposes the applicable options. Do not set Codex's wire protocol to Chat Completions to work around an upstream mismatch: its client-facing path remains Responses, with the upstream format selected in Vary AI. Authentication and model catalog behavior also differ between official account integrations and API-key providers.

Conversion cannot guarantee that every vendor extension, tool schema, reasoning parameter or streaming event is interchangeable. Confirm the intended feature with a small synthetic request before using production data.

## Enable routing

For supported local-proxy clients, start the routing master switch and enable routing for the selected client. The default listener is `127.0.0.1:15721`. A running listener and a configured provider alone do not establish that the client is using it; check the per-client routing state and the client's active configuration.

Taking over a client changes its live endpoint configuration. The client-facing URL can include `/v1` where its protocol requires it. Keep the provider's upstream endpoint separate from that loopback URL. Do not change the listener to a public interface just to resolve an endpoint-format error.

The current local proxy covers Claude Code, Codex, Gemini CLI and Grok Build. Other client integrations have separate capabilities. [WorkBuddy](workbuddy.md) and [Qoder](qoder.md) have their own takeover and recovery paths; a provider appearing in the app does not imply identical proxy coverage.

## Verify and recover

- Confirm the client, active provider, routing state, selected format and exact model ID.
- Make a short synthetic request and review its status. A successful model-list fetch is not a completed inference request.
- For 404, inspect the base URL and API format. For unavailable-model or 503 errors, check the actual catalog, account access and upstream routing. For 413, check request size and the upstream limit.
- Disable takeover through the corresponding application controls before manually editing client configuration. If recovery is needed after an abnormal exit, inspect saved backups and preserve unrelated settings.

Refer to [troubleshooting](../troubleshooting.md) for diagnostics. Share only redacted configuration excerpts and logs.

## Loopback-only listener

The local gateway accepts literal loopback IP addresses only, such as `127.0.0.1` or `::1`. The interface also accepts `localhost` and saves it as `127.0.0.1`. Wildcard and LAN addresses are rejected before saving or starting the listener. This application is not a shared LAN gateway.

If an older installation saved a non-loopback address, change it to a loopback address before restarting local routing. When a VPN intercepts loopback requests, see [local proxy bypass examples](proxy-configs/README.md).
