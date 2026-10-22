#!/usr/bin/env bash
set -euo pipefail

mkdir -p /work/source
# Keep Linux dependencies and compilation caches separate from macOS files.
rsync -a --delete \
  --exclude '/.git/' --exclude '/node_modules/' --exclude '/src-tauri/target/' \
  --exclude '/dist/' --exclude '/release/' --exclude '/release-assets/' \
  --exclude '/.env*' --exclude '/.npmrc' \
  /input/ /work/source/
cd /work/source
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=1024}"

printf 'Linux build environment: %s\n' "$(uname -m)"
node --version
pnpm --version
cargo --version
pnpm install --frozen-lockfile
rm -rf src-tauri/target/release/bundle
pnpm build:desktop

bundle=src-tauri/target/release/bundle
packages=()
for spec in 'appimage:AppImage' 'deb:deb' 'rpm:rpm'; do
  folder="${spec%%:*}"
  extension="${spec#*:}"
  matches=("$bundle/$folder/"*".$extension")
  if [ "${#matches[@]}" -ne 1 ] || [ ! -s "${matches[0]}" ]; then
    printf 'Expected one nonempty %s package in %s\n' "$extension" "$bundle/$folder" >&2
    exit 1
  fi
  name="$(basename "${matches[0]}")"
  mkdir -p "/output/$folder"
  cp "${matches[0]}" "/output/$folder/$name"
  packages+=("$folder/$name")
done
cd /output
sha256sum "${packages[@]}" > SHA256SUMS
cat SHA256SUMS
