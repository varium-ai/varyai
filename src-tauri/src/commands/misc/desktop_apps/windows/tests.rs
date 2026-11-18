use super::*;

fn file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "synthetic presence fixture; never execute").unwrap();
}

#[test]
fn every_desktop_has_presence_without_running_it_or_requiring_metadata() {
    for app in [
        "claude-desktop",
        "codex",
        "workbuddy",
        "workbuddy-ai",
        "qoder",
        "doubaowork",
        "trae",
        "trae-intl",
        "kimi",
        "zcode",
        "deepseek-harness",
        "t3code",
    ] {
        for (folder, executable) in layouts(app) {
            let root = tempfile::tempdir().unwrap();
            let snapshot = Snapshot {
                roots: vec![root.path().into()],
                ..Snapshot::default()
            };
            assert!(detect(app, &snapshot).is_none(), "{app}");
            file(&root.path().join(folder).join(executable));
            assert!(detect(app, &snapshot).is_some(), "{app}: {folder}");
        }
    }
}

#[test]
fn every_desktop_can_be_found_at_a_registered_custom_installation() {
    for app in [
        "claude-desktop",
        "codex",
        "workbuddy",
        "workbuddy-ai",
        "qoder",
        "doubaowork",
        "trae",
        "trae-intl",
        "kimi",
        "zcode",
        "deepseek-harness",
        "t3code",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (name, executable) = layouts(app)[0];
        let path = root.path().join("Custom Apps").join(executable);
        file(&path);
        for source in ["location", "icon", "uninstall"] {
            let mut record = Registration {
                name: format!("{name} (User)"),
                version: "5.6.2".into(),
                ..Registration::default()
            };
            match source {
                "location" => {
                    record.location = path.parent().unwrap().to_string_lossy().into_owned()
                }
                "icon" => record.icon = format!("\"{}\",0", path.display()),
                _ => {
                    record.uninstall = format!(
                        "\"{}\" /currentuser",
                        path.parent().unwrap().join("Uninstall.exe").display()
                    )
                }
            }
            let snapshot = Snapshot {
                registrations: vec![record],
                ..Snapshot::default()
            };
            assert_eq!(
                detect(app, &snapshot).unwrap().version.as_deref(),
                Some("5.6.2"),
                "{app}: {source}"
            );
        }
    }
}

#[test]
fn stale_or_malformed_registrations_and_leftover_configuration_are_not_installations() {
    let root = tempfile::tempdir().unwrap();
    for (name, location, icon) in [
        ("TRAE CN (User)", root.path().to_str().unwrap(), ""),
        ("TRAE CN", "relative/path", "invalid"),
        ("Unrelated", root.path().to_str().unwrap(), ""),
    ] {
        let snapshot = Snapshot {
            registrations: vec![Registration {
                name: name.into(),
                location: location.into(),
                icon: icon.into(),
                ..Registration::default()
            }],
            ..Snapshot::default()
        };
        assert!(detect("trae", &snapshot).is_none());
    }
    std::fs::create_dir(root.path().join(".workbuddy")).unwrap();
    assert!(detect(
        "workbuddy",
        &Snapshot {
            roots: vec![root.path().into()],
            ..Snapshot::default()
        }
    )
    .is_none());
    assert!(registered_executable("not an executable").is_none());
    assert!(registered_executable("\"unterminated.exe").is_none());
}

#[test]
fn domestic_and_international_editions_never_substitute_for_each_other() {
    for (app, other) in [("workbuddy", "workbuddy-ai"), ("trae", "trae-intl")] {
        for (installed, absent) in [(app, other), (other, app)] {
            let root = tempfile::tempdir().unwrap();
            let (folder, executable) = layouts(installed)[0];
            file(&root.path().join(folder).join(executable));
            let snapshot = Snapshot {
                roots: vec![root.path().into()],
                registrations: vec![Registration {
                    name: folder.into(),
                    location: root.path().join(folder).to_string_lossy().into_owned(),
                    ..Registration::default()
                }],
                ..Snapshot::default()
            };
            assert!(detect(installed, &snapshot).is_some());
            assert!(detect(absent, &snapshot).is_none());
        }
    }
    assert!(!registration_matches("qoder", "Qoder"));
}

#[test]
fn store_packages_require_the_main_product_and_valid_version() {
    let snapshot = Snapshot {
        packages: vec![
            "OpenAI.Codex_26.928.2636.0_x64__2p2nqsd0c76g0".into(),
            "OpenAI.Codex_26.1002.7124.0_arm64__2p2nqsd0c76g0".into(),
            "OpenAI.Codex_99.0.0.0_x64__wrong-publisher".into(),
            "OpenAI.Codex_99.0.0.0_neutral_split.scale-125_2p2nqsd0c76g0".into(),
            "OpenAI.Codex_bad_x64__2p2nqsd0c76g0".into(),
            "OpenAI.ChatGPT_99.0.0.0_x64__2p2nqsd0c76g0".into(),
        ],
        ..Snapshot::default()
    };
    assert_eq!(
        detect("codex", &snapshot).unwrap().version.as_deref(),
        Some("26.1002.7124.0")
    );
    assert!(detect("claude-desktop", &snapshot).is_none());
    for name in ["Claude", "AnthropicPBC.Claude"] {
        assert!(detect(
            "claude-desktop",
            &Snapshot {
                packages: vec![format!("{name}_1.2.3.4_x64__synthetic")],
                ..Snapshot::default()
            }
        )
        .is_some());
    }
}

#[test]
fn squirrel_claude_is_found_without_its_launcher_and_never_confused_with_cli() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = Snapshot {
        local: Some(root.path().into()),
        ..Snapshot::default()
    };
    file(&root.path().join("AnthropicClaude/app-1.2.3/claude.exe"));
    assert!(detect("claude-desktop", &snapshot).is_some());
    for app in [
        "claude",
        "gemini",
        "grokbuild",
        "opencode",
        "openclaw",
        "hermes",
        "pi",
        "qwen",
        "unknown",
    ] {
        assert!(!supported(app));
        assert!(detect(app, &snapshot).is_none());
    }
}
