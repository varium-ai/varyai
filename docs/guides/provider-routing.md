# Provider setup and local routing

Select the target client before adding or editing a provider. Provider credentials, endpoint formats and model catalogs belong to that client configuration; a preset does not establish account access or model availability.

## Configure a provider

1. Install and initialize the client separately. Back up existing client configuration.
2. Add a provider from an appropriate preset, or enter a custom endpoint and your own credentials. Use the provider's actual API documentation for account requirements and supported models.
3. Select the upstream API format supported by that endpoint. A model name does not determine the transport protocol.
4. Fetch models when that provider supports discovery, review the resulting catalog and mappings, then save. Existing custom model settings may take precedence over imported defaults.
5. Select or enable the provider according to the client integration. Reload the client when it must reread configuration.

See [Codex model catalogs](codex-model-catalog.md) for import behavior. Do not use an internal gateway address or a usable key in a shared example.

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
