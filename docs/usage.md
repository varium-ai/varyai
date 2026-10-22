# Using Vary AI

Vary AI manages client configuration and can route supported model traffic through a local gateway. Install the coding clients separately. Build instructions are in the [README](../README.md) and [platform guides](README.md).

## Providers

Select a client, add a provider, review its endpoint, credentials and models, then save. Select or enable the provider using that integration's controls. Some clients switch one active configuration; others maintain multiple explicitly configured providers. Reload the client when required.

Use your own credentials. A preset is a configuration starting point, not an endorsement, subscription or guarantee of access. See [provider setup and routing](guides/provider-routing.md) for API-format selection and verification.

## Routing and security

Local routing applies only to enabled, supported integrations. Enable the master listener and the relevant client routing switch when conversion or inspection is required. Back up client settings before first takeover and use the application's disable/recovery controls when restoring direct access.

The security center displays DLP events and tool-call risk observations and provides the applicable rules and settings. Review enablement, rule actions and coverage rather than assuming every request or every tool execution is inspected. DLP can redact selected request text; a failed safe-redaction check can block forwarding. Tool-risk observations provide warnings and records and do not authorize or sandbox tool execution.

The [DLP](architecture/security-audit.md) and [tool-risk](architecture/tool-risk.md) references describe current coverage and persistence limits. The [network behavior record](data-and-network.md) explains the application's separate statistics and update behavior.

## Configuration profiles

Capture selected client settings and resources, then apply or explicitly refresh a saved profile. Review the scope and warnings before replacing local configuration. See [configuration profiles](guides/profiles.md).

## MCP, prompts and Skills

Use the resources available for the selected client:

- MCP management writes supported server configuration. Review executable commands, arguments and environment values before enabling an imported server.
- Prompt management edits the supported client prompt or instruction files. Preserve unrelated instructions and review replacements before saving.
- Skills management installs or enables client-discoverable files. Review a repository's content before installation and keep a backup before migration or restore operations.

Availability differs by client; configuration support does not establish that an external server or skill is trustworthy. Do not publish exported configurations containing credentials.

## Sessions and local files

Session management reads supported local session formats and offers the actions available for that client. A visible conversation can include private source, prompts, file paths and tool outputs. Review content before copying it or reporting a parsing problem. A proposed tool call in a conversation is not proof that it was executed.

Workspace and client-specific configuration panels operate on local files. Review the target client and directory before saving changes. Back up files that are also edited by the client itself.

## Statistics, backups and updates

The statistics view summarizes recorded usage; coverage depends on the observed traffic and available usage fields. It is not the provider's billing ledger. Security event counts likewise depend on enabled rules and successful event collection.

Backups and synchronization can include credentials and private client configuration. Keep them private and use the existing restore/import workflow rather than manually replacing one database file. Vary AI uses a shared `cc-switch.db` and an extension `vary-ai.db`; see [storage and recovery](architecture/storage.md).

The application retains its configured update feed. A locally built package is distinct from an accepted official installer. See [release distribution](guides/release-distribution.md) and [data/network behavior](data-and-network.md) before distributing a fork or changing update configuration.

## Problems and reports

Start with [troubleshooting](troubleshooting.md). Include the application revision, platform, affected client, synthetic reproduction and sanitized diagnostics. Use the project's [security policy](../SECURITY.md) for private vulnerability reports; credentials and private conversations do not belong in public issues.
