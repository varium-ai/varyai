# Data and network behavior

Vary AI routes configured AI traffic through a local gateway. This document distinguishes that traffic from installation statistics and application update checks. It describes the source implementation; it does not establish a provider's or service operator's retention policy.

## Defaults and controls

| Behavior                  | Default                                                                                                        | Control and implementation                                                                                                                                                                                                       |
| ------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Local gateway             | Listens on `127.0.0.1:15722`; non-loopback bind addresses are rejected.                                        | Configure a loopback address and port in Settings. See [local routing](guides/provider-routing.md), [`proxy/server.rs`](../src-tauri/src/proxy/server.rs) and [`proxy/types.rs`](../src-tauri/src/proxy/types.rs).               |
| AI provider requests      | Requests use the endpoints and credentials selected by the user.                                               | Provider and client routing settings determine the destination. Inspect those settings before sending sensitive content.                                                                                                         |
| Installation statistics   | On. A fresh installation persists a random ID before using Google Analytics directly.                          | Settings → General → Privacy lets users disable reporting. Builds can override or disable the public measurement ID; disabled choices survive restarts. See [`telemetry.rs`](../src-tauri/src/telemetry.rs).                     |
| Remote analytics script   | A separate hidden, incognito WebView loads gtag.js. The main application view does not load this script.       | Analytics has its own CSP and native command guard, without plugin permissions. See [`telemetry_window.rs`](../src-tauri/src/telemetry_window.rs) and [`analytics.ts`](../src/lib/analytics.ts).                                 |
| Application update checks | The application checks the configured updater feed one second after startup and every six hours while running. | Manual checks and user-triggered installation are available. Dismissing an update notice does not disable future checks. See [`UpdateContext.tsx`](../src/contexts/UpdateContext.tsx) and [`updater.ts`](../src/lib/updater.ts). |

Settings → About shows Vary AI's own version, update check and official website. It no longer lists client application versions, diagnoses installation conflicts, or offers client installation, upgrade or manual installation commands. Opening or revisiting About does not launch client version probes or request their latest versions. This applies to macOS and Windows alike; it does not uninstall clients or remove the separate Tools-page lifecycle controls.

Local audit storage and redaction do not prevent a configured provider from receiving the content that remains in a forwarded request. Review detection rules, handling methods and client coverage for your workflow. The gateway is a local application service, not an authenticated network service for other machines.

## Storage and optional synchronization

Application data uses an independent `varyai.db` / `varyai-ext.db` pair under `~/.varyai` or the selected data directory. Device settings, account files, gateway secret copies, certificates and local runtime files stay under `~/.varyai`. Client-native configuration and an explicitly selected `~/.agents/skills` directory can still be shared with other tools.

New installations create independent empty storage. VaryAI manages configuration independently; old VaryAI installation upgrades are outside this flow. Portable exports and cloud snapshots can contain provider credentials and private configuration; device account files are outside their payloads. WebDAV/S3 use a reserved `varyai` path segment and the `varyai-sync` manifest. Configure cloud accounts and initial sync direction independently. See [storage ownership and recovery](architecture/storage.md) for exact boundaries.

## Installation statistics

Desktop statistics go directly to Google Analytics using gtag.js and public Measurement ID `G-2R2MZ76G1E`. They do not use the Vary AI statistics API or a server-side Measurement Protocol secret. `VARYAI_GA4_MEASUREMENT_ID` is a compile-time override; an empty or invalid ID disables the integration. Settings → General → Privacy provides an enabled-by-default switch.

A fresh installation persists a random UUID v4 in `installation-analytics.json` in the application-local data directory, outside configuration exports and synchronization. The Google tag receives a stable `client_id` derived from two nonzero 32-bit portions of that UUID in GA4's two-number format. The native bridge gives the isolated analytics view only that ID, public measurement ID, application version, supported OS/architecture, UTC+8 statistics date, allowed event names and an ephemeral acknowledgement token. The acknowledgement token is not included in Google events.

| Custom event               | Trigger                                                                                                                                                                         |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `desktop_install_observed` | First dispatch for this local ID and measurement stream. This includes existing users upgrading from the previous backend integration. It does not prove a new OS installation. |
| `desktop_daily_active`     | First dispatch for the current local UTC+8 statistics date; a continuously running application checks again after the next day boundary.                                        |
| `desktop_version_changed`  | A version change after a dispatch on the same statistics date. This does not add a second daily-activity event.                                                                 |

Custom event parameters are restricted to `app_version`, `desktop_os`, `desktop_arch` and `statistics_date`. `send_to` selects the public measurement stream. The tag uses a fixed virtual page location, `https://www.varyai.net/desktop`, an empty referrer and a fixed page title. It does not see the main application DOM, private page URLs, prompts, conversations, provider credentials, tool content or audit records. That virtual URL is a reporting label, not a request to the Vary AI statistics backend.

The Google script can collect additional standard analytics and connection metadata, including source IP, WebView user agent/device information and temporary cookies. It runs in an incognito WebView with no native plugin capabilities; an explicit command guard denies every application command except token-scoped dispatch acknowledgement. Ads storage, ad user data, ad personalization and Google signals are disabled. Automatic page-view reporting is disabled. Other Google automatic event/metadata behavior depends on the tag and stream configuration. This is not an anonymous or unlinkable data flow.

