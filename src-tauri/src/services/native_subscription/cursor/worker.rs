//! Private loopback transport for the pinned Cursor protocol helper.
use super::super::{random, root, FAILED};
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin},
    sync::Mutex,
};

pub(super) struct Worker {
    directory: PathBuf,
    child: Mutex<Child>,
    _input: ChildStdin,
    secret: String,
    port: u16,
    client: reqwest::Client,
}
static WORKER: Mutex<Option<Arc<Worker>>> = Mutex::const_new(None);

pub(super) async fn running() -> Option<Arc<Worker>> {
    WORKER
        .lock()
        .await
        .as_ref()
        .filter(|worker| worker.directory == root().join("cursor-runtime"))
        .cloned()
}
pub(super) async fn get() -> Result<Arc<Worker>, String> {
    let directory = root().join("cursor-runtime");
    let mut slot = WORKER.lock().await;
    if let Some(worker) = slot.as_ref() {
        if worker.directory == directory
            && worker
                .child
                .lock()
                .await
                .try_wait()
                .map_err(|_| FAILED)?
                .is_none()
        {
            return Ok(Arc::clone(worker));
        }
    }
    let node = crate::commands::reward_node_runtime().ok_or("nativeSubscription.cursorRuntime")?;
    let destination = directory.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(&destination).map_err(|_| FAILED)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| FAILED)?;
        }
        for (name, bytes) in [
            (
                "LICENSE",
                include_bytes!("../../../../../third_party/cursor-subscription/LICENSE").as_slice(),
            ),
            (
                "NOTICE.md",
                include_bytes!("../../../../../third_party/cursor-subscription/NOTICE.md")
                    .as_slice(),
            ),
            (
                "worker.mjs",
                include_bytes!("../../../../resources/cursor-subscription/worker.mjs").as_slice(),
            ),
            (
                "index.mjs",
                include_bytes!("../../../../resources/cursor-subscription/index.mjs").as_slice(),
            ),
            (
                "native-session.mjs",
                include_bytes!("../../../../resources/cursor-subscription/native-session.mjs")
                    .as_slice(),
            ),
        ] {
            crate::config::atomic_write_private(&destination.join(name), bytes)
                .map_err(|_| FAILED)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| FAILED)??;
    let mut command = tokio::process::Command::new(node);
    command
        .arg(directory.join("worker.mjs"))
        .current_dir(&directory)
        .env("XDG_DATA_HOME", &directory)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|_| "nativeSubscription.cursorRuntime")?;
    let mut input = child.stdin.take().ok_or(FAILED)?;
    let output = child.stdout.take().ok_or(FAILED)?;
    let secret = random();
    input
        .write_all(format!("{}\n", json!({"secret":secret})).as_bytes())
        .await
        .map_err(|_| FAILED)?;
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        BufReader::new(output).take(4096).read_line(&mut line),
    )
    .await
    .map_err(|_| "nativeSubscription.cursorRuntime")?
    .map_err(|_| "nativeSubscription.cursorRuntime")?;
    let ready: Value =
        serde_json::from_str(&line).map_err(|_| "nativeSubscription.cursorRuntime")?;
    let port = ready["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .ok_or(FAILED)?;
    let worker = Arc::new(Worker {
        directory,
        child: Mutex::new(child),
        _input: input,
        secret,
        port,
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| FAILED)?,
    });
    *slot = Some(Arc::clone(&worker));
    Ok(worker)
}
impl Worker {
    pub(super) async fn request(
        &self,
        path: &str,
        value: &Value,
    ) -> Result<reqwest::Response, String> {
        let response = self
            .client
            .post(format!("http://127.0.0.1:{}{}", self.port, path))
            .bearer_auth(&self.secret)
            .json(value)
            .timeout(Duration::from_secs(if path == "/chat" { 905 } else { 95 }))
            .send()
            .await
            .map_err(|_| FAILED)?;
        Ok(response)
    }
    pub(super) async fn json(&self, path: &str, value: &Value) -> Result<Value, String> {
        let mut response = self.request(path, value).await?;
        match response.status().as_u16() {
            200 => {}
            401 => return Err(super::super::EXPIRED.into()),
            _ => return Err(FAILED.into()),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| FAILED)? {
            if bytes.len().saturating_add(chunk.len()) > 16 * 1024 * 1024 {
                return Err(FAILED.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| FAILED.into())
    }
}
