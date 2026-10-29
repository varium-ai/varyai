use super::*;
use std::io::{Read, Write};
use std::process::Command;

fn checked(command: &mut Command, error: &str) -> Result<std::process::Output, String> {
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .ok_or_else(|| error.to_owned())
}

fn bundle_field(bundle: &Path, key: &str) -> Result<String, String> {
    let output = checked(
        Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist")),
        "identity",
    )?;
    let value = String::from_utf8(output.stdout).map_err(|_| "identity".to_owned())?;
    let value = value.trim();
    if value.is_empty() {
        return Err("identity".to_owned());
    }
    Ok(value.to_owned())
}

fn signing_team(bundle: &Path) -> Result<String, String> {
    let output = checked(
        Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=4"])
            .arg(bundle),
        "signature",
    )?;
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| team.len() == 10 && team.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_owned)
        .ok_or_else(|| "signature".to_owned())
}

fn installed_bundle(app: &str) -> Result<PathBuf, String> {
    for root in [
        PathBuf::from("/Applications"),
        crate::config::get_home_dir().join("Applications"),
    ] {
        for name in agent_desktop_bundles(app) {
            let path = root.join(name);
            if path.is_dir() && !path.is_symlink() {
                return Ok(path);
            }
        }
    }
    Err("notInstalled".to_owned())
}

async fn download(
    client: &reqwest::Client,
    app: &str,
    release: &Release,
    path: &Path,
) -> Result<(), String> {
    let mut response = client
        .get(release.url.clone())
        .timeout(std::time::Duration::from_secs(15 * 60))
        .send()
        .await
        .map_err(|_| "download".to_owned())?
        .error_for_status()
        .map_err(|_| "download".to_owned())?;
    if trusted_url(app, response.url().as_str()).is_none() {
        return Err("download".to_owned());
    }
    let mut file = std::fs::File::create(path).map_err(|_| "download".to_owned())?;
    let mut sha512 = Sha512::new();
    let mut sha256 = Sha256::new();
    let mut size = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|_| "download".to_owned())? {
        size += chunk.len();
        if size > 4 * 1024 * 1024 * 1024usize {
            return Err("download".to_owned());
        }
        sha512.update(&chunk);
        sha256.update(&chunk);
        file.write_all(&chunk).map_err(|_| "download".to_owned())?;
    }
    file.sync_all().map_err(|_| "download".to_owned())?;
    if size == 0 || !verify_digests(&sha512.finalize(), &sha256.finalize(), release) {
        return Err("checksum".to_owned());
    }
    Ok(())
}

fn validate_zip(path: &Path) -> Result<(), String> {
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(path).map_err(|_| "archive".to_owned())?)
            .map_err(|_| "archive".to_owned())?;
    let mut entries = Vec::new();
    let mut links = Vec::new();
    let mut total_size = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|_| "archive".to_owned())?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| "archive".to_owned())?
            .to_path_buf();
        total_size = total_size.saturating_add(file.size());
        if total_size > 8 * 1024 * 1024 * 1024 {
            return Err("archive".to_owned());
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            if file.size() > 4096 {
                return Err("archive".to_owned());
            }
            let mut target = String::new();
            file.read_to_string(&mut target)
                .map_err(|_| "archive".to_owned())?;
            let mut depth = name
                .parent()
                .map_or(0, |parent| parent.components().count());
            for part in Path::new(&target).components() {
                match part {
                    std::path::Component::ParentDir if depth > 0 => depth -= 1,
                    std::path::Component::Normal(_) => depth += 1,
                    std::path::Component::CurDir => {}
                    _ => return Err("archive".to_owned()),
                }
            }
            links.push(name.clone());
        }
        entries.push(name);
    }
    // No extraction may write through an archive-provided symlink.
    if entries.iter().any(|entry| {
        links
            .iter()
            .any(|link| entry != link && entry.starts_with(link))
    }) {
        return Err("archive".to_owned());
    }
    Ok(())
}

struct MountedImage(PathBuf);
impl Drop for MountedImage {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.0)
            .output();
    }
}

