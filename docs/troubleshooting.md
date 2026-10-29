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

For a generic Gemini `INVALID_ARGUMENT`, compare the `GeminiRequestShape` entries for the affected clients. These entries record message and function counts, parameter formats, and output-token ranges without model names, prompts, tool names, schemas, or argument values. The summary helps identify request differences; it does not establish which field the upstream rejected.

### HTTP 404: endpoint or protocol mismatch

An upstream that implements `/v1/chat/completions` may not implement `/v1/responses`. Check the provider's configured API format and base URL. When the upstream requires Chat Completions, select the matching conversion/routing mode supported by that client integration. Confirm the resulting endpoint with a synthetic request. Changing a model name alone does not add an unsupported API endpoint.

Protocol detection uses a bounded 1,024-token output budget so reasoning models can finish thinking before returning the short test answer. Older 128-token probes could receive only reasoning with `max_tokens` or `incomplete`, even when authentication and the API format worked. Detection still requires the exact test answer and normal completion; a truncated response never establishes compatibility.

A successful minimal Responses probe does not establish support for every model and native coding-agent input. If ZCode receives an upstream 502, compare the same synthetic request with another model on that gateway. Current company acceptance passes with GLM while the reported Qwen model can fail on ZCode's full input despite accepting a minimal message. Preserve the original protocol parameters when diagnosing this difference; record the model ID and a sanitized status, without credentials or raw client input.

### Gemini model discovery fails with a valid API key

Select **Gemini Native** for a Google Generative Language API key. Model discovery uses the `x-goog-api-key` header, not a Bearer token or a generated `key` query parameter. For `https://generativelanguage.googleapis.com`, it requests `/v1beta/models`; base URLs ending in `/v1` or `/v1beta` are normalized while gateway path prefixes are retained. An explicit model-list URL takes precedence.

The catalog reads `models[].name`, removes the `models/` resource prefix, and follows `nextPageToken` on the same endpoint. Results are sorted and deduplicated. Repeated tokens or more than 100 pages fail rather than silently returning an incomplete catalog. Switching protocols refreshes the discovery catalog. Other protocols retain their existing model-discovery authentication behavior.

Fetching the catalog does not generate model output. If discovery still fails, check the HTTP status, API restrictions, and network connectivity without publishing the API key. A successful catalog request does not establish access or quota for every inference model.

### HTTP 503: unavailable model or channel

Check the upstream error and the models available to the account. Fetch the provider's model list when supported, review the imported catalog and default model, and select an available model in the client. If the gateway supports model mapping, map only to an actual available target. Check account quota and routing rules as well as model spelling.

### HTTP 413: request too large

The upstream reverse proxy or provider can reject a request even when it is below the local proxy's limit. Reduce oversized attachments and pasted logs, or compact the conversation using the client's supported command. Operators of an upstream gateway should check its documented body limits before changing them. A local setting does not override the provider's limit.

## macOS launch and installation problems

Local development packages may be unsigned or use an ad-hoc signature. Gatekeeper warnings and signature failures require checking the artifact's origin, hash and signing status. Follow the [macOS build guide](guides/local-macos-build.md) for deliberate local build and signing options. Unit-test success does not establish notarization or installer acceptance.

A missing Launchpad entry does not establish that an application failed to install. Check the actual application path and package before changing system registration or replacing the installed app.

## Agent installation checks

Settings > About > Local environment check lists every supported agent. The eight managed CLI cards retain version checks and installation/update actions. A missing Codex CLI shows **Not installed** and an **Install** button; an installed CLI that cannot run has a separate environment warning. Failed checks show a retry message rather than an installation prompt.

Claude Desktop, WorkBuddy, Qoder CN, Doubao Work, TRAE CN, Kimi Code, Qwen Code and ZCode also display their installed version. On macOS, desktop cards read the product version from the installed application's bundle metadata without launching it. Desktop installations take precedence over CLI installations and are labeled explicitly; an embedded editor's version is not the desktop product version. Kimi can instead run a bounded `--version` check when only its CLI is installed. Qwen Code uses the same version checks, installation diagnostics and upgrade pipeline as the other managed CLI tools. A WorkBuddy launcher alone establishes presence but is not executed as a version-only CLI. Missing metadata or failed version commands leave the installation marked present and show a version-read failure. Desktop application detection currently supports macOS; unsupported checks on other platforms are labeled explicitly.

The About page uses one shared card layout for CLI and desktop agents: current version, latest version and an Upgrade button when a newer version is available. On macOS, Claude Desktop, WorkBuddy, Qoder CN, Doubao Work, TRAE CN, Kimi Desktop and ZCode query verified publisher release sources. Missing or older remote metadata is not evidence that an installation is current. Refresh reruns detection.

