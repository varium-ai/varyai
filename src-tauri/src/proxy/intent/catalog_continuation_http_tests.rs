//! Synthetic native wire requests across the complete shared Agent registry.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{
    extract::OriginalUri,
    routing::{get, post},
    Json, Router,
};
use http::HeaderMap;
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Security(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for Security {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

fn agents() -> Vec<&'static str> {
    crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(["trae", "trae-intl"])
        .collect()
}

fn wire(app: &str, alias: &str) -> String {
    if app == "claude-desktop" {
        crate::claude_desktop_config::shared_reasoning_model_name(alias)
    } else if app == "codex-menu" {
        alias.to_owned()
    } else {
        "synthetic-model".into()
    }
}

fn receipt(app: &str, alias: &str) -> crate::services::agent_connections::Receipt {
    crate::services::agent_connections::Receipt {
        app: app.into(),
        token: format!("synthetic-{app}"),
        model: alias.into(),
        model_map: std::collections::BTreeMap::from([
            ("synthetic-model".into(), alias.into()),
            (wire(app, alias), alias.into()),
        ]),
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: None,
    }
}

fn endpoint(protocol: Protocol, model: &str, stream: bool) -> String {
    match protocol {
        Protocol::Anthropic => "/v1/messages".into(),
        Protocol::Chat => "/v1/chat/completions".into(),
        Protocol::Responses => "/v1/responses".into(),
        Protocol::Gemini => format!(
            "/v1beta/models/{model}:{}",
            if stream {
                "streamGenerateContent?alt=sse"
            } else {
                "generateContent"
            }
        ),
    }
}

fn response(protocol: Protocol, call: &str, stream: bool) -> Response {
    let args = json!({"value":1});
    let function = json!({"id":call,"type":"function","function":{"name":"synthetic_tool","arguments":args.to_string()}});
    let item = json!({"type":"function_call","id":format!("item-{call}"),"call_id":call,"name":"synthetic_tool","arguments":args.to_string(),"status":"completed"});
    let block = json!({"type":"tool_use","id":call,"name":"synthetic_tool","input":args});
    let complete = match protocol {
        Protocol::Chat => {
            json!({"id":format!("reply-{call}"),"object":"chat.completion","model":"exact-model","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[function.clone()]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})
        }
        Protocol::Responses => {
            json!({"id":format!("reply-{call}"),"object":"response","status":"completed","model":"exact-model","output":[item.clone()],"usage":{"input_tokens":1,"output_tokens":1}})
        }
        Protocol::Anthropic => {
            json!({"id":format!("reply-{call}"),"type":"message","role":"assistant","model":"exact-model","content":[block.clone()],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}})
        }
        Protocol::Gemini => {
            json!({"responseId":format!("reply-{call}"),"modelVersion":"exact-model","candidates":[{"index":0,"content":{"role":"model","parts":[{"functionCall":{"id":call,"name":"synthetic_tool","args":args}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}})
        }
    };
    if !stream {
        return Json(complete).into_response();
    }
    let event = |name: &str, value: Value| format!("event: {name}\ndata: {value}\n\n");
    let data = |value: Value| format!("data: {value}\n\n");
    let frames = match protocol {
        Protocol::Chat => {
            let mut function = function;
            function["index"] = json!(0);
            format!(
                "{}{}data: [DONE]\n\n",
                data(
                    json!({"id":format!("reply-{call}"),"object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[function]},"finish_reason":null}]})
                ),
                data(
                    json!({"id":format!("reply-{call}"),"object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
                )
            )
        }
        Protocol::Responses => format!(
            "{}{}",
            event(
                "response.output_item.added",
                json!({"type":"response.output_item.added","output_index":0,"item":item})
            ),
            event(
                "response.completed",
                json!({"type":"response.completed","response":complete})
            )
        ),
        Protocol::Anthropic => format!(
            "{}{}{}{}{}{}",
            event(
                "message_start",
                json!({"type":"message_start","message":{"id":format!("reply-{call}"),"type":"message","role":"assistant","model":"exact-model","content":[],"usage":{"input_tokens":1,"output_tokens":0}}})
            ),
            event(
                "content_block_start",
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call,"name":"synthetic_tool","input":{}}})
            ),
            event(
                "content_block_delta",
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args.to_string()}})
            ),
            event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":0})
            ),
            event(
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1}})
            ),
            event("message_stop", json!({"type":"message_stop"}))
        ),
        Protocol::Gemini => data(complete),
    };
    ([(header::CONTENT_TYPE, "text/event-stream")], frames).into_response()
}

