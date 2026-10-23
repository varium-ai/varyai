# Vary AI troubleshooting

Use synthetic examples when reporting a problem. Logs, provider configuration, client authentication files and database backups may contain credentials or private prompts. Review and redact them before sharing.

## Local proxy port conflicts

The local proxy normally listens on `127.0.0.1:15721`. An existing application instance can cause `address already in use` errors. A second launch can also foreground the existing instance and exit because the application uses a single-instance plugin.

On macOS or Linux, identify the listener:

```bash
lsof -nP -iTCP:15721 -sTCP:LISTEN
```

Quit Vary AI normally from its tray menu before restarting. Confirm which process owns the port before stopping anything. For automated tests, use temporary configuration directories and dynamic ports instead of interfering with a running application.

## Proxy request errors

Inspect the relevant status code and sanitized logs. The historical log directory may still be under `~/.cc-switch/logs/`; use the application's diagnostics to locate the active file rather than assuming a new branding-specific directory.

### HTTP 404: endpoint or protocol mismatch

An upstream that implements `/v1/chat/completions` may not implement `/v1/responses`. Check the provider's configured API format and base URL. When the upstream requires Chat Completions, select the matching conversion/routing mode supported by that client integration. Confirm the resulting endpoint with a synthetic request. Changing a model name alone does not add an unsupported API endpoint.

### HTTP 503: unavailable model or channel

Check the upstream error and the models available to the account. Fetch the provider's model list when supported, review the imported catalog and default model, and select an available model in the client. If the gateway supports model mapping, map only to an actual available target. Check account quota and routing rules as well as model spelling.

### HTTP 413: request too large

The upstream reverse proxy or provider can reject a request even when it is below the local proxy's limit. Reduce oversized attachments and pasted logs, or compact the conversation using the client's supported command. Operators of an upstream gateway should check its documented body limits before changing them. A local setting does not override the provider's limit.

## macOS launch and installation problems

Local development packages may be unsigned or use an ad-hoc signature. Gatekeeper warnings and signature failures require checking the artifact's origin, hash and signing status. Follow the [macOS build guide](guides/local-macos-build.md) for deliberate local build and signing options. Unit-test success does not establish notarization or installer acceptance.

A missing Launchpad entry does not establish that an application failed to install. Check the actual application path and package before changing system registration or replacing the installed app.

## Client takeover and recovery

Common configuration locations include:

| Client      | Configuration                                |
| ----------- | -------------------------------------------- |
| Codex       | `~/.codex/config.toml`, `~/.codex/auth.json` |
| Claude Code | `~/.claude/settings.json`                    |

Local proxy configuration can refer to `http://127.0.0.1:15721` or its `/v1` endpoint. If a client still points at that endpoint after an abnormal exit, review the application's takeover state and recovery options. A normal disable/exit path can restore the saved configuration; inspect the relevant backup before manual recovery. Back up the current files before editing them and preserve unrelated client settings. Do not publish authentication files or entire backups in an issue.

### Codex Official login asks to sign in again after switching

Codex can rotate its refresh token while an Official provider card still has an older login snapshot. When switching back, Vary AI now retains a newer native login for the same user and workspace instead of restoring the older token. A different user's or workspace's login is not substituted. A token already invalidated by the server still requires signing in again through Codex; this change cannot restore it.

### Kimi shows an internal ID as the provider group

Apply the saved provider again to migrate its Kimi group to the provider name shown in Vary AI. Renaming it also updates model references while preserving model aliases and the selected model. Choose a unique provider name: Vary AI refuses to overwrite an existing native group with the same name. Restart Kimi when it needs to reload its configuration.

## Windows VC++ runtime errors

If launch fails with a missing `VCRUNTIME140_1.dll` or `VCRUNTIME140.dll`, an older artifact may still depend on the external Visual C++ runtime. Use the platform vendor's official runtime installer for the artifact's architecture when necessary; do not download individual DLL files from third-party sites.

For a new build, use `pnpm build:windows` and follow the [Windows build guide](guides/local-windows-build.md#cross-build-windows-x64-from-macos). The build helper requests `STATIC_VCRUNTIME=true` and rejects a generated EXE whose import table still contains the checked external VC++ runtime dependencies. Verify the exact public source SHA and artifact SHA-256: a version label alone cannot distinguish rebuilt packages.

Check a downloaded artifact in PowerShell, substituting its actual filename:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath "$env:USERPROFILE\Downloads\Vary AI-Windows-setup.exe"
```

## macOS notifications

If events and in-app messages appear but system banners do not, check the notification permission and presentation options in Vary AI settings and macOS System Settings. Test foreground and background delivery separately. System acceptance of a notification does not prove that a banner appeared.

The macOS implementation uses `UNUserNotificationCenter` for permission and delivery; it does not fall back to legacy notification APIs when the application identity is unavailable. Source changes require a rebuilt development instance or a newly packaged application. Development builds use a separate temporary app bundle and do not replace an installed application. Exit an existing instance normally before starting a separate one.

Screen sharing or mirroring can suppress notification banners and sound. Check the macOS setting that permits notifications during sharing. In-app warnings are independent of system delivery; a recorded warning does not prove that the user saw a banner.
