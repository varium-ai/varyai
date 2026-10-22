#!/usr/bin/env bash
# Notarize a signed macOS app, then create and notarize a DMG locally.
set +x
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/notarize-macos.sh [--check-config]

Run pnpm build:desktop first. The app must have a Developer ID Application signature.
Explicitly set APPLE_SIGNING_IDENTITY, APPLE_TEAM_ID, and NOTARY_KEYCHAIN_PROFILE.
The Keychain profile must already exist; this script never provisions credentials.
--check-config validates configuration without Keychain, signing, or network operations.
EOF
}

case "${1:-}" in
  "") ;;
  --check-config) ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then usage >&2; exit 2; fi

for variable_name in APPLE_SIGNING_IDENTITY APPLE_TEAM_ID NOTARY_KEYCHAIN_PROFILE; do
  value="${!variable_name:-}"
  if [ -z "${value//[[:space:]]/}" ] ||
    [[ "$value" == [[:space:]]* || "$value" == *[[:space:]] || "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo "$variable_name must be explicitly configured without surrounding whitespace for notarization." >&2
    exit 1
  fi
done
case "$APPLE_SIGNING_IDENTITY" in
  "Developer ID Application: "?*) ;;
  *) echo "APPLE_SIGNING_IDENTITY must name a Developer ID Application identity." >&2; exit 1 ;;
esac
if [[ ! "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]]; then
  echo "APPLE_TEAM_ID must be a ten-character Apple developer team identifier." >&2
  exit 1
fi
if [ "${1:-}" = --check-config ]; then exit 0; fi

if [ "$(uname -s)" != Darwin ]; then
  echo "This script requires macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
for command_name in node codesign security ditto hdiutil xcrun spctl shasum; do
  command -v "$command_name" >/dev/null || { echo "Missing command: $command_name" >&2; exit 1; }
done
version="$(node -p "require('./package.json').version")"
bundle_dir="src-tauri/target/universal-apple-darwin/release/bundle"
app_path="$bundle_dir/macos/Vary AI.app"
output_dir="$bundle_dir/notarized"
output_path="$output_dir/Vary AI_${version}_universal.dmg"
if [ -e "$output_path" ]; then
  echo "Output already exists: $output_path (move it aside before retrying)." >&2
  exit 1
fi

test -d "$app_path" || { echo "App not found: $app_path" >&2; exit 1; }
if ! codesign --verify --deep --strict "$app_path" >/dev/null 2>&1; then
  echo "Wait for pnpm build:desktop to finish successfully before notarizing." >&2
  exit 1
fi
if ! signature_info="$(codesign -dv --verbose=4 "$app_path" 2>&1)"; then
  echo "Could not inspect the app signature." >&2
  exit 1
fi
signing_identity="$(sed -n 's/^Authority=\(Developer ID Application:.*\)$/\1/p' <<<"$signature_info" | head -n 1)"
team_id="$(sed -n 's/^TeamIdentifier=//p' <<<"$signature_info" | head -n 1)"
if [ "$signing_identity" != "$APPLE_SIGNING_IDENTITY" ] || [ "$team_id" != "$APPLE_TEAM_ID" ]; then
  echo "The app signature must match APPLE_SIGNING_IDENTITY and APPLE_TEAM_ID." >&2
  exit 1
fi
if ! available_identities="$(security find-identity -v -p codesigning 2>/dev/null)" ||
  ! grep -Fq "\"$signing_identity\"" <<<"$available_identities"; then
  echo "The app signing identity is not available in the keychain for DMG signing." >&2
  exit 1
fi
notary_profile="$NOTARY_KEYCHAIN_PROFILE"
if ! xcrun notarytool history --keychain-profile "$notary_profile" >/dev/null 2>&1; then
  echo "The configured notarization profile is unavailable or could not authenticate. Check it privately before retrying." >&2
  exit 1
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/varyai-notarize.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
dmg_path="$work_dir/Vary AI_${version}_universal.dmg"

notarize() {
  local artifact="$1" result
  if ! result="$(xcrun notarytool submit "$artifact" \
    --keychain-profile "$notary_profile" --wait --timeout 30m \
    --output-format json 2>/dev/null)"; then
    echo "Notarization request failed; inspect the submission privately with the configured profile." >&2
    exit 1
  fi
  if ! printf '%s' "$result" | node -e 'let s="";process.stdin.on("data",c=>s+=c).on("end",()=>{try{if(JSON.parse(s).status!=="Accepted")process.exitCode=1;}catch{process.exitCode=1;}})' >/dev/null 2>&1; then
    echo "Notarization was not accepted; inspect the submission privately with the configured profile." >&2
    exit 1
  fi
  echo "    Notarization accepted"
}

echo "==> Notarizing app"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$work_dir/Vary AI.zip"
notarize "$work_dir/Vary AI.zip"
xcrun stapler staple "$app_path"
xcrun stapler validate "$app_path"
spctl --assess --type execute --verbose=4 "$app_path"

echo "==> Creating DMG with stapled app"
mkdir -p "$work_dir/dmg"
ditto "$app_path" "$work_dir/dmg/Vary AI.app"
ln -s /Applications "$work_dir/dmg/Applications"
hdiutil create -volname "Vary AI" -srcfolder "$work_dir/dmg" -format UDZO -ov "$dmg_path" >/dev/null
codesign --force --sign "$signing_identity" --timestamp "$dmg_path"
codesign --verify --verbose=2 "$dmg_path"

echo "==> Notarizing DMG"
notarize "$dmg_path"
xcrun stapler staple "$dmg_path"
xcrun stapler validate "$dmg_path"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_path"
hdiutil verify "$dmg_path" >/dev/null

mkdir -p "$output_dir"
mv "$dmg_path" "$output_path"
echo "Ready: $output_path"
echo "SHA-256: $(shasum -a 256 "$output_path" | awk '{print $1}')"
