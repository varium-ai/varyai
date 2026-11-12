//! Specialized subscription execution; all client wire formats use the shared IR.
use super::{
    hyper_client::ProxyResponse,
    protocol_bridge::{ir, Protocol},
    ProxyError,
};
use crate::{
    provider::Provider,
    services::{native_subscription as accounts, routing_upstreams::SubscriptionProvider},
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};
use std::{pin::Pin, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
fn invalid() -> ProxyError {
    ProxyError::TransformError("Subscription response is unavailable".into())
}
fn chatgpt_body(mut body: Value) -> Result<Value, ProxyError> {
    for key in [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        if let Some(object) = body.as_object_mut() {
            object.remove(key);
        }
    }
    body["stream"] = json!(true);
    body["store"] = json!(false);
    if let Some(items) = body["input"].as_array_mut() {
        for item in items {
            if item["role"] == "system" {
                item["role"] = json!("developer");
            }
        }
    }
    if body["tools"].as_array().is_some_and(|tools| {
        tools.iter().any(|tool| {
            matches!(
                tool["type"].as_str(),
                Some(
                    "image_generation"
                        | "file_search"
                        | "code_interpreter"
                        | "computer_use"
                        | "computer_use_preview"
                        | "computer"
                        | "mcp"
                        | "tool_search"
                )
            )
        })
    }) {
        return Err(ProxyError::InvalidRequest(
            "Hosted tools are unavailable for ChatGPT plan inference".into(),
        ));
    }
    Ok(body)
}
pub(crate) fn specialized(provider: &Provider) -> bool {
    crate::services::subscription_provider::binding(provider).is_some_and(|binding| {
        matches!(
            binding.provider,
            SubscriptionProvider::Claude
                | SubscriptionProvider::XaiOauth
                | SubscriptionProvider::ChatgptApi
                | SubscriptionProvider::GeminiCli
                | SubscriptionProvider::Antigravity
        )
    })
}
fn envelope(
    provider: SubscriptionProvider,
    project: &str,
    model: &str,
    mut request: Value,
    session: Option<&str>,
) -> Value {
    if let Some(object) = request.as_object_mut() {
        object.remove("model");
        object.remove("stream");
    }
    let mut body = json!({"model":model,"project":project,"request":request});
    if provider == SubscriptionProvider::Antigravity {
        body["userAgent"] = json!("antigravity");
        body["requestType"] = json!("agent");
        body["requestId"] = json!(format!("agent-{}", uuid::Uuid::new_v4()));
        if let Some(session) = session {
            body["request"]["sessionId"] = json!(session);
        }
    } else {
        body["user_prompt_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    body
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward(
    provider: &Provider,
    protocol: Protocol,
    payload: &Value,
    endpoint: &str,
    session: Option<&str>,
    shadow: Arc<super::providers::gemini_shadow::GeminiShadowStore>,
    history: Arc<super::providers::codex_chat_history::CodexChatHistoryStore>,
    client: reqwest::Client,
    app: Option<&tauri::AppHandle>,
) -> Result<ProxyResponse, ProxyError> {
    let binding = crate::services::subscription_provider::binding(provider).ok_or_else(invalid)?;
    let account = if binding.provider == SubscriptionProvider::XaiOauth {
        None
    } else {
        Some(
            accounts::resolve(binding.provider, &binding.account_id)
                .await
                .map_err(|_| ProxyError::AuthError("Subscription account is unavailable".into()))?,
        )
    };
    let mut payload = payload.clone();
    if protocol == Protocol::Responses {
        let (_, owners) = history
            .enrich_request_with_provenance(
                &mut payload,
                None,
                super::security::chat_enrichment::ChatHistoryProvenance::StrictResume,
            )
            .await
            .map_err(ProxyError::DlpForward)?;
        if owners.is_some() {
            drop(payload);
            drop(owners);
            return Err(ProxyError::DlpForward(
                super::security::http_restoration::DlpForwardError::ContextUnavailable,
            ));
        }
        super::security::http_restoration::validate_unleased_enriched_body(&payload)
            .map_err(ProxyError::DlpForward)?;
    }
    let mut request = ir::parse::request(protocol, &payload, endpoint)?;
    if endpoint
        .split('?')
        .next()
        .is_some_and(|path| path.ends_with("/count_tokens"))
    {
        return Err(ProxyError::InvalidRequest(
            "Subscription token counting is unavailable".into(),
        ));
    }
    ir::continuity::restore(&mut request, &shadow, &provider.id, session);
    let streaming = request.stream;
    let source_protocol = match binding.provider {
        SubscriptionProvider::Claude => Protocol::Anthropic,
        SubscriptionProvider::ChatgptApi | SubscriptionProvider::XaiOauth => Protocol::Responses,
        _ => Protocol::Gemini,
    };
    let context =
        super::providers::transform_codex_chat::build_codex_tool_context_from_request(&payload);
    let source = if binding.provider == SubscriptionProvider::XaiOauth {
        let body = if protocol == Protocol::Responses {
            payload.clone()
        } else {
            ir::build::request(&request, Protocol::Responses)?
        };
        let token = provider
            .settings_config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let mut response = crate::services::grok_subscription::inference(
            &client,
            crate::services::grok_subscription::BASE,
            token,
            body.clone(),
        )
        .await
        .map_err(|_| invalid())?;
        if response.status() == StatusCode::UNAUTHORIZED {
            use tauri::Manager;
            if let Some(state) =
                app.and_then(|app| app.try_state::<crate::commands::XaiOAuthState>())
            {
                let manager = state.0.read().await;
                let renewed = manager
                    .renew_rejected_token(&binding.account_id, token)
                    .await
                    .map_err(|_| {
                        ProxyError::AuthError("Subscription authorization is unavailable".into())
                    })?;
                response = crate::services::grok_subscription::inference(
                    &client,
                    crate::services::grok_subscription::BASE,
                    &renewed,
                    body,
                )
                .await
                .map_err(|_| invalid())?;
            }
        }
        if !response.status().is_success() {
            return Err(ProxyError::UpstreamError {
                status: response.status().as_u16(),
                body: None,
            });
        }
        Box::pin(response.bytes_stream().map(|chunk| {
            chunk.map_err(|_| std::io::Error::other("Subscription stream interrupted"))
        })) as ByteStream
    } else if binding.provider == SubscriptionProvider::Claude {
        claude(account.as_ref().ok_or_else(invalid)?, &request).await?
    } else if binding.provider == SubscriptionProvider::ChatgptApi {
        let account = account.as_ref().ok_or_else(invalid)?;
        let body = chatgpt_body(ir::build::request(&request, Protocol::Responses)?)?;
        // Sign in with ChatGPT requires stateless streamed Responses inference.
        let response = client
            .post(format!("{}/responses", binding.provider.endpoint()))
            .bearer_auth(account.token())
            .header(header::ACCEPT, "text/event-stream")
            .header(header::ACCEPT_ENCODING, "identity")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|_| invalid())?;
        if response.status() == StatusCode::UNAUTHORIZED {
            accounts::observe_rejection(account).await;
        }
        if !response.status().is_success() {
            return Err(ProxyError::UpstreamError {
                status: response.status().as_u16(),
                body: None,
            });
        }
        Box::pin(response.bytes_stream().map(|chunk| {
            chunk.map_err(|_| std::io::Error::other("Subscription stream interrupted"))
        })) as ByteStream
    } else {
        let account = account.as_ref().ok_or_else(invalid)?;
        let project = accounts::google_project(account).await.map_err(|_| {
            ProxyError::ConfigError("Subscription Cloud project is unavailable".into())
        })?;
        let model = if binding.provider == SubscriptionProvider::GeminiCli {
            accounts::gemini_wire_model(account, &project, &request.model).await
        } else {
            request.model.clone()
        };
        let body = envelope(
            binding.provider,
            &project,
            &model,
            ir::build::request(&request, Protocol::Gemini)?,
            session,
        );
        let response = client
            .post(format!(
                "{}/v1internal:streamGenerateContent?alt=sse",
                binding.provider.endpoint()
            ))
            .bearer_auth(account.token())
            .header(header::ACCEPT, "text/event-stream")
            .header(header::ACCEPT_ENCODING, "identity")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|_| invalid())?;
        if response.status() == StatusCode::UNAUTHORIZED {
            accounts::observe_rejection(account).await;
        }
        if !response.status().is_success() {
            return Ok(ProxyResponse::buffered(
                response.status(),
                HeaderMap::from_iter([(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                )]),
                Bytes::from_static(
                    b"{\"error\":{\"message\":\"Subscription upstream rejected the request\"}}",
                ),
            ));
        }
        Box::pin(response.bytes_stream().map(|chunk| {
            chunk.map_err(|_| std::io::Error::other("Subscription stream interrupted"))
        })) as ByteStream
    };
    let provider_id = provider.id.clone();
    let session = session.map(str::to_owned);
    let observer: ir::stream::ReplyObserver = Arc::new(move |reply| {
        if source_protocol == Protocol::Gemini {
            ir::continuity::record(reply, &shadow, &provider_id, session.as_deref());
        }
    });
    if streaming {
        let stream = ir::stream::transcode_with_observer(
            source,
            source_protocol,
            protocol,
            context,
            Some(observer),
        );
        let stream: ByteStream = if protocol == Protocol::Responses {
            Box::pin(
                super::providers::codex_chat_history::record_responses_sse_stream(stream, history),
            )
        } else {
            stream
        };
        let headers = HeaderMap::from_iter([
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ]);
        Ok(ProxyResponse::streamed(StatusCode::OK, headers, stream))
    } else {
        let reply = ir::stream::collect(source, source_protocol).await?;
        observer(&reply);
        let body = ir::reply::render_with_context(protocol, &reply, &context)?;
        if protocol == Protocol::Responses {
            history.record_response(&body).await;
        }
        Ok(ProxyResponse::buffered(
            StatusCode::OK,
            HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )]),
            Bytes::from(serde_json::to_vec(&body).map_err(|_| invalid())?),
        ))
    }
}

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn tool_server(tools: Vec<Value>) -> Result<(Server, String), ProxyError> {
    use axum::{extract::State, routing::post, Json, Router};
    async fn rpc(
        State(tools): State<Arc<Vec<Value>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let id = body.get("id").cloned();
        if id.is_none() {
            return StatusCode::ACCEPTED.into_response();
        }
        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"Vary AI caller tools","version":"1"}}),
            "ping" => json!({}),
            "tools/list" => json!({"tools":&*tools}),
            // The caller executes its own tools. The CLI is stopped at tool_use;
            // holding this call prevents it from inventing a local tool result.
            "tools/call" => { std::future::pending::<()>().await; return StatusCode::GONE.into_response(); }
            _ => return Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})).into_response(),
        };
        Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| invalid())?;
    let path = format!("/{}", uuid::Uuid::new_v4());
    let url = format!(
        "http://127.0.0.1:{}{path}",
        listener.local_addr().map_err(|_| invalid())?.port()
    );
    let app = Router::new()
        .route(&path, post(rpc))
        .with_state(Arc::new(tools));
    let server = Server(tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    }));
    Ok((server, url))
}
fn claude_history(
    body: &Value,
    work: &str,
    model: &str,
    session: &str,
) -> Result<(Vec<Value>, Value), ProxyError> {
    let messages = body["messages"].as_array().ok_or_else(invalid)?;
    let (last, history) = messages.split_last().ok_or_else(invalid)?;
    if last["role"] != "user" {
        return Err(invalid());
    }
    let mut parent = Value::Null;
    let mut lines = Vec::new();
    for original in history {
        let role = original["role"]
            .as_str()
            .filter(|r| matches!(*r, "user" | "assistant"))
            .ok_or_else(invalid)?;
        let mut message = original.clone();
        if let Some(blocks) = message["content"].as_array_mut() {
            for block in blocks {
                if block["type"] == "tool_use" {
                    if let Some(name) = block["name"].as_str() {
                        block["name"] = json!(format!("mcp__vary__{name}"));
                    }
                }
            }
        }
        if role == "assistant" {
            message["id"] = json!(format!("msg_{}", uuid::Uuid::new_v4().simple()));
            message["type"] = json!("message");
            message["model"] = json!(model);
            message["usage"] = json!({"input_tokens":0,"output_tokens":0});
        }
        let uuid = json!(uuid::Uuid::new_v4().to_string());
        lines.push(json!({"parentUuid":parent,"isSidechain":false,"userType":"external","cwd":work,"sessionId":session,"type":role,"message":message,"uuid":uuid,"timestamp":chrono::Utc::now().to_rfc3339()}));
        parent = uuid;
    }
    Ok((
        lines,
        json!({"type":"user","session_id":session,"message":last}),
    ))
}
struct ReplayDirectory(std::path::PathBuf);
impl Drop for ReplayDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli_event(value: &Value) -> Option<Value> {
    if value["type"] != "stream_event" {
        return None;
    }
    let mut event = value["event"].clone();
    if event["content_block"]["type"] == "tool_use" {
        if let Some(name) = event["content_block"]["name"]
            .as_str()
            .and_then(|s| s.strip_prefix("mcp__vary__"))
        {
            event["content_block"]["name"] = json!(name);
        }
    }
    Some(event)
}
async fn claude(
    account: &accounts::Account,
    request: &ir::Request,
) -> Result<ByteStream, ProxyError> {
    claude_with_command(account, request, accounts::claude_command()).await
}
async fn claude_with_command(
    account: &accounts::Account,
    request: &ir::Request,
    mut command: tokio::process::Command,
) -> Result<ByteStream, ProxyError> {
    let body = ir::build::request(request, Protocol::Anthropic)?;
    let tools = body["tools"].as_array().into_iter().flatten().map(|tool| json!({"name":tool["name"],"description":tool["description"],"inputSchema":tool["input_schema"]})).collect();
    let (server, url) = tool_server(tools).await?;
    let temporary = tempfile::tempdir().map_err(|_| invalid())?;
    let work = std::fs::canonicalize(temporary.path()).map_err(|_| invalid())?;
    let work_name = work
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let session = uuid::Uuid::new_v4().to_string();
    let (lines, prompt) = claude_history(&body, &work.to_string_lossy(), &request.model, &session)?;
    let replay_dir = account.claude_dir().join("projects").join(work_name);
    let replay = ReplayDirectory(replay_dir.clone());
    if !lines.is_empty() {
        let mut bytes = Vec::new();
        for line in &lines {
            serde_json::to_writer(&mut bytes, line).map_err(|_| invalid())?;
            bytes.push(b'\n');
        }
        crate::config::atomic_write_private(&replay_dir.join(format!("{session}.jsonl")), &bytes)
            .map_err(|_| invalid())?;
    }
    let mcp = temporary.path().join("mcp.json");
    let system = temporary.path().join("system.txt");
    let config = json!({"mcpServers":{"vary":{"type":"http","url":url}}});
    crate::config::atomic_write_private(&mcp, config.to_string().as_bytes())
        .map_err(|_| invalid())?;
    let mut instructions = body["system"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    match &request.tool_choice {
        Some(ir::ToolChoice::Required) => instructions
            .push_str("\nCall at least one of the caller's available tools before answering."),
        Some(ir::ToolChoice::Named(name)) => {
            instructions.push_str(&format!("\nCall the caller's {name} tool."))
        }
        _ => {}
    }
    crate::config::atomic_write_private(&system, instructions.as_bytes()).map_err(|_| invalid())?;
    // Claude Code owns execution; caller generation options are per-run only.
    let mut extra = serde_json::Map::new();
    for key in [
        "max_tokens",
        "temperature",
        "top_p",
        "stop_sequences",
        "safeguards",
        "metadata",
        "output_config",
        "thinking",
    ] {
        if let Some(value) = body.get(key) {
            extra.insert(key.into(), value.clone());
        }
    }
    if matches!(request.tool_choice, Some(ir::ToolChoice::None)) {
        extra.insert("tool_choice".into(), json!({"type":"none"}));
    }
    command.env("CLAUDE_CODE_EXTRA_BODY", Value::Object(extra).to_string());
    command
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .current_dir(temporary.path())
        .args([
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
            "--model",
            &request.model,
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--no-session-persistence",
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            "mcp__vary__*",
        ])
        .arg("--mcp-config")
        .arg(&mcp)
        .arg("--append-system-prompt-file")
        .arg(&system)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(effort) = &request.reasoning.effort {
        command.args(["--effort", effort]);
    }
    if !lines.is_empty() {
        command.args(["--resume", &session]);
    }
    let mut child = command.spawn().map_err(|_| {
        ProxyError::ConfigError("Claude Code must be installed and signed in".into())
    })?;
    let mut input = child.stdin.take().ok_or_else(invalid)?;
    let mut prompt = serde_json::to_vec(&prompt).map_err(|_| invalid())?;
    prompt.push(b'\n');
    input.write_all(&prompt).await.map_err(|_| invalid())?;
    drop(input);
    let output = child.stdout.take().ok_or_else(invalid)?;
    Ok(Box::pin(async_stream::stream! {
        let _server=server; let _temporary=temporary; let _replay=replay;
        let mut reader=BufReader::new(output); let mut buffer=Vec::new(); let mut ended=false;
        loop {
            buffer.clear();
            match tokio::time::timeout(Duration::from_secs(120), reader.read_until(b'\n', &mut buffer)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) if buffer.len() <= 16*1024*1024 => {},
                _ => { yield Err(std::io::Error::other("Claude Code stream interrupted")); break; }
            }
            let Ok(value)=serde_json::from_slice::<Value>(&buffer) else { continue; };
            if let Some(event)=cli_event(&value) {
                let stop=event["type"]=="message_stop";
                let frame=format!("event: {}\ndata: {}\n\n",event["type"].as_str().unwrap_or("message"),event);
                yield Ok(Bytes::from(frame));
                if stop { ended=true; break; }
            } else if value["type"]=="result" && value["is_error"]==true { break; }
        }
        let _=child.kill().await;
        if !ended { yield Err(std::io::Error::other("Claude Code did not complete the reply")); }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn fake_claude_resumes_typed_history_and_returns_the_following_turn(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let script = dir.path().join("fixture-cli");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import os, sys, json
args=sys.argv[1:]
sid=args[args.index('--resume')+1]
work=''.join(c if c.isascii() and c.isalnum() else '-' for c in os.path.realpath(os.getcwd()))
with open(os.path.join(os.environ['CLAUDE_CONFIG_DIR'],'projects',work,sid+'.jsonl')) as f:
    rows=[json.loads(line) for line in f]
prompt=json.loads(sys.stdin.readline())
call=rows[-1]['message']['content'][0]
result=prompt['message']['content'][0]
if call['type']!='tool_use' or call['id']!=result['tool_use_id'] or call['name']!='mcp__vary__fixture':
    sys.exit(2)
for event in [{'type':'message_start','message':{'id':'fixture-reply','role':'assistant','model':'fixture','content':[],'usage':{'input_tokens':1,'output_tokens':0}}},{'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}},{'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'synthetic continuation'}},{'type':'content_block_stop','index':0},{'type':'message_delta','delta':{'stop_reason':'end_turn'},'usage':{'output_tokens':1}},{'type':'message_stop'}]:
    print(json.dumps({'type':'stream_event','event':event}),flush=True)
"#,
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let home = dir.path().join("account");
        let account: accounts::Account = serde_json::from_value(
            json!({"id":"fixture","provider":"claude","login":"fixture","claudeHome":home}),
        )?;
        let body = json!({"model":"fixture","messages":[{"role":"user","content":[{"type":"text","text":"synthetic"}]},{"role":"assistant","content":[{"type":"tool_use","id":"fixture-call","name":"fixture","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"fixture-call","content":"synthetic result"}]}]});
        let request = ir::parse::request(Protocol::Anthropic, &body, "/v1/messages")?;
        let source =
            claude_with_command(&account, &request, tokio::process::Command::new(script)).await?;
        let reply = ir::stream::collect(source, Protocol::Anthropic).await?;
        assert!(reply
            .parts
            .iter()
            .any(|part| matches!(part,ir::Part::Text{text,..} if text=="synthetic continuation")));
        assert!(std::fs::read_dir(home.join("projects"))?.next().is_none());
        Ok(())
    }
    #[test]
    fn claude_history_keeps_roles_tool_links_images_and_thinking() {
        let body = json!({"messages":[
            {"role":"user","content":[{"type":"text","text":"synthetic question"}]},
            {"role":"assistant","content":[{"type":"thinking","thinking":"synthetic thought","signature":"synthetic-signature"},{"type":"tool_use","id":"call-fixture","name":"fixture","input":{"n":1}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-fixture","content":[{"type":"text","text":"synthetic result"}]}]}
        ]});
        let (history, prompt) =
            claude_history(&body, "/fixture", "fixture-model", "fixture-session")
                .expect("typed history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1]["message"]["role"], "assistant");
        assert_eq!(
            history[1]["message"]["content"][0]["signature"],
            "synthetic-signature"
        );
        assert_eq!(
            history[1]["message"]["content"][1]["name"],
            "mcp__vary__fixture"
        );
        assert_eq!(
            prompt["message"]["content"][0]["tool_use_id"],
            "call-fixture"
        );
        assert_eq!(history[1]["parentUuid"], history[0]["uuid"]);
    }
    #[test]
    fn chatgpt_plan_requests_are_stateless_and_keep_caller_functions() {
        let body=chatgpt_body(json!({"model":"fixture","stream":false,"store":true,"temperature":0.3,"previous_response_id":"fixture-old","input":[{"role":"system","content":"synthetic instruction"}],"tools":[{"type":"function","name":"fixture","parameters":{"type":"object"}}]})).expect("plan body");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["tools"][0]["name"], "fixture");
        assert!(body.get("temperature").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert!(chatgpt_body(json!({"tools":[{"type":"code_interpreter"}]})).is_err());
    }
    #[tokio::test]
    async fn codeassist_sse_envelopes_stream_text_and_retain_tool_signatures() {
        let frames = [
            json!({"response":{"responseId":"fixture-reply","modelVersion":"fixture-model","candidates":[{"content":{"parts":[{"text":"synthetic "}]}}]}}),
            json!({"response":{"candidates":[{"content":{"parts":[{"text":"reply"},{"functionCall":{"name":"fixture","args":{"n":1}},"thoughtSignature":"synthetic-signature"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}}),
        ];
        let source = Box::pin(futures::stream::iter(
            frames
                .into_iter()
                .map(|frame| Ok(Bytes::from(format!("data: {frame}\n\n")))),
        ));
        let reply = ir::stream::collect(source, Protocol::Gemini)
            .await
            .expect("Code Assist SSE");
        assert!(reply.parts.iter().any(|part|matches!(part,ir::Part::ToolCall{name,signature:Some(signature),..}if name=="fixture"&&signature=="synthetic-signature")));
        assert_eq!(reply.usage.input, 3);
        assert_eq!(reply.usage.output, 2);
    }
    #[test]
    fn codeassist_envelope_keeps_parameters_and_adds_only_transport_fields() {
        let request = json!({"model":"fixture","stream":true,"contents":[{"role":"user","parts":[{"text":"synthetic"}]}],"generationConfig":{"temperature":0.37},"tools":[{"functionDeclarations":[{"name":"fixture"}]}]});
        let body = envelope(
            SubscriptionProvider::Antigravity,
            "fixture-project",
            "fixture-model",
            request.clone(),
            Some("fixture-session"),
        );
        assert_eq!(body["model"], "fixture-model");
        assert_eq!(
            body["request"]["generationConfig"],
            request["generationConfig"]
        );
        assert_eq!(body["request"]["tools"], request["tools"]);
        assert_eq!(body["request"]["sessionId"], "fixture-session");
        assert!(body["request"].get("model").is_none());
    }
    #[test]
    fn cli_events_return_caller_tool_names() {
        let event = cli_event(
            &json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"fixture","name":"mcp__vary__fixture_tool","input":{}}}}),
        );
        assert_eq!(
            event.map(|e| e["content_block"]["name"].clone()),
            Some(json!("fixture_tool"))
        );
    }
    #[tokio::test]
    async fn mcp_lists_only_callers_tools_and_does_not_execute_them() {
        let (_server, url) = tool_server(vec![
            json!({"name":"fixture","inputSchema":{"type":"object"}}),
        ])
        .await
        .expect("loopback fixture");
        let body: Value = reqwest::Client::new()
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .expect("list fixture")
            .json()
            .await
            .expect("JSON");
        assert_eq!(body["result"]["tools"][0]["name"], "fixture");
    }
}
