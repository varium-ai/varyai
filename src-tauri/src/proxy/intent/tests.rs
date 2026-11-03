use super::*;
use serde_json::json;

#[tokio::test]
#[serial_test::serial]
async fn codex_account_pool_falls_back_within_one_member_and_pins_the_answering_account(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{response::IntoResponse, routing::post, Json, Router};
    use http::HeaderMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let (a, b) = (first.clone(), second.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let (a, b) = (a.clone(), b.clone());
            async move {
                assert_eq!(body["model"], "exact-model");
                if headers.get("x-synthetic-account").and_then(|v| v.to_str().ok()) == Some("first") {
                    a.fetch_add(1, Ordering::SeqCst);
                    return (StatusCode::TOO_MANY_REQUESTS, "synthetic quota exhausted").into_response();
                }
                b.fetch_add(1, Ordering::SeqCst);
                Json(json!({"id":"synthetic-response","model":"exact-model","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}]})).into_response()
            }
        }))).await;
    });
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"account-pool", "name":"Synthetic pool", "revision":0, "enabled":false,
        "members":[{"id":"one-member","providerId":"fixture","modelId":"exact-model"}],
        "defaultMemberId":"one-member", "fallbackMemberIds":[], "intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"patience":-1
    }))?;
    let targets = ["first", "second"]
        .into_iter()
        .map(|account| Target {
            fixed_effort: None,
            provider_id: "fixture".into(),
            model: "exact-model".into(),
            upstream: None,
            effort: Default::default(),
            url: format!("http://{address}/responses")
                .parse()
                .expect("fixture URL"),
            headers: HeaderMap::from_iter([(
                http::header::HeaderName::from_static("x-synthetic-account"),
                http::HeaderValue::from_static(account),
            )]),
            official: false,
            credentials: None,
            scheduling: Arc::new(scheduling::Scheduling::new(false)),
        })
        .collect();
    let runtime = Arc::new(Runtime::default());
    let token = runtime.install(
        Session {
            group: group.clone(),
            targets,
            classifier: None,
            client: reqwest::Client::builder().no_proxy().build()?,
            created: Instant::now(),
            bindings: Arc::default(),
        },
        None,
    )?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for previous in [None, Some("synthetic-response")] {
        let mut body = json!({"model":group.alias(),"input":"synthetic"});
        if let Some(previous) = previous {
            body["previous_response_id"] = json!(previous);
        }
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/intent/v1/responses",
                info.port
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let _: Value = response.json().await?;
    }
    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(second.load(Ordering::SeqCst), 2);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn codex_group_and_temporary_routes_preserve_bound_gateway_policies(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        database::Database,
        proxy::{
            gateway::{KeyPolicy, Period},
            server::ProxyServer,
            ProxyConfig,
        },
    };
    use axum::{routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |Json(body): Json<Value>| {
            observed.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["model"], "synthetic-upstream-model");
            std::future::ready(Json(json!({"model":"supplier-echo", "usage":{"input_tokens":7,"output_tokens":3}})))
        }))).await;
    });
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"policy-group", "name":"Synthetic group", "revision":0,
        "client":"codex", "enabled":false,
        "members":[{"id":"a","providerId":"synthetic-provider","modelId":"synthetic-upstream-model"}],
        "defaultMemberId":"a", "fallbackMemberIds":[], "intentRecognitionEnabled":false,
        "classifierTarget":null, "classificationBudgetMs":2000, "rules":[]
    }))?;
    let alias = group.alias();
    let session = || Session {
        group: group.clone(),
        targets: vec![Target {
            fixed_effort: None,
            provider_id: "synthetic-provider".into(),
            model: "synthetic-upstream-model".into(),
            upstream: None,
            effort: Default::default(),
            url: format!("http://{address}/responses")
                .parse()
                .expect("fixture URL"),
            headers: Default::default(),
            official: false,
            credentials: None,
            scheduling: Arc::new(scheduling::Scheduling::new(false)),
        }],
        classifier: None,
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("fixture client"),
        created: Instant::now(),
        bindings: Arc::default(),
    };
    let runtime = Arc::new(Runtime::default());
    let temporary = runtime.install(session(), None).expect("temporary session");
    runtime
        .set_menu(Arc::new(Menu {
            token: concat!("synthetic", "-menu-token").into(),
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::from([(alias.clone(), Arc::new(session()))]),
        }))
        .expect("menu");
    let db = Arc::new(Database::memory()?);
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic Codex policy".into(),
        models: vec![alias.clone()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    })?;
    db.bind_gateway_agent("codex", Some(&key.key.id))?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let tokens = [temporary.as_str(), concat!("synthetic", "-menu-token")];
    for token in tokens {
        let response = client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({"model":"not-allowed", "input":"synthetic"}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: Value = response.json().await?;
        assert_eq!(error["error"]["code"], "model_not_allowed");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for token in tokens {
        let response = client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({"model":alias, "input":"synthetic"}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let reply: Value = response.json().await?;
        assert_eq!(reply["model"], "supplier-echo");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = db.list_gateway_usage(Some(&key.key.id)).expect("usage");
            if rows.iter().any(|row| row.requests == 2 && row.tokens == 20) {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].agent, "codex");
                assert_eq!(rows[0].model, alias);
                assert_eq!(
                    rows[0].upstream_model.as_deref(),
                    Some("synthetic-upstream-model")
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let mut policy = key.key.policy;
    policy.token_limit = Some(0);
    db.update_gateway_key(&key.key.id, policy)?;
    for token in tokens {
        let response = client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({"model":alias, "input":"synthetic"}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn unknown_or_conflicting_continuations_fail_closed() {
    let mut bindings = Bindings::default();
    assert!(bindings
        .resolve(&json!({"previous_response_id":"unknown"}), None)
        .is_err());
    assert!(bindings
        .resolve(
            &json!({"input":[{"type":"function_call_output","call_id":"unknown","output":"ok"}]}),
            None
        )
        .is_err());
    remember(&mut bindings.responses, "response", "r1", 0);
    remember(&mut bindings.calls, "call", "c1", 1);
    assert!(bindings.resolve(&json!({"previous_response_id":"r1","input":[{"type":"function_call_output","call_id":"c1"}]}), None).is_err());
    assert_eq!(
        bindings.resolve(&json!({"previous_response_id":"r1"}), None),
        Ok(Some(0))
    );
}

#[test]
fn response_and_tool_ids_pin_exact_candidate() {
    let mut bindings = Bindings::default();
    bindings.observe(&json!({"type":"response.completed","response":{"id":"resp","output":[{"type":"function_call","call_id":"call"}]}}), 2);
    assert_eq!(
        bindings.resolve(&json!({"previous_response_id":"resp"}), None),
        Ok(Some(2))
    );
    assert_eq!(bindings.resolve(&json!({"input":[{"type":"function_call_output","call_id":"call","output":"synthetic"}]}), None), Ok(Some(2)));
    assert!(bindings
        .resolve(
            &json!({"input":[{"type":"reasoning","encrypted_content":"opaque"}]}),
            None
        )
        .is_err());
    bindings.bind_session("session", 2);
    assert_eq!(
        bindings.resolve(
            &json!({"input":[{"type":"reasoning","encrypted_content":"opaque"}]}),
            Some("session")
        ),
        Ok(Some(2))
    );
}

#[test]
fn fragmented_sse_records_ids_without_rewriting_frames() {
    let mut bindings = Bindings::default();
    let mut observer = Observer::default();
    for bytes in b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"r\"}}\r\n\r\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c\"}}\n\n".chunks(3) {
        observer.push(bytes, &mut bindings, 1);
    }
    assert_eq!(
        bindings.responses.get(&continuation::hash("response", "r")),
        Some(&1)
    );
    assert_eq!(
        bindings.calls.get(&continuation::hash("call", "c")),
        Some(&1)
    );
}

#[test]
fn scheduling_observer_ignores_headers_and_hidden_reasoning_and_counts_usage_on_drop() {
    let scheduling = Arc::new(scheduling::Scheduling::new(false));
    let start = Instant::now() - Duration::from_secs(2);
    let mut observer = Observer::default();
    observer.scheduling = Some(scheduling.clone());
    observer.started = Some(start);
    let mut bindings = Bindings::default();
    observer.observe_at(
        &json!({"type":"response.created","response":{"id":"r"}}),
        &mut bindings,
        0,
        start + Duration::from_millis(100),
    );
    assert!(observer.first_content.is_none());
    observer.observe_at(
        &json!({"type":"response.reasoning_summary_text.delta","delta":"synthetic"}),
        &mut bindings,
        0,
        start + Duration::from_millis(300),
    );
    observer.observe_at(
        &json!({"type":"response.output_text.delta","delta":"synthetic"}),
        &mut bindings,
        0,
        start + Duration::from_millis(1000),
    );
    observer.observe_at(
        &json!({"type":"response.output_text.delta","delta":" reply"}),
        &mut bindings,
        0,
        start + Duration::from_millis(2000),
    );
    observer.observe_at(&json!({"type":"response.completed","response":{"usage":{"input_tokens":20,"output_tokens":120,"output_tokens_details":{"reasoning_tokens":100}}}}), &mut bindings, 0, start + Duration::from_millis(2100));
    let sample = observer.sample();
    assert_eq!(sample, (140, 300.0, 1100.0, 20));
    drop(observer);
    let other = scheduling::Scheduling::new(false);
    other.served(10, 100.0, 1000.0, 20, chrono::Utc::now().timestamp());
    assert_eq!(
        scheduling::arrange(
            crate::services::intent_routing::RoutingStrategy::LeastUsed,
            &[0, 1],
            &[scheduling.snapshot(), other.snapshot()],
            &mut Default::default(),
            chrono::Utc::now().timestamp()
        ),
        [1, 0]
    );
}

#[test]
fn non_streaming_or_burst_responses_never_produce_a_decode_rate() {
    let start = Instant::now();
    let mut observer = Observer::default();
    let mut bindings = Bindings::default();
    observer.observe_at(
        &json!({"usage":{"input_tokens":10,"output_tokens":100}}),
        &mut bindings,
        0,
        start,
    );
    assert_eq!(observer.sample(), (110, 0.0, 0.0, 100));
    observer.started = Some(start);
    for at in [1000, 1010] {
        observer.observe_at(
            &json!({"type":"response.output_text.delta","delta":"synthetic"}),
            &mut bindings,
            0,
            start + Duration::from_millis(at),
        );
    }
    observer.observe_at(
        &json!({"type":"response.completed","response":{"usage":{"output_tokens":100}}}),
        &mut bindings,
        0,
        start + Duration::from_millis(2000),
    );
    assert_eq!(observer.sample().2, 0.0);

    let mut held = Observer::default();
    held.started = Some(start);
    for (at, delta) in [
        (100, "x".to_owned()),
        (1000, "x".repeat(1000)),
        (1200, "x".to_owned()),
    ] {
        held.observe_at(
            &json!({"type":"response.output_text.delta","delta":delta}),
            &mut bindings,
            0,
            start + Duration::from_millis(at),
        );
    }
    held.observe_at(
        &json!({"type":"response.completed","response":{"usage":{"output_tokens":200}}}),
        &mut bindings,
        0,
        start + Duration::from_millis(1250),
    );
    assert_eq!(held.sample().2, 0.0);
}

#[test]
fn refreshed_groups_share_account_telemetry_and_keep_only_exact_model_latency() {
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"shared","name":"Shared","revision":0,"client":"codex","enabled":false,
        "members":[{"id":"a","providerId":"provider","modelId":"model-a"}],"defaultMemberId":"a",
        "fallbackMemberIds":[],"intentRecognitionEnabled":false,"classifierTarget":null,
        "classificationBudgetMs":2000,"rules":[]
    }))
    .expect("group");
    let target = Target {
        fixed_effort: None,
        provider_id: "provider".into(),
        model: "model-a".into(),
        upstream: None,
        effort: Default::default(),
        url: "http://synthetic.invalid/responses".parse().expect("url"),
        headers: Default::default(),
        official: false,
        credentials: None,
        scheduling: Arc::new(scheduling::Scheduling::new(false)),
    };
    let now = chrono::Utc::now().timestamp();
    target.scheduling.served(100, 100.0, 1000.0, 20, now);
    let make = |alias: &str, target: Target| {
        (
            alias.to_owned(),
            Arc::new(Session {
                group: group.clone(),
                targets: vec![target],
                classifier: None,
                client: reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("client"),
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        )
    };
    let previous = Menu {
        token: "original".into(),
        models: Default::default(),
        default_model: None,
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([make("direct", target.clone())]),
    };
    let mut other = target.clone();
    other.model = "model-b".into();
    other.scheduling = Arc::new(scheduling::Scheduling::new(false));
    let mut rotated = target.clone();
    rotated.headers.insert(
        header::AUTHORIZATION,
        http::HeaderValue::from_static("Bearer synthetic-new-account"),
    );
    rotated.scheduling = Arc::new(scheduling::Scheduling::new(false));
    let mut fresh = target.clone();
    fresh.scheduling = Arc::new(scheduling::Scheduling::new(false));
    let mut next = Menu {
        token: "new".into(),
        models: Default::default(),
        default_model: None,
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([
            make("direct", fresh.clone()),
            make("added-group", fresh),
            make("other-model", other),
            make("rotated", rotated),
        ]),
    };
    next.preserve_from(&previous);
    assert_eq!(next.token, "original");
    assert!(Arc::ptr_eq(
        &next.groups["direct"].bindings,
        &previous.groups["direct"].bindings
    ));
    assert!(!Arc::ptr_eq(
        &next.groups["added-group"].bindings,
        &previous.groups["direct"].bindings
    ));
    assert!(Arc::ptr_eq(
        &next.groups["added-group"].targets[0].scheduling,
        &previous.groups["direct"].targets[0].scheduling
    ));
    let a = &next.groups["added-group"].targets[0].scheduling;
    let b = &next.groups["other-model"].targets[0].scheduling;
    assert!(!Arc::ptr_eq(a, b));
    b.served(100, 0.0, 0.0, 0, now);
    let empty = scheduling::Scheduling::default();
    empty.served(150, 0.0, 0.0, 0, now);
    // Both models see the same 200 recent tokens; rotated credentials see none.
    let states = [
        a.snapshot(),
        b.snapshot(),
        next.groups["rotated"].targets[0].scheduling.snapshot(),
        empty.snapshot(),
    ];
    let order = scheduling::arrange(
        crate::services::intent_routing::RoutingStrategy::LeastUsed,
        &[0, 1, 2, 3],
        &states,
        &mut Default::default(),
        now,
    );
    assert_eq!(&order[..2], &[2, 3]);
    // Only the exact original model retains its latency measurement.
    assert_eq!(
        scheduling::arrange(
            crate::services::intent_routing::RoutingStrategy::Fastest,
            &[0, 1],
            &states,
            &mut Default::default(),
            now
        ),
        [0, 1]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn all_automatic_strategies_reach_their_selected_exact_upstream() {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
        services::intent_routing::RoutingStrategy,
    };
    use axum::{routing::post, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/responses",
                post(|Json(body): Json<Value>| async move {
                    Json(
                        json!({"model":body["model"],"usage":{"input_tokens":5,"output_tokens":5}}),
                    )
                }),
            ),
        )
        .await
        .expect("fixture");
    });
    let runtime = Arc::new(Runtime::default());
    let now = chrono::Utc::now().timestamp();
    let strategies = [
        RoutingStrategy::Smart,
        RoutingStrategy::Ordered,
        RoutingStrategy::RoundRobin,
        RoutingStrategy::LeastUsed,
        RoutingStrategy::Pace,
        RoutingStrategy::FreeFirst,
        RoutingStrategy::Fastest,
        RoutingStrategy::Weighted,
    ];
    let mut groups = HashMap::new();
    for (k, routing) in strategies.iter().enumerate() {
        let mut group:RoutingGroup=serde_json::from_value(json!({
            "id":format!("strategy-{k}"),"name":"Synthetic strategy","revision":0,"client":"codex","enabled":false,
            "members":[{"id":"a","providerId":"a","modelId":"exact-a","weight":3},{"id":"b","providerId":"b","modelId":"exact-b","weight":1,"free":true}],
            "defaultMemberId":"a","fallbackMemberIds":["b"],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        })).expect("group");
        group.routing = *routing;
        let targets=group.members.iter().enumerate().map(|(i,m)| {
            let scheduling=Arc::new(scheduling::Scheduling::new(false));
            if matches!(routing,RoutingStrategy::Smart|RoutingStrategy::Pace) {
                scheduling.observe_event(&json!({"type":"codex.rate_limits","rate_limits":{"primary":{"used_percent":if i==0 {20} else {80},"window_minutes":10080,"reset_at":now+if i==0 {360000} else {3600}}}}),now);
            }
            if *routing==RoutingStrategy::LeastUsed && i==0 { scheduling.served(1000,0.0,0.0,0,now); }
            if *routing==RoutingStrategy::Fastest { scheduling.served(1,if i==0 {100.0} else {200.0},1000.0,if i==0 {10} else {100},now); }
            Target { fixed_effort: None,provider_id:m.provider_id.clone(),upstream: None, model:m.model_id.clone(),effort:Default::default(),url:format!("http://{address}/responses").parse().expect("url"),headers:Default::default(),official:false,credentials:None,scheduling}
        }).collect();
        groups.insert(
            group.alias(),
            Arc::new(Session {
                group,
                targets,
                classifier: None,
                client: reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("client"),
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        );
    }
    runtime
        .set_menu(Arc::new(Menu {
            token: concat!("synthetic", "-strategies").into(),
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups,
        }))
        .expect("menu");
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory().expect("db")),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await.expect("start");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    for (k, routing) in strategies.iter().enumerate() {
        let mut picks = Vec::new();
        let n = if *routing == RoutingStrategy::Weighted {
            8
        } else if *routing == RoutingStrategy::RoundRobin {
            3
        } else {
            1
        };
        for _ in 0..n {
            let value = client
                .post(format!(
                    "http://127.0.0.1:{}/intent/v1/responses",
                    info.port
                ))
                .bearer_auth(concat!("synthetic", "-strategies"))
                .json(&json!({"model":format!("vary-auto/strategy-{k}"),"input":"synthetic"}))
                .send()
                .await
                .expect("send")
                .json::<Value>()
                .await
                .expect("json");
            picks.push(value["model"].as_str().expect("model").to_owned());
        }
        match routing {
            RoutingStrategy::Ordered => assert_eq!(picks, ["exact-a"]),
            RoutingStrategy::RoundRobin => assert_eq!(picks, ["exact-a", "exact-b", "exact-a"]),
            RoutingStrategy::Weighted => assert_eq!(
                picks,
                [
                    "exact-a", "exact-a", "exact-b", "exact-a", "exact-a", "exact-a", "exact-b",
                    "exact-a"
                ]
            ),
            _ => assert_eq!(picks, ["exact-b"], "{routing:?}"),
        }
    }
    server.stop().await.expect("stop");
    task.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn manual_routes_only_the_pick_pauses_classification_and_never_falls_back() {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
        services::intent_routing::RoutingStrategy,
    };
    use axum::{response::IntoResponse, routing::post, Json, Router};
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = seen.clone();
    let upstream = Router::new().route(
        "/responses",
        post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            async move {
                captured
                    .lock()
                    .expect("seen")
                    .push(body["model"].as_str().expect("model").to_owned());
                if body["input"] == "fail" {
                    (StatusCode::TOO_MANY_REQUESTS, "synthetic").into_response()
                } else {
                    Json(json!({"id":"manual-response", "model":body["model"]})).into_response()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        axum::serve(listener, upstream).await.expect("fixture");
    });
    let mut group: RoutingGroup = serde_json::from_value(json!({
        "id":"development","name":"Manual fixture","revision":0,"client":"codex","enabled":false,
        "members":[{"id":"balanced","providerId":"fixture","modelId":"balanced-model"},{"id":"strong","providerId":"fixture","modelId":"strong-model"}],
        "defaultMemberId":"balanced","fallbackMemberIds":["balanced","strong"],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,
        "rules":[{"id":"debug","name":"Debug","description":"Synthetic diagnosis","enabled":true,"targetMemberId":"balanced"}]
    })).expect("fixture");
    group.routing = RoutingStrategy::Manual;
    group.manual_member_id = Some("strong".into());
    group.intent_recognition_enabled = true;
    // No classifier is available: Manual must never try to prepare or call it.
    group.classifier_target = None;
    let session = Session {
        targets: group
            .members
            .iter()
            .map(|m| Target {
                fixed_effort: None,
                provider_id: m.provider_id.clone(),
                model: m.model_id.clone(),
                upstream: None,
                effort: Default::default(),
                url: format!("http://{address}/responses").parse().expect("url"),
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            })
            .collect(),
        group,
        classifier: None,
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        created: Instant::now(),
        bindings: Arc::default(),
    };
    let runtime = Arc::new(Runtime::default());
    let session = Arc::new(session);
    runtime
        .set_menu(Arc::new(Menu {
            token: concat!("synthetic", "-manual").into(),
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::from([("vary-auto/development".into(), session.clone())]),
        }))
        .expect("menu");
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory().expect("db")),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await.expect("start");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    for input in ["synthetic", "fail"] {
        let response = client
            .post(&url)
            .bearer_auth(concat!("synthetic", "-manual"))
            .json(&json!({"model":"vary-auto/development","input":input}))
            .send()
            .await
            .expect("send");
        if input == "fail" {
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        } else {
            assert_eq!(
                response.json::<Value>().await.expect("json")["model"],
                "strong-model"
            );
        }
    }
    assert_eq!(
        *seen.lock().expect("seen"),
        ["strong-model", "strong-model"]
    );
    // A later manual pick cannot move a response/tool continuation to another account.
    let mut group = session.group.clone();
    group.manual_member_id = Some("balanced".into());
    runtime
        .set_menu(Arc::new(Menu {
            token: concat!("synthetic", "-manual").into(),
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::from([(
                "vary-auto/development".into(),
                Arc::new(Session {
                    group,
                    targets: session.targets.clone(),
                    classifier: None,
                    client: client.clone(),
                    created: Instant::now(),
                    bindings: session.bindings.clone(),
                }),
            )]),
        }))
        .expect("refresh");
    let continued=client.post(&url).bearer_auth(concat!("synthetic", "-manual")).json(&json!({"model":"vary-auto/development","previous_response_id":"manual-response","input":"continue"})).send().await.expect("send").json::<Value>().await.expect("json");
    assert_eq!(continued["model"], "strong-model");
    let next = client
        .post(&url)
        .bearer_auth(concat!("synthetic", "-manual"))
        .json(&json!({"model":"vary-auto/development","input":"next"}))
        .send()
        .await
        .expect("send")
        .json::<Value>()
        .await
        .expect("json");
    assert_eq!(next["model"], "balanced-model");
    server.stop().await.expect("stop");
    task.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn readable_auto_alias_uses_smart_order_but_continuations_ignore_cooldown() {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Json, Router};
    let upstream = Router::new().route("/responses", post(|Json(body): Json<Value>| async move {
        Json(json!({"id":format!("response-{}", body["model"].as_str().unwrap_or("unknown")), "model":body["model"]}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        axum::serve(listener, upstream).await.expect("server");
    });
    let mut group: RoutingGroup = serde_json::from_value(json!({
        "id":"smart","name":"Smart fixture","revision":0,"client":"codex","enabled":false,
        "members":[{"id":"balanced","providerId":"fixture","modelId":"balanced-model"},{"id":"strong","providerId":"fixture","modelId":"strong-model"}],
        "defaultMemberId":"balanced","fallbackMemberIds":["strong"],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
    })).expect("group");
    group.routing = crate::services::intent_routing::RoutingStrategy::Smart;
    group.model_alias = Some("auto/synthetic-model".into());
    group.fallback_member_ids = vec!["strong".into()];
    let now = chrono::Utc::now().timestamp();
    let targets: Vec<_> = group
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let scheduling = Arc::new(scheduling::Scheduling::default());
            let headers = http::HeaderMap::from_iter([
                (
                    "x-codex-primary-used-percent".parse().expect("name"),
                    http::HeaderValue::from_static("20"),
                ),
                (
                    "x-codex-primary-window-minutes".parse().expect("name"),
                    http::HeaderValue::from_static("10080"),
                ),
                (
                    "x-codex-primary-reset-at".parse().expect("name"),
                    http::HeaderValue::from_str(
                        &(now + if i == 0 { 18000 } else { 3600 }).to_string(),
                    )
                    .expect("value"),
                ),
            ]);
            scheduling.observe_headers(&headers, now);
            Target {
                fixed_effort: None,
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                upstream: None,
                effort: Default::default(),
                url: format!("http://{address}/responses").parse().expect("url"),
                headers: http::HeaderMap::new(),
                official: false,
                credentials: None,
                scheduling,
            }
        })
        .collect();
    let preferred_health = targets[1].scheduling.clone();
    let runtime = Arc::new(Runtime::default());
    let token = runtime
        .install(
            Session {
                group,
                targets,
                classifier: None,
                client: reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("client"),
                created: Instant::now(),
                bindings: Arc::default(),
            },
            None,
        )
        .expect("install");
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory().expect("db")),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await.expect("proxy");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let send = |body: Value| client.post(&url).bearer_auth(&token).json(&body).send();
    let first = send(json!({"model":"auto/synthetic-model","input":"synthetic"}))
        .await
        .expect("first")
        .json::<Value>()
        .await
        .expect("json");
    assert_eq!(first["model"], "strong-model");
    preferred_health.failed(&http::HeaderMap::new(), now);
    let continuation = send(json!({"model":"auto/synthetic-model","previous_response_id":"response-strong-model","input":"synthetic"})).await.expect("continuation").json::<Value>().await.expect("json");
    assert_eq!(continuation["model"], "strong-model");
    preferred_health.failed(&http::HeaderMap::new(), now);
    let next = send(json!({"model":"auto/synthetic-model","input":"new synthetic"}))
        .await
        .expect("next")
        .json::<Value>()
        .await
        .expect("json");
    assert_eq!(next["model"], "balanced-model");
    server.stop().await.expect("stop");
    task.abort();
}

#[tokio::test]
#[serial_test::serial]
async fn live_session_forwards_exact_models_and_pins_tools_without_current_provider() {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Json, Router};
    use std::sync::Mutex;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let upstream = Router::new().route("/responses", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            captured.lock().expect("fixture lock").push((headers, body.clone()));
            if body["model"] == "balanced-model" {
                (StatusCode::SERVICE_UNAVAILABLE, "retry synthetic").into_response()
            } else if body["stream"] == true {
                ([(header::CONTENT_TYPE, "text/event-stream")], "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-stream\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"tool-stream\"}]}}\n\n").into_response()
            } else {
                Json(json!({"id":"response-json","model":"strong-model","output":[{"type":"function_call","call_id":"tool-json"}]})).into_response()
            }
        }
    }));
    use axum::response::IntoResponse;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener");
    let address = listener.local_addr().expect("fixture address");
    let task = tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("fixture server");
    });
    let runtime = Arc::new(Runtime::default());
    let mut group: RoutingGroup = serde_json::from_value(json!({
        "id":"development","name":"Fixture","revision":0,"client":"codex","enabled":false,
        "members":[{"id":"balanced","providerId":"fixture","modelId":"balanced-model"},{"id":"strong","providerId":"fixture","modelId":"strong-model"}],
        "defaultMemberId":"balanced","fallbackMemberIds":[],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
    })).expect("fixture group");
    group.fallback_member_ids = vec!["strong".into()];
    let targets = group
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let mut headers = http::HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                http::HeaderValue::from_static(if i == 0 {
                    "Bearer synthetic-a"
                } else {
                    "Bearer synthetic-b"
                }),
            );
            Target { fixed_effort: None,
                provider_id: member.provider_id.clone(),
                upstream: None, model: member.model_id.clone(),
                effort: crate::services::intent_routing::effort::Capability::parse(&json!({"reasoningLevels": if i == 0 { vec!["low", "high"] } else { vec!["low", "medium", "high"] }})),
                url: format!("http://{address}/responses").parse().expect("url"),
                headers,
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            }
        })
        .collect();
    let token = runtime
        .install(
            Session {
                group,
                targets,
                classifier: None,
                client: reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("client"),
                created: Instant::now(),
                bindings: Arc::default(),
            },
            None,
        )
        .expect("session");
    let db = Arc::new(Database::memory().expect("fixture db"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await.expect("proxy");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let send = |body: Value| {
        client
            .post(&url)
            .bearer_auth(&token)
            .header("chatgpt-account-id", "caller-workspace")
            .header("cookie", "caller-cookie")
            .json(&body)
            .send()
    };
    let response = send(json!({"model":"vary-auto/development","input":"synthetic task","stream":false,"reasoning":{"effort":"medium","summary":"auto"},"vendor_extension":{"retain":true}})).await.expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await.expect("response");
    assert_eq!(value["id"], "response-json");
    let response = send(json!({"model":"vary-auto/development","previous_response_id":"response-json","input":[{"type":"function_call_output","call_id":"tool-json","output":"synthetic"}],"stream":true})).await.expect("continuation");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .text()
        .await
        .expect("sse")
        .contains("response-stream"));
    let response = send(json!({"model":"vary-auto/development","previous_response_id":"unknown"}))
        .await
        .expect("unknown continuation");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    {
        let seen = seen.lock().expect("fixture lock");
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].1["model"], "balanced-model");
        assert_eq!(seen[1].1["model"], "strong-model");
        assert_eq!(seen[2].1["model"], "strong-model");
        assert_eq!(seen[1].0[header::AUTHORIZATION], "Bearer synthetic-b");
        assert!(!seen[1].0.contains_key("chatgpt-account-id"));
        assert!(!seen[1].0.contains_key("cookie"));
        assert_eq!(seen[0].1["reasoning"]["effort"], "high");
        assert_eq!(seen[1].1["reasoning"]["effort"], "medium");
        assert_eq!(seen[1].1["reasoning"]["summary"], "auto");
        assert_eq!(seen[1].1["vendor_extension"]["retain"], true);
    }
    let original_session = runtime
        .sessions
        .read()
        .expect("runtime")
        .get(&token)
        .map(|entry| entry.snapshot.clone())
        .expect("session");
    runtime.revoke(&token);
    assert_eq!(
        send(json!({"model":"vary-auto/development","input":"test"}))
            .await
            .expect("revoked")
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut second_group = original_session.group.clone();
    second_group.id = "second".into();
    second_group.members.truncate(1);
    second_group.members[0].provider_id = "second-provider".into();
    second_group.members[0].model_id = "second-exact-model".into();
    second_group.fallback_member_ids.clear();
    let mut second_headers = http::HeaderMap::new();
    second_headers.insert(
        header::AUTHORIZATION,
        http::HeaderValue::from_static("Bearer synthetic-second"),
    );
    let second = Arc::new(Session {
        group: second_group,
        classifier: None,
        targets: vec![Target {
            fixed_effort: None,
            provider_id: "second-provider".into(),
            upstream: None,
            model: "second-exact-model".into(),
            effort: crate::services::intent_routing::effort::Capability::parse(
                &json!({"input_modalities":["text"]}),
            ),
            url: format!("http://{address}/responses").parse().expect("url"),
            headers: second_headers,
            official: false,
            credentials: None,
            scheduling: Arc::default(),
        }],
        client: client.clone(),
        created: Instant::now() - Duration::from_secs(7200),
        bindings: Arc::default(),
    });
    let direct = Arc::new(Session {
        group: second.group.clone(),
        targets: second.targets.clone(),
        classifier: None,
        client: client.clone(),
        created: Instant::now(),
        bindings: Arc::default(),
    });
    runtime
        .set_menu(Arc::new(Menu {
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            token: "menu-token".into(),
            groups: HashMap::from([
                ("vary-auto/development".into(), original_session),
                ("vary-auto/second".into(), second.clone()),
                ("vary-provider/direct".into(), direct),
            ]),
        }))
        .ok()
        .expect("menu");
    assert_eq!(
        runtime.menu_efforts("menu-token").expect("capabilities")["development"],
        ["low", "high"]
    );
    assert!(runtime
        .menu_efforts("wrong-token")
        .expect("scoped")
        .is_empty());
    assert!(runtime.menu_images("menu-token").expect("images")["development"]);
    assert!(!runtime.menu_images("menu-token").expect("images")["second"]);
    let before_images = seen.lock().expect("records").len();
    let image_input = json!([{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,c3ludGhldGlj","detail":"high"}]}]);
    let rejected = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-auto/second","input":image_input}))
        .send()
        .await
        .expect("image rejection");
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(seen.lock().expect("records").len(), before_images);
    let accepted = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-auto/development","input":image_input}))
        .send()
        .await
        .expect("image passthrough");
    assert_eq!(accepted.status(), StatusCode::OK);
    let _: Value = accepted.json().await.expect("complete stream");
    assert_eq!(
        seen.lock().expect("records").last().expect("last").1["input"],
        image_input
    );
    let list: Value = client
        .get(format!("http://127.0.0.1:{}/intent/v1/models", info.port))
        .bearer_auth("menu-token")
        .send()
        .await
        .expect("models")
        .json()
        .await
        .expect("models json");
    assert_eq!(list["data"].as_array().expect("models").len(), 3);
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == "vary-provider/direct"));
    let result = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-auto/second","input":"synthetic"}))
        .send()
        .await
        .expect("second group");
    assert_eq!(result.status(), StatusCode::OK);
    let _: Value = result.json().await.expect("response");
    {
        let records = seen.lock().expect("records");
        assert_eq!(
            records.last().expect("last").1["model"],
            "second-exact-model"
        );
        assert_eq!(
            records.last().expect("last").0[header::AUTHORIZATION],
            "Bearer synthetic-second"
        );
    }
    let direct = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-provider/direct","input":"synthetic direct"}))
        .send()
        .await
        .expect("direct request");
    assert_eq!(direct.status(), StatusCode::OK);
    let _: Value = direct.json().await.expect("response");
    {
        let records = seen.lock().expect("records");
        assert_eq!(
            records.last().expect("last").1["model"],
            "second-exact-model"
        );
        assert_eq!(
            records.last().expect("last").0[header::AUTHORIZATION],
            "Bearer synthetic-second"
        );
    }
    let result = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-provider/unknown","input":"synthetic"}))
        .send()
        .await
        .expect("unknown alias");
    assert_eq!(result.status(), StatusCode::NOT_FOUND);
    let result = client
        .post(&url)
        .bearer_auth("menu-token")
        .json(&json!({"model":"vary-auto/second","previous_response_id":"response-stream"}))
        .send()
        .await
        .expect("foreign continuation");
    assert_eq!(result.status(), StatusCode::CONFLICT);
    runtime.clear_menu();
    assert_eq!(
        client
            .get(format!("http://127.0.0.1:{}/intent/v1/models", info.port))
            .bearer_auth("menu-token")
            .send()
            .await
            .expect("revoked menu")
            .status(),
        StatusCode::UNAUTHORIZED
    );
    server.stop().await.expect("stop proxy");
    task.abort();
}

#[test]
fn colliding_upstream_ids_and_unknown_tool_types_are_not_rerouted() {
    let mut bindings = Bindings::default();
    bindings.observe(&json!({"id":"collision"}), 0);
    bindings.observe(&json!({"id":"collision"}), 1);
    assert!(bindings
        .resolve(&json!({"previous_response_id":"collision"}), None)
        .is_err());
    assert!(bindings
        .resolve(
            &json!({"input":[{"type":"custom_tool_call_output","call_id":"unknown"}]}),
            None
        )
        .is_err());
    assert!(bindings
        .resolve(
            &json!({"input":[{"type":"function_call","call_id":"unknown"}]}),
            None
        )
        .is_err());
}
