# Local Windows builds

The public application tree supports native Windows development and a macOS-to-Windows x64 cross-build helper. Neither requires the private release infrastructure. Follow the [source setup](../../README.md#develop-from-source) first. These are development build instructions; platform installer acceptance is tracked separately.

## Native Windows development

Install the repository's Node.js, pnpm, and Rust versions, the MSVC C++ build tools, and WebView2. From the repository root:

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

For a local desktop build without updater artifacts:

```powershell
pnpm build:desktop
```

This does not configure an official distribution destination or publish a release. Inspect the generated files under `src-tauri/target/release/` and validate the installer on a suitable Windows system before distributing it.

## Cross-build Windows x64 from macOS

The cross-build helper targets `x86_64-pc-windows-msvc` and produces an NSIS installer. It needs Homebrew and the usual source toolchain. Toolchain setup installs missing Homebrew packages, `cargo-xwin`, and the Rust target, so run it deliberately:

```bash
pnpm build:windows setup
pnpm build:windows
```

The build uses LLVM/LLD, NSIS, CMake, and Ninja, sets `STATIC_VCRUNTIME=true`, and strips Apple environment variables from the Windows build. It checks the resulting executable's import table and rejects external Visual C++ runtime DLL dependencies. The xwin cache defaults to `~/.cache/varyai/xwin`; `XWIN_CACHE_DIR` can override it.

`--updater` requests updater artifacts and requires an appropriate signing setup; ordinary local builds leave it off. The cross-build script does not install the Windows package, publish assets, upload to an internal service, or notify a company channel. Downloading dependencies and build toolchains still uses the network.

## Troubleshooting
