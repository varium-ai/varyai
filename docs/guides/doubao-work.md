# Doubao Work on macOS

This integration targets the standalone Doubao Work client, not the general Doubao application. Install the client separately and sign in with your own account.

## Connect

1. Add Doubao Work through Vary AI's agent-management controls; it is not added automatically.
2. Quit Doubao Work using its own menu. On its Vary AI home page, choose the action to open it with inspection enabled. Review and approve the local debugging connection when prompted.
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