fn unpack(
    package: &Path,
    root: &Path,
    is_zip: bool,
) -> Result<(PathBuf, Option<MountedImage>), String> {
    let out = root.join("unpacked");
    std::fs::create_dir(&out).map_err(|_| "archive".to_owned())?;
    if is_zip {
        validate_zip(package)?;
        checked(
            Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(package)
                .arg(&out),
            "archive",
        )?;
        Ok((out, None))
    } else {
        checked(
            Command::new("/usr/bin/hdiutil")
                .args(["attach", "-readonly", "-nobrowse", "-quiet", "-mountpoint"])
                .arg(&out)
                .arg(package),
            "archive",
        )?;
        Ok((out.clone(), Some(MountedImage(out))))
    }
}

fn staged_app(root: &Path, id: &str) -> Result<PathBuf, String> {
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|_| "archive".to_owned())? {
        let entry = entry.map_err(|_| "archive".to_owned())?;
        if !entry
            .file_type()
            .map_err(|_| "archive".to_owned())?
            .is_dir()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "app")
            && bundle_field(&path, "CFBundleIdentifier").ok().as_deref() == Some(id)
        {
            matches.push(path);
        }
    }
    if matches.len() != 1 {
        return Err("identity".to_owned());
    }
    Ok(matches.remove(0))
}

fn verify_app(path: &Path, id: &str, team: &str, version: &str) -> Result<(), String> {
    if bundle_field(path, "CFBundleIdentifier")? != id
        || agent_versions::version_number(&bundle_field(path, "CFBundleShortVersionString")?)
            .as_deref()
            != Some(version)
        || signing_team(path)? != team
    {
        return Err("identity".to_owned());
    }
    checked(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(path),
        "signature",
    )?;
    let executable = bundle_field(path, "CFBundleExecutable")?;
    if Path::new(&executable).components().count() != 1 {
        return Err("identity".to_owned());
    }
    let cpu = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    checked(
        Command::new("/usr/bin/lipo")
            .arg(path.join("Contents/MacOS").join(executable))
            .args(["-verify_arch", cpu]),
        "architecture",
    )?;
    Ok(())
}