Desktop upgrades run inside Vary AI and are included in Upgrade All. They download the matching official package to a temporary directory, verify any published checksum, product identity, product version, CPU architecture, code signature and the installed publisher's signing team. The updater stages the new application beside the existing installation before requesting a normal quit. It replaces only the application bundle at its existing location and restores the previous bundle if replacement fails; configuration and user data are not removed. A previously running application is reopened. Unsaved-work dialogs can prevent quitting; save and fully quit the agent, then retry. Folder permission failures do not trigger elevation or installation to a different location. If both replacement and recovery fail, the previous bundle remains in a `.varyai-agent-update-` folder beside the installation for recovery. Desktop automatic upgrades currently support macOS. Installation or upgrade failures show fixed localized reasons without exposing raw command output.

TRAE's displayed product version is distinct from its editor/build version. Doubao Work packages are distinct from the standard Doubao desktop app; Kimi Code Desktop packages are distinct from the Kimi chat app. The updater preserves these identities and never substitutes a similarly named product.

## Client takeover and recovery

TRAE CN does not need to be open to select or edit a provider. Vary AI saves the selection locally and shows that client delivery is pending; keep Vary AI running and connect TRAE to apply it automatically. Switching to Official while offline also queues withdrawal of previously managed models. A successful local selection does not mean the closed client has already received it. Connection setup and auditing retain their existing consent requirements.

In Settings → Advanced → Local routing, a selected TRAE CN or Doubao Work switch can be waiting for its client connection. The waiting hint means enablement was saved, but traffic is not yet protected. Open the client and follow the displayed connection instructions; deselecting the switch cancels the pending request. Waiting enablement does not show an active-routing success notification.

Gateway key budget exhaustion is a permanent policy rejection until the budget resets or its policy is changed. Vary AI returns HTTP 402 with `gateway_budget_exhausted`, `insufficient_quota` and `x-should-retry: false`, so clients do not spend minutes retrying it as temporary rate limiting. Check the key's usage and reset time in Local routing → Gateway keys. Ordinary upstream rate-limit responses retain their original handling.

For Gemini, save the selected provider again after changing its model, then restart the CLI. Active proxy takeover now refreshes both the model environment and native model selection while retaining the local routing address. Custom models are registered in the dynamic `/model` registry supported by current Gemini CLI versions. A CLI `--model` argument or an inherited `GEMINI_MODEL` can still override the saved default.

In the Gemini provider editor, **Default model** controls the startup selection. The separate **Selectable models** section contains menu entries, with distinct display-name and request-model column headers. **Fetch model list** only loads candidates. **One-click setup** imports all fetched models into the saved model list, skipping duplicate and empty IDs. Existing model metadata and the configured default remain intact; an empty default uses the first imported model. Save the provider and restart Gemini CLI, then open `/model`, select **Manual** and press Enter to see the imported models. The list uses Gemini's native `modelConfigs.modelDefinitions` with dynamic model configuration enabled. Changing the endpoint, API key or protocol clears fetched candidates so a later import cannot reuse another connection's model list.

Qwen distinguishes models by their effective API protocol, model ID and endpoint, as described in its [model-provider documentation](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/model-providers/). Chat Completions and Responses can use the same model ID, and another endpoint can expose the same ID. Switching between saved Vary AI suppliers with the same protocol, ID and endpoint replaces the conflicting supplier's entire managed model group and credential environment entry before inserting the target. Both saved suppliers remain available for subsequent switches. Unrelated model groups, manual entries and MCP settings remain intact. Conflicting manual models or externally changed credentials and endpoints still require resolution. Vary AI retains the selected endpoint through takeover and recovery, and keeps model routes stable when the native list is reordered.

An OpenAI-only upstream key can work with an Anthropic-native client when the selected upstream format enables protocol conversion. That success alone does not mean the key supports Anthropic. To diagnose a mismatch, record the selected upstream format and a sanitized error; never include the key or full client configuration.

If a Claude Code upgrade reports that another instance is updating, Vary AI retries a single-tool update twice after short delays. A persistent lock still reports the native error: let the other updater finish before trying again. Vary AI also prevents concurrent installation/update commands within its backend. Other command failures and multi-tool batches are not automatically replayed.

Common configuration locations include:

| Client      | Configuration                                |
| ----------- | -------------------------------------------- |
| Codex       | `~/.codex/config.toml`, `~/.codex/auth.json` |
| Claude Code | `~/.claude/settings.json`                    |

Local proxy configuration can refer to `http://127.0.0.1:15721` or its `/v1` endpoint. If a client still points at that endpoint after an abnormal exit, review the application's takeover state and recovery options. A normal disable/exit path can restore the saved configuration; inspect the relevant backup before manual recovery. Back up the current files before editing them and preserve unrelated client settings. Do not publish authentication files or entire backups in an issue.

### Codex Official login asks to sign in again after switching

Codex can rotate its refresh token while an Official provider card still has an older login snapshot. The fixed OpenAI Official card follows the current native login when no managed account is bound: switching only updates `config.toml` and leaves `auth.json` untouched, including imported token bundles without timestamps and a native logout. An explicitly bound managed account still supplies that account's credentials. Separately saved official account cards retain their account-switching behavior, with newer native tokens adopted only for the same user and workspace. A token already invalidated by the server requires valid login material again; this change cannot restore it.

