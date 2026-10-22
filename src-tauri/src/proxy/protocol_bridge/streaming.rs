use super::super::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use super::*;
use std::io;
const MAX_EVENT: usize = 2 * 1024 * 1024;
fn failure() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or incomplete protocol stream",
    )
}
fn frame(value: &Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

struct Tool {
    index: usize,
    id: Value,
    name: Value,
    args: String,
    initial: Value,
}
#[derive(Default)]
struct State {
    id: Value,
    model: Value,
    usage: Value,
    tools: HashMap<u64, Tool>,
    next_tool: usize,
    finished: bool,
    stopped: bool,
}
impl State {
    fn chat(&self, delta: Value, finish: Value) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    }
    fn gemini(&self, parts: Vec<Value>) -> Value {
        json!({"responseId":self.id,"modelVersion":self.model,"candidates":[{"index":0,"content":{"role":"model","parts":parts}}]})
    }
    fn accept(&mut self, event: Value, target: Protocol) -> Result<Vec<Bytes>, io::Error> {
        let mut output = Vec::new();
        let index = event["index"].as_u64().unwrap_or(0);
        match event["type"].as_str() {
            Some("message_start") => {
                self.id = event["message"]["id"].clone();
                self.model = event["message"]["model"].clone();
                self.usage = event["message"]["usage"].clone();
                if target == Protocol::Chat {
                    output.push(frame(
                        &self.chat(json!({"role":"assistant","content":""}), Value::Null),
                    ));
                }
            }
            Some("content_block_start") => {
                let block = &event["content_block"];
                match block["type"].as_str() {
                    Some("tool_use") => {
                        if self.tools.len() >= 256 {
                            return Err(failure());
                        }
                        let tool = Tool {
                            index: self.next_tool,
                            id: block["id"].clone(),
                            name: block["name"].clone(),
                            args: String::new(),
                            initial: block["input"].clone(),
                        };
                        self.next_tool += 1;
                        if target == Protocol::Chat {
                            output.push(frame(&self.chat(json!({"tool_calls":[{"index":tool.index,"id":tool.id,"type":"function","function":{"name":tool.name,"arguments":""}}]}),Value::Null)));
                        }
                        if self.tools.insert(index, tool).is_some() {
                            return Err(failure());
                        }
                    }
                    Some("text") => {
                        if let Some(text) = block["text"].as_str().filter(|s| !s.is_empty()) {
                            output.push(self.text(target, text, false));
                        }
                    }
                    Some("thinking") => {
                        if let Some(text) = block["thinking"].as_str().filter(|s| !s.is_empty()) {
                            output.push(self.text(target, text, true));
                        }
                    }
                    // A signed/redacted opaque block cannot be represented by Chat/Gemini.
                    _ => return Err(failure()),
                }
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => output.push(self.text(
                        target,
                        delta["text"].as_str().ok_or_else(failure)?,
                        false,
                    )),
                    Some("thinking_delta") => output.push(self.text(
                        target,
                        delta["thinking"].as_str().ok_or_else(failure)?,
                        true,
                    )),
                    Some("signature_delta") => return Err(failure()),
                    Some("input_json_delta") => {
                        let args = delta["partial_json"].as_str().ok_or_else(failure)?;
                        let tool = self.tools.get_mut(&index).ok_or_else(failure)?;
                        if tool.args.len() + args.len() > MAX_EVENT {
                            return Err(failure());
                        }
                        tool.args.push_str(args);
                        let tool_index = tool.index;
                        if target == Protocol::Chat {
                            output.push(frame(&self.chat(json!({"tool_calls":[{"index":tool_index,"function":{"arguments":args}}]}),Value::Null)));
                        }
                    }
                    _ => return Err(failure()),
                }
            }
            Some("content_block_stop") => {
                if let Some(tool) = self.tools.remove(&index) {
                    let args: Value = if tool.args.is_empty() {
                        tool.initial
                    } else {
                        serde_json::from_str(&tool.args).map_err(|_| failure())?
                    };
                    if !args.is_object() {
                        return Err(failure());
                    }
                    if target == Protocol::Gemini {
                        output.push(frame(&self.gemini(vec![
                            json!({"functionCall":{"id":tool.id,"name":tool.name,"args":args}}),
                        ])));
                    } else if tool.args.is_empty() {
                        output.push(frame(&self.chat(json!({"tool_calls":[{"index":tool.index,"function":{"arguments":args.to_string()}}]}),Value::Null)));
                    }
                }
            }
            Some("message_delta") => {
                if let Some(usage) = event["usage"].as_object() {
                    if !self.usage.is_object() {
                        self.usage = json!({})
                    }
                    for (key, value) in usage {
                        self.usage[key] = value.clone();
                    }
                }
                if event["delta"]["stop_reason"].as_str().is_none() {
                    return Ok(output);
                }
                let input = prompt_tokens(&self.usage);
                let out = self.usage["output_tokens"].as_u64().unwrap_or(0);
                let value = if target == Protocol::Chat {
                    let mut value = self.chat(
                        json!({}),
                        json!(chat_finish(event["delta"]["stop_reason"].as_str())),
                    );
                    value["usage"] = json!({"prompt_tokens":input,"completion_tokens":out,"total_tokens":input.saturating_add(out),"prompt_tokens_details":{"cached_tokens":self.usage["cache_read_input_tokens"].as_u64().unwrap_or(0)}});
                    value
                } else {
                    let mut value = self.gemini(Vec::new());
                    value["candidates"][0]["finishReason"] =
                        json!(gemini_finish(event["delta"]["stop_reason"].as_str()));
                    value["usageMetadata"] = json!({"promptTokenCount":input,"candidatesTokenCount":out,"totalTokenCount":input.saturating_add(out),"cachedContentTokenCount":self.usage["cache_read_input_tokens"].as_u64().unwrap_or(0)});
                    value
                };
                output.push(frame(&value));
                self.finished = true;
            }
            Some("message_stop") => {
                if !self.tools.is_empty() || !self.finished {
                    return Err(failure());
                }
                if target == Protocol::Chat {
                    output.push(Bytes::from_static(b"data: [DONE]\n\n"));
                }
                self.stopped = true;
            }
            Some("ping") => {}
            _ => return Err(failure()),
        }
        Ok(output)
    }
    fn text(&self, target: Protocol, text: &str, thinking: bool) -> Bytes {
        frame(&if target == Protocol::Chat {
            self.chat(
                if thinking {
                    json!({"reasoning_content":text})
                } else {
                    json!({"content":text})
                },
                Value::Null,
            )
        } else {
            self.gemini(vec![if thinking {
                json!({"text":text,"thought":true})
            } else {
                json!({"text":text})
            }])
        })
    }
}
pub(super) fn from_anthropic(
    mut source: ByteStream,
    target: Protocol,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    async_stream::try_stream! {
        let mut state=State::default();let mut buffer=String::new();let mut remainder=Vec::new();
        while let Some(chunk)=source.next().await {
            let chunk=chunk?;append_utf8_safe(&mut buffer,&mut remainder,&chunk);
            while let Some(block)=take_sse_block(&mut buffer){
                if block.len()>MAX_EVENT{Err(failure())?;}
                let data=block.lines().filter_map(|l|strip_sse_field(l,"data")).collect::<Vec<_>>().join("\n");
                if data.is_empty(){continue}
                let event:Value=serde_json::from_str(&data).map_err(|_|failure())?;
                for frame in state.accept(event,target)?{yield frame;}
                if state.stopped {return;}
            }
            if buffer.len()>MAX_EVENT{Err(failure())?;}
        }
        Err(failure())?;
    }
}
// Once HTTP headers have been sent, report a protocol-native error event.
// A dropped HTTP body alone may look like an empty successful response to clients.
pub(super) fn to_client(source: ByteStream, target: Protocol) -> ByteStream {
    Box::pin(async_stream::stream! {
        let stream = from_anthropic(source, target);
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(chunk) => yield Ok(chunk),
                Err(_) => {
                    let error = if target == Protocol::Gemini {
                        json!({"error":{"code":502,"message":"Invalid or incomplete upstream protocol stream","status":"INTERNAL"}})
                    } else {
                        json!({"error":{"message":"Invalid or incomplete upstream protocol stream","type":"upstream_error","code":"protocol_stream_error"}})
                    };
                    yield Ok(frame(&error));
                    return;
                }
            }
        }
    })
}

pub(super) fn message_sse(message: &Value) -> Result<Bytes, ProxyError> {
    let content = array(&message["content"], "Invalid upstream content")?;
    let mut events = Vec::new();
    let mut start = message.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    events.push(json!({"type":"message_start","message":start}));
    for (index, block) in content.iter().enumerate() {
        events.push(json!({"type":"content_block_start","index":index,"content_block":block}));
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":message["stop_reason"]},"usage":message["usage"]}));
    events.push(json!({"type":"message_stop"}));
    Ok(Bytes::from(
        events
            .into_iter()
            .map(|e| {
                format!(
                    "event: {}\ndata: {e}\n\n",
                    e["type"].as_str().unwrap_or("message")
                )
            })
            .collect::<String>(),
    ))
}
