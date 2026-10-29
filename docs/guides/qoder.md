# Qoder CN integration boundaries

VaryAI maintains selectable official and API-key provider configurations for the supported installed Qoder CN client. Saving a spare configuration does not itself establish that takeover has been enabled or that the client has reloaded it.

## Configuration and protocol

The integration uses the supported client's endpoint/model configuration and stores takeover backups. Unknown fields, other providers and external user changes must be preserved. Restart Qoder after changes that require rereading endpoint or trust configuration.

Native official request bodies require the supported installed protocol component. The adapter validates the component before use, decodes supported text for inspection and re-encodes/signs a redacted request. Decode/signing failures must not silently forward sensitive plaintext. The repository does not contain a user's login credentials or the installed vendor application.

## Certificate lifecycle

For the supported macOS path, explicit takeover can require local certificate trust. Explain its purpose and request system confirmation. The local CA is constrained to localhost/loopback; other applications using the user's trust store can also be affected by trust decisions. Do not claim that local trust is invisible to the rest of the system.

Valid certificate metadata and trust can be reused across temporary routing stops and normal restarts. Clearing a certificate first stops takeover and restores configuration, then removes the corresponding trust and local files. A manual-clear state must not silently regenerate trust. The application does not read or store the system authentication password.

New local CA and server certificates are valid for 30 days. Existing certificates retain their original expiration date until they expire or the user explicitly rebuilds them. The one-minute certificate override remains available only in development builds for expiration testing.

See [takeover service](../../src-tauri/src/services/qoder_proxy.rs), [certificate handling](../../src-tauri/src/services/qoder_certificate.rs), and [native codec](../../src-tauri/src/proxy/qoder_codec.rs). Synthetic codec tests are not a claim that every vendor version, account, model, installed package or system trust configuration has been accepted.
