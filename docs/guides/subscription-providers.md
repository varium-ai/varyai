# Subscription providers

The global provider picker offers Claude, ChatGPT / Codex, ChatGPT API,
GitHub Copilot, Gemini CLI, Antigravity, and Grok. Subscription accounts are
separate from API-key vendors. Each saved provider binds an explicit account;
changing another application's default account does not change that binding.

## Account setup

- **Claude** uses an installed `claude` executable. Browser sign-in runs
  `claude auth login --claudeai` in an isolated configuration directory. Local
  credential import copies the original sign-in without editing its source.
  Claude Code may rotate the copied credentials when it refreshes them. Its
  cached account model options and entitlements supplement the CLI aliases
  Sonnet, Opus, and Haiku.
- **ChatGPT / Codex** keeps its existing Codex account manager and backend.
- **ChatGPT API** uses the independent public-client Sign in with ChatGPT flow.
  It registers Vary AI, validates state, PKCE, ID-token signature, issuer,
  audience, expiry, nonce and granted plan-usage scope, and uses the issued
  account-specific client ID for refresh. Inference goes to the OpenAI API.
  A Codex token is never reused for this entry.
- **GitHub Copilot** and **Grok** keep their existing device authorization flows.
- **Gemini CLI** uses Google authorization or imports `.gemini/oauth_creds.json`.
  Code Assist selects the project, model availability and backend model IDs.
  Standard or Enterprise accounts can supply a Google Cloud project explicitly.
- **Antigravity** uses its own Google OAuth client and Code Assist endpoint.
  Set `VARYAI_ANTIGRAVITY_CLIENT_ID` and `VARYAI_ANTIGRAVITY_CLIENT_SECRET`
  in the application environment; these credentials are not bundled in source.
  Model discovery uses `fetchAvailableModels` for the selected account. Account
  JSON imports accept Google OAuth fields, optionally nested under `token`.

The four additional account types keep credentials under
`~/.varyai/subscription-accounts`, outside portable routing data. JSON writes
are atomic and owner-only on Unix. The renderer receives account summaries;
the routing registry stores account IDs and model preferences, never tokens.
Removing a Claude account also removes its isolated credential directory and
corresponding macOS keychain item. Removing other accounts forgets the local
credentials; it does not revoke the remote authorization.

## Execution

Google subscription requests use the Code Assist request envelope, not the
public Gemini API-key endpoint. SSE responses pass incrementally through the
shared protocol bridge. Provider- and conversation-scoped continuity retains
opaque tool signatures for subsequent turns.

Claude requests run through Claude Code with built-in local tools disabled.
An ephemeral loopback MCP endpoint lists only the caller's tools. Tool names
are mapped back to caller names; the CLI stops after the model's message and
the caller performs tool execution. Subsequent requests carry the supplied
conversation and tool results. The implementation currently starts a fresh
CLI process for each request rather than reusing Magpie's persistent runs.
Claude Code owns its system prompt and may normalize generation options.
Subscription token-counting endpoints are not supported.

Automated checks use synthetic credentials and dynamic loopback ports. They
do not establish live account entitlement, OAuth approval, or compatibility
with every installed Claude Code version. Real sign-in and inference require
validation with the user's own authorized account.

## References

- [OpenAI public-client registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [OpenAI ChatGPT plan models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- Magpie revision `1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`:
  `internal/provider/google.go`, `chatgpt_api.go`, `claude_signin.go`,
  `claude_dirs.go`, `claude_code_models.go`, and
  `internal/gateway/claude_subscription.go` / `codeassist.go`.
