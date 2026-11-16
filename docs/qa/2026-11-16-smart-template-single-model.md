# Smart template with one task model

The user requested allowing the strongest task model to handle both hard and easy tasks when a cheaper model is unavailable. This supersedes the earlier missing-cheap-model disabling decision for the first template only.

The template keeps one member and directs its hard-task, easy-task and compaction rules to that member. It creates neither duplicate members nor self fallback. The card explains that reusing the model provides no model-cost reduction. An eligible intent-classification provider is still required. Existing distinct cheap-model behavior, the thrift template and the separate-provider resilience template remain unchanged. No protocol/model conversion was introduced.

## Verification

- Base: `d156a4c098b08b36d30b1ba917a98ca70a6d1b06`, macOS arm64, Node 20.20.2 and pnpm 10.12.3.
- Test-first regression: the previous implementation failed the single-model and enabled-Add expectations.
- Focused routing utility and UI tests: 132 passed across four files. Coverage includes unique member references, no self fallback, missing-classifier rejection, original distinct-model routing, valid import, explanatory text and opening the editor without saving.
- Typecheck, renderer build, formatting and whitespace checks: passed.
- Full frontend tests: 4,243 passed across 318 files. Logs: `/tmp/varyai-smart-single-*.log`.
- Typecheck was repeated before committing and passed. Production dependency audit passed with no known vulnerabilities. Client/build helper sources and dependency manifests are unchanged from the successful main-integration checks recorded in `2026-11-16-main-integration.md`.
- Shared renderer logic applies to macOS and Windows. No backend/proxy changes or native-client acceptance were performed. Exact-revision remote CI remains separate from these local checks.
