# Doubao Work on macOS

This integration targets the standalone Doubao Work client, not the general Doubao application. Install the client separately and sign in with your own account.

## Connect

1. Add Doubao Work through Vary AI's agent-management controls; it is not added automatically.
2. Quit Doubao Work using its own menu. In Vary AI, open **Tools → Doubao Work → Details**, then choose **Open Doubao Work with inspection** in the connection instructions. The action is shown while the client is disconnected, including when its audit switch is already on and waiting. Review and approve the local debugging connection when prompted.
3. Wait for the connected state and check that security inspection is enabled. Start a new conversation before sending a synthetic test message.

After enabling or re-enabling inspection, use a new conversation. Earlier messages sent without inspection remain in the conversation and may still be available to the model. Reconnecting does not retrospectively redact them.

A normally launched client may not expose its debugging interface. A waiting-for-connection state does not establish that messages are protected. Vary AI does not automatically launch or force-restart the client in the background.

## Coverage

The integration uses the client's local Chromium debugging interface to inspect supported message text before the official `/chat/completion` request. Enabled rules can redact, warn, or forward without inspection. Security events use the existing event and notification infrastructure. Session identifiers use the `doubaowork_` prefix.

Redaction changes the transmitted message text, which may also appear redacted in the client's conversation history. Turning inspection off cannot restore messages already sent. Authentication, model options, and unrelated request fields are preserved. This integration does not install a certificate, change the system proxy, or replace client account/model configuration.

Coverage is limited to recognized text messages. Attachments and other client APIs are outside this coverage. Token usage and cost are not inferred when the response provides no supported usage fields.

## Disconnect and restore

Disabling inspection for this client or exiting Vary AI removes inspection; later messages are sent directly. The client's random debugging port remains available on the local machine until the client fully exits. Other local programs with access to that interface may be able to read or control the client. Quit it fully, then reopen it normally to close the debugging interface.

Vary AI does not rewrite this client's persistent account or model files. Reconnecting can reuse an existing debugging session; a normally launched client needs the explicit connection workflow above.

## Verify

- Use clearly synthetic text with a configured redaction rule, then review the transmitted text and recorded event.
- Change that rule to warning-only and verify forwarding and the warning.
- Change it to allow and verify that the corresponding security notification is absent.
- Test a new conversation, an existing inspected conversation, and normal use after disconnecting.

The integration was developed against Doubao Work 2.32.14. Client updates require fresh verification of its interfaces and text structure; this document is not acceptance evidence for every later version.

## Daily check-in and accounts

The home page lists accounts already signed in through the official client. Add further accounts there, then choose **Refresh accounts** in VaryAI. Select an account to view its result and automatic check-in preference. **Switch and check in** calls the official client's account-switch flow, waits for that account to become current, and then claims its reward. The selected account stays active afterward; VaryAI does not restore the previous login.

The check-in helper uses the connected client's own account stores, campaign configuration and initialized API service. It queries `GetLandingPage` before calling `DailyCheckIn`, using the campaign scene supplied by the client. It checks the current account again after the status query and before accepting the claim result. Cookies, signing and login tokens remain inside the official runtime. The full official switch flow can reload the client page; VaryAI reconnects and verifies the selected identity before claiming. A refused or unconfirmed switch produces a failure for the selected account, without claiming for the previous login.

After a confirmed check-in, `GetCampaignV3Detail` supplies the `daily_signin` task's quota reward, consecutive days and total days. The card displays these official values separately for each account. Refresh also queries the current native account without claiming or switching; other accounts use their saved daily results. Missing or invalid details remain absent, and a failed or slow details request does not change a confirmed check-in into a failure.

Automatic check-in defaults to off separately for every account. While VaryAI and the connected client are running, the worker processes enabled accounts in sequence once per minute when work is due. It switches through the official SDK and leaves the last processed account active. A batch stops on an uncertain outcome or failed switch. Successful or already-completed check-ins are cached for the Beijing calendar day and do not trigger another switch; unconfirmed outcomes have a 30-minute automatic retry delay. Manual retry can check them immediately. Closing VaryAI stops this worker.

`doubao-checkin.json` in Tauri's local application data directory stores only hashed account identifiers, preferences, dates, outcomes and optional official reward/day counts. It contains no cookies, tokens or account labels and is excluded from shared configuration exports and synchronization. Unreadable records stop check-in without overwriting the file. Check-in does not enable message interception or restart the client. It requires the existing local connection consent. A local debugging connection remains available until the native client exits, independently of whether message inspection is enabled.

Account and campaign discovery supports both the legacy and current desktop chunk entry points. Stores and the initialized campaign service are selected by stable module markers and their supported interfaces, rather than minified export names. Ambiguous modules or missing interfaces stop the operation. Message inspection can remain connected even when a newer client changes these SDK interfaces; an inspection connection alone does not establish that account discovery or check-in is available.

Helper tests use synthetic SDK stores and verify account identity, existing check-ins, inactive campaigns, reward validation and account changes during a request. The opt-in native acceptance check reads campaign status without claiming a reward. Reward amounts and day counts come from structured official task data; VaryAI does not infer them from UI text or local record counts.
