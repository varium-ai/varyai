# Tool observations and risk evaluation

The current implementation observes supported tool-call payloads, normalizes the known tool semantics, evaluates applicable enabled rules and records sanitized results. It provides warnings and records, not execution authorization or a sandbox.

## Manual rule preview

The rule preview evaluates synthetic or user-entered tool arguments without executing a tool or recording a live observation. It remains available when live tool observation is disabled. Its results do not indicate that live protection is enabled; the panel explicitly identifies the disabled live-observation state. Malformed object, array or quoted JSON input is rejected before evaluation instead of being reinterpreted as plain text. Free-form inputs remain available. OpenAI `write_file` requires `path`; Claude and Gemini use `file_path`. An `unchecked` result indicates missing applicable evidence or rules, not a safety decision.

## Evidence boundaries

A model response describes a proposed call. It does not prove that a client selected or executed it, that permissions were granted, or that a later hook did not change the arguments. Provider-side execution must not be represented as local pre-execution evidence.

Parsing a tool name and JSON arguments is separate from understanding the tool's behavior. Coverage depends on the observed protocol, input completeness, known adapter, selected rule and available context. Unknown or out-of-scope operations must not be described as safe.

## Forwarding and privacy

Observation is a bounded side channel. It must not pause or rewrite ordinary streaming output, authorize commands, or interrupt forwarding when its own queue/worker fails. Keep raw tool inputs, secrets, untrusted names and model explanations out of ordinary diagnostics. UI records use the sanitized summary and explicit coverage/execution limitations.

Schema collection scans at most 1,024 total catalog entries, including expanded Gemini function declarations. Extension tools count toward this work budget but do not establish built-in semantics. Large supported catalogs retain recognized built-ins without copying tool descriptions or whole schemas. Duplicate aliases (including trimmed Codex aliases), custom namespaces, unsupported argument schemas and per-schema byte/depth overflow still refuse built-in attribution. Catalog overflow disables attribution for that response without interrupting forwarding.

WorkBuddy and WorkBuddy AI native `Bash` calls use separate trusted client schema references with a required string `command` field. Their shell calls use the same rule engine and command lists as Pi and Codex, while retaining the original WorkBuddy edition in event records. This does not register arbitrary OpenAI tools or WorkBuddy PowerShell semantics. A command is only warned about when an enabled rule or blacklist matches; `kubectl delete` is not an implicit Kubernetes destructive-operation rule. An explicit `kubectl delete` prefix blacklist can cover dry-run test commands without executing them during rule preview.

Qoder CN native `Bash` calls follow the same bounded schema validation and retain `qoder` as their event client. Kimi Code's native `Shell` and Qwen Code's `run_shell_command` also register required string `command` schemas and preserve their respective client identities. Registered shell semantics currently cover Claude Code/Desktop, Codex, Gemini CLI, Pi, WorkBuddy domestic/international, Qoder CN, Kimi Code and Qwen Code. Other Agent identities do not automatically inherit semantics merely because they use an OpenAI-compatible protocol or a similar tool name. The [full Agent coverage ledger](../qa/agent-feature-coverage.md) records the remaining gaps and distinguishes implementation from native acceptance; the overall feature is incomplete.

The implementation is under [protocol observation](../../src-tauri/src/proxy/tool_observation) and [tool-risk services](../../src-tauri/src/services/tool_risk). Read their regression tests before extending an adapter or transfer rule.

The current worker uses static rules. No local-model reviewer is connected to the runtime. Persisted rule fields reserved by older configurations remain readable for compatibility.

Shell command list editors expose all five match scopes directly in the scope menu: prefix, program, exact, keyword and regular expression. Keyword and regular-expression validation and explicit scope confirmation remain required; there is no separate advanced-mode visibility switch.

Policy history shows localized built-in rule names and the current name of an existing custom rule beside its stable ID. Names come from current configuration, not a historical snapshot; deleted rules retain their IDs without inventing a name. Manual refresh reloads both history and current rule names, keeps refresh and clear actions disabled until both reads finish, and reports failure if either read fails without exposing backend error details. Saving identical settings does not advance the policy version or announce an update; real changes still apply when live observation is disabled.

Tool-event session IDs come only from a recognized client request session field or header, carried alongside the bounded response observation. IDs must be nonempty, at most 256 bytes and contain no whitespace or control characters. Missing or invalid IDs remain absent; generated routing identities and historical events are never relabeled as client sessions. The asynchronous worker persists the optional ID and the unified security-event list exposes it without copying raw tool arguments.

Pi 1.5.2's official `before_provider_headers` hook can supply the current native session-manager ID even on Gemini requests that omit provider session metadata. Active Pi connection synchronization installs the isolated `extensions/varyai-session-id.js` helper in the configured Pi agent directory. Gateway startup also repairs a missing helper for an active Pi connection independently of model-catalog publication, including when the gateway is already running. Unconnected and suspended shared connections do not trigger installation. It leaves existing configuration and extension files untouched, never overwrites an unexpected file at its own path, and becomes inactive outside local VaryAI Pi routes. The helper must be loaded by restarting Pi or using its native resource reload. It stores no session IDs, reads no histories or credentials, and does not affect request success if session access fails. The local private header is consumed for correlation and excluded from upstream forwarding. Stable Pi affinity headers remain accepted as a fallback; per-request IDs are never promoted to sessions. Historical missing IDs are not backfilled.
