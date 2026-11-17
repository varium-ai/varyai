//! Pi's official extension hook supplies native session identity to local routes only.
use std::{fs, path::Path};

const SOURCE: &str = include_str!("../../resources/pi-session-id.mjs");

pub(crate) fn ensure_installed() {
    // Tests may install only into an explicitly isolated Pi directory.
    #[cfg(test)]
    if !super::TEST_AGENT_DIR
        .lock()
        .is_ok_and(|directory| directory.is_some())
    {
        return;
    }
    if super::get_pi_agent_dir()
        .map_err(|_| ())
        .and_then(|directory| install_at(&directory))
        .is_err()
    {
        log::warn!("Pi session identity extension remains unavailable");
    }
}

fn install_at(directory: &Path) -> Result<(), ()> {
    let extensions = directory.join("extensions");
    if let Ok(metadata) = fs::symlink_metadata(&extensions) {
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(());
        }
    }
    let path = extensions.join("varyai-session-id.js");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.is_symlink() || !metadata.is_file() || metadata.len() != SOURCE.len() as u64
            {
                return Err(());
            }
            return (fs::read(&path).map_err(|_| ())? == SOURCE.as_bytes())
                .then_some(())
                .ok_or(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(()),
    }
    crate::config::atomic_write_private(&path, SOURCE.as_bytes()).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_an_idempotent_extension_without_changing_existing_configuration() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("settings.json"),
            "synthetic untouched configuration",
        )
        .unwrap();
        install_at(directory.path()).unwrap();
        install_at(directory.path()).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("extensions/varyai-session-id.js")).unwrap(),
            SOURCE
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("settings.json")).unwrap(),
            "synthetic untouched configuration"
        );
        fs::write(
            directory.path().join("extensions/varyai-session-id.js"),
            "user-owned extension",
        )
        .unwrap();
        assert!(install_at(directory.path()).is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("extensions/varyai-session-id.js")).unwrap(),
            "user-owned extension"
        );
    }
}
