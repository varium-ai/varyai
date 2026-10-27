# DLP architecture and safety boundaries

The DLP engine inspects supported outbound text extracted from requests handled by enabled local routing. It is not a disk scanner or a guarantee that all client traffic or every JSON field is inspected.

For Claude Desktop, the home status offers to enable protection when its proxy route is connected but detection is off. If the Desktop route is not connected, it instead opens routing settings. A running gateway alone does not establish that Desktop is configured to use it.

For clients with a route takeover action, the home status offers **Enable local routing** when that client is disconnected, regardless of the global detection setting. This action enables the client's route and security detection. A connected client with detection off instead shows **Activate protection**, including configured Claude Desktop and Pi routes. The footer also prioritizes the current client's routing state over the global detection setting. Selecting an upstream format alone does not activate routing or protocol conversion.

## Processing

1. Extract the supported text fields without changing unrelated protocol values.
2. Use a multi-pattern prefilter to select applicable built-in expressions, PII checks and configured custom rules.
3. Apply the runtime rule settings. Risk levels classify findings; they do not independently decide the action.
4. When redaction is required, update only selected text values in a copy and inspect the result again. Return a protocol-shaped error if safe redaction cannot be established.
5. Queue audit work for bounded asynchronous persistence rather than writing to SQLite in the forwarding hot path.

## Current actions

The base credential/PII rules, including private-key material, default to redaction; private RFC1918 IPs default to audit-only. Optional catalog rules default to allow. The persistence boundary normalizes saved/imported `block` settings to `redact`; historical audit events retain their original action. The engine still has protective error paths when redaction fails. The generic `sk-` notice has its own restricted actions.

Confirm behavior in `builtin_rules.rs`, `runtime.rs`, and the security DAO before changing settings or documentation.

Alert sends the request unchanged; Hide replaces detected sensitive text before forwarding. Both actions keep sensitive values masked in audit details. Comparing audit snippets alone cannot establish whether the forwarded request was redacted.

For conversation arrays (`messages`, `input`, and `contents`), entries before the last user message are historical. They remain inspected and redacted, but successful forwarding does not create new audit events or notifications for historical findings. Current input takes precedence when the same sensitive value appears in both history and the new turn. Blocked requests still report the findings that explain the block. Requests without a user message, including tool-result deltas, remain fully eligible for reporting.

The Password Field rule inspects password string values in actual structured tool arguments. JSON pasted into a chat message is plain request text and uses Password Assignment instead. The visible rule description states this boundary; changing the pasted text's appearance does not turn it into tool arguments.

## Privacy and persistence

Client errors and notifications must not contain matched sensitive plaintext. Stored audit summaries are masked; masking is not a proof of complete anonymity. Event aggregation and asynchronous notifications do not establish that every attempted event was persisted or displayed.

The detector itself executes locally. That statement does not describe the whole application's network behavior: see the [statistics and update review](../data-and-network.md). No performance benchmark is asserted here.

## Security Center refresh

The records refresh button returns to the first filtered page and reloads the unified event list and range summary, including all three action totals. The table area shows a centered loading spinner and message, while the button's original refresh icon rotates. Loading remains visible for at least 300 milliseconds and until every query finishes; refresh and pagination controls are disabled during that time. Successful refreshes restore the table without a success message. If any query fails, the page shows a generic failure message without backend error details and allows another attempt.

The clear button's native `title` tooltip and confirmation dialog explain that clearing permanently deletes matching sensitive-information and tool-observation events across all pages of the current view and filters. Opening or canceling the dialog does not delete records. Confirmation starts the deletion and disables both dialog actions until it finishes; a failure keeps the dialog open for retry. Successful deletion reloads the records and statistics and returns pagination to the first page.

## Implementation references

- [Rules](../../src-tauri/src/proxy/security/builtin_rules.rs)
- [Extraction](../../src-tauri/src/proxy/security/extractor.rs)
- [Engine and redaction](../../src-tauri/src/proxy/security/engine.rs)
- [Runtime policy](../../src-tauri/src/proxy/security/runtime.rs)
- [Audit worker](../../src-tauri/src/proxy/security/audit_logger.rs)
- [Persistence normalization](../../src-tauri/src/database/dao/security.rs)
- [Regression tests](../../src-tauri/src/proxy/security/tests.rs)

Pattern literals, private-IP examples and synthetic credentials in tests are intentional detector fixtures. They must not be replaced with live credentials or removed merely because a secret scanner flags their shape. Existing third-party notices remain applicable.
