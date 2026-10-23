# Bug reproduction and regression record — 2026-10-22

The legacy review covered 23 reported issues, 19 assigned to Baomi. TRAE was explicitly excluded. Unreproduced reports did not justify changing detection rules or login credentials. External acceptance statuses were not edited. Private tracker links are retained only in the original repository.

## Reproduced and fixed

| Report | Reproduction | Change and regression coverage |
| --- | --- | --- |
| Redis credentials without a username | A password-bearing connection string did not produce a finding | Permit an empty username while requiring a password; check detection, outbound masking, log masking, and password-free negatives |
| Audit refresh feedback | Repeated clicks remained possible, without animation or completion feedback | Disable and animate while refreshing; wait for list/statistics; report success/error and allow retry |
| Empty provider name | Empty and whitespace-only names produced a generic error | Specific name-required feedback in Qoder and shared WorkBuddy forms |
| Repeated historical findings | Replayed history generated new events; identical current input was deduplicated to an old location | Inspect/redact all text, suppress new nonblocking history events, and prefer current-turn findings; cover Messages, Responses, Gemini, repeated inputs, different PII categories, and role-free Responses tool output |
| Audit excerpt wording | Warning-mode log content was labeled as hidden outbound content | Label it as a masked audit excerpt and explain that warning mode preserves outbound text |
| WorkBuddy connection state | Enabled preference was true without a takeover backup; restored/custom endpoints still appeared connected | Require a running gateway, valid backup, and the injected current endpoint; cover repeat enablement, exit restore, reconnect, unrelated setting edits, external endpoint edits, and local mock forwarding |

The original complete history-mixing case still needs client verification. WorkBuddy connection-state regressions pass, but actual client messages and OS notifications still need validation with a new package.

## Reports awaiting original-environment reproduction

- Codex official-login refresh: existing login-switch, credential-rotation, and managed-account tests passed; no real credentials were changed.
- Private-key warning versus redaction: tests preserve warning-mode text, redact outbound text in redaction mode, and mask both logs. Original UI behavior remains unconfirmed.
- PyPI: the recorded short FAKE-suffixed sample did not satisfy rule length. Valid synthetic tokens passed; the rule was not weakened.
- Bedrock long-term keys: valid synthetic samples passed warning/redaction tests; the original recording still needs detailed review.
- Missing notifications for GitHub/OpenAI/Anthropic/database credentials: notification regressions passed and the running app could emit redaction notifications; the original per-token environment was not reproduced.
- Other tokens and structured passwords: valid synthetic provider-token samples passed explicitly selected actions. Real structured tool arguments have separate password coverage; plain JSON pasted as chat text correctly uses the text-assignment rule.
- TRAE connection waiting: excluded as requested.

## Historical validation and limits

The original repository passed typecheck, renderer build, 2172 frontend tests in 214 files, cargo check, and 3809 backend library tests (12 ignored). Failing regressions preceded fixes. The later pre-migration combined tree passed 2282 frontend tests and 3827 backend library tests (13 ignored).

Tests used mock upstreams, dynamic ports, temporary directories, or in-memory databases. The installed app was not replaced; automated acceptance does not establish new-package client acceptance. A temporary WorkBuddy homepage entry was removed after inspection; client models, route URLs, credentials, and real messages were not changed. Existing protocol work was preserved. New-repository verification is reported separately.

## Public-repository migration verification — 2026-10-22

The rebased tree passed typecheck, renderer build, Prettier, rustfmt, cargo check, and Clippy with warnings denied. Frontend: 2143 tests across 198 files passed. Rust: 3829 library tests and 152 integration tests passed; 13 existing ignored library tests remain outside this run. Tests used the public repository's dependency versions and preserved its privacy controls, CI, and documentation baseline.
