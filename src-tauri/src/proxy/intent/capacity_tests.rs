use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{response::IntoResponse, routing::post, Json, Router};
use std::sync::Mutex;

struct Fixture {
    server: ProxyServer,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
    url: String,
    token: String,
    seen: Arc<Mutex<Vec<String>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(
        windows: [Option<u64>; 2],
        manual: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = Router::new().route("/responses", post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            async move {
                let model = body["model"].as_str().unwrap_or("").to_owned();
                captured.lock().expect("fixture capture").push(model.clone());
                let mode = body["instructions"].as_str().unwrap_or("");
                if mode == "rate" || (mode == "roomy-rate" && model == "strong-model") {
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":{"code":"unavailable"}}))).into_response();
                }
                if mode == "overflow-all" || (mode == "overflow" && model == "balanced-model") {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error":{"code":"context_length_exceeded","message":"synthetic"}}))).into_response();
                }
                if mode == "invalid" {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error":{"code":"invalid_request_error","message":"synthetic"}}))).into_response();
                }
                if mode == "byte" && model == "balanced-model" {
                    return StatusCode::PAYLOAD_TOO_LARGE.into_response();
                }
                if mode == "tool" {
                    return Json(json!({"id":"response-tool","model":model,"status":"completed",
                        "output":[{"type":"function_call","call_id":"synthetic-call","name":"fixture","arguments":"{}"}]})).into_response();
                }
                Json(json!({"id":format!("response-{model}"),"model":model,"status":"completed","error":null,"output":[]})).into_response()
            }
        }));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let mut group: RoutingGroup = serde_json::from_value(json!({
            "id":"development","name":"Fixture","revision":0,"client":"codex","enabled":false,
            "members":[{"id":"balanced","providerId":"fixture","modelId":"balanced-model"},
                {"id":"strong","providerId":"fixture","modelId":"strong-model"}],
            "defaultMemberId":"balanced","fallbackMemberIds":[],"intentRecognitionEnabled":false,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        }))?;
        group.rules.clear();
        group.fallback_member_ids = vec!["strong".into()];
        if manual {
            group.routing = crate::services::intent_routing::RoutingStrategy::Manual;
            group.manual_member_id = Some("balanced".into());
        }
        let targets = group
            .members
            .iter()
            .enumerate()
            .map(|(i, member)| {
                Ok(Target {
                    fixed_effort: None,
                    provider_id: member.provider_id.clone(),
                    model: member.model_id.clone(),
                    upstream: None,
                    effort: crate::services::intent_routing::effort::Capability::parse(
                        &json!({"context_window":windows[i]}),
                    ),
                    url: format!("http://{address}/responses").parse()?,
                    headers: Default::default(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                })
            })
            .collect::<Result<Vec<_>, url::ParseError>>()?;
        let runtime = Arc::new(Runtime::default());
        let client = reqwest::Client::builder().no_proxy().build()?;
        let token = runtime.install(
            Session {
                group,
                targets,
                classifier: None,
                client: client.clone(),
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
        Ok(Self {
            server,
            task,
            client,
            url: format!("http://127.0.0.1:{}/intent/v1/responses", info.port),
            token,
            seen,
        })
    }
    async fn send(&self, mut body: Value, session: &str) -> reqwest::Response {
        body["model"] = json!("vary-auto/development");
        self.client
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("session-id", session)
            .json(&body)
            .send()
            .await
            .expect("synthetic request")
    }
    fn models(&self) -> Vec<String> {
        self.seen.lock().expect("fixture capture").clone()
    }
}

#[tokio::test]
#[serial_test::serial]
async fn growing_plain_history_reselects_but_response_references_remain_bound(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new([Some(2048), Some(16000)], false).await?;
    let first = fixture
        .send(json!({"input":"short"}), "growing")
        .await
        .json::<Value>()
        .await?;
    assert_eq!(first["model"], "balanced-model");
    let continuation = fixture
        .send(
            json!({"input":"x".repeat(12000),"previous_response_id":"response-balanced-model"}),
            "growing",
        )
        .await;
    assert_eq!(continuation.status(), StatusCode::OK);
    let _ = continuation.bytes().await?;
    assert_eq!(fixture.models(), ["balanced-model", "balanced-model"]);
    let grown = fixture
        .send(
            json!({"input":[{"role":"user","content":"x".repeat(12000)}]}),
            "growing",
        )
        .await
        .json::<Value>()
        .await?;
    assert_eq!(grown["model"], "strong-model");
    assert_eq!(
        fixture.models(),
        ["balanced-model", "balanced-model", "strong-model"]
    );
    let foreign = fixture
        .send(
            json!({"input":[{"type":"reasoning","encrypted_content":"synthetic-unknown"}]}),
            "fresh-opaque",
        )
        .await;
    assert_eq!(foreign.status(), StatusCode::CONFLICT);
    assert_eq!(fixture.models().len(), 3);
    fixture.server.stop().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn overflow_advances_only_to_members_with_room_and_rate_errors_are_not_overflow(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new([Some(2048), Some(16000)], false).await?;
    let value = fixture
        .send(json!({"input":"short","instructions":"overflow"}), "retry")
        .await
        .json::<Value>()
        .await?;
    assert_eq!(value["model"], "strong-model");
    assert_eq!(fixture.models(), ["balanced-model", "strong-model"]);
    let byte = fixture
        .send(json!({"input":"short","instructions":"byte"}), "bytes")
        .await
        .json::<Value>()
        .await?;
    assert_eq!(byte["model"], "strong-model");
    let rate = fixture
        .send(json!({"input":"short","instructions":"rate"}), "rate")
        .await;
    assert_eq!(rate.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        rate.json::<Value>().await?["error"]["code"],
        "routing_candidates_unavailable"
    );
    let before = fixture.models().len();
    let too_long = fixture
        .send(
            json!({"input":"x".repeat(80000),"instructions":"overflow-all"}),
            "too-long",
        )
        .await;
    assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        too_long.json::<Value>().await?["error"]["code"],
        "context_length_exceeded"
    );
    assert_eq!(fixture.models().len(), before + 1);
    let last_resort = fixture
        .send(
            json!({"input":"x".repeat(12000),"instructions":"roomy-rate"}),
            "last-resort",
        )
        .await
        .json::<Value>()
        .await?;
    assert_eq!(last_resort["model"], "balanced-model");
    let invalid = fixture
        .send(
            json!({"input":"x".repeat(80000),"instructions":"invalid"}),
            "invalid",
        )
        .await;
    assert_eq!(
        invalid.json::<Value>().await?["error"]["code"],
        "routing_upstream_rejected"
    );
    fixture.server.stop().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn unknown_capacity_stays_eligible_and_manual_selection_never_moves(
) -> Result<(), Box<dyn std::error::Error>> {
    for manual in [false, true] {
        let fixture = Fixture::new(
            [if manual { Some(2048) } else { None }, Some(16000)],
            manual,
        )
        .await?;
        let value = fixture
            .send(json!({"input":"x".repeat(12000)}), "synthetic")
            .await
            .json::<Value>()
            .await?;
        assert_eq!(value["model"], "balanced-model");
        if manual {
            let before = fixture.models().len();
            let response = fixture
                .send(json!({"input":"short","instructions":"overflow"}), "manual")
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let _ = response.bytes().await?;
            assert_eq!(fixture.models().len(), before + 1);
        }
        fixture.server.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn pending_tools_do_not_become_soft_affinity_after_json_completion(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new([Some(2048), Some(16000)], false).await?;
    let first = fixture
        .send(json!({"input":"short","instructions":"tool"}), "tool-turn")
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let _ = first.bytes().await?;
    let next = fixture
        .send(json!({"input":"x".repeat(12000)}), "tool-turn")
        .await
        .json::<Value>()
        .await?;
    assert_eq!(next["model"], "balanced-model");
    assert_eq!(fixture.models(), ["balanced-model", "balanced-model"]);
    fixture.server.stop().await?;
    Ok(())
}
