# Provider setup and local routing

Select the target client before adding or editing an existing per-agent provider. Its settings remain client-specific; a preset does not establish account access or model availability. **Settings → Routing → Routing groups** automatically uses the saved provider lists from all agents, without a separate upstream list or import. Manage each connection in its source agent's provider editor; see [shared providers and routing groups](intent-routing.md).

## Agent versions and updates

The home **Tools / Agent** list checks installed CLI and desktop versions. Supported macOS desktop update feeds include Claude Desktop, WorkBuddy, Qoder CN, Doubao Work, TRAE CN, Kimi Code, ZCode and the supported DeepSeek Harness desktop channel. Desktop updates appear alongside CLI updates in the per-agent controls and **Update all**. Codex continues to show and update its CLI version in this list. An unknown latest version, unreadable installed version or unmatched release channel does not establish that an app is current and does not offer an automatic update.

See [DeepSeek Harness Desktop](deepseek-harness.md) for its profile, native model selection and inspection boundaries.

Checking versions does not launch or update a client. Selecting an update uses the existing official installer flow; a desktop update can close and reopen that client. Each result is verified against a fresh installed version from the same desktop or CLI source. Kimi installations detected only as a CLI show their CLI version without using the desktop update feed.

## Configure a provider

1. Install and initialize the client separately. Back up existing client configuration.
2. Add a provider from an appropriate preset, or enter a custom endpoint and your own credentials. Use the provider's actual API documentation for account requirements and supported models.
3. Select the upstream API format supported by that endpoint. A model name does not determine the transport protocol.
4. Fetch models when that provider supports discovery, review the resulting catalog and mappings, then save. Existing custom model settings may take precedence over imported defaults.
5. Select or enable the provider according to the client integration. Reload the client when it must reread configuration.

See [Codex model catalogs](codex-model-catalog.md) for import behavior. Do not use an internal gateway address or a usable key in a shared example.

## Replace managed model catalogs

On the **Tools** page, WorkBuddy, Qoder and TRAE expose a searchable **Model list** containing the shared catalog. This is a preview while disconnected or waiting for TRAE's bridge, not confirmation that the client has loaded it. Browsing the list does not enable a connection or select a starting model; choose the chat model inside the client after connecting and loading its catalog. Doubao Work shows **Official models** because it supports official-session auditing only.

Selecting a supplier in Kimi, Qwen, ZCode, Qoder or WorkBuddy replaces the models managed by VaryAI with the target supplier's complete saved catalog. Selecting the official entry withdraws those managed models. File adapters stage validation and replacement before writing; native login state, MCP configuration and unrelated providers remain intact. Recognizable ownership markers also cover models left after deletion of the shared database. Unmarked historical native configurations cannot be distinguished from user-created configurations and remain protected.

TRAE performs replacement through its native model API. The bridge checks recovery data before deleting owned models and attempts restoration on failure. Recovery credentials remain in the operation's memory; ownership storage contains only identifiers. The connection checks its helper revision and refreshes an older resident helper before replacing models. Repeated checks reuse the current helper. Existing random debug endpoints remain reusable after local connection metadata is removed.

Claude Desktop and Grok Build already replace their generated model configuration as complete snapshots. Doubao Work exposes only its native official login and has no custom provider catalog to replace. OpenCode, OpenClaw, Hermes and Pi retain their multiple-provider mode.

## Reuse a named connection

Every agent's provider form includes **Connection preset**. Open **Manage presets** to show a separate dialog above the provider form, enter a name, the supplier's API base URL and an API key, choose its upstream format, and save. The preset table shows the saved upstream format. Leave the format unspecified to preserve each agent's current choice, including for existing presets. Closing the dialog or pressing Escape returns to the provider form with its unsaved values intact. Use the API endpoint supplied for client configuration, not the supplier's website homepage. Presets can be edited or deleted from the same dialog; keys are masked in the editor.

