# Gateway policy acceptance

Validated on macOS on October 26, 2026, using the development application with a separate bundle identifier, temporary home directory, dynamic loopback port and synthetic upstream. The installed application and production client configuration were not replaced.

## Desktop application checks

- Keys opens as a list. Create key explicitly opens the form; its name describes the credential rather than a running model.
- Created a daily 50-token key, hid its one-time secret, and bound Claude Desktop through Local agents.
- Starting the listener from the key panel left other agents' takeover switches off.
- Two native Desktop Messages requests returned 200 and consumed 25 tokens each. The next returned 429 without an upstream call.
- Editing the limit to 100 preserved the first 50 tokens. Another request succeeded.
- Incorrect native Desktop authentication remained rejected and made no upstream call. This completed error response had no usage and appeared in the missing-usage count.
- Selected the Desktop client route ID from the configured model catalog. A different model returned 403; the permitted route succeeded and mapped to the synthetic upstream model.
- At 100 tokens the next request returned 429. The Usage tab showed the key, Desktop agent, client route ID, accounting period, 100 tokens and five completed responses, including one without usage.
- Local agents showed all 16 integrations. Desktop showed its binding active only with the matching native profile. OpenCode, OpenClaw and Hermes explicitly described their current direct configuration and lack of automatic binding.
- Separate manual gateway requests using Messages, Chat Completions, Responses and Gemini generateContent all returned 200. Their shared synthetic key recorded four requests, 100 tokens and no missing usage.

## Automated verification

The required TypeScript check, renderer build, frontend formatting, Rust formatting, cargo check, Clippy, library tests and complete Rust test suite passed. The final frontend suite passed 204 files and 2,372 tests. The Rust library suite passed 3,860 tests with 14 existing ignored tests; integration targets also passed.

Coverage includes legacy restrictive-policy compatibility, extension database upgrades, exact-model enforcement, shared budgets, revocation, missing usage, native authentication/body preservation across all 13 managed route families, Desktop route mapping and filtering of per-key usage.

## Acceptance boundaries

These checks exercise the running VaryAI gateway with synthetic clients and upstream responses. They do not establish live acceptance in all 16 agent applications, vendor billing accuracy, installer/signing acceptance or production release acceptance. TRAE's observational official audit bridge cannot enforce a blocking policy; its managed custom-provider route can. Direct requests outside VaryAI remain outside these budgets. In-flight and concurrent requests can exceed a completed-usage admission limit.
