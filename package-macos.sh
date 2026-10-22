#!/usr/bin/env bash
# Local macOS universal packaging helper.
#
# Usage:
#   ./package-macos.sh [--skip-build] [--install]
#   ./package-macos.sh --help
#
# Builds local artifacts unless --skip-build is supplied.
# Applies an ad-hoc signature and recreates the DMG.
# Does not perform Apple notarization or publish a release.
#
# --install replaces /Applications/Vary AI.app after making a backup.
# Quit any existing instance before opening the installed version.
# No installation is performed without --install.
set -euo pipefail

APP_NAME="Vary AI"
INSTALL_PATH="/Applications/${APP_NAME}.app"
DO_INSTALL=0
SKIP_BUILD=0

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --install) DO_INSTALL=1 ;;
    --skip-build) SKIP_BUILD=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Error: this script supports macOS only." >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

if [ ! -f package.json ] || [ ! -f src-tauri/tauri.conf.json ]; then
  echo "Error: package.json or src-tauri/tauri.conf.json is missing." >&2
  echo "      Run this script from the repository root." >&2
  exit 1
fi

if ! command -v pnpm >/dev/null 2>&1; then
  echo "Error: install pnpm and make it available on PATH." >&2
  exit 1
fi

VERSION="$(node -p "require('./package.json').version")"
BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
APP_PATH="${BUNDLE_DIR}/macos/${APP_NAME}.app"
DMG_PATH="${BUNDLE_DIR}/dmg/${APP_NAME}_${VERSION}_universal.dmg"

if [ "$SKIP_BUILD" -eq 0 ]; then
  echo "==> Building ${APP_NAME} ${VERSION} (universal)"
  pnpm build:desktop
else
  echo "==> Skipping the build; using existing artifacts"
fi

if [ ! -d "$APP_PATH" ]; then
  echo "Error: application artifact not found: $APP_PATH" >&2
  exit 1
fi

echo "==> Applying an ad-hoc signature"
codesign --force --deep --sign - "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

echo "==> Checking architectures"
ARCHS="$(lipo -archs "$APP_PATH/Contents/MacOS/varyai")"
echo "    $ARCHS"
case "$ARCHS" in
  *arm64*x86_64*|*x86_64*arm64*) ;;
  *) echo "Warning: a universal arm64 + x86_64 binary was not detected." >&2 ;;
esac

echo "==> Recreating the DMG with the signed application"
mkdir -p "$(dirname "$DMG_PATH")"
hdiutil create -volname "$APP_NAME" -srcfolder "$APP_PATH" -ov -format UDZO "$DMG_PATH" >/dev/null
hdiutil verify "$DMG_PATH" >/dev/null
echo "    DMG verification passed"

echo "==> Artifacts"
echo "    App: $APP_PATH"
echo "    DMG: $DMG_PATH"
echo "    SHA-256: $(shasum -a 256 "$DMG_PATH" | awk '{print $1}')"
echo "    Note: local ad-hoc signature; not notarized by Apple."

if [ "$DO_INSTALL" -eq 1 ]; then
  STAMP="$(date +%Y%m%d-%H%M%S)"
  STAGED="/Applications/${APP_NAME}.app.installing-${STAMP}"
  BACKUP="/Applications/${APP_NAME}.app.backup-${STAMP}"

  echo "==> Installing to $INSTALL_PATH"
  ditto "$APP_PATH" "$STAGED"
  codesign --verify --deep --strict --verbose=2 "$STAGED"
  if [ -d "$INSTALL_PATH" ]; then
    mv "$INSTALL_PATH" "$BACKUP"
    echo "    Previous version backed up to: $BACKUP"
  fi
  if ! mv "$STAGED" "$INSTALL_PATH"; then
    [ -d "$BACKUP" ] && mv "$BACKUP" "$INSTALL_PATH"
    echo "Error: installation failed; rollback attempted." >&2
    exit 1
  fi
  codesign --verify --deep --strict --verbose=2 "$INSTALL_PATH"
  echo "    Installed: $INSTALL_PATH"

  if lsof -nP -iTCP:15721 -sTCP:LISTEN >/dev/null 2>&1; then
    echo "    Port 15721 is in use; an application instance may be running."
    echo "    Quit the existing instance before opening the newly installed version."
  fi
fi