### A configuration scheme references a removed supplier

Scheme switching changes only Agents captured in the scheme whose saved supplier references still exist. An Agent added after the scheme was saved keeps its current supplier, model and files. If a saved primary supplier or enabled supplier has been removed, the entire Agent entry is treated as uncaptured: the preview names the Agent, keeps its current configuration and lets the remaining entries switch. No replacement supplier is guessed and the saved scheme is not rewritten. Update its snapshot explicitly to adopt current selections.

Rollback and undo cover only Agents actually applied. If no entries remain applicable, the scheme does not report success or replace the current scheme or undo record. Other preparation failures name the Agent and a fixed, localized reason without exposing configuration or credentials.

### Shell command list cannot be saved

The editor distinguishes an existing match type and pattern (including disabled entries), a stale or deleted entry, and the 256-entry limit. Edit the existing item for a duplicate, reopen the latest item after a concurrent edit, or remove an unused item at the limit. Failed saves keep the draft. Other storage failures retain a generic message so database internals and command contents are not exposed. A successful regex preview validates the pattern; it does not establish that a later database write will succeed.

A successful save closes the editor and shows a success toast. If the entry was persisted but runtime application or audit recording failed, the existing warning appears instead of a success toast.

### No tool risk records after a Claude Desktop request

Enable tool risk collection and confirm the request passes through Vary AI. A text answer containing a command does not create a Shell event: the model must return a supported tool call. For Claude Code, ask it to call Bash with `pwd`, then refresh the tool risk records. A low-risk command can still produce a record.

Find low-risk calls under Security Center → Security Events → All Tool Calls. They do not increase the risk-warning total. For a Shell list pattern such as `^kubectl\s+delete\s+`, select Regex rather than Command Prefix; regex syntax in a prefix entry is treated as literal command words. Validate the regex, confirm its scope and save. The rule sandbox can verify a match without executing the command or creating a historical record.

The time-range summary counts sensitive-data records plus all saved tool-call records, including low-risk calls. Its tool-call total is independent of page filters and pagination. The warning card continues to count warnings only. A failed tool-count query displays an unavailable count and supports retry instead of substituting zero or an old warning count.

Claude Desktop uses a separate client identity. Builds missing its tool-schema registration discard otherwise supported calls. The registration fix recognizes its declared built-in Bash, Read, Write, Edit and WebFetch schemas while retaining a distinct Claude Desktop source. MCP tools and unknown schemas remain unsupported by these built-in adapters. Model-response records indicate a proposed operation, not proof of execution. Load the updated backend before testing the fix; the command-list editor can validate and save a pattern even when historical preview has no records.

Claude Code and Desktop providers using OpenAI Chat, Responses or Gemini conversion require observation before response conversion. This path binds native upstream calls to matching validated definitions from both the client request and the actual outbound request. Converted output is not promoted to trusted native evidence. A successful request or visible tool execution alone does not establish that an older backend recorded it.

### A Claude provider incorrectly shows no configured credential

Claude providers can authenticate with `env.ANTHROPIC_AUTH_TOKEN` as well as `env.ANTHROPIC_API_KEY`. The home card checks both fields and displays only the credential status. Older cards omitted the auth-token field and could report missing credentials even while proxy requests authenticated successfully.

### Qoder or WorkBuddy reports missing provider fields

Custom providers require a provider name, model ID, URL, and API key. Saving lists all empty or whitespace-only fields together and omits fields already filled in. Additional model rows also require an ID. Official login entries do not require custom model credentials unless a custom model is being added. A toast showing `providerForm.nameRequired` indicates a build with a missing translation reference; use a build containing the fix.

### Qoder asks to install the certificate after every restart

The certificate lasts 30 days and is retained across normal restarts. An older trust check supplied only the localhost server certificate, which could fail to locate its CA even when the CA was already trusted. The check now supplies the server and CA certificates together and still uses macOS SSL trust evaluation; it does not force the CA to be trusted. A valid, trusted saved certificate is reused without another installation prompt.

### Kimi shows an internal ID as the provider group

Apply the saved provider again to migrate its Kimi group to the provider name shown in Vary AI. Renaming it also updates model references while preserving model aliases and the selected model. Choose a unique provider name: Vary AI refuses to overwrite an existing native group with the same name. Restart Kimi when it needs to reload its configuration.

### TRAE CN switching reports a model configuration failure

TRAE can keep an older model-list cache after a custom model is added. Older Vary AI bridges may then report a failed switch even though the model was saved; retrying can return a native duplicate-model error. The bridge now refreshes TRAE's native model store before matching existing models and after each addition. It recovers models carrying the provider's legacy ownership label and retains readable labels without duplicating or adopting unrelated user models. Fully quit and reopen TRAE CN after loading a build containing this fix so its in-memory bridge uses the updated script, then apply the provider again. Direct custom-provider switches also withdraw the previous managed models before adding the target, because TRAE rejects overlapping request model IDs under different labels. A failed switch attempts to restore the previous models and retains the previous provider selection; an incomplete restoration is reported explicitly.

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