struct Case {
    app: &'static str,
    stream: bool,
    named: bool,
    session: String,
    body: Value,
}
impl Case {
    fn advance(&mut self, protocol: Protocol, call: &str) -> TestResult {
        let args = json!({"value":1});
        let (field, items) = match protocol {
            Protocol::Chat => (
                "messages",
                vec![
                    json!({"role":"assistant","content":null,"tool_calls":[{"id":call,"type":"function","function":{"name":"synthetic_tool","arguments":args.to_string()}}]}),
                    json!({"role":"tool","tool_call_id":call,"content":"Synthetic tool result"}),
                ],
            ),
            Protocol::Responses => (
                "input",
                vec![
                    json!({"type":"function_call","call_id":call,"name":"synthetic_tool","arguments":args.to_string()}),
                    json!({"type":"function_call_output","call_id":call,"output":"Synthetic tool result"}),
                ],
            ),
            Protocol::Anthropic => (
                "messages",
                vec![
                    json!({"role":"assistant","content":[{"type":"tool_use","id":call,"name":"synthetic_tool","input":args}]}),
                    json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call,"content":"Synthetic tool result"}]}),
                ],
            ),
            Protocol::Gemini => (
                "contents",
                vec![
                    json!({"role":"model","parts":[{"functionCall":{"id":call,"name":"synthetic_tool","args":args}}]}),
                    json!({"role":"user","parts":[{"functionResponse":{"id":call,"name":"synthetic_tool","response":{"result":"Synthetic tool result"}}}]}),
                ],
            ),
        };
        self.body[field]
            .as_array_mut()
            .ok_or("missing history")?
            .extend(items);
        Ok(())
    }

    fn request(
        &self,
        client: &reqwest::Client,
        port: u16,
        protocol: Protocol,
    ) -> reqwest::RequestBuilder {
        let path = match self.app {
            "codex-native" => "/backend-api/codex/responses".into(),
            "codex-menu" => "/intent/v1/responses".into(),
            app => format!(
                "/connections/{app}{}",
                endpoint(protocol, "synthetic-model", self.stream)
            ),
        };
        let token = if self.app == "codex-menu" {
            "synthetic-local-token".into()
        } else {
            format!("synthetic-{}", self.app)
        };
        let request = client
            .post(format!("http://127.0.0.1:{port}{path}"))
            .bearer_auth(token)
            .header("version", "0.167.0");
        if self.named {
            request
                .header("x-session-id", &self.session)
                .header("session-id", &self.session)
        } else {
            request
        }
    }
}

fn cases(protocol: Protocol, alias: &str) -> Vec<Case> {
    let mut apps = agents();
    if protocol == Protocol::Responses {
        apps.extend(["codex-menu", "codex-native"]);
    }
    let mut cases = Vec::new();
    for app in apps {
        for stream in [false, true] {
            for named in [false, true] {
                let session = format!("synthetic-{app}-{stream}-{named}");
                let mut body = json!({"model":wire(app,alias),"stream":stream,"metadata":{"fixture":session},"temperature":0.25});
                match protocol {
                    Protocol::Chat | Protocol::Anthropic => {
                        body["messages"] = json!([{"role":"user","content":session}]);
                        body["max_tokens"] = json!(256);
                    }
                    Protocol::Responses => {
                        body["input"] = json!([{"role":"user","content":session}]);
                        body["max_output_tokens"] = json!(256);
                    }
                    Protocol::Gemini => {
                        body["contents"] = json!([{"role":"user","parts":[{"text":session}]}]);
                        body["generationConfig"] =
                            json!({"temperature":0.25,"maxOutputTokens":256});
                    }
                }
                cases.push(Case {
                    app,
                    stream,
                    named,
                    session,
                    body,
                });
            }
        }
    }
    cases
}