New API-key providers and new connection presets detect protocol support automatically after URL and key editing ends. Discovery tries the standard and Gemini model catalogs. Detection uses the selected model when it appears in the catalog, otherwise the first advertised model; if discovery fails, enter a model manually. Background discovery does not change the agent's default model. Official/OAuth, Bedrock and explicit full-URL modes retain their own setup flows.

Detection uses the standard four-protocol JSON/SSE probes and can send up to eight small inference requests per check that may incur charges. It selects the first protocol whose text and streaming responses both verify, in this order: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Gemini Native. Background detection preserves existing providers and manual protocol choices. Clicking **Detect configuration** retries detection and applies the first fully verified protocol, even when the current protocol also works; **Use this format** can apply another verified result. Claude Desktop uses one model-mapping editor. The explicitly chosen detection model stays selected when detection updates its upstream protocol; routing is controlled outside the provider form. Selecting a different model invalidates the previous request immediately; late results and failures cannot replace the current result. Results apply only to that URL, key and test model; an advertised catalog entry does not prove inference access, and temporary failures do not prove permanent incompatibility. The preset's detection model is not copied into agent configurations.

Choose a saved connection when adding or editing a custom provider to fill its name, API URL, key and selected upstream format. Save the provider normally to persist those values for that agent. This copies the values once: the agent remains independently editable, and later preset edits or deletion do not change previously configured providers. Model mappings and other agent-specific settings remain unchanged. Presets with an explicit protocol unavailable to the target client cannot be applied; use a compatible protocol or an unspecified-format preset instead. Selecting a format does not enable route takeover. Official account and managed OAuth configurations retain their own sign-in flow.

Connection presets differ from Universal Providers, whose application-specific configurations can be synchronized across clients. Saving a connection preset alone does not configure an agent or activate local routing.

## Claude Desktop menu mappings

When protocol detection initializes the default mappings, the first row uses the selected detection model for both its actual request model and its menu display name. The display name remains editable.

Use **Import all models** in Model Mapping mode to fetch the supplier catalog and add every model in one action. If the catalog has already been fetched, it is reused. The import skips blank and duplicate model IDs, fills empty rows before adding new ones, and preserves configured mappings and custom names, including models absent from the current catalog. New entries receive unique compatible route IDs and use the supplier model ID as their editable menu display name. Importing does not automatically declare 1M context support.

In model-mapping mode, add or delete as many model rows as needed. Sonnet, Opus, Fable and Haiku are compatibility roles; multiple rows may share a role, and each receives a unique route ID. Existing mappings, including several models in one role, are preserved when editing or switching between direct and mapped mode. Menu display names are customizable. Selecting a model from either dropdown updates both its requested model and display name; you can rename it afterwards. Only rows with an actual request model are saved to the Desktop menu. Delete a row or clear its actual request model and save to remove that entry; an old display label or checked 1M declaration does not keep it published. Reopen Desktop after applying the provider to reload its menu. Three configured roles with 1M declared can appear as six options in Desktop: regular and 1M context for each role. The declaration does not grant additional upstream capacity.

An omitted Haiku role remains available for background sub-agent calls through an internal fallback to Sonnet or the first configured route, without an extra menu entry. Explicit Haiku mappings take precedence. Gateway allowlists still check the incoming client route ID before this mapping.

When routing Claude Desktop to a third-party endpoint, Vary AI omits Desktop-specific `anthropic-client-*` metadata headers, including `anthropic-client-version` and `anthropic-client-platform`, because some compatible gateways reject them. Anthropic Messages requests retain the protocol's `anthropic-version` header. Requests to `api.anthropic.com` and Claude Code header handling keep their existing behavior.

## Import all Claude Code models

In the Claude Code provider's model mapping section, **Quick Set** imports every model returned by discovery as a separate editable row. If no catalog has been fetched, the button fetches it first using the current endpoint/key or selected managed account. Empty role fields do not disable import. Failed or superseded requests do not change the rows.

