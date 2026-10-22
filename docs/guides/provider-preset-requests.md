# Provider preset requests

The request dialog sends the entered provider information to the configured VaryAI request service. It is not a place to submit API keys, passwords, private configuration, customer data or confidential internal endpoints.

Use a public provider homepage and describe supported API formats with sanitized information. Review every field before submitting. The service can receive the submitted contact/content information; do not describe this action as local-only.

The current service host is the public product host used by [the request command](../../src-tauri/src/commands/provider_request.rs). Development builds can override its HTTPS origin with `VARYAI_PROVIDER_REQUEST_BASE_URL`. Internal notification credentials or webhook configuration are not required by the desktop client.

See [request validation tests](../../src-tauri/src/commands/provider_request_tests.rs). These tests use synthetic inputs and mocked requests; they do not verify production retention or receiving-service operations.
