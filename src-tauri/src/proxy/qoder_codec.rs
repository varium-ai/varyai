//! Isolated binding to the supported, locally installed Qoder protocol codec.
use super::ProxyError;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, sync::OnceLock, time::Duration};
use tokio::{io::AsyncWriteExt, sync::Semaphore};

fn installation() -> Result<(PathBuf, PathBuf), ProxyError> {
    let app = [
        PathBuf::from("/Applications/Qoder CN.app"),
        crate::config::get_home_dir().join("Applications/Qoder CN.app"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
    .ok_or_else(unavailable)?;
    Ok((app.join("Contents/MacOS/Qoder CN"),app.join("Contents/Resources/app.asar.unpacked/node_modules/@qoder-ai/qoder-cn-agent-sdk/dist/_worker/qoder-worker-runtime.obf.mjs")))
}
fn unavailable() -> ProxyError {
    ProxyError::ConfigError("Qoder CN codec is unavailable or this client version is unsupported; the request was not forwarded".into())
}
static LIMIT: OnceLock<Semaphore> = OnceLock::new();
pub(crate) async fn call(input: Value) -> Result<Value, ProxyError> {
    let operation = async {
        let _permit = LIMIT
            .get_or_init(|| Semaphore::new(4))
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        let (binary, worker) = installation()?;
        let mut child = tokio::process::Command::new(binary)
            .args(["-e", include_str!("qoder_codec.cjs")])
            .env_clear()
            .env("ELECTRON_RUN_AS_NODE", "1")
            .env("VARYAI_QODER_WORKER", worker)
            .env("NODE_NO_WARNINGS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| unavailable())?;
        let data = serde_json::to_vec(&input).map_err(|_| unavailable())?;
        let mut stdin = child.stdin.take().ok_or_else(unavailable)?;
        stdin.write_all(&data).await.map_err(|_| unavailable())?;
        drop(stdin);
        let output = child.wait_with_output().await.map_err(|_| unavailable())?;
        if !output.status.success() {
            return Err(unavailable());
        }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| unavailable())?;
        if value.get("error").is_some() {
            return Err(unavailable());
        }
        Ok(value)
    };
    tokio::time::timeout(Duration::from_secs(20), operation)
        .await
        .map_err(|_| unavailable())?
}
pub(crate) async fn check() -> Result<(), ProxyError> {
    call(json!({"operation":"check"})).await.map(|_| ())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn qoder_installed_runtime_decodes_redacts_and_resigns_large_native_payload() {
        if installation().is_err() {
            return;
        }
        check().await.expect("supported installed SDK");
        let headers = json!({"authorization":format!("Bearer test.{}.test",base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD,br#"{"info":"synthetic-user-info"}"#)),"cosy-machineid":"synthetic-machine","cosy-version":"test","cosy-user":"synthetic-user","cosy-key":"synthetic-key"});
        let text = format!(
            "{}-----BEGIN RSA PRIVATE KEY-----\n1234556\n-----END RSA PRIVATE KEY-----",
            "SYNTHETIC ".repeat(20000)
        );
        let body = json!({"session_id":"synthetic-session","model_config":{"key":"test","source":"system"},"chat_context":{"text":text,"extra":{"originalContent":text}},"messages":[{"role":"user","contents":[{"type":"text","text":text}]}],"future":{"keep":true},"tools":[{"name":"synthetic_tool"}],"parameters":{"temperature":0.2}});
        let encoded = call(json!({"operation":"encode","body":body,"headers":headers}))
            .await
            .expect("encode");
        let decoded = call(json!({"operation":"decode","body":encoded["body"]}))
            .await
            .expect("decode");
        assert_eq!(decoded["body"], body);
        let mut changed = decoded["body"].clone();
        let result = crate::proxy::security::SecurityAuditEngine::inspect_and_redact_with_runtime(
            &mut changed,
            &Default::default(),
        );
        assert!(!result.blocked);
        assert!(!changed.to_string().contains("1234556"));
        assert_eq!(changed["tools"], body["tools"]);
        assert_eq!(changed["future"], body["future"]);
        let resigned = call(json!({"operation":"encode","body":changed,"headers":headers}))
            .await
            .expect("resign");
        let final_body = call(json!({"operation":"decode","body":resigned["body"]}))
            .await
            .expect("decode edited");
        assert_eq!(final_body["body"], changed);
    }
}
