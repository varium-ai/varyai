# Security Policy

## Reporting vulnerabilities

Report vulnerabilities through **[GitHub Private Vulnerability Reporting](https://github.com/varium-ai/varyai/security/advisories/new)** in `varium-ai/varyai`. This channel is enabled and sends reports privately to the repository's maintainers. You can also find it under **Security → Advisories → Report a vulnerability**.

Sign in to GitHub to submit a report. If the form is temporarily unavailable, wait until private reporting is available; do not put vulnerability details in a public issue. No security email address is currently published.

Do not publish usable credentials, private conversations, or an exploit against a live service. Do not send VaryAI-specific reports to CC Switch maintainers.

## What to report

VaryAI is a desktop application with a local HTTP gateway and native capabilities. It handles provider credentials, local configuration, imported data, and model traffic. Local execution does not make all input trusted.

Reports may concern credential disclosure, unsafe request forwarding, unauthorized access through a configured listener, untrusted deep-link or sync payloads, unsafe file access, renderer-to-native privilege crossings, or update verification. Include the untrusted input source and the complete path to the affected behavior. Existing local access or user interaction should be stated as part of the prerequisites, rather than hidden.

This policy does not claim that the renderer, proxy, or other component has passed an independent security audit. Changes in trust boundaries must be assessed against the current code.

## Report contents

Provide:

- Affected VaryAI version or full commit SHA, operating system, and relevant configuration.
- A minimal reproduction using synthetic accounts, keys, and data.
- Expected and actual behavior, prerequisites, and potential impact.
- Redacted logs or screenshots and any suggested mitigation.

Do not include production secrets. If a real credential has been exposed, arrange revocation or rotation with its owner; deleting it from a file or rewriting Git history does not revoke it.

## Supported versions and disclosure

Include the affected release or source commit in every report, including reports against `main`. VaryAI does not currently offer a long-term support policy, guaranteed backports, or a fixed response-time commitment. CC Switch's release numbers and support commitments do not apply to VaryAI.

For a confirmed report, coordinate reproduction, remediation, release information, and disclosure with the reporter. Attribution should follow the reporter's preference. Any future security advisory must identify the affected VaryAI versions or commits and available mitigation.