Explicitly clicking **Fetch models** replaces an empty or unlisted primary/role default with the first valid ID in the fetched catalog. Available defaults, context markers and Subagent's `inherit` setting remain intact. **Quick Set** applies the same check and appends missing model IDs without duplicating existing IDs or replacing customized labels. Save the provider to persist the rows in `modelPicker.options` and the updated defaults; reopening the provider restores them. Rows and role defaults remain editable. With local routing enabled, an explicitly listed model takes precedence over role and fallback mapping, so selecting it preserves its request ID.

Claude Code's [`modelPicker` setting](https://code.claude.com/docs/en/settings-reference#modelpicker) requires version 2.1.242 or later. Restart Claude Code after saving to load the menu. Older clients can still use their role mappings or select an explicit model with `/model <model-id>`; importing rows does not upgrade the client. A discovery result lists advertised IDs and does not verify inference access for every model.

## Routing prerequisites on a new installation

Installing VaryAI creates independent manager storage; it does not initialize native clients or transfer login/authorization from another application. Add and apply a provider before enabling routing when that client uses a saved active configuration.

Existing native client configuration stays in its original directory and remains readable after changing VaryAI's data root. It does not need to be recreated because VaryAI storage changed. An empty isolated test home also hides existing client configuration; use private copies or representative existing-client fixtures for that acceptance scenario.

| Client                     | Prerequisite                                                                                                                  |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Claude Code                | Existing native settings, created by client setup or applying a provider.                                                     |
| Codex                      | Native `config.toml` or `auth.json`; config-only API-key installations are supported.                                         |
| Gemini CLI                 | Native environment configuration from client setup or applying a provider.                                                    |
| Grok Build                 | Native configuration with a routable custom provider; official-only mode does not supply a custom provider table.             |
| Claude Desktop             | Select a provider with available models. Its routing switch takes over only Desktop; mappings and conversion require routing. |
| Kimi Code, Qwen Code       | Supported native official/API-key model configuration or an applied provider with models.                                     |
| ZCode                      | Supported model configuration; official models additionally need the native runtime product configuration.                    |
| Pi                         | At least one supported configured model source, with valid recovery state. Restart Pi after enabling routing.                 |
| WorkBuddy                  | A supported native model endpoint, or product configuration populated by starting and signing in to WorkBuddy.                |
| Qoder                      | Normal certificate/connection consent and valid trusted local certificate material; complete its setup flow.                  |
| TRAE CN                    | Explicit local connection consent; fully quit and restart TRAE after the first debug configuration, then wait for connection. |
| Doubao Work                | Explicit local connection consent and a running supported client; waiting is distinct from active routing.                    |
| OpenCode, OpenClaw, Hermes | Native provider configuration through their own adapters; they do not use the generic takeover switch.                        |

If Claude/Codex/Gemini/Grok configuration is absent, enabling routing gives setup guidance before starting the listener or creating a live backup. Do not fabricate production client files just to make the switch succeed. A running gateway still does not establish successful model access: verify a small synthetic request after setup.

## Choose the correct protocol

The format controls conversion when the supported client is routed through Vary AI:

| Client request       | Upstream selection                    | Behavior                                                                                                                    |
| -------------------- | ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Codex Responses      | Responses                             | Native upstream format; conversion is unnecessary. Routing can still be used for supported inspection and routing features. |
| Codex Responses      | Chat Completions                      | Requires local routing to convert requests and supported JSON/SSE responses.                                                |
| Codex Responses      | Anthropic Messages                    | Requires local routing to convert supported Messages payloads and responses.                                                |
| Claude Code Messages | Anthropic Messages                    | Native upstream format.                                                                                                     |
| Claude Code Messages | Chat Completions, Responses or Gemini | Requires the corresponding local conversion path. Endpoint capabilities and conversion limits still apply.                  |

Custom HTTP provider forms use **Upstream protocol** for the supplier-facing format: Anthropic Messages, OpenAI Chat Completions, OpenAI Responses or Gemini Native generateContent. OpenCode, OpenClaw and Pi use their native SDK/API identifiers for these formats. Model discovery uses the selected upstream format where supported; selecting Gemini does not establish that a particular supplier implements Gemini's API.

