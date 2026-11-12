//! A user projection must not override an administrator's cloud transport policy.
use super::{read, Format, CONFLICT};
use std::path::Path;

pub(super) const CLOUD_SWITCHES: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_CODE_USE_MANTLE",
];
fn check_at(path: &Path) -> Result<(), String> {
    let Some(text) = read(path)? else {
        return Ok(());
    };
    let policy = Format::Json.parse(Some(&text))?;
    for key in CLOUD_SWITCHES {
        let value = &policy["env"][*key];
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ) {
            return Err(CONFLICT.into());
        }
    }
    Ok(())
}
pub(super) fn check() -> Result<(), String> {
    #[cfg(test)]
    let path = crate::config::get_home_dir().join(".claude/managed-settings.json");
    #[cfg(all(not(test), target_os = "macos"))]
    let path =
        std::path::PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json");
    #[cfg(all(not(test), windows))]
    let path = std::env::var_os("ProgramFiles")
        .map(std::path::PathBuf::from)
        .ok_or(super::FAILED)?
        .join("ClaudeCode/managed-settings.json");
    #[cfg(all(not(test), not(windows), not(target_os = "macos")))]
    let path = std::path::PathBuf::from("/etc/claude-code/managed-settings.json");
    check_at(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parity_managed_cloud_policy_is_preserved() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| super::super::FAILED)?;
        let path = dir.path().join("policy.json");
        for value in ["\"1\"", "true", "\" YES \""] {
            let original = format!("{{\"env\":{{\"CLAUDE_CODE_USE_BEDROCK\":{value}}}}}");
            std::fs::write(&path, &original).map_err(|_| super::super::FAILED)?;
            assert_eq!(check_at(&path), Err(CONFLICT.into()));
            assert_eq!(
                std::fs::read_to_string(&path).map_err(|_| super::super::FAILED)?,
                original
            );
        }
        Ok(())
    }
}
