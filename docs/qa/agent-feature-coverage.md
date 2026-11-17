# Agent feature coverage

This is a required implementation and acceptance ledger, not a declaration that every listed feature is supported. Follow the mandatory Agent coverage rules in [AGENTS.md](../../AGENTS.md). Derive the inventory from `src/config/appConfig.tsx`, `AppType` and the connection registries, including native integrations outside the shared `APPS` list.

For every Agent-facing change, assess all entries below against its entry point, protocol/native adapter, runtime behavior, UI state and supported platforms. Record implementation, synthetic regression and native acceptance separately. New Agents must be added here and assessed against existing cross-Agent features. Missing or unverified entries prevent an all-Agent completion claim unless the user explicitly accepts a narrower scope.

## Tool-call risk warnings: current gaps

The full feature remains incomplete. Gateway connectivity and DLP text inspection do not establish tool-call observation. Registered native shell semantics require structured tool calls and a matching enabled risk policy. Warnings and records observe proposed calls; they do not block local execution.

| Agent ID | Agent | Implementation status | Native acceptance evidence |
| --- | --- | --- | --- |
| `claude` | Claude Code | Registered `Bash` schema and shell semantics | Not rechecked in this follow-up |
| `claude-desktop` | Claude Desktop | Registered `Bash` schema and shell semantics | Not rechecked in this follow-up |
| `codex` | Codex | Registered `exec_command` and `shell_command` | Not rechecked in this follow-up; keep the user's route off |
| `gemini` | Gemini CLI | Registered `run_shell_command` | Not rechecked in this follow-up |
| `pi` | Pi | Registered `bash`; native session helper delivered | User confirmed warning/session ID and `pi_` display acceptance |
| `workbuddy` | WorkBuddy China | Registered `Bash` | A native risk event with a conversation ID was observed |
| `workbuddy-ai` | WorkBuddy International | Registered `Bash` | A native risk event with a conversation ID was observed |
| `qoder` | Qoder CN | Registered `Bash` | Native retest remains pending |
| `kimi` | Kimi Code | Registered native `Shell`; schema, policy and shared-routing regressions passed | User reported no warning; native retest pending |
| `qwen` | Qwen Code | Registered native `run_shell_command`; schema, policy and shared-routing regressions passed | User reported no warning; native retest pending |
| `opencode` | OpenCode | Missing native semantic registration; protocol coverage unaudited | Not verified |
| `grokbuild` | Grok Build | Missing native semantic registration; protocol coverage unaudited | Not verified |
| `openclaw` | OpenClaw | Missing native semantic registration; protocol coverage unaudited | Not verified |
| `hermes` | Hermes | Missing native semantic registration; protocol coverage unaudited | Not verified |
| `zcode` | ZCode | Missing native semantic registration; repair required | User reported no warning; not accepted |
| `deepseek-harness` | DeepSeek Harness | Missing native semantic registration; repair required | User reported no warning; not accepted |
| `t3code` | T3 Code | Missing native semantic registration; repair required | User reported no warning; not accepted |
| `trae` | TRAE CN | Native integration currently inspects message text without a complete tool collector | User reported no warning; not accepted |
| `trae-intl` | TRAE International | Native integration currently inspects message text without a complete tool collector | Not verified |
| `doubaowork` | Doubao Work | Native integration currently inspects message text without a complete tool collector | User reported no warning; not accepted |

Shared Rust implementation applies across platforms, but native Windows/macOS/Linux acceptance must be recorded for each applicable path. A passed macOS fixture is not Windows acceptance. Do not infer session availability where a client supplies no recognized stable identity, and never substitute request IDs or generated per-request IDs.

Required verification for each missing adapter includes a structured native schema, JSON/SSE transport as applicable, enabled and disabled policy, malformed/duplicate/namespaced definitions, bounded collection, preserved forwarding, client identity, session correlation when supplied, persisted event and notification delivery. Native collectors must distinguish a proposed call from executed output and preserve existing security decisions and user configuration.

Kimi's native `Shell` command input is defined by its [official shell implementation](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/tools/shell/__init__.py). Qwen's `run_shell_command` identity and string command schema are defined in its [official tool names](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-names.ts) and [shell implementation](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/shell.ts). These contracts establish the bounded adapters; they do not prove native acceptance on the user's installed versions.

## Explicit configuration and reconnection

Source assessment covers every entry point. Native acceptance below is pending for this follow-up. Shared reconnection archives the old receipt, retains external edits as its new baseline and writes the current managed projection. Existing native consent, credential access and platform support remain applicable. TRAE's explicit replacement intent is distinct from ordinary model selection and background refresh.

| Agent ID | Implementation path | Native acceptance in this follow-up |
| --- | --- | --- |
| `claude` | Shared receipt release, rebuild and install | Not rechecked |
| `claude-desktop` | Desktop-aware shared release, visibility validation and install | Not rechecked |
| `codex` | Explicit native Codex reconnect and durable selection | Not rechecked; keep the user's route off |
| `gemini` | Shared receipt release, rebuild and install | Not rechecked |
| `pi` | Shared receipt release, rebuild and install | Not rechecked |
| `workbuddy` | Desktop-aware shared release, rebuild and install | Not rechecked |
| `workbuddy-ai` | Desktop-aware shared release, rebuild and install | Not rechecked |
| `qoder` | Desktop-aware shared release, rebuild and install | Not rechecked |
| `opencode` | Shared receipt release, rebuild and install | Not rechecked |
| `grokbuild` | Shared receipt release, rebuild and install | Not rechecked |
| `openclaw` | Shared receipt release, rebuild and install | Not rechecked |
| `hermes` | Shared receipt release, rebuild and install | Not rechecked |
| `kimi` | Shared receipt release, rebuild and install | Not rechecked |
| `qwen` | Shared receipt release, rebuild and install | Not rechecked |
| `zcode` | Shared release with native rule-envelope repair, rebuild and install | Not rechecked |
| `deepseek-harness` | Native credential-aware release, rebuild and install | Not rechecked |
| `t3code` | Shared receipt release, rebuild and install | Not rechecked |
| `trae` | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `trae-intl` | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `doubaowork` | Native audit connection/launch path; the product exposes no editable provider/model projection | Not rechecked |
