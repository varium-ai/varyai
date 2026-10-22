# Local Linux container builds

Start Docker, then run from the repository root:

```bash
pnpm build:linux
pnpm build:linux --arch arm64
pnpm build:linux --arch x64
```

The default builds ARM64 and x64 sequentially, starting with the host architecture. AppImage, deb, rpm and SHA256SUMS output is written under the corresponding `src-tauri/target/<target>/release/bundle/` directory. The helper mounts source read-only and uses architecture-specific Docker caches; it does not launch the application or upload releases.

The Ubuntu 22.04 build image and container script are under `scripts/linux/`. First use downloads system dependencies and Rust/Node tools. Cross-architecture emulation is slower and can need substantial memory or swap; the helper checks Docker's available memory and prevents concurrent builds sharing an architecture cache.

Verify hashes in the output directory:

```bash
shasum -a 256 -c SHA256SUMS
```

A package build does not establish that a target desktop's graphics/webview dependencies are installed. The current helper has a known x64 AppImage limitation under Apple Silicon emulation: the bundled linuxdeploy patchelf can fail, and the manual system-patchelf workaround is not automated. Validate the intended platform and package separately before distribution.
