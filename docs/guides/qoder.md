# Qoder CN integration boundaries

VaryAI maintains selectable official and API-key provider configurations for the supported installed Qoder CN client. Saving a spare configuration does not itself establish that takeover has been enabled or that the client has reloaded it.

## Configuration and protocol

The integration uses the supported client's endpoint/model configuration and stores takeover backups. Unknown fields, other providers and external user changes must be preserved. Restart Qoder after changes that require rereading endpoint or trust configuration.

Native official request bodies require the supported installed protocol component. The adapter validates the component before use, decodes supported text for inspection and re-encodes/signs a redacted request. Decode/signing failures must not silently forward sensitive plaintext. The repository does not contain a user's login credentials or the installed vendor application.

## Certificate lifecycle

For the supported macOS path, explicit takeover can require local certificate trust. Explain its purpose and request system confirmation. The local CA is constrained to localhost/loopback; other applications using the user's trust store can also be affected by trust decisions. Do not claim that local trust is invisible to the rest of the system.

Valid certificate metadata and trust can be reused across temporary routing stops and normal restarts. Clearing a certificate first stops takeover and restores configuration, then removes the corresponding trust and local files. A manual-clear state must not silently regenerate trust. The application does not read or store the system authentication password.

New local CA and server certificates are valid for 30 days. Each generation uses a random serial so that macOS Keychain does not confuse a renewed root with an earlier root that has the same issuer name. Legacy roots with the fixed `1000` serial are renewed during the next explicit routing enable, using the normal trust-authorization flow. Other valid certificates retain their original expiration date until they expire or the user explicitly rebuilds them. The one-minute certificate override remains available only in development builds for expiration testing.

Takeover verifies the server certificate against the installed keychain without supplying the local root as an extra chain input, then confirms that the exact root is available in the supported Qoder runtime's system CA store. Both checks must pass after trust installation before endpoint takeover is applied. A successful authorization dialog or trust command alone does not establish usable runtime trust. Restart Qoder completely after installing or replacing trust because an existing daemon can cache its certificate store.

Provider-list reads run on a background worker. They can wait for the routing switch lock without blocking the application main thread needed to present certificate authorization dialogs.

Certificate failures report a fixed operation stage and exit status, without exposing command output or certificate configuration. An approved macOS prompt does not establish that subsequent trust or keychain operations succeeded. During clearing, a failed keychain deletion is accepted only when a successful, recognizable certificate listing confirms that the matching certificate is already absent. An unreadable keychain or an unrecognized listing still fails and preserves recovery metadata.

Explicit supplier switches replace VaryAI-owned custom provider groups, including reserved provider IDs left after deletion of the shared database. Official account credentials, plugins and user-created provider groups remain intact. The file change is staged and rolls back if committing the corresponding database update fails.

See [takeover service](../../src-tauri/src/services/qoder_proxy.rs), [certificate handling](../../src-tauri/src/services/qoder_certificate.rs), and [native codec](../../src-tauri/src/proxy/qoder_codec.rs). Synthetic codec tests are not a claim that every vendor version, account, model, installed package or system trust configuration has been accepted.
