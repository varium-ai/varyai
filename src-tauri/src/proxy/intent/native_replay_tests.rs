//! Synthetic same-conversation model-switch regressions; no native app is launched.
use super::{native_gateway, Runtime};
use crate::{
    database::Database,
    proxy::{security, server::ProxyServer, ProxyConfig},
};
use axum::{extract::Request, response::IntoResponse, Json, Router};
use http::{header, HeaderMap, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct RestorePolicy(security::SecurityRuntimeConfig);
impl Drop for RestorePolicy {
    fn drop(&mut self) {
        security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn codex_group_rechecks_priority_after_provider_reenable_and_manual_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    security::runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        let app = Router::new().fallback(move |Json(body): Json<Value>| {
            let model = body["model"].as_str().unwrap_or_default().to_owned();
            let mut calls = captured.lock().expect("fixture requests");
            calls.push(model.clone());
            let note = json!({"type":"message","role":"assistant","status":"completed",
            "id":format!("message-{}",calls.len()),"content":[
                {"type":"output_text","text":"Synthetic reply","annotations":[]}
            ]});
            let output = if body["input"]
                .as_array()
                .and_then(|items| items.last())
                .is_some_and(|item| item["content"] == "Synthetic tool opening")
            {
                json!([{"type":"function_call","name":"synthetic_tool","id":"fixture-call-item",
                    "call_id":"fixture-call","arguments":"{}","status":"completed"},note])
            } else {
                json!([note])
            };
            let reply = json!({
                "id":format!("response-{}",calls.len()),"object":"response",
                "model":model,"status":"completed","output":output
            });
            let summary = body["instructions"]
                .as_str()
                .is_some_and(|text| text.contains("continuation summary"));
            let reply = if body["stream"] == true && !summary {
                let event = json!({"type":"response.completed","response":reply});
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    format!("event: response.completed\ndata: {event}\n\n"),
                )
                    .into_response()
            } else {
                Json(reply).into_response()
            };
            std::future::ready(reply)
        });
        let _ = axum::serve(listener, app).await;
    });
    let mut menu = super::global_tests::menu(&format!("http://{address}/responses"));
    let alias = menu.groups.keys().next().ok_or("fixture alias")?.clone();
    let session = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("fixture route")?);
    session.group.levels.clear();
    session.group.affinity = "turn".into();
    session.group.template = Some(crate::services::intent_routing::RoutingTemplate::Capability);
    let first = session.targets[0].clone();
    let mut second = first.clone();
    second.provider_id = "backup-provider".into();
    second.model = "backup-model".into();
    second.scheduling = Arc::default();
    let mut backup = session.group.members[0].clone();
    backup.id = "backup".into();
    backup.provider_id = second.provider_id.clone();
    backup.model_id = second.model.clone();
    session.group.members.push(backup);
    session.group.fallback_member_ids = vec!["backup".into()];
    session.group.off = vec![session.group.default_member_id.clone()];
    session.targets = vec![second.clone()];

    let runtime = Arc::new(Runtime::default());
    let dir = tempfile::tempdir()?;
    runtime
        .prepare_continuations(dir.path().join("continuations.json"))
        .await;
    runtime
        .native
        .prepare_checkpoints(&dir.path().join("checkpoint.key"))?;
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
    let url = format!("http://127.0.0.1:{}/backend-api/codex/responses", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let visible = json!({"model":alias,"input":[
        {"role":"user","content":"Synthetic opening"},
        {"type":"reasoning","id":"synthetic-empty-reasoning","summary":[],"content":null,"encrypted_content":null},
        {"role":"assistant","content":"Synthetic previous answer"},
        {"role":"user","content":"Synthetic next turn"}
    ]});
    for (change, expected, mut request) in [
        (
            "disabled",
            second.model.as_str(),
            json!({"model":alias,"input":"Synthetic opening"}),
        ),
        ("reenabled", first.model.as_str(), visible.clone()),
        ("reordered", second.model.as_str(), visible.clone()),
        (
            "reordered with a response reference",
            second.model.as_str(),
            json!({"model":alias,"previous_response_id":"response-3","input":"Synthetic continuation"}),
        ),
        ("unchanged", first.model.as_str(), visible),
    ] {
        request["stream"] = json!(matches!(change, "disabled" | "reordered"));
        if change != "unchanged" {
            let current = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("fixture route")?);
            current.bindings = Arc::default();
            if change == "reenabled" {
                current.group.off.clear();
                current.targets = vec![first.clone(), second.clone()];
            } else if change.starts_with("reordered") {
                current.group.members.swap(0, 1);
                current.group.default_member_id = current.group.members[0].id.clone();
                current.group.fallback_member_ids = vec![current.group.members[1].id.clone()];
                current.targets.swap(0, 1);
            }
            runtime.native.publish_menu(
                Arc::new(menu.clone()),
                json!({"models":[]}),
                Default::default(),
            )?;
        }
        let reply = client
            .post(&url)
            .bearer_auth("synthetic-native-token")
            .header("session_id", "synthetic-routing-session")
            .json(&request)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.bytes().await?;
        assert_eq!(status, StatusCode::OK, "{change}: {body:?}");
        assert_eq!(
            seen.lock()
                .expect("fixture requests")
                .last()
                .map(String::as_str),
            Some(expected),
            "{change}"
        );
    }
    // Repeat the disable/enable sequence with native-style tool history.
    // Gemini remains the owner of the pending tool result even after the
    // higher-priority provider is published again.
    let mut tool_menu = menu.clone();
    let current = Arc::make_mut(tool_menu.groups.get_mut(&alias).ok_or("fixture route")?);
    current.bindings = Arc::default();
    current.group.off = vec![current.group.default_member_id.clone()];
    current.targets = vec![second.clone()];
    let publish = |menu: &super::Menu| {
        runtime.native.publish_menu(
            Arc::new(menu.clone()),
            json!({"models":[]}),
            Default::default(),
        )
    };
    let send = |input: Vec<Value>| {
        client
            .post(&url)
            .bearer_auth("synthetic-native-token")
            .header("session_id", "synthetic-provider-reenable")
            .json(&json!({"model":alias,"input":input,"stream":false}))
            .send()
    };
    publish(&tool_menu)?;
    let mut history = vec![json!({"role":"user","content":"Synthetic tool opening"})];
    let opening: Value = send(history.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(opening["model"], second.model);
    history.extend(
        opening["output"]
            .as_array()
            .ok_or("fixture output")?
            .iter()
            .cloned(),
    );
    runtime.flush_continuations().await;
    let current = Arc::make_mut(tool_menu.groups.get_mut(&alias).ok_or("fixture route")?);
    current.bindings = Arc::default();
    current.group.off.clear();
    current.targets = vec![first.clone(), second.clone()];
    publish(&tool_menu)?;
    history.push(
        json!({"type":"function_call_output","call_id":"fixture-call","output":"Synthetic result"}),
    );
    let tool_reply: Value = send(history.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        tool_reply["model"], second.model,
        "active tool result stays on the backup"
    );
    history.extend(
        tool_reply["output"]
            .as_array()
            .ok_or("fixture reply")?
            .iter()
            .cloned(),
    );
    history.push(json!({"role":"user","content":"Synthetic follow-up after reenable"}));
    let resumed: Value = send(history).await?.error_for_status()?.json().await?;
    assert_eq!(
        resumed["model"], first.model,
        "a new turn uses the reenabled first provider"
    );

    let current = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("fixture route")?);
    current.group.template = Some(crate::services::intent_routing::RoutingTemplate::Smart);
    current.group.rules = serde_json::from_value(json!([
        {"id":"compact", "name":"Compaction", "enabled":true, "description":"",
         "targetMemberId":"backup", "conditions":{"compact":true}}
    ]))?;
    runtime
        .native
        .publish_menu(Arc::new(menu), json!({"models":[]}), Default::default())?;
    let compact: Value = client
        .post(format!("{url}/compact"))
        .bearer_auth("synthetic-native-token")
        .header("session_id", "synthetic-routing-session")
        .json(&json!({"model":alias,"input":[{"role":"user","content":"Synthetic opening"}]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        seen.lock().expect("fixture requests").last(),
        Some(&second.model)
    );
    let checkpoint = compact["output"][0]["encrypted_content"]
        .as_str()
        .ok_or("missing synthetic checkpoint")?;
    assert!(checkpoint.starts_with("varyai1:"));
    client
        .post(&url)
        .bearer_auth("synthetic-native-token")
        .header("session_id", "synthetic-routing-session")
        .json(&json!({"model":alias,"input":[
            {"type":"compaction","encrypted_content":checkpoint},
            {"type":"message","role":"user","content":"Synthetic resumed question"}
        ]}))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    assert_eq!(
        seen.lock().expect("fixture requests").last(),
        Some(&first.model)
    );
    server.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn codex_reorder_replays_closed_tool_turns_without_moving_active_outputs(
) -> Result<(), Box<dyn std::error::Error>> {
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    security::runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<(bool, Value)>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        let app = Router::new().fallback(move |uri: axum::extract::OriginalUri, Json(body): Json<Value>| {
            let mut requests = captured.lock().expect("synthetic observations");
            requests.push((uri.path() == "/official/responses", body.clone()));
            let model = body["model"].as_str().unwrap_or_default();
            let output = match body["input"].as_array().and_then(|items| items.last()) {
                Some(last) if last["content"] == "Synthetic first" || last["content"] == "Synthetic second" || last["content"] == "Synthetic steering" => json!([{
                    "type":"function_call","id":format!("fc-{model}"),"call_id":format!("call-{model}"),
                    "name":"synthetic_tool","arguments":"{}","status":"completed"
                }]),
                _ => json!([{"type":"message","id":format!("msg-{}", requests.len()),
                    "role":"assistant","status":"completed","content":[{"type":"output_text","text":"Synthetic answer"}]
                }]),
            };
            let response = json!({"id":format!("resp-{}",requests.len()),"object":"response",
                "model":model,"status":"completed","output":output});
            let event = json!({"type":"response.completed","response":response});
            std::future::ready(([(header::CONTENT_TYPE, "text/event-stream")],
                format!("event: response.completed\ndata: {event}\n\n")))
        });
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/official"));
    let base_menu = super::global_tests::menu(&format!("http://{address}/responses"));
    let alias = base_menu
        .groups
        .keys()
        .next()
        .ok_or("missing synthetic alias")?
        .clone();
    let runtime = Arc::new(Runtime::default());
    let dir = tempfile::tempdir()?;
    runtime
        .prepare_continuations(dir.path().join("continuations.json"))
        .await;
    runtime
        .native
        .prepare_checkpoints(&dir.path().join("checkpoint.key"))?;
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    let publish = |menu: &super::Menu| -> Result<(), Box<dyn std::error::Error>> {
        let menu = Arc::new(menu.clone());
        runtime.set_menu(menu.clone())?;
        runtime
            .native
            .publish_menu(menu, json!({"models":[]}), Default::default())?;
        Ok(())
    };
    for (path, thread, interrupted) in [
        (
            "/backend-api/codex/responses",
            "synthetic-native-complete",
            false,
        ),
        (
            "/backend-api/codex/responses",
            "synthetic-native-interrupted",
            true,
        ),
        (
            "/intent/v1/responses",
            "synthetic-standalone-complete",
            false,
        ),
    ] {
        let mut menu = base_menu.clone();
        let session = Arc::make_mut(
            menu.groups
                .get_mut(&alias)
                .ok_or("missing synthetic route")?,
        );
        session.group.levels.clear();
        session.group.affinity = "turn".into();
        session.bindings = Arc::default();
        let first = session.targets[0].model.clone();
        let mut second = session.targets[0].clone();
        second.provider_id = "synthetic-new-provider".into();
        second.model = "synthetic-new-model".into();
        second.scheduling = Arc::default();
        let mut member = session.group.members[0].clone();
        member.id = "synthetic-new-member".into();
        member.provider_id = second.provider_id.clone();
        member.model_id = second.model.clone();
        session.group.fallback_member_ids = vec![member.id.clone()];
        session.group.members.push(member);
        session.targets.push(second.clone());
        let token = menu.token.clone();
        publish(&menu)?;
        let url = format!("http://127.0.0.1:{}{path}", info.port);
        let send = |body: Value| {
            client
                .post(&url)
                .bearer_auth(&token)
                .header("session_id", thread)
                .json(&body)
                .send()
        };
        let request =
            |input: Vec<Value>| json!({"model":alias,"input":input,"stream":true,"store":false});
        let user = |text: &str, turn: &str| {
            json!({"role":"user","content":text,
            "internal_chat_message_metadata_passthrough":{"turn_id":turn}})
        };
        let tool = |model: &str| {
            json!({"type":"function_call","id":format!("fc-{model}"),
            "call_id":format!("call-{model}"),"name":"synthetic_tool","arguments":"{}","status":"completed"})
        };
        let result = |model: &str| {
            json!({"type":"function_call_output","call_id":format!("call-{model}"),
            "output":"Synthetic result"})
        };
        let first_user = user("Synthetic first", "turn-a");
        send(request(vec![first_user.clone()]))
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let reordered = Arc::make_mut(
            menu.groups
                .get_mut(&alias)
                .ok_or("missing synthetic route")?,
        );
        reordered.bindings = Arc::default();
        reordered.group.members.swap(0, 1);
        reordered.targets.swap(0, 1);
        reordered.group.default_member_id = reordered.group.members[0].id.clone();
        reordered.group.fallback_member_ids = vec![reordered.group.members[1].id.clone()];
        publish(&menu)?;
        let mut history = vec![first_user, tool(&first), result(&first)];
        if interrupted {
            // A steering message with the old turn ID must still go to the call owner.
            let mut steering = history.clone();
            steering.push(user("Synthetic steering", "turn-a"));
            send(request(steering))
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            assert_eq!(
                seen.lock()
                    .map_err(|_| "fixture observations")?
                    .last()
                    .ok_or("missing request")?
                    .1["model"],
                first
            );
            history.push(json!({"role":"user","content":[{"type":"input_text","text":"<turn_aborted>Synthetic stop</turn_aborted>"}],
                "internal_chat_message_metadata_passthrough":{"turn_id":"turn-a","content_item_kinds":["generic.turn_aborted"]}}));
        } else {
            send(request(history.clone()))
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            assert_eq!(
                seen.lock()
                    .map_err(|_| "fixture observations")?
                    .last()
                    .ok_or("missing tool result")?
                    .1["model"],
                first
            );
            history.push(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Synthetic answer","annotations":[]}]}));
        }
        if interrupted {
            history.push(json!({"type":"function_call_output","name":"browser_tabs","namespace":"agent",
                "output":"Synthetic page context","internal_chat_message_metadata_passthrough":{"turn_id":"turn-b"}}));
        }
        history.push(user("Synthetic second", "turn-b"));
        let new_turn = request(history.clone());
        send(new_turn.clone())
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        {
            let seen = seen.lock().map_err(|_| "fixture observations")?;
            let (_, sent) = seen.last().ok_or("missing new turn")?;
            assert_eq!(sent["model"], second.model, "{thread}: use the saved order");
            assert_eq!(sent["input"], new_turn["input"], "history is not rewritten");
        }
        runtime.flush_continuations().await;
        let reordered = Arc::make_mut(
            menu.groups
                .get_mut(&alias)
                .ok_or("missing synthetic route")?,
        );
        reordered.bindings = Arc::default();
        reordered.group.members.swap(0, 1);
        reordered.targets.swap(0, 1);
        reordered.group.default_member_id = reordered.group.members[0].id.clone();
        reordered.group.fallback_member_ids = vec![reordered.group.members[1].id.clone()];
        publish(&menu)?;
        history.extend([tool(&second.model), result(&second.model)]);
        let active = request(history.clone());
        send(active.clone())
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        {
            let seen = seen.lock().map_err(|_| "fixture observations")?;
            let (_, sent) = seen.last().ok_or("missing active result")?;
            assert_eq!(
                sent["model"], second.model,
                "earlier tool pairs cannot veto the active owner"
            );
            assert_eq!(sent["input"], active["input"]);
        }
        let mut opaque = history.clone();
        opaque.push(json!({"type":"reasoning","id":"synthetic-sealed-item",
            "summary":[],"encrypted_content":"synthetic-seal"}));
        opaque.push(user("Synthetic third", "turn-c"));
        let before = seen.lock().map_err(|_| "fixture observations")?.len();
        let reply = send(request(opaque)).await?;
        assert_eq!(reply.status(), StatusCode::CONFLICT);
        assert_eq!(
            seen.lock().map_err(|_| "fixture observations")?.len(),
            before
        );

        if path.starts_with("/backend-api/") {
            // Codex may emit visible assistant text alongside a pending tool
            // call. Only the later tool output and final reply close that turn.
            let output = history.pop().ok_or("missing active result")?;
            let note = json!({"type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":"Synthetic note","annotations":[]}]});
            history.extend([note.clone(), output, note]);
            history.push(user("Synthetic native turn", "turn-c"));
            let mut official = request(history.clone());
            official["model"] = json!("synthetic-built-in");
            send(official.clone())
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            {
                let seen = seen.lock().map_err(|_| "fixture observations")?;
                let (official_path, sent) = seen.last().ok_or("missing built-in request")?;
                assert!(*official_path, "built-in selection uses the official path");
                assert_eq!(sent, &official);
            }
            history.extend([
                json!({"type":"configuration_update","reasoning":{"effort":"high"}}),
                json!({"role":"user","content":[{"type":"input_text","text":"<turn_aborted>Synthetic stop</turn_aborted>"}],
                    "internal_chat_message_metadata_passthrough":{"turn_id":"turn-c","content_item_kinds":["generic.turn_aborted"]}}),
                user("Synthetic return to group", "turn-d"),
            ]);
            let returned = request(history);
            let reply = send(returned.clone()).await?;
            assert_eq!(
                reply.status(),
                StatusCode::OK,
                "{thread}: switch back to group"
            );
            reply.bytes().await?;
            {
                let seen = seen.lock().map_err(|_| "fixture observations")?;
                let (official_path, sent) = seen.last().ok_or("missing return to group")?;
                assert!(!official_path);
                assert_eq!(sent["input"], returned["input"]);
                assert_eq!(sent["model"], first);
            }
            // A different published group or single-model alias must not
            // inherit the old group's target solely from the same thread ID.
            for (selection, model) in [
                ("group/other", "other-group-model"),
                ("vary-provider/single", "single-model"),
            ] {
                let mut other = base_menu.groups[&alias].clone();
                let target = Arc::make_mut(&mut other);
                target.bindings = Arc::default();
                target.group.id = selection.to_owned();
                target.group.members[0].model_id = model.into();
                target.targets[0].model = model.into();
                menu.groups.insert(selection.into(), other);
                publish(&menu)?;
                let mut next = returned.clone();
                next["model"] = json!(selection);
                send(next.clone())
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                let seen = seen.lock().map_err(|_| "fixture observations")?;
                let (official_path, sent) = seen.last().ok_or("missing alias switch")?;
                assert!(!official_path);
                assert_eq!(sent["input"], next["input"]);
                assert_eq!(sent["model"], model);
            }

            let mut history = returned["input"]
                .as_array()
                .ok_or("missing replay input")?
                .clone();
            let mut official = request(history.clone());
            official["model"] = json!("synthetic-built-in");
            let reply = send(official).await?.error_for_status()?.text().await?;
            let data = reply
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("missing official response")?;
            let event: Value = serde_json::from_str(data)?;
            let mut official_answer = event["response"]["output"][0].clone();
            official_answer["phase"] = json!("final_answer");
            official_answer["content"][0]["annotations"] = json!([]);
            official_answer["content"][0]["logprobs"] = json!([]);
            history.push(official_answer);

            runtime.flush_continuations().await;
            let group = Arc::make_mut(
                menu.groups
                    .get_mut(&alias)
                    .ok_or("missing original group")?,
            );
            group.bindings = Arc::default();
            let mut added_target = group.targets[0].clone();
            added_target.provider_id = "synthetic-added-provider".into();
            added_target.model = "synthetic-added-model".into();
            added_target.scheduling = Arc::default();
            let mut added_member = group.group.members[0].clone();
            added_member.id = "synthetic-added-member".into();
            added_member.provider_id = added_target.provider_id.clone();
            added_member.model_id = added_target.model.clone();
            group.group.members.insert(0, added_member);
            group.targets.insert(0, added_target);
            for (turn, expected) in [
                ("turn-e", "synthetic-added-model"),
                ("turn-f", first.as_str()),
            ] {
                runtime.flush_continuations().await;
                let group = Arc::make_mut(
                    menu.groups
                        .get_mut(&alias)
                        .ok_or("missing original group")?,
                );
                group.bindings = Arc::default();
                if turn == "turn-f" {
                    group.group.members.swap(0, 1);
                    group.targets.swap(0, 1);
                }
                group.group.default_member_id = group.group.members[0].id.clone();
                group.group.fallback_member_ids = group.group.members[1..]
                    .iter()
                    .map(|member| member.id.clone())
                    .collect();
                publish(&menu)?;
                history.push(user("Synthetic question after saving model order", turn));
                let next = request(history.clone());
                let response = send(next.clone()).await?.error_for_status()?.text().await?;
                let data = response
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .ok_or("missing routed response")?;
                let event: Value = serde_json::from_str(data)?;
                history.push(event["response"]["output"][0].clone());
                let seen = seen.lock().map_err(|_| "fixture observations")?;
                let (official_path, sent) = seen.last().ok_or("missing reordered request")?;
                assert!(!official_path);
                assert_eq!(sent["input"], next["input"]);
                assert_eq!(
                    sent["model"], expected,
                    "{thread}: {turn} should use the saved first model"
                );
            }
        }
    }
    server.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn native_responses_model_switch_recovers_all_transparent_agent_adapters(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::services::agent_clients::{self, Backup, Route};
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            if let Ok(mut seen) = captured.lock() { seen.push(body.clone()); }
            if body["input"].as_array().into_iter().flatten().any(|item| item["id"] == "rs_native_missing") {
                return (StatusCode::NOT_FOUND, Json(json!({"error":{"message":"Referenced reasoning item 'rs_native_missing' was not found or has expired."}}))).into_response();
            }
            let reply = json!({"id":"resp_synthetic","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1}});
            if body["stream"] == true {
                ([("content-type", "text/event-stream")], format!("event: response.completed\ndata: {reply}\n\n")).into_response()
            } else { Json(reply).into_response() }
        }
    });
    let upstream = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let db = Arc::new(Database::memory()?);
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = proxy.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for agent in agent_clients::APPS.iter().copied().chain(["trae", "qoder"]) {
        let local = if agent == "qoder" {
            db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "true")?;
            db.save_provider("qoder", &crate::provider::Provider::with_id("synthetic".into(), "Synthetic".into(), json!({
                "baseUrl":format!("http://{address}/v1"), "apiKey":"synthetic-native-key", "protocol":"openai-responses", "model":"synthetic-model", "models":[{"model":"synthetic-model"}]
            }), None))?;
            format!(
                "http://127.0.0.1:{}/qoder/providers/synthetic/v1",
                info.port
            )
        } else {
            let local = format!(
                "http://127.0.0.1:{}/agents/{agent}/providers/synthetic/v1",
                info.port
            );
            let backup = Backup {
                path: "unused-synthetic-config".into(),
                routes: vec![Route {
                    id: "synthetic".into(),
                    pointer: "/unused".into(),
                    upstream: format!("http://{address}/v1"),
                    local: local.clone(),
                    models: vec!["synthetic-model".into()],
                    provider_id: "synthetic-provider".into(),
                    created_provider: None,
                }],
            };
            db.save_live_backup(agent, &serde_json::to_string(&backup)?)
                .await?;
            db.set_setting(&agent_clients::enabled_key(agent), "true")?;
            local
        };
        for enabled in [false, true] {
            security::runtime::set_security_runtime_for_test(security::SecurityRuntimeConfig {
                enabled,
                custom_rules: vec![security::runtime::CompiledCustomRule {
                    id: "synthetic-native".into(),
                    name: "Synthetic native rule".into(),
                    rule_type: "regex".into(),
                    pattern: "synthetic-native-secret".into(),
                    regex: regex::Regex::new("synthetic-native-secret").ok(),
                    risk_level: security::RiskLevel::High,
                    action: security::AuditAction::Redact,
                    enabled: true,
                }],
                ..Default::default()
            });
            for stream in [false, true] {
                let before = seen.lock().map_err(|_| "synthetic lock poisoned")?.len();
                let reply = client.post(format!("{local}/responses")).bearer_auth(if agent == "qoder" { "varyai-local" } else { "synthetic-native-key" }).json(&json!({
                    "model":"synthetic-model", "stream":stream, "store":false, "input":[
                        {"type":"function_call","id":"fc_synthetic","call_id":"call_synthetic","name":"synthetic_tool","arguments":"{}"},
                        {"type":"function_call_output","call_id":"call_synthetic","output":"Synthetic result"},
                        {"type":"reasoning","id":"rs_native_missing","summary":[],"encrypted_content":"synthetic-foreign"},
                        {"role":"user","content":"Synthetic continue synthetic-native-secret"}
                    ], "synthetic_extension":{"keep":true}
                })).send().await?;
                assert_eq!(
                    reply.status(),
                    StatusCode::OK,
                    "agent={agent} enabled={enabled} stream={stream}"
                );
                let text = reply.text().await?;
                assert!(text.contains("resp_synthetic"));
                assert_eq!(text.starts_with("event:"), stream);
                let seen = seen.lock().map_err(|_| "synthetic lock poisoned")?;
                assert_eq!(seen.len() - before, 2);
                let mut expected = seen[before].clone();
                expected["input"]
                    .as_array_mut()
                    .ok_or("synthetic input")?
                    .remove(2);
                let last = &seen[before + 1];
                assert_eq!(last, &expected);
                assert_eq!(
                    last["input"][2]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("synthetic-native-secret"),
                    !enabled
                );
            }
        }
    }
    proxy.stop().await?;
    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn native_model_switch_filters_bare_reasoning_and_recovers_named_404(
) -> Result<(), Box<dyn std::error::Error>> {
    let _policy = RestorePolicy((*security::get_security_runtime()).clone());
    let seen = Arc::new(Mutex::new(Vec::<(HeaderMap, Value)>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(move |request: Request| {
        let captured = captured.clone();
        async move {
            let (parts, body) = request.into_parts();
            let Ok(raw) = axum::body::to_bytes(body, 1024 * 1024).await else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let Ok(body) = serde_json::from_slice::<Value>(&raw) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if let Ok(mut seen) = captured.lock() { seen.push((parts.headers, body.clone())); }
            let bad = body["input"].as_array().into_iter().flatten().find(|item| {
                item["id"] == "rs_0" || item["id"] == "rs_foreign"
            });
            if let Some(item) = bad {
                return (StatusCode::NOT_FOUND, Json(json!({"error":{"message":format!(
                    "Item with id '{}' not found. Items are not persisted when `store` is set to false.",
                    item["id"].as_str().unwrap_or_default())}}))).into_response();
            }
            ([("x-synthetic-reply", "preserved")], Json(json!({
                "id":"resp_synthetic", "model":"synthetic-native", "status":"completed",
                "output":[{"type":"message","id":"msg_synthetic","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":body["input"].as_array().and_then(|items| items.last()).and_then(|item| item.get("content")).cloned().unwrap_or(json!("Synthetic reply"))}]}]
            }))).into_response()
        }
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (enabled, restore) in [(false, false), (true, false), (true, true)] {
        security::runtime::set_security_runtime_for_test(security::SecurityRuntimeConfig {
            enabled,
            custom_rules: if restore {
                vec![security::runtime::CompiledCustomRule {
                    id: "synthetic-native-replay".into(),
                    name: "Synthetic native replay".into(),
                    rule_type: "regex".into(),
                    pattern: "synthetic-replay-secret".into(),
                    regex: regex::Regex::new("synthetic-replay-secret").ok(),
                    risk_level: security::RiskLevel::High,
                    action: security::AuditAction::RedactRestore,
                    enabled: true,
                }]
            } else {
                vec![]
            },
            ..Default::default()
        });
        for encrypted in [false, true] {
            let before = seen.lock().map_err(|_| "synthetic lock poisoned")?.len();
            let reasoning = if encrypted {
                json!({"type":"reasoning","id":"rs_foreign","summary":[],"encrypted_content":"synthetic-foreign-state"})
            } else {
                json!({"type":"reasoning","id":"rs_0","summary":[{"type":"summary_text","text":"Synthetic prior thought"}],"encrypted_content":null})
            };
            let input = json!([
                reasoning,
                {"type":"reasoning","id":"rs_valid","summary":[],"encrypted_content":"synthetic-valid-state"},
                {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Synthetic prior answer"}]},
                {"type":"function_call","id":"fc_synthetic","call_id":"call_synthetic","name":"synthetic_tool","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_synthetic","output":"Synthetic result"},
                {"role":"user","content":"Synthetic continue synthetic-replay-secret"}
            ]);
            let response = client.post(format!("http://127.0.0.1:{}/backend-api/codex/responses?synthetic=1", info.port))
                .bearer_auth("synthetic-native-token")
                .header("chatgpt-account-id", "synthetic-workspace")
                .header("session_id", "synthetic-same-conversation")
                .json(&json!({"model":"synthetic-native","store":false,"stream":false,"input":input,"synthetic_extension":{"keep":true}}))
                .send().await?;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "enabled={enabled}, encrypted={encrypted}"
            );
            assert_eq!(response.headers()["x-synthetic-reply"], "preserved");
            assert_eq!(
                response.json::<Value>().await?["output"][0]["content"][0]["text"],
                "Synthetic continue synthetic-replay-secret"
            );
            let seen = seen.lock().map_err(|_| "synthetic lock poisoned")?;
            assert_eq!(seen.len() - before, if encrypted { 2 } else { 1 });
            let (headers, last) = seen.last().ok_or("missing synthetic request")?;
            assert_eq!(
                headers[header::AUTHORIZATION],
                "Bearer synthetic-native-token"
            );
            assert_eq!(headers["chatgpt-account-id"], "synthetic-workspace");
            assert_eq!(headers["session_id"], "synthetic-same-conversation");
            if restore {
                assert!(!last["input"][4]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("synthetic-replay-secret"));
                assert_eq!(last["input"][1], input[2]);
                assert_eq!(last["input"][2], input[3]);
                assert_eq!(last["input"][3], input[4]);
            } else {
                assert_eq!(
                    last["input"],
                    Value::Array(input.as_array().ok_or("synthetic input")?[1..].to_vec())
                );
            }
            assert_eq!(last["synthetic_extension"], json!({"keep":true}));
        }
    }
    server.stop().await?;
    task.abort();
    Ok(())
}