async fn publish(runtime: &Arc<Runtime>, menu: Menu) -> TestResult {
    let alias = menu.groups.keys().next().ok_or("missing alias")?.clone();
    runtime.set_global(menu, 0)?;
    let menu = runtime.global_snapshot()?.ok_or("missing published menu")?;
    runtime.set_menu(menu.clone())?;
    runtime.native.publish_menu(
        menu,
        json!({"models":[{"slug":"synthetic-model"}]}),
        std::collections::BTreeMap::from([("synthetic-model".into(), alias.clone())]),
    )?;
    for app in agents() {
        runtime.connections.install(&receipt(app, &alias))?;
    }
    Ok(())
}

async fn server(
    menu: Menu,
    root: &Path,
) -> Result<(ProxyServer, Arc<Runtime>, u16), Box<dyn std::error::Error>> {
    let runtime = Arc::new(Runtime::default());
    runtime
        .prepare_continuations(root.join("continuations.json"))
        .await;
    runtime
        .native
        .prepare_checkpoints(&root.join("checkpoint.key"))?;
    publish(&runtime, menu).await?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    Ok((server, runtime, info.port))
}

fn earlier_key(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .is_some_and(|value| value == "Bearer synthetic-earlier")
        || headers
            .get("x-api-key")
            .is_some_and(|value| value == "synthetic-earlier")
        || headers
            .get("x-goog-api-key")
            .is_some_and(|value| value == "synthetic-earlier")
}

