# Vary AI security features: qualified messaging brief

**Status:** Source-verified feature copy for review, **not** an announcement that any particular installer, store listing, or distribution build has shipped. Recheck the intended release's settings, UI and legal/privacy wording before external publication.

## Short positioning

Vary AI provides a local gateway for **configured** AI client traffic. Its outbound DLP rules can detect and redact supported sensitive text before forwarding; separate tool-call observations can flag certain proposed risky operations. Coverage depends on routing, protocol, enabled policies and observable inputs. It is an aid to review, not a guarantee against leaks or unsafe execution.

## Why this matters

Coding assistants can include credentials, configuration, logs and personal data in requests to AI providers. A local checkpoint can help spot recognized private-key headers, selected token formats, database credentials, private IPs and supported personal identifiers before supported text is forwarded. Review routing and provider settings: text allowed by the policy or missed by its detectors may still reach the chosen provider.

## Implemented in the current source

### Outbound DLP: configurable detection and handling

- **Nine default-active rules** cover private-key headers, GitHub tokens, AWS access keys, OpenAI and Anthropic API keys, password-bearing database URIs, RFC1918 private IPs, mainland China ID numbers (checksum validation) and mobile numbers. **21 optional catalog rules** default to Allow, including additional token and password patterns. Users can add keyword or regex rules and test text in a local sandbox.
- Current selectable actions are **Redact**, **Alert**, and **Allow**. Default credential/PII actions are Redact; private IPs default to Alert. Alert passes original text and records a warning; Allow does not protect that rule's match. This is **not** a general user-selectable blocking policy. Historical saved `block` settings normalize to Redact; the engine can still return a protective error when safe redaction cannot be established.
- The engine inspects supported outbound text fields in locally routed requests and changes selected text values in a copy when redaction is needed. It does not scan every field or every request made by every client. Findings and stored summaries are masked; no claim of perfect detection or anonymization is implied.

### Tool-call risk: observation, not authorization

- The tool-risk feature is **off by default** at its top-level switch. With it enabled, six default-enabled built-in rules cover certain destructive file/command actions, sensitive-file reads, sensitive-data transfer to non-loopback destinations, remote download/execute chains, privilege changes and security-configuration changes. A seventh database-change rule is **disabled and visibly marked Not ready**: do not claim general SQL coverage.
- The UI supports custom semantic rules, Shell command blacklists and limited whitelists, a sandbox and policy change history. A whitelist cannot erase known dangerous classifications; a blacklist takes precedence on overlap. Results depend on recognized tool semantics, trusted identity, input completeness and active policy.
- Warnings and sanitized observations do **not** block commands, grant execution permissions or establish that a proposed model tool call actually ran. Unknown or unobserved calls are not proven safe. No local-model tool reviewer is connected to the runtime.

### Unified Security Events

- The current Security Center UI has **Security Events**, **Detection Rules** and **Tool Call Risk** tabs. The Security Events default **Event Details** view combines persisted DLP rows with **risky** tool observations; **All Tool Calls** also shows other _recorded_ outcomes such as unchecked, uncertain and low risk. Filters, summaries and detail dialogs help inspect the available sanitized evidence. Where appropriate, a user can start a command-list draft or a custom rule from a tool event; the user must review before saving/enabling it.
- DLP audits and tool observations use bounded asynchronous persistence. Records and notifications are useful signals, **not** proof every attempted event was captured. Stored masked snippets and sanitized summaries reduce exposure but do not guarantee anonymity.

### Safe testing and network boundary

The [security testing guide](../guides/security-testing-guide.md) gives synthetic, rule-by-rule examples for the **in-app** DLP and tool-risk sandboxes. The sandboxes neither forward requests to a model nor execute commands or persist security events. The detector runs locally, but that **does not mean the entire application is offline or sends no data**: configured provider traffic, optional opt-in installation statistics and application update checks have separate boundaries. See [data and network behavior](../data-and-network.md). Do not market an unmeasured latency number, zero telemetry, complete protocol coverage or a universal no-leak promise.

## Proposed, not available as a runtime security feature

- **Indirect prompt-injection detection:** exploration of warning on suspicious instructions in retrieved material. Do not describe it as a shipped inbound defense, scanner, or guaranteed protection.
- **Reversible tokenization:** a proposed local substitution/restoration design, not a shipped mechanism. Do not imply the model never sees original secrets in current builds.
- **Security assistant / agent:** proposed audit analysis and policy suggestions requiring review, not an active autonomous enforcement system.

These ideas are roadmap concepts only; no timeline or commitment is asserted.

## Responsible use and publication checks

Applicable protection is limited to configured traffic through the local gateway and the supported extracted fields. Redaction is pattern-dependent; user-customized Alert/Allow settings may pass matching text through unchanged. The app is not a disk-wide secret scanner, a compliance certification, or a tool execution-control layer. Avoid claims such as “prevents every leak,” “zero false positives,” “guarantees privacy/compliance,” or “released on all platforms.”

Before publishing, confirm the actual distribution build, platform, defaults, UI wording and provider privacy policy against the intended release. Source references for this brief: [DLP architecture](../architecture/security-audit.md), [tool observations](../architecture/tool-risk.md), [network behavior](../data-and-network.md), and the current [DLP rules](../../src-tauri/src/proxy/security/builtin_rules.rs), [tool-risk built-ins](../../src-tauri/src/services/tool_risk/builtin.rs) and [Security Center UI](../../src/components/security/SecurityCenterPage.tsx).
