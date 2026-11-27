use super::*;
use std::{fs, io::Cursor};

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
    let report = export_raw_file(&source, &output).unwrap();
    assert_eq!(fs::read_to_string(&output).unwrap(), text);
    assert_eq!(report.source_bytes, text.len() as u64);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn application_log_exports_raw_source_bytes_without_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let output = dir.path().join("raw.log");
    let input = [
        b"INFO operation=connect status=started\n".as_slice(),
        b"WARN [Proxy] Upstream send failed: connect=true timeout=false request=false body=false status=None\n",
        b"INFO request_body={\"arguments\":\"synthetic-private-input\"}\n",
        b"WARN api_key=fixture-credential\n",
        b"ERROR upstream https://private.invalid/secret C:\\Users\\private\\config\n",
        b"INFO\0raw\xffbytes\n",
    ]
    .concat();
    fs::write(&source, &input).unwrap();

    let report = export_raw_file(&source, &output).unwrap();
    let exported = fs::read(&output).unwrap();
    assert_eq!(exported, input);
    assert_eq!(report.source_bytes, input.len() as u64);
}

#[test]
fn application_log_rejects_missing_and_empty_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    let output = dir.path().join("support.log");
    fs::write(&output, "existing destination").unwrap();
    assert_eq!(export_raw_file(&source, &output).unwrap_err(), MISSING);
    fs::write(&source, "").unwrap();
    assert_eq!(export_raw_file(&source, &output).unwrap_err(), EMPTY);
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing destination");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn application_log_never_replaces_the_live_source_or_its_hardlink() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("varyai.log");
    fs::write(&source, "INFO source remains intact\n").unwrap();
    assert_eq!(export_raw_file(&source, &source).unwrap_err(), SAME_SOURCE);
    let alias = dir.path().join("alias.log");
    fs::hard_link(&source, &alias).unwrap();
    assert_eq!(export_raw_file(&source, &alias).unwrap_err(), SAME_SOURCE);
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
    assert!(export_raw_file(&source, &alias).is_err());
    assert!(fs::symlink_metadata(alias)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn application_log_snapshot_ignores_new_appends_and_rejects_premature_eof() {
    let mut output = Vec::new();
    let report = write_snapshot(Cursor::new(b"INFO old\nINFO new\n"), 9, &mut output).unwrap();
    assert_eq!(output, b"INFO old\n");
    assert_eq!(report.source_bytes, 9);
    assert!(write_snapshot(Cursor::new(b"short"), 9, &mut Vec::new()).is_err());
}

#[test]
fn application_log_retains_long_lines_and_final_line_without_tail_limits() {
    let text = format!("INFO {}", "diagnostic-context ".repeat(18000));
    let mut output = Vec::new();
    let report =
        write_snapshot(Cursor::new(text.as_bytes()), text.len() as u64, &mut output).unwrap();
    assert_eq!(output, text.as_bytes());
    assert_eq!(report.source_bytes, text.len() as u64);
}
