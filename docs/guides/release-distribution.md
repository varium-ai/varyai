# Builds and release distribution

## Repository responsibilities

The application repository contains product code, issues, pull requests, tests, source tags, and development documentation. Release artifacts must identify the source revision from this repository.

A separate private infrastructure repository holds production release orchestration, company environment configuration, upload and notification adapters, and internal operations documentation. Credentials are injected by the release environment; they are never checked into either repository.

Official installers and updater artifacts are published to [varium-ai/varyai](https://github.com/varium-ai/varyai/releases). Every release must explicitly identify its public application source repository, tag, full commit SHA, and artifact hashes. Internally, also record the fixed private tool SHA and build receipts.

## Development builds

Developers can use public build tools without the private release repository. Start with the [README](../../README.md) and the platform guides:

- [macOS local builds and optional notarization](local-macos-build.md)
- [Windows native and macOS cross builds](local-windows-build.md)
- [Linux local container builds](local-linux-build.md)

Development builds and optional self-signing do not constitute an official VaryAI release. Installing or notarizing an application is a separate deliberate action. The local scripts do not publish a release or send a company notification.

## Traceable official releases

1. Review and merge the source change in the public application repository. Align the application version files and complete the documented checks.
2. Select a reviewed source tag and record its full commit SHA. Fix the private tool revision independently; do not resolve a source SHA against the tool repository.
3. Build the intended platforms from that same source revision in a controlled environment. Production credentials are reserved for trusted source revisions.
4. Verify packages, signatures, updater artifacts, hashes, and source receipts. Record actual platform acceptance; a successful unit test is not an installer test.
5. Publish packages before the updater manifest, and verify that every manifest entry points to the exact corresponding artifact. Avoid concurrent release systems writing the same version.
6. Publish source links and hashes with the release notes. Retain enough internal evidence to reproduce the source/tool selection and investigate a failed release.

Environment-specific operations are documented with the private release infrastructure. Validate platform installers, signatures, and update behavior separately from source and unit-test checks.

## Update compatibility

The application checks `https://github.com/varium-ai/varyai/releases/latest/download/latest.json`, configured in `src-tauri/tauri.conf.json`. The manual update command opens the same repository's latest release page. Its updater public key remains unchanged and is a verification key; it is safe and necessary to distribute with the application. The matching private signing key stays in the release environment.

Clients built with the previous `secure-typex/varyai-releases` endpoint continue checking that address. Keep a migration release available there, signed with the key trusted by those clients, or direct users to manually install a release from the new repository. Changing this source configuration alone does not migrate installed clients.

Before changing the distribution repository, endpoint, signing key, or artifact naming, plan and test the update path for already installed clients. Do not assume older clients will automatically discover a new endpoint or public key.

`src-tauri/tauri.no-updater.conf.json` disables updater artifact generation. It does not disable the application's runtime update checks or substitute a different update endpoint. Fork maintainers must review both runtime configuration and artifact signing before distributing their own packages.
