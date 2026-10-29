# WorkBuddy provider configuration

VaryAI supports selectable official and API-key WorkBuddy configurations. Official mode uses the installed client's login state; API-key mode uses the configured endpoint, credential and model. Creating a duplicate provider does not switch the active configuration or write client files.

Enable takeover deliberately, back up the client settings and restart WorkBuddy when it needs to reread its endpoint. Disabling takeover or exiting normally restores the owned settings. An abnormal termination cannot perform normal exit cleanup; recovery uses the persisted backup at the next start.

When no official endpoint is set in `settings.json`, routing reads the newest product cache in the WorkBuddy configuration directory. Both `cache/acc-product-config-v3.json` and `cache/acc-product-config-v3-*.json` are supported. Start WorkBuddy first if no cache exists. An unreadable or invalid newest cache stops setup without changing client settings.

If adding a provider reports an existing model ID, use the saved provider or import the native configuration before editing it. WorkBuddy's native model catalog requires unique model IDs; adding the same IDs again does not select the existing provider. Click **Use this** on its card to make it current.

WorkBuddy displays each custom model as its name followed by its model ID. VaryAI uses the provider name for that display name without appending the ID again. To update labels written by an older version, edit the saved provider and save it again. Restart WorkBuddy if it still displays the old catalog. This updates display names while preserving model IDs.

After enabling protection, restart WorkBuddy and open a new conversation before testing. The global protection confirmation includes this restart instruction. The saved takeover status means routing is configured; it does not prove that an already running client has reloaded its endpoint. If Security Center shows no WorkBuddy events, verify that new requests reach the local proxy before diagnosing individual detection rules. Optional catalog rules must also have Alert or Hide enabled; they default to Allow.

The active status requires a running proxy, a recovery backup, and the injected endpoint still present in WorkBuddy settings. A saved enabled preference alone is insufficient: after normal exit restores direct routing, enabling protection must apply the endpoint again. External changes to the endpoint also invalidate the active status.

Official forwarding retains the relevant login state. API-key forwarding uses the selected provider and strips official-only authentication data. Both follow the enabled inspection settings; a successful provider response is not a guarantee that every tool, payload or future vendor version is covered.

Use a disposable profile and explicitly synthetic message content when testing. Keep actual API keys and private settings out of prompts, logs and screenshots. Coverage and recovery behavior are implemented in [the takeover service](../../src-tauri/src/services/workbuddy_proxy.rs), [provider handling](../../src-tauri/src/services/provider/workbuddy.rs), and [proxy adapter](../../src-tauri/src/proxy/workbuddy.rs).