async fn exercise(protocol: Protocol, use_expiry: bool) -> TestResult {
    let _security = Security((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let root = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut received) = tokio::sync::mpsc::channel(4);
    let sends = Arc::new(AtomicUsize::new(0));
    let count = sends.clone();
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new()
                .route(
                    "/v1/usage",
                    get(|headers: HeaderMap| async move {
                        let seconds = if earlier_key(&headers) { 3600 } else { 7200 };
                        let expires = chrono::Utc::now() + chrono::Duration::seconds(seconds);
                        Json(json!({
                            "mode":"unrestricted","isValid":true,"remaining":1,
                            "subscription":{"expires_at":expires.to_rfc3339()},
                        }))
                    }),
                )
                .route(
                    "/*path",
                    post(
                        move |OriginalUri(uri): OriginalUri,
                              headers: HeaderMap,
                              Json(body): Json<Value>| {
                            let tx = tx.clone();
                            let count = count.clone();
                            async move {
                                let call = format!(
                                    "synthetic-call-{}",
                                    count.fetch_add(1, Ordering::SeqCst)
                                );
                                let stream = body["stream"] == true
                                    || uri.path().ends_with(":streamGenerateContent");
                                let reply = response(protocol, &call, stream);
                                let _ = tx.send((body, call, earlier_key(&headers))).await;
                                reply
                            }
                        },
                    ),
                ),
        )
        .await;
    });
    let base = format!("http://{address}");
    let mut menu = global_tests::menu(&format!(
        "{base}{}",
        endpoint(protocol, "exact-model", false)
    ));
    let route = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing route")?);
    route.group.levels.clear();
    route.group.routing = crate::services::intent_routing::RoutingStrategy::Ordered;
    let mut provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"base_url":format!("{base}/v1"),"auth":{"OPENAI_API_KEY":"synthetic-upstream"},"env":{"ANTHROPIC_BASE_URL":base,"ANTHROPIC_AUTH_TOKEN":"synthetic-upstream","GOOGLE_GEMINI_BASE_URL":base,"GEMINI_API_KEY":"synthetic-upstream"},"apiFormat":protocol.format(),"modelCatalog":{"models":[{"model":"exact-model"}]},"config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='{base}/v1'\nwire_api='{}'\n",if protocol==Protocol::Chat {"chat"}else{"responses"})}),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some(protocol.format().into()),
        ..Default::default()
    });
    if use_expiry {
        use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
        use crate::services::intent_routing::catalog::freeze_execution;
        use crate::services::intent_routing::{QuotaOrder, RoutingStrategy};
        route.group.routing = RoutingStrategy::Ordered;
        route.group.quota_order = Some(QuotaOrder::EarliestExpiry);
        route.group.affinity = "turn".into();

        let mut member = route.group.members[0].clone();
        member.id = "earlier".into();
        member.provider_id = "earlier".into();
        route.group.fallback_member_ids.push(member.id.clone());
        route.group.members.push(member);

        let mut earlier_provider = provider.clone();
        earlier_provider.id = "earlier".into();
        earlier_provider.settings_config["auth"]["OPENAI_API_KEY"] = json!("synthetic-earlier");
        earlier_provider.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] =
            json!("synthetic-earlier");
        earlier_provider.settings_config["env"]["GEMINI_API_KEY"] = json!("synthetic-earlier");
        let mut earlier = route.targets[0].clone();
        earlier.provider_id = "earlier".into();
        earlier.headers.insert(
            header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer synthetic-earlier"),
        );
        earlier.upstream = Some(earlier_provider);
        earlier.scheduling = Arc::default();
        route.targets.push(earlier);

        // Wait for the detached read to complete before checking the warm route.
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let now = chrono::Utc::now().timestamp();
                let expiry: Vec<_> = route
                    .targets
                    .iter()
                    .map(|target| {
                        crate::services::routing_provider_quota::observe_expiry(
                            &target.url,
                            &target.headers,
                            now,
                        )
                    })
                    .collect();
                if let [Some(later), Some(earlier)] = expiry.as_slice() {
                    assert!(earlier < later);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;

        let now = chrono::Utc::now().timestamp();
        for (id, official) in [("exhausted-codex", true), ("exhausted-claude", false)] {
            let mut member = route.group.members[0].clone();
            member.id = id.into();
            member.provider_id = id.into();
            route.group.fallback_member_ids.push(member.id.clone());
            route.group.members.push(member);

            let mut target = route.targets[0].clone();
            target.provider_id = id.into();
            target.official = official;
            target.scheduling = Arc::new(scheduling::Scheduling::new(official));
            target.scheduling.claim_budget_refresh(now);
            if official {
                let manager = Arc::new(CodexOAuthManager::new(root.path().to_owned()));
                manager
                    .add_test_account_with_user_identity(
                        "fixture-codex",
                        "synthetic-usage-token",
                        "fixture-user",
                    )
                    .await?;
                let mut account = crate::provider::Provider::with_id(
                    id.into(),
                    "Synthetic Codex".into(),
                    json!({}),
                    None,
                );
                account.category = Some("official".into());
                crate::services::codex_provider::bind(&mut account, "fixture-codex");
                target.credentials = Some(Arc::new(
                    freeze_execution(&account, manager)
                        .await
                        .ok_or("missing fixture Codex identity")?,
                ));
                target.scheduling.observe_event(
                    &json!({"type":"codex.rate_limits","rate_limits":{
                        "primary":{"window_minutes":300,"used_percent":100,"reset_at":now+1800},
                        "secondary":{"window_minutes":10080,"used_percent":10,"reset_at":now+7200}
                    }}),
                    now,
                );
            } else {
                let mut bound = provider.clone();
                bound.id = id.into();
                bound.settings_config["routingSubscription"] =
                    json!({"provider":"claude","accountId":"synthetic-quota-account"});
                target.upstream = Some(bound);
                target.scheduling.observe_claude_quota(
                    crate::services::native_subscription::ClaudeRoutingQuota {
                        tiers: vec![crate::services::subscription::QuotaTier {
                            name: "five_hour".into(),
                            utilization: 100.0,
                            resets_at: chrono::DateTime::from_timestamp(now + 1800, 0)
                                .map(|time| time.to_rfc3339()),
                            used_value_usd: None,
                            max_value_usd: None,
                        }],
                        short_only: false,
                    },
                    now,
                );
            }
            route.targets.push(target);
        }
    }
    route.targets[0].upstream = Some(provider);
    let alias = route.group.alias();
    menu.models.insert(
        "fixture".into(),
        MenuModel {
            alias: alias.clone(),
            label: "Synthetic model".into(),
            description: "VaryAI routing group",
            order: 0,
        },
    );
    let (mut proxy, mut runtime, mut port) = server(menu, root.path()).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut cases = cases(protocol, &alias);
    for phase in 0..3 {
        if phase > 0 {
            runtime.flush_continuations().await;
            let mut next = (*runtime.global_snapshot()?.ok_or("missing menu")?).clone();
            let route = Arc::make_mut(next.groups.values_mut().next().ok_or("missing route")?);
            route.bindings = Arc::default();
            route.targets[0]
                .upstream
                .as_mut()
                .ok_or("missing provider")?
                .settings_config["modelCatalog"] = json!({"models":[{"model":"exact-model","contextWindow":128000,"reasoningLevels":["low","high"]}],"fixtureRefresh":phase});
            if phase == 1 {
                publish(&runtime, next).await?;
            } else {
                proxy.stop().await?;
                (proxy, runtime, port) = server(next, root.path()).await?;
            }
        }
        for case in &mut cases {
            let reply = case
                .request(&client, port, protocol)
                .json(&case.body)
                .send()
                .await?;
            assert_eq!(
                reply.status(),
                StatusCode::OK,
                "agent={} protocol={} phase={phase} stream={} named={}",
                case.app,
                protocol.format(),
                case.stream,
                case.named
            );
            let text = reply.text().await?;
            let (sent, call, used_earlier) =
                tokio::time::timeout(Duration::from_secs(2), received.recv())
                    .await?
                    .ok_or("missing upstream request")?;
            assert_eq!(
                used_earlier, use_expiry,
                "incorrect allowance preference for {}",
                case.app
            );
            assert!(
                text.contains(&call),
                "the native tool call must reach the client"
            );
            if use_expiry && phase == 0 {
                let traces = runtime.traces.snapshot();
                let row = traces.first().ok_or("missing route trace")?;
                assert_eq!(
                    row.candidates.len(),
                    2,
                    "full subscription was planned for {}",
                    case.app
                );
                assert!(row
                    .candidates
                    .iter()
                    .all(|seat| !seat.provider_id.starts_with("exhausted-")));
            }
            let history = match protocol {
                Protocol::Responses => "input",
                Protocol::Gemini => "contents",
                _ => "messages",
            };
            assert_eq!(
                sent[history], case.body[history],
                "history changed for {}",
                case.app
            );
            assert_eq!(sent["metadata"], case.body["metadata"]);
            if protocol == Protocol::Gemini {
                assert_eq!(sent["generationConfig"], case.body["generationConfig"]);
            } else {
                assert_eq!(sent["temperature"], case.body["temperature"]);
            }
            case.advance(protocol, &call)?;
            if phase == 2 && !case.app.starts_with("codex-") {
                let before = sends.load(Ordering::SeqCst);
                let malformed = case
                    .request(&client, port, protocol)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body("{")
                    .send()
                    .await?;
                assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
                runtime.connections.revoke(case.app);
                let disabled = case
                    .request(&client, port, protocol)
                    .json(&case.body)
                    .send()
                    .await?;
                assert_eq!(disabled.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(sends.load(Ordering::SeqCst), before);
                runtime.connections.install(&receipt(case.app, &alias))?;
            }
        }
    }
    assert_eq!(sends.load(Ordering::SeqCst), cases.len() * 3);
    proxy.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn catalog_continuation_http_messages_all_agents() -> TestResult {
    exercise(Protocol::Anthropic, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn catalog_continuation_http_chat_all_agents() -> TestResult {
    exercise(Protocol::Chat, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn catalog_continuation_http_responses_all_agents_and_codex_entries() -> TestResult {
    exercise(Protocol::Responses, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn catalog_continuation_http_gemini_all_agents() -> TestResult {
    exercise(Protocol::Gemini, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn expiring_subscription_template_preserves_native_wire_and_continuations() -> TestResult {
    for protocol in [
        Protocol::Anthropic,
        Protocol::Chat,
        Protocol::Responses,
        Protocol::Gemini,
    ] {
        exercise(protocol, true).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn responses_model_switch_named_item_recovery_all_connection_entries() -> TestResult {
    use crate::proxy::security::{self, runtime::CompiledCustomRule, AuditAction, RiskLevel};
    let _security = Security((*security::get_security_runtime()).clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().fallback(post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            async move {
                if let Ok(mut seen) = captured.lock() { seen.push(body.clone()); }
                if body["input"].as_array().into_iter().flatten().any(|item| item["id"] == "rs_missing") {
                    return (StatusCode::NOT_FOUND, Json(json!({"error":{"message":"Item with id 'rs_missing' not found. Items are not persisted when `store` is set to false."}}))).into_response();
                }
                let call = format!("call_{}", body["metadata"]["fixture"].as_str().unwrap_or("synthetic"));
                response(Protocol::Responses, &call, body["stream"] == true)
            }
        }))).await;
    });
    let base = format!("http://{address}");
    let mut menu = global_tests::menu(&format!("{base}/v1/responses"));
    let alias = menu.groups.keys().next().ok_or("missing alias")?.clone();
    let route = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing route")?);
    route.group.levels.clear();
    route.group.routing = crate::services::intent_routing::RoutingStrategy::Ordered;
    let mut provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({
            "base_url":format!("{base}/v1"), "auth":{"OPENAI_API_KEY":"synthetic-upstream"},
            "config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='{base}/v1'\nwire_api='responses'\n")
        }),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".into()),
        ..Default::default()
    });
    route.targets[0].upstream = Some(provider);
    let root = tempfile::tempdir()?;
    let (proxy, _runtime, port) = server(menu, root.path()).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for restore in [false, true] {
        security::runtime::set_security_runtime_for_test(security::SecurityRuntimeConfig {
            enabled: restore,
            custom_rules: vec![CompiledCustomRule {
                id: "synthetic-replay".into(),
                name: "Synthetic replay rule".into(),
                rule_type: "regex".into(),
                pattern: "synthetic-replay-secret".into(),
                regex: regex::Regex::new("synthetic-replay-secret").ok(),
                risk_level: RiskLevel::High,
                action: AuditAction::RedactRestore,
                enabled: true,
            }],
            ..Default::default()
        });
        for mut case in cases(Protocol::Responses, &alias) {
            case.session.push_str(&format!("-restore-{restore}"));
            case.body["metadata"]["fixture"] = json!(case.session);
            case.body["input"][0]["content"] = json!(case.session);
            let call = format!("call_{}", case.session);
            // Populate an actual completed native call before replaying its result.
            let reply = case
                .request(&client, port, Protocol::Responses)
                .json(&case.body)
                .send()
                .await?;
            assert_eq!(
                reply.status(),
                StatusCode::OK,
                "first turn: {} restore={restore}",
                case.app
            );
            let _ = reply.bytes().await?;
            let input = json!([
                case.body["input"][0],
                {"type":"function_call","id":format!("item-{call}"),"call_id":call,"name":"synthetic_tool","arguments":"{\"value\":1}","status":"completed"},
                {"type":"function_call_output","call_id":call,"output":"Synthetic result"},
                {"type":"reasoning","id":"rs_missing","summary":[],"encrypted_content":null},
                {"role":"user","content":"Synthetic continue synthetic-replay-secret"}
            ]);
            case.body["store"] = json!(false);
            case.body["input"] = input;
            let before = seen.lock().map_err(|_| "synthetic lock poisoned")?.len();
            let reply = case
                .request(&client, port, Protocol::Responses)
                .json(&case.body)
                .send()
                .await?;
            assert_eq!(
                reply.status(),
                StatusCode::OK,
                "replay: {} restore={restore} stream={} named={}",
                case.app,
                case.stream,
                case.named
            );
            let _ = reply.bytes().await?;
            let seen = seen.lock().map_err(|_| "synthetic lock poisoned")?;
            assert_eq!(seen.len() - before, 2, "{} restore={restore}", case.app);
            let first = &seen[before];
            let last = &seen[before + 1];
            let mut expected = first.clone();
            expected["input"]
                .as_array_mut()
                .ok_or("synthetic input missing")?
                .remove(3);
            assert_eq!(last, &expected, "only the named reasoning item may change");
            if restore {
                assert!(!last["input"][3]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("synthetic-replay-secret"));
            }
        }
    }
    proxy.stop().await?;
    upstream.abort();
    Ok(())
}
