# Local macOS builds and notarization

This guide describes the public source scripts. Company release orchestration, signing-account handover, upload destinations, and production credentials are maintained separately. The commands below do not install or launch the app, upload a release, or announce an official public download.

## Development packages

Use the toolchain described in the [README](../../README.md), install Xcode Command Line Tools, and run from the repository root:

```bash
pnpm install --frozen-lockfile
pnpm build:desktop
```

On macOS, the script prepares the Apple Silicon and Intel Rust targets and builds a universal `.app` and `.dmg`. Rustup may need to download the targets. Without an explicit `APPLE_SIGNING_IDENTITY`, it passes `--no-sign` to Tauri, does not enumerate Keychain identities, and does not select an organization or company certificate. These are development packages without Developer ID signing or notarization from this workflow.

Build output is under `src-tauri/target/universal-apple-darwin/release/bundle/`. If you need only compilation, without installer bundles, use the `--no-bundle` command in the README instead.

## Sign with your own identity

Use an authorized Developer ID Application identity already available in your own Keychain. Set its exact name explicitly; the following values are placeholders and must be replaced:

```bash
export APPLE_SIGNING_IDENTITY='Developer ID Application: Your Organization (YOURTEAMID)'
pnpm build:desktop
```

The script passes this identity to Tauri, verifies the resulting app signature, signs the DMG, and verifies the DMG. It does not search for a default identity. A signed build alone is not a notarized build.

Apple account and notarization API credentials are removed from the Tauri build subprocess, even if they exist in the caller's environment. Notarization uses the separate profile-based workflow below rather than being inferred from those variables.

## Explicit notarization

Notarization requires all three settings:

| Variable                  | Meaning                                                                                                                     |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `APPLE_SIGNING_IDENTITY`  | The exact Developer ID Application identity used to sign the app.                                                           |
| `APPLE_TEAM_ID`           | Its ten-character Apple developer team identifier. The signed app must report the same team.                                |
| `NOTARY_KEYCHAIN_PROFILE` | An existing notarytool Keychain profile that you have configured and authorized for that team. There is no default profile. |

Create and maintain your own notarytool profile using Apple's tooling outside these scripts. The scripts do not prompt for an Apple Account, create a profile, accept account-password arguments, or store credentials in the repository. Keep profile credentials in Keychain or your controlled private release environment.

Supply each setting exactly, without leading or trailing whitespace. The build and notarization entry points reject surrounding whitespace before building, signing, accessing Keychain, or making network requests; they do not silently trim configuration. App signature verification compares the complete signing authority name, rather than accepting a matching name prefix.

Replace the example identity, team, and profile with your own configuration:

```bash
export APPLE_SIGNING_IDENTITY='Developer ID Application: Your Organization (YOURTEAMID)'
export APPLE_TEAM_ID='YOURTEAMID'
export NOTARY_KEYCHAIN_PROFILE='your-notary-profile'

bash scripts/notarize-macos.sh --check-config
pnpm build:macos:notarized
```

`--check-config` checks required fields and their format without calling system tools, accessing Keychain, or making network requests. It does not prove that a certificate, permission, or profile is usable. The combined command runs that check before starting its build.

Equivalent explicit entry points are:

```bash
# Build, sign, then run the profile-based notarization workflow.
pnpm build:desktop -- --notarize

# Notarize an app that has already been built and signed with the selected identity.
pnpm notarize:macos
```

These commands contact Apple's notarization service and use the selected Keychain credentials. They validate the app's signature identity and team, check the existing profile, submit the app, staple and assess it, create and sign a DMG, then notarize and validate that DMG. The final DMG is placed under `src-tauri/target/universal-apple-darwin/release/bundle/notarized/`. An existing output file stops the workflow instead of being overwritten.

Missing or invalid required settings fail before signing, Keychain access, or network calls. A configured profile that cannot authenticate fails without interactive account setup. Raw notarytool responses and failure diagnostics are not echoed; inspect the submission privately with your own profile when troubleshooting.

## Updater artifacts and runtime behavior

Ordinary `pnpm build:desktop` uses [`tauri.no-updater.conf.json`](../../src-tauri/tauri.no-updater.conf.json) to disable updater artifact generation. `--updater` explicitly requests those artifacts and requires independently configured updater signing. This signing is separate from the Developer ID identity used for macOS packages.

Neither the no-updater configuration nor `--no-bundle` disables runtime update checks or changes the embedded update destination. Statistics and update behavior in the current development tree remain under [data and network review](../data-and-network.md); final public distribution and contact channels are still pending.
