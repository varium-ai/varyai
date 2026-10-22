//! Give `tauri dev` a real bundle identity without losing hot reload or its PID.
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn reexec_if_needed(config: &tauri::Config) -> io::Result<()> {
    if !tauri::is_dev() {
        return Ok(());
    }
    let source = std::env::current_exe()?;
    if app_for_executable(&source).is_some() {
        return Ok(());
    }
    let executable = prepare_bundle(
        &source,
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.icns"),
    )?;
    let bundle = app_for_executable(&executable)
        .ok_or_else(|| io::Error::other("invalid development app layout"))?;
    // A freshly compiled executable needs a fresh ad-hoc signature. No installed
    // app is changed, and no certificate or notarization credentials are needed.
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(&bundle)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !signed.success() {
        return Err(io::Error::other("development app signing failed"));
    }
    // exec preserves the PID watched by Tauri, inherited environment, working
    // directory, stdout/stderr and app arguments. It creates no second gateway.
    use std::os::unix::process::CommandExt;
    Err(Command::new(executable)
        .args(std::env::args_os().skip(1))
        .exec())
}

fn app_for_executable(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn prepare_bundle(source: &Path, config: &tauri::Config, icon: &Path) -> io::Result<PathBuf> {
    let parent = source
        .parent()
        .ok_or_else(|| io::Error::other("missing executable directory"))?;
    let name = config.product_name.as_deref().unwrap_or("Vary AI");
    if name.contains('/') || name.contains('\0') {
        return Err(io::Error::other("invalid development app name"));
    }
    let binary = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("invalid development executable name"))?;
    let contents = parent
        .join("bundle/dev")
        .join(format!("{name}.app"))
        .join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    let executable = macos.join(binary);
    // Replace the file rather than truncate an image that may still be running.
    let staged = tempfile::NamedTempFile::new_in(&macos)?;
    fs::copy(source, staged.path())?;
    staged.persist(&executable).map_err(|error| error.error)?;
    write_atomic(&resources.join("icon.icns"), &fs::read(icon)?)?;
    let name = xml(name);
    let identifier = xml(&config.identifier);
    let binary = xml(binary);
    let version = xml(env!("CARGO_PKG_VERSION"));
    write_atomic(&contents.join("Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleDisplayName</key><string>{name}</string>
<key>CFBundleExecutable</key><string>{binary}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>"#).as_bytes())?;
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_executables_inside_an_app_and_prevents_reexec_loops() {
        assert_eq!(
            app_for_executable(Path::new("/tmp/Vary AI.app/Contents/MacOS/varyai")),
            Some(PathBuf::from("/tmp/Vary AI.app"))
        );
        assert_eq!(
            app_for_executable(Path::new("/tmp/target/debug/varyai")),
            None
        );
        assert_eq!(
            app_for_executable(Path::new("/tmp/Vary AI.app/varyai")),
            None
        );
    }

    #[test]
    fn builds_matching_identity_and_atomically_refreshes_binary_without_touching_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("varyai");
        let icon = temp.path().join("icon.icns");
        std::fs::write(&source, b"old binary").unwrap();
        std::fs::write(&icon, b"icon").unwrap();
        let config: tauri::Config = serde_json::from_str(r#"{"identifier":"com.varyai.desktop","productName":"Vary AI & Dev","version":"0.2.4"}"#).unwrap();
        let executable = prepare_bundle(&source, &config, &icon).unwrap();
        let bundle = app_for_executable(&executable).unwrap();
        let plist = bundle.join("Contents/Info.plist");
        let output = std::process::Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            config.identifier
        );
        let output = std::process::Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleName", "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "Vary AI & Dev"
        );
        assert_eq!(
            std::fs::read(bundle.join("Contents/Resources/icon.icns")).unwrap(),
            b"icon"
        );
        // An existing running image keeps its inode while the next build is staged.
        let mut old_image = std::fs::File::open(&executable).unwrap();
        std::fs::write(&source, b"new binary").unwrap();
        assert_eq!(prepare_bundle(&source, &config, &icon).unwrap(), executable);
        assert_eq!(std::fs::read(&executable).unwrap(), b"new binary");
        assert_eq!(std::fs::read(&source).unwrap(), b"new binary");
        let mut old_bytes = Vec::new();
        std::io::Read::read_to_end(&mut old_image, &mut old_bytes).unwrap();
        assert_eq!(old_bytes, b"old binary");
    }
}
