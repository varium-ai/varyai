# Tool observations and risk evaluation

The current implementation observes supported tool-call payloads, normalizes the known tool semantics, evaluates applicable enabled rules and records sanitized results. It provides warnings and records, not execution authorization or a sandbox.

## Manual rule preview

The rule preview evaluates synthetic or user-entered tool arguments without executing a tool or recording a live observation. It remains available when live tool observation is disabled. Its results do not indicate that live protection is enabled; the panel explicitly identifies the disabled live-observation state. Malformed object, array or quoted JSON input is rejected before evaluation instead of being reinterpreted as plain text. Free-form inputs remain available. OpenAI `write_file` requires `path`; Claude and Gemini use `file_path`. An `unchecked` result indicates missing applicable evidence or rules, not a safety decision.

## Evidence boundaries

A model response describes a proposed call. It does not prove that a client selected or executed it, that permissions were granted, or that a later hook did not change the arguments. Provider-side execution must not be represented as local pre-execution evidence.

Parsing a tool name and JSON arguments is separate from understanding the tool's behavior. Coverage depends on the observed protocol, input completeness, known adapter, selected rule and available context. Unknown or out-of-scope operations must not be described as safe.

## Forwarding and privacy

Observation is a bounded side channel. It must not pause or rewrite ordinary streaming output, authorize commands, or interrupt forwarding when its own queue/worker fails. Keep raw tool inputs, secrets, untrusted names and model explanations out of ordinary diagnostics. UI records use the sanitized summary and explicit coverage/execution limitations.

The implementation is under [protocol observation](../../src-tauri/src/proxy/tool_observation) and [tool-risk services](../../src-tauri/src/services/tool_risk). Read their regression tests before extending an adapter or transfer rule.

The current worker uses static rules. No local-model reviewer is connected to the runtime. Persisted rule fields reserved by older configurations remain readable for compatibility.

Shell command list editors expose all five match scopes directly in the scope menu: prefix, program, exact, keyword and regular expression. Keyword and regular-expression validation and explicit scope confirmation remain required; there is no separate advanced-mode visibility switch.
