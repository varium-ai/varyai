# Data and network behavior

Vary AI routes configured AI traffic through a local gateway. This document distinguishes that traffic from optional installation statistics and application update checks. It describes the source implementation; it does not establish a provider's or service operator's retention policy.

## Defaults and controls

| Behavior                  | Default                                                                                                                      | Control and implementation                                                                                                                                                                                                       |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Local gateway             | Listens on `127.0.0.1:15721`; non-loopback bind addresses are rejected.                                                      | Configure a loopback address and port in Settings. See [local routing](guides/provider-routing.md), [`proxy/server.rs`](../src-tauri/src/proxy/server.rs) and [`proxy/types.rs`](../src-tauri/src/proxy/types.rs).               |
| AI provider requests      | Requests use the endpoints and credentials selected by the user.                                                             | Provider and client routing settings determine the destination. Inspect those settings before sending sensitive content.                                                                                                         |
| Installation statistics   | Off. A fresh installation creates no statistics identifier. Builds without a statistics destination cannot enable reporting. | Settings → General → Privacy provides an explicit switch. See [`PrivacySettings.tsx`](../src/components/settings/PrivacySettings.tsx) and [`telemetry.rs`](../src-tauri/src/telemetry.rs).                                       |
| Remote page analytics     | The renderer includes no Google Analytics loader or page-tracking bootstrap.                                                 | See [`src/index.html`](../src/index.html) and the content security policy in [`tauri.conf.json`](../src-tauri/tauri.conf.json).                                                                                                  |
| Application update checks | The application checks the configured updater feed shortly after startup.                                                    | Manual checks and user-triggered installation are available. Dismissing an update notice does not disable future checks. See [`UpdateContext.tsx`](../src/contexts/UpdateContext.tsx) and [`updater.ts`](../src/lib/updater.ts). |

Local audit storage and redaction do not prevent a configured provider from receiving the content that remains in a forwarded request. Review detection rules, handling methods and client coverage for your workflow. The gateway is a local application service, not an authenticated network service for other machines.

## Optional installation statistics

Reporting requires both an HTTPS destination configured when building the application and an explicit local opt-in. The source default for `VARYAI_ANALYTICS_BASE_URL` is empty. Runtime consent is stored locally, separately from synchronized application settings, in `installation-analytics.json` in the application-local data directory.

Enabling statistics creates and persists a random UUID v4. Reports contain only these fields:

| JSON field        | Value                                                              |
| ----------------- | ------------------------------------------------------------------ |
| `installation_id` | The installation identifier generated when statistics are enabled. |
| `app_version`     | The compiled application version.                                  |
| `os`              | The supported operating-system label.                              |
| `arch`            | The supported architecture label.                                  |

The request constructor does not include prompts, conversations, provider credentials, tool arguments or audit content. The persistent identifier permits reports from the same opt-in period to be linked; these reports should not be described as unlinkable or anonymous. Network services can also observe connection metadata such as the source IP address.

After opt-in, the worker waits 30–90 seconds before checking whether a report is due. A report is due when the last successful date differs from the current UTC+8 statistics day or the application version changes. Retryable failures receive up to three attempts per round. Subsequent checks occur after the next statistics-day boundary plus jitter.

Turning statistics off clears the local identifier and successful-report markers, interrupts pending waits and retries, and cancels an in-progress request future. It cannot retract a request already received by the service. Enabling statistics again creates a different identifier. Preferences must be saved successfully before reporting is enabled. A missing, corrupt or unsupported state fails closed; legacy files created by automatic-reporting versions are not treated as consent.

Backend retention, access and deletion rules depend on the operator of the configured destination. A distributor enabling this integration must publish the applicable policy and contact information alongside its build.

## Updates and distribution

The updater contacts the feed configured in [`tauri.conf.json`](../src-tauri/tauri.conf.json). Installation is a separate user-triggered action in [`AppUpdateDialog.tsx`](../src/components/AppUpdateDialog.tsx). Statistics consent does not control update checks or user-configured provider traffic.

[`tauri.no-updater.conf.json`](../src-tauri/tauri.no-updater.conf.json) disables updater artifact generation. Combined with `--no-bundle`, the README build command compiles without release bundles or updater artifacts; it does not remove the updater plugin or disable runtime update checks. Fork distributors should review [release distribution and update compatibility](guides/release-distribution.md).

## Client integrations

Client configuration, local bridges, model catalogs, endpoint tests and user-requested downloads can involve additional files or network requests. The table above is not a complete network allowlist. Refer to the [integration guides](README.md), especially the explicit launch and debugging-port boundaries for [Doubao Work](guides/doubao-work.md) and the captured configuration in [profiles](guides/profiles.md).

Security concerns can be reported through the channel and availability status in [SECURITY.md](../SECURITY.md).
