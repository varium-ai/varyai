use super::*;
use crate::proxy::security::SecurityRuntimeConfig;
use std::{fs, io::Cursor, sync::Arc};

fn policy() -> Option<Arc<SecurityRuntimeConfig>> {
    Some(Arc::new(SecurityRuntimeConfig::default()))
}

#[test]
fn application_log_exports_the_whole_file_without_collecting_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let output = dir.path().join("support.log");
    let text = format!(
        "{}last line without newline",
        "INFO connection ready\r\n".repeat(16000)
    );
    fs::write(&source, &text).unwrap();
    fs::write(
        dir.path().join("varyai.log.1"),
        "synthetic private rotation",
    )
    .unwrap();
    fs::write(dir.path().join("config.json"), "invalid neighboring config").unwrap();
    let report = export_with_policy(&source, &output, policy()).unwrap();
    assert_eq!(fs::read_to_string(&output).unwrap(), text);
    assert_eq!(report.source_bytes, text.len() as u64);
    assert_eq!(report.redacted_lines, 0);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn application_log_preserves_diagnostics_and_redacts_payloads_and_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let output = dir.path().join("support.log");
    let secret = "fixture-credential";
    let key = format!(
        "-----{} {}-----\nfixture-key-material\n-----{} {}-----",
        "BEGIN", "PRIVATE KEY", "END", "PRIVATE KEY"
    );
    let input = format!(
        "INFO operation=connect status=started\nINFO request_body={{\n  \"arguments\":\"synthetic-private-input\"\n}}\nWARN api_key={secret}\nERROR upstream https://private.invalid/secret C:\\Users\\private\\config\n{key}\nERROR status=502 elapsed_ms=123\n"
    );
    fs::write(&source, input).unwrap();
    let report = export_with_policy(&source, &output, policy()).unwrap();
    let text = fs::read_to_string(&output).unwrap();
    for private in [
        "synthetic-private-input",
        "fixture-credential",
        "private.invalid",
        "Users",
        "fixture-key-material",
    ] {
        assert!(!text.contains(private));
    }
    assert!(text.contains("operation=connect status=started"));
    assert!(text.contains("status=502 elapsed_ms=123"));
    assert!(report.redacted_lines > 0);
}

#[test]
fn application_log_rejects_missing_empty_binary_and_unavailable_privacy() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let output = dir.path().join("support.log");
    fs::write(&output, "existing destination").unwrap();
    assert_eq!(
        export_with_policy(&source, &output, policy()).unwrap_err(),
        MISSING
    );
    fs::write(&source, "").unwrap();
    assert_eq!(
        export_with_policy(&source, &output, policy()).unwrap_err(),
        EMPTY
    );
    for input in [b"INFO\0private".as_slice(), b"INFO\xffprivate".as_slice()] {
        fs::write(&source, input).unwrap();
        assert_eq!(
            export_with_policy(&source, &output, policy()).unwrap_err(),
            FAILED
        );
    }
    fs::write(&source, "INFO connection ready").unwrap();
    assert_eq!(
        export_with_policy(&source, &output, None).unwrap_err(),
        PRIVACY
    );
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing destination");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn application_log_never_replaces_the_live_source_or_its_hardlink() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    fs::write(&source, "INFO source remains intact\n").unwrap();
    assert_eq!(
        export_with_policy(&source, &source, policy()).unwrap_err(),
        SAME_SOURCE
    );
    let alias = dir.path().join("alias.log");
    fs::hard_link(&source, &alias).unwrap();
    assert_eq!(
        export_with_policy(&source, &alias, policy()).unwrap_err(),
        SAME_SOURCE
    );
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        "INFO source remains intact\n"
    );
}

#[cfg(unix)]
#[test]
fn application_log_rejects_symlink_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let alias = dir.path().join("alias.log");
    fs::write(&source, "INFO source\n").unwrap();
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    assert!(export_with_policy(&source, &alias, policy()).is_err());
    assert!(fs::symlink_metadata(alias)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn application_log_snapshot_ignores_new_appends_and_rejects_premature_eof() {
    let privacy = text_logs::LogPrivacy::capture(policy());
    let mut output = Vec::new();
    write_snapshot(
        Cursor::new(b"INFO old\nINFO new\n"),
        9,
        &mut output,
        &privacy,
    )
    .unwrap();
    assert_eq!(output, b"INFO old\n");
    assert!(write_snapshot(Cursor::new(b"short"), 9, &mut Vec::new(), &privacy).is_err());
}

#[test]
fn application_log_retains_long_lines_and_final_line_without_tail_limits() {
    let privacy = text_logs::LogPrivacy::capture(policy());
    let text = format!("INFO {}", "diagnostic-context ".repeat(18000));
    let mut output = Vec::new();
    write_snapshot(
        Cursor::new(text.as_bytes()),
        text.len() as u64,
        &mut output,
        &privacy,
    )
    .unwrap();
    assert_eq!(output, text.as_bytes());
}

#[test]
fn application_log_applies_custom_privacy_even_when_forwarding_policy_is_disabled() {
    for enabled in [false, true] {
        let mut policy = SecurityRuntimeConfig {
            enabled,
            ..Default::default()
        };
        policy
            .audit_custom_rules
            .push(crate::proxy::security::runtime::CompiledCustomRule {
                id: "synthetic".into(),
                name: "synthetic".into(),
                rule_type: "keyword".into(),
                pattern: "synthetic-private-value".into(),
                regex: Some(regex::Regex::new("synthetic-private-value").unwrap()),
                risk_level: crate::proxy::security::types::RiskLevel::High,
                action: crate::proxy::security::types::AuditAction::Audit,
                enabled: false,
            });
        let privacy = text_logs::LogPrivacy::capture(Some(Arc::new(policy)));
        let mut file = privacy.file();
        assert!(!file
            .redact("INFO token=fixture-credential")
            .0
            .contains("fixture-credential"));
        assert!(!file
            .redact("INFO synthetic-private-value")
            .0
            .contains("synthetic-private-value"));
        assert_eq!(
            file.redact("WARN connection refused").0,
            "WARN connection refused"
        );
    }
}

#[test]
fn application_log_redacts_raw_structured_tool_values() {
    let privacy = text_logs::LogPrivacy::capture(policy());
    let mut file = privacy.file();
    for line in [
        r#"["fixture-private-tool-input"]"#,
        r#"{"level":"error","message":"connection refused","arguments":{"text":"fixture-private-tool-input"}}"#,
    ] {
        let safe = file.redact(line).0;
        assert!(!safe.contains("fixture-private-tool-input"));
        if line.starts_with('{') {
            assert!(safe.contains("connection refused"));
        }
    }
}