fn quit_running(id: &str, target: &Path) -> Result<bool, String> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    let running =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
    let apps: Vec<_> = running
        .iter()
        .filter(|app| {
            app.bundleURL()
                .and_then(|url| url.path())
                .is_some_and(|path| Path::new(&path.to_string()) == target)
        })
        .collect();
    for app in &apps {
        app.terminate();
    }
    let start = std::time::Instant::now();
    while apps.iter().any(|app| !app.isTerminated()) {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            return Err("running".to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(!apps.is_empty())
}

fn replace_with_rollback(
    target: &Path,
    staged: &Path,
    backup: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    rename(target, backup).map_err(|_| "replace".to_owned())?;
    if rename(staged, target).is_err() {
        return if rename(backup, target).is_ok() {
            Err("replace".to_owned())
        } else {
            Err("restore".to_owned())
        };
    }
    Ok(())
}

pub(super) fn update(runtime: &tokio::runtime::Handle, app: &str) -> Result<(), String> {
    let target = installed_bundle(app)?;
    let id = bundle_field(&target, "CFBundleIdentifier")?;
    let team = signing_team(&target)?;
    let version =
        agent_versions::version_number(&bundle_field(&target, "CFBundleShortVersionString")?)
            .ok_or_else(|| "identity".to_owned())?;
    let client = crate::proxy::http_client::get();
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let release = runtime
        .block_on(latest(&client, app, arch))
        .ok_or_else(|| "unavailable".to_owned())?;
    if compare_semver(&release.version, &version) != Some(std::cmp::Ordering::Greater) {
        return Ok(());
    }
    let temp = tempfile::tempdir().map_err(|_| "download".to_owned())?;
    let package = temp.path().join("package");
    runtime.block_on(download(&client, app, &release, &package))?;
    let (unpacked, _mount) = unpack(&package, temp.path(), release.url.path().ends_with(".zip"))?;
    let candidate = staged_app(&unpacked, &id)?;
    verify_app(&candidate, &id, &team, &release.version)?;
    let parent = target.parent().ok_or_else(|| "replace".to_owned())?;
    let stage = tempfile::Builder::new()
        .prefix(".varyai-agent-update-")
        .tempdir_in(parent)
        .map_err(|_| "replace".to_owned())?;
    let staged = stage.path().join("updated.app");
    let backup = stage.path().join("previous.app");
    checked(
        Command::new("/usr/bin/ditto").arg(&candidate).arg(&staged),
        "replace",
    )?;
    verify_app(&staged, &id, &team, &release.version)?;
    // Recheck identity immediately before replacing an installation, after all downloads.
    if agent_versions::version_number(&bundle_field(&target, "CFBundleShortVersionString")?)
        .as_deref()
        != Some(version.as_str())
        || signing_team(&target)? != team
    {
        return Err("changed".to_owned());
    }
    let was_running = quit_running(&id, &target)?;
    let result = replace_with_rollback(&target, &staged, &backup, |from, to| {
        std::fs::rename(from, to)
    });
    if result.as_ref().is_err_and(|error| error == "restore") {
        let _ = stage.keep();
        return result;
    }
    if was_running {
        let _ = Command::new("/usr/bin/open").arg(&target).output();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_failure_restores_original_and_success_replaces_it() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("app");
        let staged = root.path().join("staged");
        let backup = root.path().join("backup");
        std::fs::write(&target, "original").unwrap();
        std::fs::write(&staged, "updated").unwrap();
        let result = replace_with_rollback(&target, &staged, &backup, |from, to| {
            if from == staged {
                return Err(std::io::Error::other("synthetic failure"));
            }
            std::fs::rename(from, to)
        });
        assert_eq!(result, Err("replace".into()));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
        assert!(!backup.exists());
        replace_with_rollback(&target, &staged, &backup, |from, to| {
            std::fs::rename(from, to)
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "updated");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "original");
    }

    #[test]
    fn archive_path_escape_is_rejected_before_extraction() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("../outside", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"synthetic").unwrap();
        zip.finish().unwrap();
        assert_eq!(validate_zip(&path), Err("archive".into()));
    }

    #[test]
    fn failed_recovery_keeps_the_previous_app_available() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("app");
        let staged = root.path().join("staged");
        let backup = root.path().join("backup");
        std::fs::write(&target, "previous app").unwrap();
        let result = replace_with_rollback(&target, &staged, &backup, |from, to| {
            if from == target {
                std::fs::rename(from, to)
            } else {
                Err(std::io::Error::other("synthetic failure"))
            }
        });
        assert_eq!(result, Err("restore".into()));
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "previous app");
    }

    #[test]
    fn archives_cannot_write_through_symlinks_or_escape_via_link_targets() {
        for escaping in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("package.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.add_symlink(
                "link",
                if escaping { "../outside" } else { "inside" },
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            if !escaping {
                zip.start_file("link/file", zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"synthetic").unwrap();
            }
            zip.finish().unwrap();
            assert_eq!(validate_zip(&path), Err("archive".into()));
        }
    }

    #[test]
    fn bundle_identity_and_signature_are_checked_before_replacement() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("Synthetic.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>test.synthetic</string><key>CFBundleShortVersionString</key><string>1.2.3</string></dict></plist>"#).unwrap();
        assert_eq!(
            verify_app(&app, "other.app", "SYNTHETIC1", "1.2.3"),
            Err("identity".into())
        );
        assert_eq!(
            verify_app(&app, "test.synthetic", "SYNTHETIC1", "2.0.0"),
            Err("identity".into())
        );
        assert_eq!(
            verify_app(&app, "test.synthetic", "SYNTHETIC1", "1.2.3"),
            Err("signature".into())
        );
    }

    #[tokio::test]
    #[ignore = "Explicit publisher download acceptance; never installs or launches an application"]
    async fn official_package_can_be_staged_without_installing() {
        let client = crate::proxy::http_client::get();
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        for (app, id, team) in [
            ("qoder", "com.qodercn.app", "9DFNGU9AK5"),
            ("doubaowork", "com.work.pc.doubao", "96L78H6LMH"),
        ] {
            let release = latest(&client, app, arch)
                .await
                .expect("public release metadata");
            let temp = tempfile::tempdir().unwrap();
            let package = temp.path().join("package");
            download(&client, app, &release, &package)
                .await
                .expect("public artifact download");
            let (unpacked, _mount) =
                unpack(&package, temp.path(), release.url.path().ends_with(".zip"))
                    .expect("safe extraction");
            let candidate = staged_app(&unpacked, id).expect("expected product identity");
            verify_app(&candidate, id, team, &release.version)
                .expect("official signature, version and architecture");
        }
    }
}