The detection model is used only to probe protocol support. Selecting it does not change saved model IDs, mappings or display names. Detection and model configuration share the discovered catalog within the same editor: model-fetch and fill actions reuse that list for the current connection. Changing the URL or credentials invalidates it, and stale responses cannot populate the new connection. Model configuration changes only through its own controls or an explicit fill action.

Qoder, Kimi, Qwen, ZCode and TRAE retain a supported native client protocol when Gemini is selected and convert through local routing. WorkBuddy keeps its native Chat Completions requests and can convert to any of the other three upstream formats. Hermes exposes **Client protocol** separately; its HTTP modes can use any of the four upstream formats, while Bedrock Converse retains its native configuration. Hermes and TRAE receive a provider-specific loopback endpoint when conversion is required; their saved provider keeps the original supplier URL and key. Keep the local gateway running while using these projected configurations. Official account configurations retain their existing authentication and routing paths.

With TRAE routing enabled, managed native Responses models also use this loopback endpoint for multi-turn request compatibility, even when security auditing is disabled. The route supplies a missing reasoning `summary` array while preserving the original history and native Responses protocol. Routing changes update existing native model IDs in place; disabling routing restores the saved upstream configuration. Client delivery remains pending while TRAE is disconnected. See [TRAE multi-turn HTTP 502 troubleshooting](../troubleshooting.md#trae-cn-reports-http-502-with-security-auditing-off).

Do not set Codex's wire protocol to Chat Completions to work around an upstream mismatch: its client-facing path remains Responses, with the upstream format selected in Vary AI. Authentication and model catalog behavior also differ between official account integrations and API-key providers.

Conversion cannot guarantee that every vendor extension, tool schema, reasoning parameter or streaming event is interchangeable. Confirm the intended feature with a small synthetic request before using production data.

## Enable routing

Open **Settings → Routing** and expand **Local routing**. Routing has its own settings tab, separate from Advanced; the home routing shortcut opens the same tab. The routing master switch, gateway keys, per-client takeover, listener configuration and failover controls remain together here, with their existing save behavior.

For supported local-proxy clients, start the routing master switch and enable routing for the selected client. Codex has a single routing switch. After one confirmation, supported provider models and routing groups appear together in its model menu; there is no mode selector or group-publication checklist. Select a provider-labeled model to use that exact provider, or a group-labeled model to use the group plan. Vary AI keeps one configuration owner and synchronizes group changes automatically while the switch is on. Turning the Codex switch off restores the original configuration. The default listener is `127.0.0.1:15722`. A running listener and a configured provider alone do not establish that the client is using it; check the per-client routing state and the client's active configuration.

Taking over a client changes its live endpoint configuration. The client-facing URL can include `/v1` where its protocol requires it. Keep the provider's upstream endpoint separate from that loopback URL. Do not change the listener to a public interface just to resolve an endpoint-format error.

Claude Desktop uses the same per-agent takeover controls as other clients. Its provider editor configures models and upstream protocol without a separate direct/proxy mode selector. Enabling routing writes the local gateway profile; disabling it restores a compatible native provider or official login and rejects requests from cached gateway connections. Providers requiring model mapping or protocol conversion remain saved and require Desktop routing to be enabled before use. The Desktop switch does not stop the shared gateway or disconnect other agents. Restart Desktop to reload its native profile after changing routing. Normal application exit restores native configuration while retaining the routing preference for the next startup.

The current local proxy covers Claude Code, Codex, Gemini CLI and Grok Build. Other client integrations have separate capabilities. [WorkBuddy](workbuddy.md) and [Qoder](qoder.md) have their own takeover and recovery paths; a provider appearing in the app does not imply identical proxy coverage.

## Verify and recover

TRAE CN displays managed models as **Model name · Provider name**. An explicit model display name takes precedence; otherwise the last segment of the supplier model ID is used. Labels fit TRAE's 64-character limit, with a short number distinguishing duplicate labels. Supplier request model IDs remain unchanged. Older UUID and hash labels are replaced when the provider is applied or saved again. Ownership is stored separately in TRAE's local browser storage as native model IDs and request model IDs, without credentials; switching and removal do not depend on readable labels. Clearing that storage removes those ownership records, so remove managed models before clearing it. The current helper is refreshed automatically when an older bridge is connected. The initial debug setting still requires one complete TRAE CN restart. A native model configuration failure is reported separately from a missing local connection.

- Confirm the client, active provider, routing state, selected format and exact model ID.
- Make a short synthetic request and review its status. A successful model-list fetch is not a completed inference request.
- For 404, inspect the base URL and API format. For unavailable-model or 503 errors, check the actual catalog, account access and upstream routing. For 413, check request size and the upstream limit.
- Disable takeover through the corresponding application controls before manually editing client configuration. If recovery is needed after an abnormal exit, inspect saved backups and preserve unrelated settings.

Refer to [troubleshooting](../troubleshooting.md) for diagnostics. Share only redacted configuration excerpts and logs.

## Loopback-only listener

The local gateway accepts literal loopback IP addresses only, such as `127.0.0.1` or `::1`. The interface also accepts `localhost` and saves it as `127.0.0.1`. Wildcard and LAN addresses are rejected before saving or starting the listener. This application is not a shared LAN gateway.

If an older installation saved a non-loopback address, change it to a loopback address before restarting local routing. When a VPN intercepts loopback requests, see [local proxy bypass examples](proxy-configs/README.md).

## Reasoning effort in Codex routing groups

Published routing-group aliases no longer inherit the generic `none`/`high`
model template. Vary AI reads each exact member model's reasoning capabilities
from its provider catalog. Official ChatGPT/OAuth discovery retains the normalized
capabilities in the existing account-scoped, 24-hour cache, not just model IDs.
Older ID-only cache entries remain readable but need a model-list refresh.

The automatic group menu intersects the levels of members with
known selectable levels. Unknown and explicitly non-reasoning members do not
remove other members' levels. If no member supplies levels, or the intersection
is empty, the group advertises no selectable levels rather than inventing them
from a model name. The default follows Codex's existing `model_reasoning_effort`
when supported (otherwise the nearest level); without a matching preference it
prefers `medium`, then `high`, then the first available level.

Each forwarding attempt adapts the original requested effort independently to
that target's known levels. Equal-distance choices prefer the higher level;
`none` is never chosen as an implicit fallback. `ultra` is sent as `max` (or the
nearest supported level), and unknown-level `xhigh` is capped at `high`, following
the reference gateway fitting behavior. Explicitly non-reasoning targets have the
effort field removed. Absent/null effort is left alone. Unrelated reasoning
options, opaque input and vendor number representations are preserved. This is
capability-based adaptation, not evidence that every advertised model can be
called successfully with the current account.

After upgrading, reload the routing model catalogs and enable the Codex routing
switch, then reopen Codex if it retains the previous catalog. Catalog reloads and
group changes synchronize automatically while routing is enabled. The gateway must remain running. This implementation covers automatic
capability-derived levels and per-target fitting; reference manual group-level
overrides and per-member fixed-effort editor are not included.

Menu synchronization actively refreshes official account capabilities and uses
that same provider snapshot for the published catalog and execution targets.
This upgrades ID-only caches without requiring a separate reload action. A
failed discovery still uses a valid cached catalog; it cannot establish missing
reasoning levels. The gateway does not invent levels for ID-only catalogs.

Routing-group catalogs allow image input when at least one member is not
explicitly text-only, consistent with Vary AI's unknown-capability passthrough
policy. Official discovery caches retain input modalities. Image requests skip
explicitly text-only candidates within the approved plan; pinned conversations
are never moved to another account to accommodate an image. Images are forwarded
unchanged, not stripped. A group can advertise images while a particular pinned
conversation or rule has no eligible image candidate; that request fails with a
sanitized capability error instead of silently changing accounts.
