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

## In-app release notes

The update dialog displays Markdown notes for published releases newer than the
installed version and up to the target version, ordered newest first. It keeps
the installed version visible and supports headings, lists, inline code and external links,
scrolls the notes independently and keeps update actions fixed below them.
Installation and portable-download behavior remain unchanged. Release notes do
not execute HTML or load remote images. For example, updating from `0.3.0` to
`0.3.4` includes the `0.3.3` and `0.3.2` descriptions rather than showing only
`0.3.4`.

Only the section matching the interface language is displayed. Start translated
sections with `<!-- lang:zh -->`, `<!-- lang:zh-TW -->`, or `<!-- lang:ja -->`.
English can appear before the first marker or after `<!-- lang:en -->`. Regional
locales first use their exact translation, then the base language (for example,
Traditional Chinese falls back to `zh`), then English. Empty translations also
fall back. Keep headings and lists inside each language section. Descriptions
without markers remain supported as a single English fallback.

Each release has a version heading, and each list item displays its own source
release version in muted text. Keep each published description specific to that
release so attribution stays accurate. The list uses compact spacing, with version
labels following the item text and wrapping together when needed. Relative links
are resolved against the corresponding release page.

History is fetched independently from the public GitHub releases API. The range
excludes the installed version, drafts, newer releases, and prereleases for a
stable target. Semantic version ordering is used instead of publication order or
string ordering. Duplicate versions are removed, with the target manifest's
description taking precedence over the public description for that same version.
Requests are limited to eight seconds, five pages of 100 releases, and the latest
40 versions in the displayed range. An incomplete-history message is shown when
requests fail or those bounds omit records. History loading never blocks update
actions or the immediately available target manifest notes. If the installed
version is unknown, only the target notes are displayed.

Stage the public Markdown description as `release-assets/release-notes.md` before
running `scripts/create-updater-manifest.mjs`. The manifest builder embeds that
file in `latest.json`'s `notes` field. Programmatic callers can instead provide
`notes` or `notesFile`; the CLI accepts an explicit notes file after the platform
list:

```bash
node scripts/create-updater-manifest.mjs release-assets v1.2.3 owner/repo macos,windows,linux path/to/release-notes.md
```

The renderer reads Tauri's `Update.body` property, which contains the manifest's
`notes`. If an older manifest omits the notes or contains the former GitHub-page
placeholder, it makes a bounded, unauthenticated request for the exact target
release's GitHub description. Failure to fetch this optional description never
blocks the update; the version-specific release-page link remains available.
Keep the public release description and manifest notes consistent when publishing.
