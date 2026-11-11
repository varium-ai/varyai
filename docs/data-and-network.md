# Data and network behavior

Vary AI routes configured AI traffic through a local gateway. This document distinguishes that traffic from optional installation statistics and application update checks. It describes the source implementation; it does not establish a provider's or service operator's retention policy.

## Defaults and controls

| Behavior                  | Default                                                                                                                      | Control and implementation                                                                                                                                                                                                       |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Local gateway             | Listens on `127.0.0.1:15722`; non-loopback bind addresses are rejected.                                                      | Configure a loopback address and port in Settings. See [local routing](guides/provider-routing.md), [`proxy/server.rs`](../src-tauri/src/proxy/server.rs) and [`proxy/types.rs`](../src-tauri/src/proxy/types.rs).               |
| AI provider requests      | Requests use the endpoints and credentials selected by the user.                                                             | Provider and client routing settings determine the destination. Inspect those settings before sending sensitive content.                                                                                                         |
| Installation statistics   | Off. A fresh installation creates no statistics identifier. Builds without a statistics destination cannot enable reporting. | Settings → General → Privacy provides an explicit switch. See [`PrivacySettings.tsx`](../src/components/settings/PrivacySettings.tsx) and [`telemetry.rs`](../src-tauri/src/telemetry.rs).                                       |
| Remote page analytics     | The renderer includes no Google Analytics loader or page-tracking bootstrap.                                                 | See [`src/index.html`](../src/index.html) and the content security policy in [`tauri.conf.json`](../src-tauri/tauri.conf.json).                                                                                                  |
| Application update checks | The application checks the configured updater feed shortly after startup.                                                    | Manual checks and user-triggered installation are available. Dismissing an update notice does not disable future checks. See [`UpdateContext.tsx`](../src/contexts/UpdateContext.tsx) and [`updater.ts`](../src/lib/updater.ts). |

Settings → About shows Vary AI's own version, update check and official website. It no longer lists client application versions, diagnoses installation conflicts, or offers client installation, upgrade or manual installation commands. Opening or revisiting About does not launch client version probes or request their latest versions. This applies to macOS and Windows alike; it does not uninstall clients or remove the separate Tools-page lifecycle controls.

Local audit storage and redaction do not prevent a configured provider from receiving the content that remains in a forwarded request. Review detection rules, handling methods and client coverage for your workflow. The gateway is a local application service, not an authenticated network service for other machines.

## Storage and optional synchronization

Application data uses an independent `varyai.db` / `varyai-ext.db` pair under `~/.varyai` or the selected data directory. Device settings, account files, gateway secret copies, certificates and local runtime files stay under `~/.varyai`. Client-native configuration and an explicitly selected `~/.agents/skills` directory can still be shared with other tools.

New installations create independent empty storage. VaryAI manages configuration independently; old VaryAI installation upgrades are outside this flow. Portable exports and cloud snapshots can contain provider credentials and private configuration; device account files are outside their payloads. WebDAV/S3 use a reserved `varyai` path segment and the `varyai-sync` manifest. Configure cloud accounts and initial sync direction independently. See [storage ownership and recovery](architecture/storage.md) for exact boundaries.

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

The update notice displays Markdown notes supplied by the updater feed. If an older feed omits them or contains only the former release-page placeholder, the visible notice requests the exact target version from `https://api.github.com/repos/varium-ai/varyai/releases/tags/`. This request has an eight-second timeout, omits credentials and the referrer, and does not attach account details or local configuration. Failure to load these optional notes does not block installation. Notes do not execute embedded HTML or load remote images; their links open in the external browser only when clicked.

[`tauri.no-updater.conf.json`](../src-tauri/tauri.no-updater.conf.json) disables updater artifact generation. Combined with `--no-bundle`, the README build command compiles without release bundles or updater artifacts; it does not remove the updater plugin or disable runtime update checks. Fork distributors should review [release distribution and update compatibility](guides/release-distribution.md).

## Client integrations

New API-key provider and connection-preset forms automatically discover models after URL/key editing ends, then send bounded inference probes to the configured supplier to select a verified protocol. Each check can send up to eight small requests and may incur supplier charges. Existing configurations and manual protocol selections require an explicit detection action. See [provider setup](guides/provider-routing.md#reuse-a-named-connection) for model selection and verification limits.

While an owned Codex connection and the gateway are active, the Tools view periodically checks the predicted Codex proxy route. It reads proxy environment and system settings, including exported login-shell proxy variables on macOS, and sends bounded gateway health requests directly and through the selected proxy without Codex or provider credentials. Proxy authentication, when configured, remains confined to the proxy transport. A configured proxy can observe the loopback target of that health request. The diagnostic sends no prompts, conversation history, supplier keys, or Codex login material. Only sanitized proxy/target origins and an environment/system source label are returned to the interface. It does not change shell or system proxy settings. See [Codex proxy diagnostics](troubleshooting.md#codex-cannot-reach-local-routing-through-a-proxy).

Client configuration, local bridges, model catalogs, endpoint tests and user-requested downloads can involve additional files or network requests. The table above is not a complete network allowlist. Refer to the [integration guides](README.md), especially the explicit launch and debugging-port boundaries for [Doubao Work](guides/doubao-work.md) and the captured configuration in [profiles](guides/profiles.md).

Security concerns can be reported through the channel and availability status in [SECURITY.md](../SECURITY.md).
