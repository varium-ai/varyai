# WorkBuddy provider configuration

VaryAI supports selectable official and API-key WorkBuddy configurations. Official mode uses the installed client's login state; API-key mode uses the configured endpoint, credential and model. Creating a duplicate provider does not switch the active configuration or write client files.

Enable takeover deliberately, back up the client settings and restart WorkBuddy when it needs to reread its endpoint. Disabling takeover or exiting normally restores the owned settings. An abnormal termination cannot perform normal exit cleanup; recovery uses the persisted backup at the next start.

Official forwarding retains the relevant login state. API-key forwarding uses the selected provider and strips official-only authentication data. Both follow the enabled inspection settings; a successful provider response is not a guarantee that every tool, payload or future vendor version is covered.

Use a disposable profile and explicitly synthetic message content when testing. Keep actual API keys and private settings out of prompts, logs and screenshots. Coverage and recovery behavior are implemented in [the takeover service](../../src-tauri/src/services/workbuddy_proxy.rs), [provider handling](../../src-tauri/src/services/provider/workbuddy.rs), and [proxy adapter](../../src-tauri/src/proxy/workbuddy.rs).
