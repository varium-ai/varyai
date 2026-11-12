//! Use the installed Homebrew channel instead of npm to advertise updates.
use super::*;

fn public_source(real: &Path) -> Option<(String, &'static str)> {
    let package = real.ancestors().find(|path| {
        path.parent().and_then(Path::file_name).is_some_and(|name| {
            name.eq_ignore_ascii_case("Cellar") || name.eq_ignore_ascii_case("Caskroom")
        })
    })?;
    let formula = package
        .parent()?
        .file_name()?
        .eq_ignore_ascii_case("Cellar");
    let receipt = if formula {
        package
            .join(
                real.strip_prefix(package)
                    .ok()?
                    .components()
                    .next()?
                    .as_os_str(),
            )
            .join("INSTALL_RECEIPT.json")
    } else {
        package.join(".metadata/INSTALL_RECEIPT.json")
    };
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(receipt).ok()?).ok()?;
    let (tap, kind, pointer) = if formula {
        ("homebrew/core", "formula", "/versions/stable")
    } else {
        ("homebrew/cask", "cask", "/version")
    };
    // Custom taps keep their existing upstream checks; never substitute core's package.
    if receipt.pointer("/source/tap")?.as_str()? != tap {
        return None;
    }
    let mut url = reqwest::Url::parse("https://formulae.brew.sh/api/").ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(kind)
        .push(&format!("{}.json", package.file_name()?.to_str()?));
    Some((url.into(), pointer))
}

fn metadata_version(data: &serde_json::Value, pointer: &str) -> Option<String> {
    agent_versions::version_number(data.pointer(pointer)?.as_str()?)
}

// Some(None) means Homebrew was identified but its release could not be checked.
// Do not fall back to npm and advertise a version this installation cannot receive.
pub(super) async fn latest(client: &reqwest::Client, tool: &str) -> Option<Option<String>> {
    let tool = tool.to_owned();
    let (url, pointer) = tokio::task::spawn_blocking(move || {
        let path = locate_default_tool(
            &tool,
            CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
        )
        .ok()?;
        public_source(&path.canonicalize().ok()?)
    })
    .await
    .ok()
    .flatten()?;
    Some(
        async {
            let data = client
                .get(url)
                .timeout(LATEST_PROBE_TIMEOUT)
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .json::<serde_json::Value>()
                .await
                .ok()?;
            metadata_version(&data, pointer)
        }
        .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_versions_come_from_homebrew_instead_of_a_newer_npm_release() {
        let formula = serde_json::json!({"versions":{"stable":"0.66.0"},"npm_latest":"0.67.0"});
        assert_eq!(
            metadata_version(&formula, "/versions/stable").as_deref(),
            Some("0.66.0")
        );
        assert_eq!(
            metadata_version(&serde_json::json!({"version":"0.167.0,1234"}), "/version").as_deref(),
            Some("0.167.0")
        );
        assert!(metadata_version(
            &serde_json::json!({"versions":{"stable":null}}),
            "/versions/stable"
        )
        .is_none());
    }

    #[test]
    fn official_formula_and_cask_follow_their_own_release_channels() {
        let root = tempfile::tempdir().unwrap();
        for (layout, receipt, tap, url, pointer) in [
            (
                "Cellar/gemini-cli/0.66.0/bin/gemini",
                "Cellar/gemini-cli/0.66.0/INSTALL_RECEIPT.json",
                "homebrew/core",
                "https://formulae.brew.sh/api/formula/gemini-cli.json",
                "/versions/stable",
            ),
            (
                "Caskroom/codex/0.167.0/bin/codex",
                "Caskroom/codex/.metadata/INSTALL_RECEIPT.json",
                "homebrew/cask",
                "https://formulae.brew.sh/api/cask/codex.json",
                "/version",
            ),
        ] {
            let receipt = root.path().join(receipt);
            std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
            std::fs::write(
                &receipt,
                serde_json::json!({"source": {"tap": tap}}).to_string(),
            )
            .unwrap();
            assert_eq!(
                public_source(&root.path().join(layout)),
                Some((url.into(), pointer))
            );
            std::fs::write(&receipt, r#"{"source":{"tap":"synthetic/custom"}}"#).unwrap();
            assert!(public_source(&root.path().join(layout)).is_none());
        }
    }
}
