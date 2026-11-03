# WorkBuddy provider configuration

VaryAI supports selectable official and API-key WorkBuddy configurations. Official mode uses the installed client's login state; API-key mode uses the configured endpoint, credential and model. Creating a duplicate provider does not switch the active configuration or write client files.

Enable takeover deliberately, back up the client settings and restart WorkBuddy when it needs to reread its endpoint. Disabling takeover or exiting normally restores the owned settings. An abnormal termination cannot perform normal exit cleanup; recovery uses the persisted backup at the next start.

When no official endpoint is set in `settings.json`, routing reads the newest product cache in the WorkBuddy configuration directory. Both `cache/acc-product-config-v3.json` and `cache/acc-product-config-v3-*.json` are supported. Start WorkBuddy first if no cache exists. An unreadable or invalid newest cache stops setup without changing client settings.

Switching suppliers replaces all identifiable VaryAI-managed models with the selected supplier's catalog. Switching to the official entry removes those managed custom models while preserving native login, settings and unrelated user models. A private `models.json.varyai-owned` file beside the native catalog records model IDs and SHA-256 ownership fingerprints, without plaintext credentials. It survives deletion of the shared database and participates in profile undo. Configuration changes restore the previous ownership file if the database commit fails. URL changes during routing do not change model ownership.

Historical models without ownership records cannot be identified after their database records are deleted. Import or resolve those native conflicts rather than deleting unrelated models automatically.

If adding a provider reports an existing model ID, use the saved provider or import the native configuration before editing it. WorkBuddy's native model catalog requires unique model IDs; adding the same IDs again does not select the existing provider. Click **Use this** on its card to make it current.

WorkBuddy displays each custom model as its name followed by its model ID. VaryAI uses the provider name for that display name without appending the ID again. To update labels written by an older version, edit the saved provider and save it again. Restart WorkBuddy if it still displays the old catalog. This updates display names while preserving model IDs.

After enabling protection, restart WorkBuddy and open a new conversation before testing. The global protection confirmation includes this restart instruction. The saved takeover status means routing is configured; it does not prove that an already running client has reloaded its endpoint. If Security Center shows no WorkBuddy events, verify that new requests reach the local proxy before diagnosing individual detection rules. Optional catalog rules must also have Alert or Hide enabled; they default to Allow.

The active status requires a running proxy, a recovery backup, and the injected endpoint still present in WorkBuddy settings. A saved enabled preference alone is insufficient: after normal exit restores direct routing, enabling protection must apply the endpoint again. External changes to the endpoint also invalidate the active status.

Official forwarding retains the relevant login state. API-key forwarding uses the selected provider and strips official-only authentication data. Both follow the enabled inspection settings; a successful provider response is not a guarantee that every tool, payload or future vendor version is covered.

Use a disposable profile and explicitly synthetic message content when testing. Keep actual API keys and private settings out of prompts, logs and screenshots. Coverage and recovery behavior are implemented in [the takeover service](../../src-tauri/src/services/workbuddy_proxy.rs), [provider handling](../../src-tauri/src/services/provider/workbuddy.rs), and [proxy adapter](../../src-tauri/src/proxy/workbuddy.rs).

New suppliers are saved as inactive alternatives. Creating a supplier does not apply it to WorkBuddy; select it explicitly to replace the managed catalog.

## Daily check-in

The WorkBuddy page offers **Check in now** for the account currently signed in to the WorkBuddy China desktop client. It reads the client's standard `CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info` session without changing it. It does not use supplier API keys, refresh or replace login tokens, or support WorkBuddy AI's international account. Open the official client and sign in again if its session is expired; encrypted credential fields cannot be read.

Check-in queries the official campaign status before requesting a reward. The card distinguishes a claimed reward, an existing check-in, an inactive campaign, an ineligible account, an expired login, and an unconfirmed failure. Credit amounts and streaks appear only when the service supplies them. Some accounts require WorkBuddy's device verification; complete their check-in in the official client. VaryAI does not generate or bypass device tokens.

**Automatic daily check-in** defaults to off. When enabled, VaryAI checks the current native account while the application is running, independently of the selected API supplier and of local message routing. Closing VaryAI stops automatic check-in; it does not create an operating-system background task. The worker checks whether work is due every minute and retries unconfirmed failures after 30 minutes. Before noon Beijing time, an already-checked, inactive, or ineligible answer is checked again after 30 minutes because the vendor's campaign day may not have turned yet. A confirmed claim is reused for the rest of the Beijing day, including across restarts and concurrent manual actions. Manual retry can recheck other outcomes immediately.

The switch and daily records live in `workbuddy-checkin.json` in Tauri's local application data directory, outside exported and synchronized configuration. Records use a SHA-256 account identifier and contain only dates, outcomes, credits and streaks. They do not contain tokens, names, raw vendor messages or client configuration. Unreadable storage stops check-in without overwriting it. Requests use the fixed HTTPS `copilot.tencent.com` billing endpoint, a 20-second request timeout and no redirects; client-customized model URLs never receive these credentials. Check-in failures do not interrupt normal message forwarding.

Protocol references inspected at Magpie commit `4b407bb79f7f91fb6db8d957304e73442b714007`: [daily check-in](https://github.com/yetone/magpie/blob/4b407bb79f7f91fb6db8d957304e73442b714007/internal/provider/workbuddy_checkin.go), [native session format](https://github.com/yetone/magpie/blob/4b407bb79f7f91fb6db8d957304e73442b714007/internal/provider/workbuddy.go), and [the WorkBuddy provider plugin at commit `63cd0e4`](https://github.com/magpie-community/plugins/tree/63cd0e4bf92e482cc338a606dea4e208bc20cdb7/packages/workbuddy). The integration is implemented independently in Rust; no Go or plugin runtime is required.
