#!/usr/bin/env bash
# Run the existing desktop build, then notarize its macOS artifacts.
set +x
set -euo pipefail

if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then
  echo 'Usage: ./scripts/build-and-notarize-macos.sh'
  exit 0
fi
if [ "$#" -ne 0 ]; then
  echo "Unexpected arguments. Use --help for usage." >&2
  exit 2
fi
if [ "$(uname -s)" != Darwin ]; then
  echo "This script requires macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Check every required setting before the build can sign or contact a service.
bash "$repo_root/scripts/notarize-macos.sh" --check-config

# The notarization script uses notarytool's local keychain profile. Keep Tauri
# from attempting a separate notarization during the build.
env -u APPLE_ID -u APPLE_PASSWORD -u APPLE_API_KEY -u APPLE_API_KEY_PATH \
  -u APPLE_API_ISSUER pnpm build:desktop
exec bash "$repo_root/scripts/notarize-macos.sh"
