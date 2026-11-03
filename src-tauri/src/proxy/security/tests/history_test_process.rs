//! Process isolation for exact accounting assertions against the production global ledger.
use super::tokenization::TokenError;
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const CASE: &str = "VARYAI_SYNTHETIC_HISTORY_ACCOUNTING_CASE";
const WITNESS: &str = "VARYAI_SYNTHETIC_HISTORY_ACCOUNTING_WITNESS";
const FAIL: &str = "VARYAI_SYNTHETIC_HISTORY_ACCOUNTING_FAIL";
const COMPLETED: &[u8] = b"synthetic_history_case_completed";

pub(crate) struct Completion(std::path::PathBuf);
impl Drop for Completion {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                std::fs::write(&self.0, COMPLETED).is_ok(),
                "history_completion_witness_unavailable"
            );
        }
    }
}

pub(super) fn enter() -> Result<Option<Completion>, TokenError> {
    let thread = std::thread::current();
    let name = thread.name().ok_or(TokenError::Unavailable)?;
    if std::env::var(CASE).ok().as_deref() == Some(name) {
        let witness = std::env::var_os(WITNESS).ok_or(TokenError::Unavailable)?;
        Ok(Some(Completion(witness.into())))
    } else {
        run_case(name, false)?;
        Ok(None)
    }
}

struct ChildOwner(Child);
impl Drop for ChildOwner {
    fn drop(&mut self) {
        // Unwinding still owns cleanup; normal paths report kill/wait failures.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_case(name: &str, fail: bool) -> Result<(), TokenError> {
    let directory = tempfile::tempdir().map_err(|_| TokenError::Unavailable)?;
    let witness = directory.path().join("completed");
    let child = Command::new(std::env::current_exe().map_err(|_| TokenError::Unavailable)?)
        .args(["--exact", name, "--test-threads=1"])
        .env(CASE, name)
        .env(WITNESS, &witness)
        .env(FAIL, if fail { "1" } else { "0" })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| TokenError::Unavailable)?;
    let mut child = ChildOwner(child);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().map_err(|_| TokenError::Unavailable)? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(60) {
            child.0.kill().map_err(|_| TokenError::Unavailable)?;
            child.0.wait().map_err(|_| TokenError::Unavailable)?;
            return Err(TokenError::Unavailable);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success()
        || std::fs::read(witness).map_err(|_| TokenError::Unavailable)? != COMPLETED
    {
        return Err(TokenError::Unavailable);
    }
    Ok(())
}

#[test]
fn isolated_history_child_probe() -> Result<(), TokenError> {
    let Some(_completion) = enter()? else {
        return Ok(());
    };
    if std::env::var(FAIL).ok().as_deref() == Some("1") {
        return Err(TokenError::Unavailable);
    }
    Ok(())
}

#[test]
fn isolated_history_child_failure_is_not_accepted() {
    assert!(
        run_case(
            "proxy::security::history_test_process::isolated_history_child_probe",
            true
        )
        .is_err(),
        "failed_history_child_accepted"
    );
}

#[test]
fn isolated_history_missing_case_is_not_accepted() {
    assert!(
        run_case(
            "proxy::security::history_test_process::synthetic_missing_case",
            false
        )
        .is_err(),
        "missing_history_child_accepted"
    );
}
