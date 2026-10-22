#!/usr/bin/env bash
# Build Linux installers locally in Docker; no GitHub publication or updater keys.
set -euo pipefail

arch=all
while [ "$#" -gt 0 ]; do
  case "$1" in
    --arch)
      if [ "$#" -lt 2 ]; then echo '--arch requires x64, arm64 or all' >&2; exit 2; fi
      arch="$2"; shift 2 ;;
    --) shift ;;
    --help|-h)
      echo 'Usage: bash scripts/build-linux-local.sh [--arch x64|arm64|all]'
      echo 'Outputs: src-tauri/target/{x86_64,aarch64}-unknown-linux-gnu/release/bundle'
      echo 'Formats: appimage/, deb/, rpm/ and SHA256SUMS'
      exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
case "$arch" in x64|arm64|all) ;; *) echo 'Architecture must be x64, arm64 or all' >&2; exit 2 ;; esac
command -v docker >/dev/null || { echo 'Install and start Docker before building.' >&2; exit 1; }
docker info >/dev/null
docker_memory="$(docker info --format '{{.MemTotal}}')"
if [ "$docker_memory" -lt 4294967296 ]; then
  echo 'Docker has insufficient memory for the GTK/Rust build. Allocate 12 GB RAM, or 6 GB RAM plus 8 GB swap, to Docker or Colima.' >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
node_version="$(tr -d '[:space:]' < "$repo_root/.node-version")"
if [[ ! "$node_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then echo 'Invalid .node-version' >&2; exit 1; fi

if [ "$arch" = all ]; then
  # Build the host architecture first, then the emulated one.
  if [ "$(uname -m)" = arm64 ] || [ "$(uname -m)" = aarch64 ]; then targets=(arm64 x64); else targets=(x64 arm64); fi
else targets=("$arch"); fi

for target in "${targets[@]}"; do
  platform=linux/arm64
  rust_target=aarch64-unknown-linux-gnu
  if [ "$target" = x64 ]; then
    platform=linux/amd64
    rust_target=x86_64-unknown-linux-gnu
  fi
  image="varyai-linux-builder:$target"
  output_dir="$repo_root/src-tauri/target/$rust_target/release/bundle"
  mkdir -p "$output_dir"
  printf '\nBuilding Linux %s installers\n' "$target"
  docker build --platform "$platform" --build-arg "NODE_VERSION=$node_version" \
    --tag "$image" "$repo_root/scripts/linux"
  build_env=(--env CARGO_BUILD_JOBS=2 --env NODE_OPTIONS=--max-old-space-size=1024)
  if [ "$target" = x64 ]; then
    # Avoid QEMU's intermittent GCC/LLD crash when emulating x86_64 on ARM hosts.
    build_env+=(--env CARGO_BUILD_JOBS=1 --env CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/x86_64-linux-gnu-gcc)
  fi
  docker run --rm --name "varyai-linux-build-$target" --platform "$platform" \
    "${build_env[@]}" \
    --mount "type=bind,src=$repo_root,dst=/input,readonly" \
    --mount "type=bind,src=$output_dir,dst=/output" \
    --mount "type=volume,src=varyai-linux-$target-work,dst=/work" \
    --mount 'type=volume,src=varyai-linux-cargo-registry,dst=/opt/cargo/registry' \
    --mount 'type=volume,src=varyai-linux-cargo-git,dst=/opt/cargo/git' \
    "$image"
  printf 'Linux %s packages: %s\n' "$target" "$output_dir"
done