After startup or re-enabling, the native worker waits 30–90 seconds before checking whether a dispatch is due. Load or callback failures get up to three attempts with short backoff and a 40-second timeout. Later checks occur after the next statistics-day boundary plus jitter. A gtag callback confirms only local dispatch processing, not Google ingestion or report availability. Lost callbacks and retries can duplicate events; count distinct client IDs by event/date rather than treating raw event counts as exact installations or DAU. Google standard First visit, Active users, sessions and engagement metrics are not equivalent to these custom installation observations and daily heartbeats. Website and desktop clients do not become the same user merely by using the same measurement ID.

Turning statistics off persists the choice, clears the local identifier and dispatch markers, cancels pending native work and destroys the analytics WebView. It cannot retract network requests Google already received. Re-enabling creates a different ID, which appears as a new observation. Schema 3 preserves valid legacy IDs and explicit disabled choices while resetting old backend dispatch markers. Missing state uses the enabled default; corrupt state, an unsupported schema and unavailable local storage disable reporting. Network/script failure does not interrupt application startup or model forwarding.

Google's handling and retention of the received data depend on the configured Analytics property and the applicable Google policies. See [Google Analytics event setup](https://developers.google.com/analytics/devguides/collection/ga4/events) and [Google tag cookie configuration](https://developers.google.com/tag-platform/security/guides/customize-cookies).

## Updates and distribution

The updater contacts the feed configured in [`tauri.conf.json`](../src-tauri/tauri.conf.json). Installation is a separate user-triggered action in [`AppUpdateDialog.tsx`](../src/components/AppUpdateDialog.tsx). Installation-statistics preferences do not control update checks or user-configured provider traffic.

On macOS and Windows, automatic checks run one second after startup and every six hours while the application remains running. Checks do not overlap, and automatic checks are skipped during update installation. A failed check does not stop the schedule; the next scheduled check can try again. Dismissing a version's notice keeps that version dismissed while allowing a later version to be shown. Closing the application stops the schedule; checks resume on its next launch.

The update notice displays Markdown notes supplied by the updater feed. If an older feed omits them or contains only the former release-page placeholder, the visible notice requests the exact target version from `https://api.github.com/repos/varium-ai/varyai/releases/tags/`. This request has an eight-second timeout, omits credentials and the referrer, and does not attach account details or local configuration. Failure to load these optional notes does not block installation. Notes do not execute embedded HTML or load remote images; their links open in the external browser only when clicked.

[`tauri.no-updater.conf.json`](../src-tauri/tauri.no-updater.conf.json) disables updater artifact generation. Combined with `--no-bundle`, the README build command compiles without release bundles or updater artifacts; it does not remove the updater plugin or disable runtime update checks. Fork distributors should review [release distribution and update compatibility](guides/release-distribution.md).

## Client integrations

Opening the tray's Allowances tab queries configured relay keys with read-only
GET requests to `/v1/usage`, falling back to `/api/usage/token` on the configured
origin. These requests carry the selected API key as a bearer credential, use
eight-second per-request timeouts, do not follow redirects, and read at most
1 MiB per response. They send no inference prompts. Returned balances and quota
windows refresh once a minute while the tab is active. A relay with no recognized
query is hidden and its result cached for six hours in renderer memory, then
probed again while the tab remains open. The header's explicit refresh can retry
sooner; saving a new provider revision also starts a new query. Failed refreshes
retain a previous valid reading with a stale indicator, while authentication
rejection clears it. Leaving or closing the tab stops its automatic polling.

New API-key provider and connection-preset forms automatically discover models after URL/key editing ends, then send bounded inference probes to the configured supplier to select a verified protocol. Each check can send up to eight small requests and may incur supplier charges. Existing configurations and manual protocol selections require an explicit detection action. See [provider setup](guides/provider-routing.md#reuse-a-named-connection) for model selection and verification limits.

While an owned Codex connection and the gateway are active, the Tools view periodically checks the predicted Codex proxy route. It reads proxy environment and system settings, including exported login-shell proxy variables on macOS, and sends bounded gateway health requests directly and through the selected proxy without Codex or provider credentials. Proxy authentication, when configured, remains confined to the proxy transport. A configured proxy can observe the loopback target of that health request. The diagnostic sends no prompts, conversation history, supplier keys, or Codex login material. Only sanitized proxy/target origins and an environment/system source label are returned to the interface. It does not change shell or system proxy settings. See [Codex proxy diagnostics](troubleshooting.md#codex-cannot-reach-local-routing-through-a-proxy).

Client configuration, local bridges, model catalogs, endpoint tests and user-requested downloads can involve additional files or network requests. The table above is not a complete network allowlist. Refer to the [integration guides](README.md), especially the explicit launch and debugging-port boundaries for [Doubao Work](guides/doubao-work.md) and the captured configuration in [profiles](guides/profiles.md).

Security concerns can be reported through the channel and availability status in [SECURITY.md](../SECURITY.md).
