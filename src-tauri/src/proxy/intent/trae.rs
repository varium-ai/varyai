//! TRAE rewrites Chat tool IDs. Retain only hashes of observed function payloads.
use super::*;

const MAX_ARGUMENTS: usize = 256 * 1024;
const MAX_CALLS: usize = 128;

/// Keep TRAE's local request alive while routing waits for upstream headers.
/// Fast responses retain their status and headers. Once SSE has opened, late
/// failures are represented as fixed, sanitized stream errors.
pub(super) async fn waiting_response(
    pending: impl std::future::Future<Output = Response> + Send + 'static,
) -> Response {
    use futures::StreamExt;
    const HEARTBEAT: &[u8] = b": varyai-waiting\n\n";
    let mut pending = Box::pin(pending);
    let mut ticks = tokio::time::interval(Duration::from_secs(5));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticks.tick().await;
    tokio::select! {
        biased;
        response = &mut pending => return response,
        _ = ticks.tick() => {}
    }
    let stream = async_stream::stream! {
        yield Ok::<_, std::io::Error>(bytes::Bytes::from_static(HEARTBEAT));
        let response = loop {
            tokio::select! {
                biased;
                response = &mut pending => break response,
                _ = ticks.tick() => yield Ok(bytes::Bytes::from_static(HEARTBEAT)),
            }
        };
        let status = response.status();
        let sse = response.headers().get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(';').next().is_some_and(|kind| kind.trim() == "text/event-stream"));
        if !status.is_success() || !sse {
            let code = if status.is_success() { "routing_upstream_stream_required" } else { "routing_request_failed" };
            let error = json!({"error":{"type":"upstream_error","code":code,"message":code,"status":if status.is_success() {502} else {status.as_u16()}}});
            yield Ok(bytes::Bytes::from(format!("data: {error}\n\ndata: [DONE]\n\n")));
            return;
        }
        // Preserve byte boundaries after forwarding begins. Inserting a comment
        // between arbitrary upstream chunks could corrupt a partial SSE frame.
        let mut upstream = response.into_body().into_data_stream();
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(chunk) => yield Ok(chunk),
                Err(_) => {
                    yield Err(std::io::Error::other("Routing upstream stream failed"));
                    return;
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-cache"),
    );
    response
}

fn fingerprint(name: &str, arguments: &str) -> Option<String> {
    if name.is_empty() || name.len() > 256 || arguments.len() > MAX_ARGUMENTS {
        return None;
    }
    let mut arguments: Value = serde_json::from_str(arguments).ok()?;
    arguments.sort_all_objects();
    let payload = serde_json::to_string(&(name, arguments)).ok()?;
    Some(format!(
        "trae-observed:{}",
        continuation::hash("trae-function", &payload)
    ))
}

/// Change the inspection projection only. Known IDs retain their authoritative owner;
/// rewritten IDs require an assistant function payload in the same request history.
pub(super) fn projection(body: &Value, known: impl Fn(&str) -> bool) -> Value {
    let mut body = body.clone();
    let Some(items) = body["input"].as_array_mut() else {
        return body;
    };
    let mut aliases: HashMap<String, Option<String>> = HashMap::new();
    for item in items.iter().filter(|item| item["type"] == "function_call") {
        let Some(id) = item["call_id"].as_str() else {
            continue;
        };
        if known(id) {
            continue;
        }
        let alias = item["name"]
            .as_str()
            .zip(item["arguments"].as_str())
            .and_then(|(name, arguments)| fingerprint(name, arguments));
        aliases
            .entry(id.into())
            .and_modify(|previous| {
                if previous != &alias {
                    *previous = None;
                }
            })
            .or_insert(alias);
    }
    for item in items {
        if matches!(
            item["type"].as_str(),
            Some("function_call" | "function_call_output")
        ) {
            if let Some(Some(alias)) = item["call_id"].as_str().and_then(|id| aliases.get(id)) {
                item["call_id"] = json!(alias);
            }
        }
    }
    body
}

#[derive(Default)]
struct Call {
    name: String,
    arguments: String,
    invalid: bool,
}

#[derive(Default)]
pub(super) struct Calls(HashMap<(u64, u64), Call>);

impl Calls {
    pub fn observe(&mut self, value: &Value, bindings: &mut Bindings, target: usize) {
        let Some(choices) = value["choices"].as_array() else {
            return;
        };
        let mut changed = false;
        for choice in choices {
            if let Some(calls) = choice["message"]["tool_calls"].as_array() {
                for call in calls.iter().take(MAX_CALLS) {
                    if let Some(alias) = call["function"]["name"]
                        .as_str()
                        .zip(call["function"]["arguments"].as_str())
                        .and_then(|(name, arguments)| fingerprint(name, arguments))
                    {
                        if !bindings.full() {
                            changed |= remember(&mut bindings.calls, "call", &alias, target);
                        }
                    }
                }
            }
            let choice_index = choice["index"].as_u64().unwrap_or(0);
            if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                for call in calls {
                    let Some(index) = call["index"].as_u64() else {
                        continue;
                    };
                    let key = (choice_index, index);
                    if self.0.len() >= MAX_CALLS && !self.0.contains_key(&key) {
                        continue;
                    }
                    let pending = self.0.entry(key).or_default();
                    if pending.invalid {
                        continue;
                    }
                    let name = call["function"]["name"].as_str().unwrap_or("");
                    let arguments = call["function"]["arguments"].as_str().unwrap_or("");
                    if pending.name.len() + name.len() > 256
                        || pending.arguments.len() + arguments.len() > MAX_ARGUMENTS
                    {
                        pending.invalid = true;
                        pending.name.clear();
                        pending.arguments.clear();
                        continue;
                    }
                    pending.name.push_str(name);
                    pending.arguments.push_str(arguments);
                }
            }
            if choice["finish_reason"].as_str().is_some() {
                self.0.retain(|(index, _), call| {
                    if *index != choice_index {
                        return true;
                    }
                    if !call.invalid && !bindings.full() {
                        if let Some(alias) = fingerprint(&call.name, &call.arguments) {
                            changed |= remember(&mut bindings.calls, "call", &alias, target);
                        }
                    }
                    false
                });
            }
        }
        if changed {
            bindings.persist();
        }
    }
}
