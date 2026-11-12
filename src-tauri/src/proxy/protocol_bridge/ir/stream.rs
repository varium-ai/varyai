//! Incremental vendor wire decoders and encoders connected through semantic events.
use super::super::ByteStream;
use super::{reply, Event, Part, Protocol, Reply, Usage};
use crate::proxy::providers::transform_codex_chat::{
    response_tool_call_item_from_chat_name, CodexToolContext,
};
use crate::proxy::{
    sse::{append_utf8_safe_observed, strip_sse_field, take_sse_block},
    ProxyError,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io;

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_REPLY: usize = 64 * 1024 * 1024;
const MAX_PARTS: usize = 4096;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or incomplete upstream protocol stream",
    )
}
fn string(v: &Value) -> String {
    v.as_str().unwrap_or_default().into()
}
fn frame(v: &Value) -> Bytes {
    Bytes::from(format!("data: {v}\n\n"))
}
fn named(v: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {}\ndata: {v}\n\n",
        v["type"].as_str().unwrap_or("message")
    ))
}
fn response_frame(mut value: Value, sequence: &mut u64) -> Bytes {
    value["sequence_number"] = json!(*sequence);
    *sequence += 1;
    named(&value)
}
fn index(v: &Value) -> Result<usize, io::Error> {
    v.as_u64().unwrap_or(0).try_into().map_err(|_| invalid())
}

#[derive(Clone)]
struct Tool {
    id: String,
    name: String,
    args: String,
    sent: usize,
    initial: Value,
    started: bool,
    ended: bool,
}
struct Custom {
    input: String,
    name: String,
    namespace: Option<String>,
}
impl Tool {
    fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            args: String::new(),
            sent: 0,
            initial: json!({}),
            started: false,
            ended: false,
        }
    }
}
struct Decoder {
    protocol: Protocol,
    started: bool,
    finished: bool,
    refused: bool,
    ended: bool,
    next: usize,
    keys: HashMap<String, usize>,
    tools: BTreeMap<usize, Tool>,
    custom: HashMap<usize, Custom>,
    namespaced: HashMap<usize, (String, String)>,
    texts: HashMap<usize, String>,
    closed: std::collections::HashSet<usize>,
    usage: Usage,
    gemini_kind: Option<(bool, usize)>,
}
impl Decoder {
    fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            started: false,
            finished: false,
            refused: false,
            ended: false,
            next: 0,
            keys: HashMap::new(),
            tools: BTreeMap::new(),
            custom: HashMap::new(),
            namespaced: HashMap::new(),
            texts: HashMap::new(),
            closed: Default::default(),
            usage: Usage::default(),
            gemini_kind: None,
        }
    }
    fn key(&mut self, key: String) -> Result<usize, io::Error> {
        if let Some(i) = self.keys.get(&key) {
            return Ok(*i);
        }
        if self.next >= MAX_PARTS {
            return Err(invalid());
        }
        let i = self.next;
        self.next += 1;
        self.keys.insert(key, i);
        Ok(i)
    }
    fn start(&mut self, id: &Value, model: &Value, events: &mut Vec<Event>) {
        if !self.started {
            events.push(Event::Start {
                id: string(id),
                model: string(model),
            });
            self.started = true;
        }
    }
    fn text(
        &mut self,
        index: usize,
        text: String,
        thinking: bool,
        events: &mut Vec<Event>,
    ) -> Result<(), io::Error> {
        let stored = self.texts.entry(index).or_default();
        if stored.len().saturating_add(text.len()) > MAX_REPLY {
            return Err(invalid());
        }
        stored.push_str(&text);
        events.push(if thinking {
            Event::Thinking { index, text }
        } else {
            Event::Text { index, text }
        });
        Ok(())
    }
    fn complete_text(
        &mut self,
        index: usize,
        text: &str,
        thinking: bool,
        events: &mut Vec<Event>,
    ) -> Result<(), io::Error> {
        let previous = self
            .texts
            .get(&index)
            .map(String::as_str)
            .unwrap_or_default();
        if let Some(tail) = text.strip_prefix(previous) {
            let tail = tail.to_owned();
            if !tail.is_empty() || previous.is_empty() {
                self.text(index, tail, thinking, events)?;
            }
        } else if previous != text {
            return Err(invalid());
        }
        Ok(())
    }
    fn tool(
        &mut self,
        index: usize,
        id: String,
        name: String,
        events: &mut Vec<Event>,
    ) -> Result<(), io::Error> {
        if self.tools.len() >= MAX_PARTS && !self.tools.contains_key(&index) {
            return Err(invalid());
        }
        let tool = self
            .tools
            .entry(index)
            .or_insert_with(|| Tool::new(id.clone(), name.clone()));
        if tool.ended {
            return Err(invalid());
        }
        if !id.is_empty() {
            if tool.id.is_empty() {
                tool.id = id;
            } else if tool.id != id {
                return Err(invalid());
            }
        }
        if !name.is_empty() {
            if tool.started && tool.name != name {
                return Err(invalid());
            }
            if !tool.started {
                tool.name = name;
            }
        }
        if !tool.started && !tool.name.is_empty() {
            if tool.id.is_empty() {
                tool.id = format!("call_{}", uuid::Uuid::new_v4().simple());
            }
            events.push(Event::ToolStart {
                index,
                id: tool.id.clone(),
                name: tool.name.clone(),
            });
            tool.started = true;
        }
        Ok(())
    }
    fn args(&mut self, index: usize, args: &str, events: &mut Vec<Event>) -> Result<(), io::Error> {
        let tool = self.tools.get_mut(&index).ok_or_else(invalid)?;
        if tool.ended || !tool.started || tool.args.len().saturating_add(args.len()) > MAX_FRAME {
            return Err(invalid());
        }
        let held = tool.sent < tool.args.len();
        tool.args.push_str(args);
        // Keep a complete JSON tail until the done event: some upstreams emit
        // '{}' as a placeholder before their final, richer argument object.
        if !args.is_empty() && !held && serde_json::from_str::<Value>(&tool.args).is_err() {
            events.push(Event::ToolArguments {
                index,
                json: args.into(),
            });
            tool.sent = tool.args.len();
        }
        Ok(())
    }
    fn finish_tool(
        &mut self,
        index: usize,
        full: Option<&Value>,
        events: &mut Vec<Event>,
    ) -> Result<(), io::Error> {
        let tool = self.tools.get(&index).ok_or_else(invalid)?;
        if tool.ended {
            return Ok(());
        }
        if !tool.started {
            return Err(invalid());
        }
        let final_args = if let Some(full) = full {
            if let Some(s) = full.as_str() {
                s.to_owned()
            } else {
                full.to_string()
            }
        } else if tool.args.is_empty() {
            tool.initial.to_string()
        } else {
            tool.args.clone()
        };
        let tail = final_args
            .strip_prefix(&tool.args[..tool.sent])
            .ok_or_else(invalid)?
            .to_owned();
        reply::arguments(&json!(final_args)).map_err(|_| invalid())?;
        if !tail.is_empty() {
            events.push(Event::ToolArguments { index, json: tail });
        }
        let tool = self.tools.get_mut(&index).ok_or_else(invalid)?;
        tool.args = final_args;
        tool.sent = tool.args.len();
        tool.ended = true;
        if let Some((namespace, name)) = self.namespaced.get(&index) {
            events.push(Event::Part {
                index,
                part: Part::NamespacedToolCall {
                    id: tool.id.clone(),
                    namespace: namespace.clone(),
                    name: name.clone(),
                    arguments: reply::arguments(&json!(tool.args)).map_err(|_| invalid())?,
                    signature: None,
                },
            });
        }
        self.close(index, events);
        Ok(())
    }
    fn close(&mut self, index: usize, events: &mut Vec<Event>) {
        if self.closed.insert(index) {
            events.push(Event::PartEnd { index });
        }
    }
    fn custom_input(&mut self, i: usize, delta: &str, e: &mut Vec<Event>) -> Result<(), io::Error> {
        let custom = self.custom.get_mut(&i).ok_or_else(invalid)?;
        if custom.input.len().saturating_add(delta.len()) > MAX_FRAME {
            return Err(invalid());
        }
        custom.input.push_str(delta);
        let quoted = serde_json::to_string(delta).map_err(|_| invalid())?;
        self.args(i, &quoted[1..quoted.len() - 1], e)
    }
    fn finish_custom(
        &mut self,
        i: usize,
        full: Option<&str>,
        e: &mut Vec<Event>,
    ) -> Result<(), io::Error> {
        if self.tools.get(&i).is_some_and(|t| t.ended) {
            return Ok(());
        }
        if let Some(full) = full {
            let current = &self.custom.get(&i).ok_or_else(invalid)?.input;
            let tail = full.strip_prefix(current).ok_or_else(invalid)?.to_owned();
            self.custom_input(i, &tail, e)?;
        }
        self.args(i, "\"}", e)?;
        let custom = self.custom.get(&i).ok_or_else(invalid)?;
        let id = self.tools.get(&i).ok_or_else(invalid)?.id.clone();
        e.push(Event::Part {
            index: i,
            part: Part::CustomToolCall {
                id,
                name: custom.name.clone(),
                input: custom.input.clone(),
                namespace: custom.namespace.clone(),
            },
        });
        self.finish_tool(i, None, e)
    }
    fn finish(&mut self, events: &mut Vec<Event>) -> Result<(), io::Error> {
        if self.ended {
            return Ok(());
        }
        if !self.finished {
            return Err(invalid());
        }
        let tools: Vec<_> = self.tools.keys().copied().collect();
        for i in tools {
            if self.custom.contains_key(&i) {
                self.finish_custom(i, None, events)?;
            } else {
                self.finish_tool(i, None, events)?;
            }
        }
        let texts: Vec<_> = self.texts.keys().copied().collect();
        for i in texts {
            self.close(i, events);
        }
        events.push(Event::Usage(self.usage.clone()));
        events.push(Event::End);
        self.ended = true;
        Ok(())
    }
    fn decode(&mut self, data: &str) -> Result<Vec<Event>, io::Error> {
        if self.ended {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        if data.trim() == "[DONE]" {
            self.finish(&mut events)?;
            return Ok(events);
        }
        let v: Value = serde_json::from_str(data).map_err(|_| invalid())?;
        if v.get("error").is_some_and(|e| !e.is_null()) {
            return Err(invalid());
        }
        match self.protocol {
            Protocol::Chat => self.chat(&v, &mut events)?,
            Protocol::Anthropic => self.anthropic(&v, &mut events)?,
            Protocol::Responses => self.responses(&v, &mut events)?,
            Protocol::Gemini => self.gemini(v.get("response").unwrap_or(&v), &mut events)?,
        }
        Ok(events)
    }
    fn chat(&mut self, v: &Value, e: &mut Vec<Event>) -> Result<(), io::Error> {
        if v["choices"].as_array().is_some_and(|a| a.len() > 1) {
            return Err(invalid());
        }
        self.start(&v["id"], &v["model"], e);
        if v["usage"].is_object() {
            self.usage = reply::usage(self.protocol, &v["usage"]);
            e.push(Event::Usage(self.usage.clone()));
        }
        let Some(c) = v["choices"].as_array().and_then(|a| a.first()) else {
            return Ok(());
        };
        let d = &c["delta"];
        if let Some(text) = d["content"].as_str().filter(|s| !s.is_empty()) {
            let i = self.key("text".into())?;
            self.text(i, text.into(), false, e)?;
        }
        if let Some(text) = d["refusal"].as_str().filter(|s| !s.is_empty()) {
            let i = self.key("refusal".into())?;
            self.text(i, text.into(), false, e)?;
            self.refused = true;
        }
        if let Some(text) = d
            .get("reasoning_content")
            .or_else(|| d.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            let i = self.key("thinking".into())?;
            self.text(i, text.into(), true, e)?;
        }
        if let Some(calls) = d["tool_calls"].as_array() {
            for call in calls {
                let i = self.key(format!("tool:{}", index(&call["index"])?))?;
                self.tool(i, string(&call["id"]), string(&call["function"]["name"]), e)?;
                if let Some(args) = call["function"]["arguments"].as_str() {
                    self.args(i, args, e)?;
                }
            }
        } else if let Some(call) = d.get("function_call").filter(|v| !v.is_null()) {
            let i = self.key("tool:legacy".into())?;
            self.tool(i, String::new(), string(&call["name"]), e)?;
            if let Some(args) = call["arguments"].as_str() {
                self.args(i, args, e)?;
            }
        }
        if let Some(reason) = c["finish_reason"].as_str() {
            self.finished = true;
            e.push(Event::Stop(if self.refused {
                "filter".into()
            } else {
                reply::stop(self.protocol, reason)
            }));
        }
        Ok(())
    }
    fn anthropic(&mut self, v: &Value, e: &mut Vec<Event>) -> Result<(), io::Error> {
        match v["type"].as_str().unwrap_or_default() {
            "message_start" => {
                self.start(&v["message"]["id"], &v["message"]["model"], e);
                self.usage = reply::usage(self.protocol, &v["message"]["usage"]);
                e.push(Event::Usage(self.usage.clone()));
            }
            "content_block_start" => {
                let i = self.key(format!("block:{}", index(&v["index"])?))?;
                let b = &v["content_block"];
                match b["type"].as_str() {
                    Some("tool_use") => {
                        self.tool(i, string(&b["id"]), string(&b["name"]), e)?;
                        self.tools.get_mut(&i).ok_or_else(invalid)?.initial =
                            b.get("input").cloned().unwrap_or(json!({}));
                    }
                    Some("text") => self.text(i, string(&b["text"]), false, e)?,
                    Some("thinking") => {
                        self.text(i, string(&b["thinking"]), true, e)?;
                        if let Some(s) = b["signature"].as_str().filter(|s| !s.is_empty()) {
                            e.push(Event::Signature {
                                index: i,
                                signature: s.into(),
                                source: self.protocol,
                            });
                        }
                    }
                    _ => {
                        if let Some(part) =
                            reply::parse_part(self.protocol, b).map_err(|_| invalid())?
                        {
                            e.push(Event::Part { index: i, part });
                        }
                    }
                }
            }
            "content_block_delta" => {
                let i = *self
                    .keys
                    .get(&format!("block:{}", index(&v["index"])?))
                    .ok_or_else(invalid)?;
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("text_delta") => self.text(i, string(&d["text"]), false, e)?,
                    Some("thinking_delta") => self.text(i, string(&d["thinking"]), true, e)?,
                    Some("signature_delta") => e.push(Event::Signature {
                        index: i,
                        signature: string(&d["signature"]),
                        source: self.protocol,
                    }),
                    Some("input_json_delta") => {
                        self.args(i, d["partial_json"].as_str().ok_or_else(invalid)?, e)?
                    }
                    _ => return Err(invalid()),
                }
            }
            "content_block_stop" => {
                let i = *self
                    .keys
                    .get(&format!("block:{}", index(&v["index"])?))
                    .ok_or_else(invalid)?;
                if self.tools.contains_key(&i) {
                    self.finish_tool(i, None, e)?;
                } else {
                    self.close(i, e);
                }
            }
            "message_delta" => {
                let u = &v["usage"];
                let next = reply::usage(self.protocol, u);
                if u.get("input_tokens").is_some() {
                    self.usage.input = next.input;
                }
                if u.get("output_tokens").is_some() {
                    self.usage.output = next.output;
                }
                if u.get("cache_read_input_tokens").is_some() {
                    self.usage.cache_read = next.cache_read;
                }
                if u.get("cache_creation_input_tokens").is_some() {
                    self.usage.cache_write = next.cache_write;
                }
                e.push(Event::Usage(self.usage.clone()));
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    self.finished = true;
                    e.push(Event::Stop(reply::stop(self.protocol, s)));
                }
            }
            "message_stop" => self.finish(e)?,
            "ping" => {}
            "error" => return Err(invalid()),
            _ => {}
        }
        Ok(())
    }
    fn responses(&mut self, v: &Value, e: &mut Vec<Event>) -> Result<(), io::Error> {
        let kind = v["type"].as_str().unwrap_or_default();
        if matches!(kind, "response.created" | "response.in_progress") {
            self.start(&v["response"]["id"], &v["response"]["model"], e);
            return Ok(());
        }
        let out = index(&v["output_index"])?;
        if matches!(
            kind,
            "response.output_item.added" | "response.output_item.done"
        ) {
            let item = &v["item"];
            let i = self.key(format!("item:{out}"))?;
            if let Some(id) = item["id"].as_str() {
                self.keys.insert(format!("id:{id}"), i);
            }
            match item["type"].as_str() {
                Some("custom_tool_call") => {
                    if !self.tools.contains_key(&i) {
                        let name = string(&item["name"]);
                        let namespace = item["namespace"].as_str().map(str::to_owned);
                        let wire_name=namespace.as_ref().map(|ns|crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(ns,&name)).unwrap_or_else(||name.clone());
                        self.tool(
                            i,
                            string(item.get("call_id").unwrap_or(&item["id"])),
                            wire_name,
                            e,
                        )?;
                        self.custom.insert(
                            i,
                            Custom {
                                input: String::new(),
                                name,
                                namespace,
                            },
                        );
                        self.args(i, "{\"input\":\"", e)?;
                    }
                    if kind.ends_with(".done") {
                        self.finish_custom(i, item["input"].as_str(), e)?;
                    } else if let Some(input) = item["input"].as_str().filter(|s| !s.is_empty()) {
                        self.custom_input(i, input, e)?;
                    }
                }
                Some("function_call") => {
                    if !self.tools.contains_key(&i) {
                        let mut name = string(&item["name"]);
                        if let Some(namespace) = item["namespace"].as_str() {
                            self.namespaced.insert(i, (namespace.into(), name.clone()));
                            name=crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(namespace,&name);
                        }
                        self.tool(
                            i,
                            string(item.get("call_id").unwrap_or(&item["id"])),
                            name,
                            e,
                        )?;
                    }
                    if kind.ends_with(".done") {
                        self.finish_tool(i, item.get("arguments"), e)?;
                    } else if let Some(args) = item["arguments"].as_str().filter(|s| !s.is_empty())
                    {
                        self.args(i, args, e)?;
                    }
                }
                Some("reasoning") => {
                    let text = item["summary"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| p["text"].as_str())
                                .collect::<String>()
                        })
                        .unwrap_or_default();
                    self.complete_text(i, &text, true, e)?;
                    if kind.ends_with(".done") {
                        if item.get("encrypted_content").is_some() {
                            if let Some(part) =
                                reply::parse_part(self.protocol, item).map_err(|_| invalid())?
                            {
                                e.push(Event::Part { index: i, part });
                            }
                        }
                        self.close(i, e);
                    }
                }
                Some("message") => {
                    if kind.ends_with(".done") {
                        if let Some(parts) = item["content"].as_array() {
                            for (n, p) in parts.iter().enumerate() {
                                let i = self.key(format!("text:{out}:{n}"))?;
                                if let Some(t) = p["text"].as_str() {
                                    self.complete_text(i, t, false, e)?;
                                    self.close(i, e);
                                }
                            }
                        }
                    }
                }
                _ => {
                    if kind.ends_with(".done") {
                        if let Some(part) =
                            reply::parse_part(self.protocol, item).map_err(|_| invalid())?
                        {
                            e.push(Event::Part { index: i, part });
                            self.close(i, e);
                        }
                    }
                }
            }
        } else if matches!(
            kind,
            "response.output_text.delta" | "response.refusal.delta"
        ) {
            let i = self.key(format!("text:{out}:{}", index(&v["content_index"])?))?;
            self.text(i, string(&v["delta"]), false, e)?;
        } else if kind == "response.reasoning_summary_text.delta"
            || kind == "response.reasoning_text.delta"
        {
            let i = self.key(format!("item:{out}"))?;
            self.text(i, string(&v["delta"]), true, e)?;
        } else if kind == "response.custom_tool_call_input.delta"
            || kind == "response.custom_tool_call_input.done"
        {
            let i = v["item_id"]
                .as_str()
                .and_then(|id| self.keys.get(&format!("id:{id}")).copied())
                .or_else(|| self.keys.get(&format!("item:{out}")).copied())
                .ok_or_else(invalid)?;
            if kind.ends_with(".delta") {
                self.custom_input(i, v["delta"].as_str().ok_or_else(invalid)?, e)?;
            } else {
                self.finish_custom(i, v["input"].as_str(), e)?;
            }
        } else if kind == "response.function_call_arguments.delta"
            || kind == "response.function_call_arguments.done"
        {
            let i = v["item_id"]
                .as_str()
                .and_then(|id| self.keys.get(&format!("id:{id}")).copied())
                .or_else(|| self.keys.get(&format!("item:{out}")).copied())
                .ok_or_else(invalid)?;
            if kind.ends_with(".delta") {
                self.args(i, v["delta"].as_str().ok_or_else(invalid)?, e)?;
            } else {
                self.finish_tool(i, v.get("arguments"), e)?;
            }
        } else if matches!(
            kind,
            "response.completed" | "response.incomplete" | "response.done"
        ) {
            let response = &v["response"];
            self.start(&response["id"], &response["model"], e);
            if response["status"] == "failed" {
                return Err(invalid());
            }
            if let Some(items) = response["output"].as_array() {
                for (n, item) in items.iter().enumerate() {
                    let i = self.key(format!("item:{n}"))?;
                    match item["type"].as_str() {
                        Some("function_call") => {
                            if !self.tools.contains_key(&i) {
                                let mut name = string(&item["name"]);
                                if let Some(namespace) = item["namespace"].as_str() {
                                    self.namespaced.insert(i, (namespace.into(), name.clone()));
                                    name=crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(namespace,&name);
                                }
                                self.tool(
                                    i,
                                    string(item.get("call_id").unwrap_or(&item["id"])),
                                    name,
                                    e,
                                )?;
                            }
                            self.finish_tool(i, item.get("arguments"), e)?;
                        }
                        Some("custom_tool_call") => {
                            if !self.tools.contains_key(&i) {
                                let name = string(&item["name"]);
                                let namespace = item["namespace"].as_str().map(str::to_owned);
                                let wire_name=namespace.as_ref().map(|ns|crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(ns,&name)).unwrap_or_else(||name.clone());
                                self.tool(
                                    i,
                                    string(item.get("call_id").unwrap_or(&item["id"])),
                                    wire_name,
                                    e,
                                )?;
                                self.custom.insert(
                                    i,
                                    Custom {
                                        input: String::new(),
                                        name,
                                        namespace,
                                    },
                                );
                                self.args(i, "{\"input\":\"", e)?;
                            }
                            self.finish_custom(i, item["input"].as_str(), e)?;
                        }
                        Some("message") => {
                            if let Some(parts) = item["content"].as_array() {
                                for (j, p) in parts.iter().enumerate() {
                                    let i = self.key(format!("text:{n}:{j}"))?;
                                    if let Some(t) = p["text"].as_str() {
                                        self.complete_text(i, t, false, e)?;
                                    }
                                }
                            }
                        }
                        Some("reasoning") => {
                            if !self.texts.contains_key(&i) {
                                if let Some(part) =
                                    reply::parse_part(self.protocol, item).map_err(|_| invalid())?
                                {
                                    e.push(Event::Part { index: i, part });
                                }
                            }
                        }
                        _ => {
                            if let Some(part) =
                                reply::parse_part(self.protocol, item).map_err(|_| invalid())?
                            {
                                e.push(Event::Part { index: i, part });
                                self.close(i, e);
                            }
                        }
                    }
                }
            }
            self.usage = reply::usage(self.protocol, &response["usage"]);
            self.finished = true;
            let mut stop = reply::stop(
                self.protocol,
                response["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or_default(),
            );
            if stop == "stop" && !self.tools.is_empty() {
                stop = "tool".into();
            }
            e.push(Event::Stop(stop));
            self.finish(e)?;
        } else if matches!(kind, "response.failed" | "error") {
            return Err(invalid());
        }
        Ok(())
    }
    fn gemini(&mut self, v: &Value, e: &mut Vec<Event>) -> Result<(), io::Error> {
        if v["candidates"].as_array().is_some_and(|a| a.len() > 1) {
            return Err(invalid());
        }
        self.start(&v["responseId"], &v["modelVersion"], e);
        if let Some(c) = v["candidates"].as_array().and_then(|a| a.first()) {
            if let Some(parts) = c["content"]["parts"].as_array() {
                for p in parts {
                    if let Some(text) = p["text"].as_str() {
                        let thinking = p["thought"] == true;
                        let i = match self.gemini_kind {
                            Some((kind, i)) if kind == thinking && !self.closed.contains(&i) => i,
                            _ => {
                                let i = self.key(format!("gemini:{}", self.next))?;
                                self.gemini_kind = Some((thinking, i));
                                i
                            }
                        };
                        self.text(i, text.into(), thinking, e)?;
                        if let Some(s) = p["thoughtSignature"].as_str() {
                            e.push(Event::Signature {
                                index: i,
                                signature: s.into(),
                                source: self.protocol,
                            });
                            self.close(i, e);
                        }
                    } else if let Some(part) =
                        reply::parse_part(self.protocol, p).map_err(|_| invalid())?
                    {
                        self.gemini_kind = None;
                        let i = self.key(format!("gemini:{}", self.next))?;
                        match part {
                            Part::ToolCall {
                                id,
                                name,
                                arguments,
                                signature,
                            } => {
                                self.tool(i, id, name, e)?;
                                self.args(i, &arguments.to_string(), e)?;
                                if let Some(signature) = signature {
                                    e.push(Event::Signature {
                                        index: i,
                                        signature,
                                        source: self.protocol,
                                    });
                                }
                                self.finish_tool(i, None, e)?;
                            }
                            part => {
                                e.push(Event::Part { index: i, part });
                                self.close(i, e);
                            }
                        }
                    }
                }
            }
            if let Some(s) = c["finishReason"].as_str() {
                self.finished = true;
                let mut stop = reply::stop(self.protocol, s);
                if stop == "stop" && !self.tools.is_empty() {
                    stop = "tool".into();
                }
                e.push(Event::Stop(stop));
            }
        } else if v["promptFeedback"]["blockReason"].is_string() {
            self.finished = true;
            e.push(Event::Stop("filter".into()));
        }
        if v["usageMetadata"].is_object() {
            self.usage = reply::usage(self.protocol, &v["usageMetadata"]);
            e.push(Event::Usage(self.usage.clone()));
        }
        Ok(())
    }
}

struct Encoder {
    protocol: Protocol,
    context: CodexToolContext,
    reply: Reply,
    parts: BTreeMap<usize, Part>,
    args: HashMap<usize, String>,
    wire_indices: HashMap<usize, usize>,
    tool_indices: HashMap<usize, usize>,
    closed: std::collections::HashSet<usize>,
    started: bool,
    ended: bool,
    sequence: u64,
}
impl Encoder {
    fn new(protocol: Protocol) -> Self {
        Self::with_context(protocol, Default::default())
    }
    fn with_context(protocol: Protocol, context: CodexToolContext) -> Self {
        Self {
            protocol,
            context,
            reply: Reply {
                id: String::new(),
                model: String::new(),
                parts: Vec::new(),
                stop: "stop".into(),
                usage: Usage::default(),
            },
            parts: BTreeMap::new(),
            args: HashMap::new(),
            wire_indices: HashMap::new(),
            tool_indices: HashMap::new(),
            closed: Default::default(),
            started: false,
            ended: false,
            sequence: 0,
        }
    }
    fn response_part(&self, part: &Part, index: usize) -> Option<Value> {
        if let Part::ToolCall {
            id,
            name,
            arguments,
            ..
        } = part
        {
            Some(response_tool_call_item_from_chat_name(
                &format!("fc_{index}"),
                "completed",
                id,
                name,
                &arguments.to_string(),
                None,
                &self.context,
            ))
        } else {
            reply::part(Protocol::Responses, part, index)
        }
    }
    fn chat(&self, delta: Value, finish: Value) -> Value {
        json!({"id":self.reply.id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":self.reply.model,"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    }
    fn gemini(&self, parts: Vec<Value>) -> Value {
        json!({"responseId":self.reply.id,"modelVersion":self.reply.model,"candidates":[{"index":0,"content":{"role":"model","parts":parts}}]})
    }
    fn response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({"id":self.reply.id,"object":"response","created_at":chrono::Utc::now().timestamp(),"model":self.reply.model,"status":status,"output":output,"error":null})
    }
    fn start(&mut self, out: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        match self.protocol{
        Protocol::Chat=>out.push(frame(&self.chat(json!({"role":"assistant","content":""}),Value::Null))),
        Protocol::Anthropic=>out.push(named(&json!({"type":"message_start","message":{"id":self.reply.id,"type":"message","role":"assistant","model":self.reply.model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":reply::render_usage(self.protocol,&self.reply.usage)}}))),
        Protocol::Responses=>{out.push(response_frame(json!({"type":"response.created","response":self.response("in_progress",vec![])}), &mut self.sequence));out.push(response_frame(json!({"type":"response.in_progress","response":self.response("in_progress",vec![])}), &mut self.sequence));},Protocol::Gemini=>{},
    }
    }
    fn open(&mut self, i: usize, part: Part, out: &mut Vec<Bytes>) -> Result<usize, io::Error> {
        if let Some(index) = self.wire_indices.get(&i) {
            return Ok(*index);
        }
        if self.parts.len() >= MAX_PARTS {
            return Err(invalid());
        }
        let index = self.wire_indices.len();
        self.wire_indices.insert(i, index);
        self.parts.insert(i, part.clone());
        match self.protocol {
            Protocol::Anthropic => {
                let block = if matches!(&part, Part::Thinking { sealed: None, .. }) {
                    Some(json!({"type":"thinking","thinking":"","signature":""}))
                } else {
                    reply::part(self.protocol, &part, index)
                };
                if let Some(mut block) = block {
                    match &part {
                        Part::Text { .. } => block["text"] = json!(""),
                        Part::Thinking { sealed: None, .. } => {
                            block["thinking"] = json!("");
                            block["signature"] = json!("");
                        }
                        Part::ToolCall { .. } => block["input"] = json!({}),
                        _ => {}
                    }
                    out.push(named(
                        &json!({"type":"content_block_start","index":index,"content_block":block}),
                    ));
                }
            }
            Protocol::Responses => {
                let item = if matches!(&part, Part::Thinking { sealed: None, .. }) {
                    Some(
                        json!({"id":format!("rs_{index}"),"type":"reasoning","status":"in_progress","summary":[]}),
                    )
                } else {
                    self.response_part(&part, index)
                };
                if let Some(mut item) = item {
                    item["status"] = json!("in_progress");
                    if let Part::Text { .. } = part {
                        item["content"] = json!([]);
                    }
                    if let Part::Thinking { .. } = part {
                        item["summary"] = json!([]);
                    }
                    if item["type"] == "custom_tool_call" {
                        item["input"] = json!("");
                    } else if let Part::ToolCall { .. } = part {
                        item["arguments"] = json!("");
                    }
                    out.push(response_frame(json!({"type":"response.output_item.added","output_index":index,"item":item}), &mut self.sequence));
                    if matches!(part, Part::Text { .. }) {
                        out.push(response_frame(json!({"type":"response.content_part.added","output_index":index,"content_index":0,"item_id":format!("msg_{index}"),"part":{"type":"output_text","text":"","annotations":[]}}), &mut self.sequence));
                    } else if matches!(part, Part::Thinking { .. }) {
                        out.push(response_frame(json!({"type":"response.reasoning_summary_part.added","output_index":index,"summary_index":0,"item_id":format!("rs_{index}"),"part":{"type":"summary_text","text":""}}), &mut self.sequence));
                    }
                }
            }
            Protocol::Chat => {
                if let Part::ToolCall { id, name, .. } = part {
                    let ti = self.tool_indices.len();
                    self.tool_indices.insert(i, ti);
                    out.push(frame(&self.chat(json!({"tool_calls":[{"index":ti,"id":id,"type":"function","function":{"name":name,"arguments":""}}]}),Value::Null)));
                }
            }
            Protocol::Gemini => {}
        }
        Ok(index)
    }
    fn close(&mut self, i: usize, out: &mut Vec<Bytes>) -> Result<(), io::Error> {
        if !self.closed.insert(i) {
            return Ok(());
        }
        let Some(index) = self.wire_indices.get(&i).copied() else {
            return Ok(());
        };
        if let Some(args) = self.args.get(&i) {
            let args = reply::arguments(&json!(args)).map_err(|_| invalid())?;
            if let Some(Part::ToolCall { arguments, .. }) = self.parts.get_mut(&i) {
                *arguments = args;
            }
        }
        let part = self.parts.get(&i).ok_or_else(invalid)?;
        match self.protocol {
            Protocol::Anthropic => {
                if reply::part(self.protocol, part, index).is_some() {
                    out.push(named(&json!({"type":"content_block_stop","index":index})));
                }
            }
            Protocol::Responses => {
                if let Some(item) = self.response_part(part, index) {
                    match part {
                        Part::Text{text,..}=>{out.push(response_frame(json!({"type":"response.output_text.done","output_index":index,"content_index":0,"item_id":format!("msg_{index}"),"text":text}), &mut self.sequence));out.push(response_frame(json!({"type":"response.content_part.done","output_index":index,"content_index":0,"item_id":format!("msg_{index}"),"part":{"type":"output_text","text":text,"annotations":[]}}), &mut self.sequence));},
                        Part::Thinking{text,..}=>out.push(response_frame(json!({"type":"response.reasoning_summary_text.done","output_index":index,"summary_index":0,"item_id":format!("rs_{index}"),"text":text}), &mut self.sequence)),
                        Part::ToolCall{arguments,..}=>{
                            if item["type"]=="custom_tool_call" {
                                let input=item["input"].as_str().unwrap_or_default();
                                out.push(response_frame(json!({"type":"response.custom_tool_call_input.delta","output_index":index,"item_id":format!("fc_{index}"),"delta":input}), &mut self.sequence));
                                out.push(response_frame(json!({"type":"response.custom_tool_call_input.done","output_index":index,"item_id":format!("fc_{index}"),"input":input}), &mut self.sequence));
                            }else if item["type"]=="function_call"{out.push(response_frame(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":format!("fc_{index}"),"arguments":arguments.to_string()}), &mut self.sequence));}
                        },_=>{}
                    }
                    out.push(response_frame(json!({"type":"response.output_item.done","output_index":index,"item":item}), &mut self.sequence));
                }
            }
            Protocol::Gemini => {
                if matches!(
                    part,
                    Part::ToolCall { .. } | Part::Image { .. } | Part::File { .. }
                ) {
                    if let Some(p) = reply::part(self.protocol, part, index) {
                        out.push(frame(&self.gemini(vec![p])));
                    }
                }
            }
            Protocol::Chat => {}
        }
        Ok(())
    }
    fn accept(&mut self, event: Event) -> Result<Vec<Bytes>, io::Error> {
        let mut out = Vec::new();
        if self.ended {
            return Err(invalid());
        }
        if let Event::Start { id, model } = event {
            if !self.started {
                self.reply.id = id;
                self.reply.model = model;
                self.start(&mut out);
            }
            return Ok(out);
        }
        self.start(&mut out);
        let thinking = matches!(&event, Event::Thinking { .. });
        match event {
            Event::Text { index: i, text } | Event::Thinking { index: i, text } => {
                let n = self.open(
                    i,
                    if thinking {
                        Part::Thinking {
                            text: String::new(),
                            signature: None,
                            source: self.protocol,
                            sealed: None,
                        }
                    } else {
                        Part::Text {
                            text: String::new(),
                            signature: None,
                        }
                    },
                    &mut out,
                )?;
                match self.parts.get_mut(&i) {
                    Some(Part::Text { text: stored, .. })
                    | Some(Part::Thinking { text: stored, .. }) => {
                        if stored.len().saturating_add(text.len()) > MAX_REPLY {
                            return Err(invalid());
                        }
                        stored.push_str(&text);
                    }
                    _ => return Err(invalid()),
                }
                if !text.is_empty() {
                    match self.protocol{
                    Protocol::Chat=>out.push(frame(&self.chat(if thinking{json!({"reasoning_content":text})}else{json!({"content":text})},Value::Null))),
                    Protocol::Anthropic=>out.push(named(&json!({"type":"content_block_delta","index":n,"delta":if thinking{json!({"type":"thinking_delta","thinking":text})}else{json!({"type":"text_delta","text":text})}}))),
                    Protocol::Responses=>out.push(response_frame(if thinking{json!({"type":"response.reasoning_summary_text.delta","output_index":n,"summary_index":0,"item_id":format!("rs_{n}"),"delta":text})}else{json!({"type":"response.output_text.delta","output_index":n,"content_index":0,"item_id":format!("msg_{n}"),"delta":text})}, &mut self.sequence)),
                    Protocol::Gemini=>out.push(frame(&self.gemini(vec![if thinking{json!({"text":text,"thought":true})}else{json!({"text":text})}]))),
                }
                }
            }
            Event::ToolStart { index: i, id, name } => {
                self.open(
                    i,
                    Part::ToolCall {
                        id,
                        name,
                        arguments: json!({}),
                        signature: None,
                    },
                    &mut out,
                )?;
                self.args.entry(i).or_default();
            }
            Event::ToolArguments {
                index: i,
                json: args,
            } => {
                let n = *self.wire_indices.get(&i).ok_or_else(invalid)?;
                let stored = self.args.get_mut(&i).ok_or_else(invalid)?;
                if stored.len().saturating_add(args.len()) > MAX_FRAME {
                    return Err(invalid());
                }
                stored.push_str(&args);
                match self.protocol {
                    Protocol::Chat=>{let ti=*self.tool_indices.get(&i).ok_or_else(invalid)?;out.push(frame(&self.chat(json!({"tool_calls":[{"index":ti,"function":{"arguments":args}}]}),Value::Null)));},
                    Protocol::Anthropic=>out.push(named(&json!({"type":"content_block_delta","index":n,"delta":{"type":"input_json_delta","partial_json":args}}))),
                    Protocol::Responses=>{if self.parts.get(&i).and_then(|part|self.response_part(part,n)).is_some_and(|item|item["type"]=="function_call"){out.push(response_frame(json!({"type":"response.function_call_arguments.delta","output_index":n,"item_id":format!("fc_{n}"),"delta":args}), &mut self.sequence));}},Protocol::Gemini=>{}
                }
            }
            Event::Signature {
                index: i,
                signature,
                source,
            } => {
                if let Some(part) = self.parts.get_mut(&i) {
                    match part {
                        Part::Thinking {
                            signature: stored,
                            source: origin,
                            ..
                        } => {
                            *origin = source;
                            stored.get_or_insert_with(String::new).push_str(&signature);
                        }
                        Part::Text {
                            signature: stored, ..
                        }
                        | Part::ToolCall {
                            signature: stored, ..
                        } if source == Protocol::Gemini => {
                            stored.get_or_insert_with(String::new).push_str(&signature)
                        }
                        _ => {}
                    }
                }
                if source == self.protocol {
                    let n = *self.wire_indices.get(&i).ok_or_else(invalid)?;
                    match self.protocol {
                        Protocol::Anthropic => out.push(named(&json!({
                            "type":"content_block_delta", "index":n,
                            "delta":{"type":"signature_delta","signature":signature}
                        }))),
                        Protocol::Gemini
                            if !matches!(self.parts.get(&i), Some(Part::ToolCall { .. })) =>
                        {
                            out.push(frame(&self.gemini(vec![json!({
                                "text":"", "thoughtSignature":signature
                            })])));
                        }
                        _ => {}
                    }
                }
            }
            Event::Part { index: i, part } => {
                if matches!(
                    &part,
                    Part::ToolResult { .. }
                        | Part::CustomToolResult { .. }
                        | Part::ToolReference { .. }
                ) || (self.protocol == Protocol::Chat && matches!(&part, Part::Image { .. }))
                {
                    return Err(invalid());
                }
                let part = if matches!(&part, Part::File { .. })
                    || (self.protocol != Protocol::Responses
                        && matches!(&part, Part::Search { .. }))
                {
                    let rendered =
                        reply::part(Protocol::Anthropic, &part, i).ok_or_else(invalid)?;
                    Part::Text {
                        text: rendered["text"].as_str().ok_or_else(invalid)?.to_owned(),
                        signature: None,
                    }
                } else {
                    part
                };
                if self.parts.contains_key(&i) {
                    if self.protocol == Protocol::Responses
                        && matches!(
                            &part,
                            Part::NamespacedToolCall { .. } | Part::CustomToolCall { .. }
                        )
                    {
                        self.parts.insert(i, part.clone());
                    }
                    if let Part::Thinking {
                        source,
                        sealed,
                        signature,
                        ..
                    } = &part
                    {
                        if let Some(Part::Thinking {
                            source: origin,
                            sealed: stored,
                            signature: sig,
                            ..
                        }) = self.parts.get_mut(&i)
                        {
                            *origin = *source;
                            *stored = sealed.clone();
                            *sig = signature.clone();
                        }
                    }
                } else {
                    match part {
                        Part::NamespacedToolCall {
                            id,
                            namespace,
                            name,
                            arguments,
                            signature,
                        } if self.protocol != Protocol::Responses => {
                            out.extend(self.accept(Event::ToolStart{index:i,id,name:crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(&namespace,&name)})?);
                            out.extend(self.accept(Event::ToolArguments {
                                index: i,
                                json: arguments.to_string(),
                            })?);
                            if let Some(signature) = signature {
                                out.extend(self.accept(Event::Signature {
                                    index: i,
                                    signature,
                                    source: Protocol::Gemini,
                                })?);
                            }
                        }
                        Part::CustomToolCall {
                            id,
                            name,
                            input,
                            namespace,
                        } if self.protocol != Protocol::Responses => {
                            let name=namespace.as_ref().map(|ns|crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(ns,&name)).unwrap_or(name);
                            out.extend(self.accept(Event::ToolStart { index: i, id, name })?);
                            out.extend(self.accept(Event::ToolArguments {
                                index: i,
                                json: json!({"input":input}).to_string(),
                            })?);
                        }
                        Part::Text { text, signature } => {
                            out.extend(self.accept(Event::Text { index: i, text })?);
                            if let Some(signature) = signature {
                                out.extend(self.accept(Event::Signature {
                                    index: i,
                                    signature,
                                    source: Protocol::Gemini,
                                })?);
                            }
                        }
                        Part::Thinking {
                            text,
                            signature,
                            source,
                            sealed,
                        } => {
                            if source == self.protocol && sealed.is_some() {
                                self.open(
                                    i,
                                    Part::Thinking {
                                        text,
                                        signature,
                                        source,
                                        sealed,
                                    },
                                    &mut out,
                                )?;
                            } else if !text.is_empty() {
                                out.extend(self.accept(Event::Thinking { index: i, text })?);
                                if let Some(signature) = signature {
                                    out.extend(self.accept(Event::Signature {
                                        index: i,
                                        signature,
                                        source,
                                    })?);
                                }
                            }
                        }
                        Part::ToolCall {
                            id,
                            name,
                            arguments,
                            signature,
                        } => {
                            out.extend(self.accept(Event::ToolStart { index: i, id, name })?);
                            out.extend(self.accept(Event::ToolArguments {
                                index: i,
                                json: arguments.to_string(),
                            })?);
                            if let Some(signature) = signature {
                                out.extend(self.accept(Event::Signature {
                                    index: i,
                                    signature,
                                    source: Protocol::Gemini,
                                })?);
                            }
                        }
                        part => {
                            self.open(i, part, &mut out)?;
                        }
                    }
                }
            }
            Event::PartEnd { index } => self.close(index, &mut out)?,
            Event::Usage(usage) => self.reply.usage = usage,
            Event::Stop(stop) => self.reply.stop = stop,
            Event::End => {
                let indices: Vec<_> = self.parts.keys().copied().collect();
                for i in indices {
                    self.close(i, &mut out)?;
                }
                self.reply.parts = self.parts.values().cloned().collect();
                let usage = reply::render_usage(self.protocol, &self.reply.usage);
                match self.protocol {
                    Protocol::Chat => {
                        let mut chunk = self.chat(
                            json!({}),
                            json!(reply::render_stop(self.protocol, &self.reply.stop)),
                        );
                        chunk["usage"] = usage;
                        out.push(frame(&chunk));
                        out.push(Bytes::from_static(b"data: [DONE]\n\n"));
                    }
                    Protocol::Anthropic => {
                        out.push(named(&json!({"type":"message_delta","delta":{"stop_reason":reply::render_stop(self.protocol,&self.reply.stop),"stop_sequence":null},"usage":usage})));
                        out.push(named(&json!({"type":"message_stop"})));
                    }
                    Protocol::Responses => {
                        let full =
                            reply::render_with_context(self.protocol, &self.reply, &self.context)
                                .map_err(|_| invalid())?;
                        let kind = if full["status"] == "incomplete" {
                            "response.incomplete"
                        } else {
                            "response.completed"
                        };
                        out.push(response_frame(
                            json!({"type":kind,"response":full}),
                            &mut self.sequence,
                        ));
                    }
                    Protocol::Gemini => {
                        let mut chunk = self.gemini(vec![]);
                        chunk["candidates"][0]["finishReason"] =
                            json!(reply::render_stop(self.protocol, &self.reply.stop));
                        chunk["usageMetadata"] = usage;
                        out.push(frame(&chunk));
                    }
                }
                self.ended = true;
            }
            Event::Start { .. } => {}
        }
        Ok(out)
    }
}

fn event_stream(
    mut source: ByteStream,
    protocol: Protocol,
) -> impl futures::Stream<Item = Result<Event, io::Error>> + Send {
    async_stream::try_stream! {
        let mut decoder=Decoder::new(protocol);let mut buffer=String::new();let mut remainder=Vec::new();let mut total=0usize;
        while let Some(chunk)=source.next().await{let chunk=chunk?;total=total.saturating_add(chunk.len());if total>MAX_REPLY{Err(invalid())?;}
            if append_utf8_safe_observed(&mut buffer,&mut remainder,&chunk){Err(invalid())?;}
            while let Some(block)=take_sse_block(&mut buffer){if block.len()>MAX_FRAME{Err(invalid())?;}let data=block.lines().filter_map(|line|strip_sse_field(line,"data")).collect::<Vec<_>>().join("\n");if data.is_empty(){continue;}for event in decoder.decode(&data)?{yield event;}if decoder.ended{return;}}
            if buffer.len()>MAX_FRAME{Err(invalid())?;}
        }
        if !remainder.is_empty()||!buffer.trim().is_empty()||matches!(protocol,Protocol::Anthropic|Protocol::Responses){Err(invalid())?;}
        let mut events=Vec::new();decoder.finish(&mut events)?;for event in events{yield event;}
    }
}
fn error_frame(protocol: Protocol, sequence: &mut u64) -> Bytes {
    let error = match protocol {
        Protocol::Anthropic => {
            json!({"type":"error","error":{"type":"api_error","message":"Invalid or incomplete upstream protocol stream"}})
        }
        Protocol::Gemini => {
            json!({"error":{"code":502,"message":"Invalid or incomplete upstream protocol stream","status":"INTERNAL"}})
        }
        Protocol::Responses => {
            json!({"type":"error","code":"protocol_stream_error","message":"Invalid or incomplete upstream protocol stream"})
        }
        Protocol::Chat => {
            json!({"error":{"message":"Invalid or incomplete upstream protocol stream","type":"upstream_error","code":"protocol_stream_error"}})
        }
    };
    if protocol == Protocol::Responses {
        response_frame(error, sequence)
    } else if protocol == Protocol::Anthropic {
        named(&error)
    } else {
        frame(&error)
    }
}
#[cfg(test)]
pub(crate) fn transcode(
    source: ByteStream,
    input_upstream: Protocol,
    client: Protocol,
) -> ByteStream {
    transcode_with_context(source, input_upstream, client, Default::default())
}
pub(crate) fn transcode_with_context(
    source: ByteStream,
    input_upstream: Protocol,
    client: Protocol,
    context: CodexToolContext,
) -> ByteStream {
    transcode_with_observer(source, input_upstream, client, context, None)
}
pub(crate) type ReplyObserver = std::sync::Arc<dyn Fn(&Reply) + Send + Sync>;
pub(crate) fn transcode_with_observer(
    source: ByteStream,
    input_upstream: Protocol,
    client: Protocol,
    context: CodexToolContext,
    observer: Option<ReplyObserver>,
) -> ByteStream {
    Box::pin(async_stream::stream! {
        let stream=event_stream(source,input_upstream);futures::pin_mut!(stream);let mut encoder=Encoder::with_context(client,context);
        let mut collector=observer.as_ref().map(|_|Encoder::new(input_upstream));
        while let Some(event)=stream.next().await{
            let result=event.and_then(|event|{
                let end=matches!(&event,Event::End);
                if let Some(collector)=collector.as_mut(){collector.accept(event.clone())?;}
                let frames=encoder.accept(event)?;
                if end{if let (Some(observer),Some(collector))=(&observer,&collector){observer(&collector.reply);}}
                Ok(frames)
            });
            match result{Ok(frames)=>{for frame in frames{yield Ok(frame);}},Err(_)=>{yield Ok(error_frame(client, &mut encoder.sequence));return;}}
        }
    })
}
pub(crate) async fn collect(source: ByteStream, protocol: Protocol) -> Result<Reply, ProxyError> {
    let stream = event_stream(source, protocol);
    futures::pin_mut!(stream);
    let mut encoder = Encoder::new(protocol);
    while let Some(event) = stream.next().await {
        encoder
            .accept(event.map_err(|_| {
                ProxyError::TransformError("Invalid or incomplete upstream protocol stream".into())
            })?)
            .map_err(|_| {
                ProxyError::TransformError("Invalid or incomplete upstream protocol stream".into())
            })?;
    }
    if !encoder.ended {
        return Err(ProxyError::TransformError(
            "Invalid or incomplete upstream protocol stream".into(),
        ));
    }
    Ok(encoder.reply)
}
#[cfg(test)]
pub(crate) fn from_reply(reply: &Reply, client: Protocol) -> Result<Bytes, ProxyError> {
    from_reply_with_context(reply, client, Default::default())
}
pub(crate) fn from_reply_with_context(
    reply: &Reply,
    client: Protocol,
    context: CodexToolContext,
) -> Result<Bytes, ProxyError> {
    let mut encoder = Encoder::with_context(client, context);
    let mut events = vec![Event::Start {
        id: reply.id.clone(),
        model: reply.model.clone(),
    }];
    for (index, part) in reply.parts.iter().enumerate() {
        events.push(Event::Part {
            index,
            part: part.clone(),
        });
        events.push(Event::PartEnd { index });
    }
    events.extend([
        Event::Usage(reply.usage.clone()),
        Event::Stop(reply.stop.clone()),
        Event::End,
    ]);
    let mut output = Vec::new();
    for event in events {
        for frame in encoder
            .accept(event)
            .map_err(|_| ProxyError::TransformError("Invalid protocol reply".into()))?
        {
            if output.len().saturating_add(frame.len()) > MAX_REPLY {
                return Err(ProxyError::TransformError(
                    "Protocol reply exceeds limit".into(),
                ));
            }
            output.extend_from_slice(&frame);
        }
    }
    Ok(Bytes::from(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_responses_sequence(bytes: &[u8]) {
        #[derive(serde::Deserialize)]
        struct ResponseEvent {
            sequence_number: u64,
        }
        let text = std::str::from_utf8(bytes).unwrap();
        let events = text
            .lines()
            .filter_map(|line| strip_sse_field(line, "data"))
            .map(|data| serde_json::from_str::<ResponseEvent>(data).unwrap())
            .collect::<Vec<_>>();
        assert!(!events.is_empty());
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.sequence_number, index as u64);
        }
    }
    fn source(s: impl Into<Bytes>) -> ByteStream {
        let bytes = s.into();
        Box::pin(futures::stream::iter(
            bytes
                .chunks(7)
                .map(Bytes::copy_from_slice)
                .map(Ok)
                .collect::<Vec<_>>(),
        ))
    }
    #[tokio::test]
    async fn fragmented_interleaved_chat_tools_cross_every_protocol() {
        let input=concat!("data: {\"id\":\"synthetic\",\"model\":\"fixture\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Synthetic thought\"}}]}\n\n","data: {\"choices\":[{\"delta\":{\"content\":\"Synthetic 你好\"}}]}\n\n","data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-first\",\"function\":{\"name\":\"first\",\"arguments\":\"{\\\"a\\\":\"}},{\"index\":1,\"id\":\"call-second\",\"function\":{\"name\":\"second\",\"arguments\":\"{\\\"b\\\":\"}}]}}]}\n\n","data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}},{\"index\":1,\"function\":{\"arguments\":\"2}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":3}}}\n\n","data: [DONE]\n\n");
        for target in [
            Protocol::Anthropic,
            Protocol::Responses,
            Protocol::Gemini,
            Protocol::Chat,
        ] {
            collect(source(input), Protocol::Chat).await.unwrap();
            let wire = transcode(source(input), Protocol::Chat, target);
            let reply = collect(wire, target)
                .await
                .unwrap_or_else(|_| panic!("interleaved Chat -> {target:?} failed"));
            assert_eq!(reply.stop, "tool");
            assert_eq!(reply.usage.input, 9);
            assert_eq!(reply.usage.cache_read, 3);
            assert!(reply
                .parts
                .iter()
                .any(|p| matches!(p,Part::Text{text,..}if text=="Synthetic 你好")));
            let calls: Vec<_> = reply
                .parts
                .iter()
                .filter_map(|p| {
                    if let Part::ToolCall {
                        id,
                        name,
                        arguments,
                        ..
                    } = p
                    {
                        Some((id.as_str(), name.as_str(), arguments.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                calls,
                vec![
                    ("call-first", "first", json!({"a":1})),
                    ("call-second", "second", json!({"b":2}))
                ]
            );
        }
    }
    #[tokio::test]
    async fn buffered_reply_streams_across_all_twelve_edges() {
        let input = Reply {
            id: "synthetic".into(),
            model: "fixture".into(),
            parts: vec![
                Part::Thinking {
                    text: "thought".into(),
                    signature: None,
                    source: Protocol::Chat,
                    sealed: None,
                },
                Part::Text {
                    text: "answer".into(),
                    signature: None,
                },
                Part::ToolCall {
                    id: "call".into(),
                    name: "inspect".into(),
                    arguments: json!({"ok":true}),
                    signature: None,
                },
            ],
            stop: "tool".into(),
            usage: Usage {
                input: 7,
                output: 3,
                cache_read: 2,
                ..Default::default()
            },
        };
        let protocols = [
            Protocol::Anthropic,
            Protocol::Responses,
            Protocol::Chat,
            Protocol::Gemini,
        ];
        for upstream in protocols {
            let s = from_reply(&input, upstream).unwrap();
            if upstream == Protocol::Responses {
                assert_responses_sequence(&s);
            }
            for target in protocols {
                if upstream == target {
                    continue;
                }
                let mut stream = transcode(source(s.clone()), upstream, target);
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    bytes.extend_from_slice(&chunk.unwrap());
                }
                if target == Protocol::Responses {
                    assert_responses_sequence(&bytes);
                }
                let actual = collect(source(bytes), target)
                    .await
                    .unwrap_or_else(|_| panic!("buffered {upstream:?} -> {target:?} failed"));
                assert_eq!(actual.stop, "tool");
                assert_eq!(actual.usage.input, 7);
                assert_eq!(actual.usage.cache_read, 2);
                assert!(actual
                    .parts
                    .iter()
                    .any(|p| matches!(p,Part::Text{text,..}if text=="answer")));
                assert!(actual
                    .parts
                    .iter()
                    .any(|p| matches!(p,Part::Thinking{text,..}if text=="thought")));
                assert!(actual
                    .parts
                    .iter()
                    .any(|p| matches!(p,Part::ToolCall{arguments,..}if arguments["ok"]==true)));
            }
        }
    }
    #[tokio::test]
    async fn incomplete_stream_emits_sanitized_error_without_success() {
        let input =
            "data: {\"id\":\"synthetic\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";
        let mut stream = transcode(source(input), Protocol::Chat, Protocol::Responses);
        let mut output = Vec::new();
        while let Some(bytes) = stream.next().await {
            output.extend_from_slice(&bytes.unwrap());
        }
        assert_responses_sequence(&output);
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("protocol_stream_error"));
        assert!(!text.contains("response.completed"));
        let mut stream = transcode(
            source("data: {synthetic-private-input}\n\n"),
            Protocol::Chat,
            Protocol::Chat,
        );
        let bytes = stream.next().await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-private-input"));
    }
    #[tokio::test]
    async fn text_is_emitted_before_the_upstream_finishes() {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let mut output = transcode(Box::pin(rx), Protocol::Chat, Protocol::Gemini);
        tx.unbounded_send(Ok(Bytes::from_static(
            b"data: {\"id\":\"synthetic\",\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
        )))
        .unwrap();
        let first = tokio::time::timeout(std::time::Duration::from_secs(1), output.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&first).contains("first"));
        drop(tx);
    }
    #[tokio::test]
    async fn responses_done_arguments_replace_a_complete_placeholder_delta() {
        let input=[
            json!({"type":"response.created","response":{"id":"synthetic","model":"fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-native","call_id":"call-native","name":"inspect","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc-native","delta":"{}"}),
            json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc-native","arguments":"{\"value\":\"synthetic\"}"}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc-native","call_id":"call-native","name":"inspect","arguments":"{\"value\":\"synthetic\"}"}}),
            json!({"type":"response.completed","response":{"id":"synthetic","model":"fixture","status":"completed","output":[],"usage":{"input_tokens":2,"output_tokens":3}}}),
        ].iter().map(|v|format!("data: {v}\n\n")).collect::<String>();
        let actual = collect(
            transcode(source(input), Protocol::Responses, Protocol::Chat),
            Protocol::Chat,
        )
        .await
        .unwrap();
        assert!(actual.parts.iter().any(|p|matches!(p,Part::ToolCall{id,arguments,..}if id=="call-native"&&arguments["value"]=="synthetic")));
    }
    #[tokio::test]
    async fn responses_stream_restores_namespace_and_custom_tool_identity() {
        let context =
            crate::proxy::providers::transform_codex_chat::build_codex_tool_context_from_request(
                &json!({"tools":[{"type":"namespace","name":"files","tools":[{"type":"function","name":"read","parameters":{"type":"object","properties":{}}}]},{"type":"custom","name":"apply_patch","format":{"type":"text"}}]}),
            );
        let input = Reply {
            id: "synthetic".into(),
            model: "fixture".into(),
            parts: vec![
                Part::ToolCall {
                    id: "call-read".into(),
                    name: "files__read".into(),
                    arguments: json!({"path":"synthetic"}),
                    signature: None,
                },
                Part::ToolCall {
                    id: "call-patch".into(),
                    name: "apply_patch".into(),
                    arguments: json!({"input":"synthetic line one\nline two"}),
                    signature: None,
                },
            ],
            stop: "tool".into(),
            usage: Usage::default(),
        };
        let upstream = from_reply(&input, Protocol::Chat).unwrap();
        let mut stream = transcode_with_context(
            source(upstream),
            Protocol::Chat,
            Protocol::Responses,
            context,
        );
        let mut output = String::new();
        while let Some(bytes) = stream.next().await {
            output.push_str(std::str::from_utf8(&bytes.unwrap()).unwrap());
        }
        let events: Vec<Value> = output
            .lines()
            .filter_map(|l| strip_sse_field(l, "data"))
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        let final_reply = &events
            .iter()
            .find(|v| v["type"] == "response.completed")
            .unwrap()["response"];
        assert_eq!(final_reply["output"][0]["namespace"], "files");
        assert_eq!(final_reply["output"][0]["name"], "read");
        assert_eq!(final_reply["output"][1]["type"], "custom_tool_call");
        assert_eq!(
            final_reply["output"][1]["input"],
            "synthetic line one\nline two"
        );
        assert!(events
            .iter()
            .any(|v| v["type"] == "response.custom_tool_call_input.done"
                && v["input"] == "synthetic line one\nline two"));
        assert!(!events.iter().any(
            |v| v["item_id"] == "fc_1" && v["type"] == "response.function_call_arguments.delta"
        ));
    }
    #[tokio::test]
    async fn native_anthropic_signed_and_sealed_reasoning_stays_in_its_api() {
        let events = [
            json!({"type":"message_start","message":{"id":"synthetic","model":"fixture","usage":{"input_tokens":7,"cache_read_input_tokens":2}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"synthetic thought"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic-signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"synthetic-sealed"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":"Synthetic 你好"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"call-native","name":"inspect","input":{"value":1}}}),
            json!({"type":"content_block_stop","index":3}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ];
        let wire = events
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        for target in [Protocol::Chat, Protocol::Responses, Protocol::Gemini] {
            let mut output = transcode(source(wire.clone()), Protocol::Anthropic, target);
            let mut bytes = Vec::new();
            while let Some(chunk) = output.next().await {
                bytes.extend_from_slice(&chunk.unwrap());
            }
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("synthetic-signature"));
            assert!(!text.contains("synthetic-sealed"));
            let result = collect(source(bytes), target).await.unwrap();
            assert_eq!(result.stop, "tool");
            assert_eq!(result.usage.input, 7);
            assert_eq!(result.usage.cache_read, 2);
            assert!(result.parts.iter().any(|p|matches!(p,Part::ToolCall{id,arguments,..}if id=="call-native"&&arguments["value"]==1)));
        }
        let truncated = events[..events.len() - 1]
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        assert!(collect(source(truncated), Protocol::Anthropic)
            .await
            .is_err());
    }
    #[tokio::test]
    async fn multiple_candidates_and_unknown_semantic_blocks_never_complete_silently() {
        for (protocol, input) in [
            (
                Protocol::Chat,
                json!({"choices":[{"delta":{"content":"a"}},{"delta":{"content":"b"}}]}),
            ),
            (
                Protocol::Gemini,
                json!({"candidates":[{"content":{"parts":[{"text":"a"}]},"finishReason":"STOP"},{"content":{"parts":[{"text":"b"}]},"finishReason":"STOP"}]}),
            ),
            (
                Protocol::Anthropic,
                json!({"type":"content_block_start","index":0,"content_block":{"type":"unknown_semantic","text":"synthetic-sensitive"}}),
            ),
        ] {
            let mut output = transcode(
                source(format!("data: {input}\n\n")),
                protocol,
                Protocol::Responses,
            );
            let mut text = String::new();
            while let Some(chunk) = output.next().await {
                text.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
            }
            assert!(text.contains("protocol_stream_error"));
            assert!(!text.contains("response.completed"));
            assert!(!text.contains("synthetic-sensitive"));
        }
    }
    #[tokio::test]
    async fn gemini_combined_server_search_stream_completes_without_client_tool_execution() {
        let input = reply::gemini_search_fixture();
        let mut output = transcode(
            source(format!("data: {input}\n\n")),
            Protocol::Gemini,
            Protocol::Responses,
        );
        let mut text = String::new();
        while let Some(chunk) = output.next().await {
            text.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }
        assert!(text.contains("response.completed"));
        assert!(text.contains("web_search_call"));
        assert!(!text.contains("function_call"));
        assert!(!text.contains("protocol_stream_error"));
        assert!(!text.contains("synthetic-call-signature"));
    }

    #[tokio::test]
    async fn dropping_client_cancels_the_upstream_and_frames_are_bounded() {
        struct DropStream(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl futures::Stream for DropStream {
            type Item = Result<Bytes, io::Error>;
            fn poll_next(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                std::task::Poll::Pending
            }
        }
        impl Drop for DropStream {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut client = transcode(
            Box::pin(DropStream(dropped.clone())),
            Protocol::Chat,
            Protocol::Responses,
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(5), client.next())
                .await
                .is_err()
        );
        drop(client);
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        let mut output = transcode(
            Box::pin(futures::stream::once(async {
                Ok(Bytes::from(format!("data: {}", "x".repeat(MAX_FRAME + 1))))
            })),
            Protocol::Chat,
            Protocol::Responses,
        );
        let text = String::from_utf8(output.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(text.contains("protocol_stream_error"));
    }
    #[tokio::test]
    async fn observer_retains_gemini_tool_signatures_only_after_complete_upstream() {
        let saved = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = saved.clone();
        let observer: ReplyObserver =
            std::sync::Arc::new(move |reply| captured.lock().unwrap().push(reply.clone()));
        let reply = json!({"responseId":"synthetic","modelVersion":"fixture","candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"call-signed","name":"inspect","args":{"synthetic":true}},"thoughtSignature":"synthetic-signature"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":3}});
        let mut output = transcode_with_observer(
            source(format!("data: {reply}\n\n")),
            Protocol::Gemini,
            Protocol::Chat,
            Default::default(),
            Some(observer.clone()),
        );
        while let Some(chunk) = output.next().await {
            assert!(!String::from_utf8_lossy(&chunk.unwrap()).contains("synthetic-signature"));
        }
        assert_eq!(saved.lock().unwrap().len(), 1);
        assert!(saved.lock().unwrap()[0].parts.iter().any(|p|matches!(p,Part::ToolCall{id,signature:Some(signature),arguments,..}if id=="call-signed"&&signature=="synthetic-signature"&&arguments["synthetic"]==true)));
        let incomplete = json!({"responseId":"synthetic","candidates":[{"content":{"parts":[{"text":"not finished"}]}}]});
        let mut output = transcode_with_observer(
            source(format!("data: {incomplete}\n\n")),
            Protocol::Gemini,
            Protocol::Chat,
            Default::default(),
            Some(observer.clone()),
        );
        while output.next().await.is_some() {}
        assert_eq!(saved.lock().unwrap().len(), 1);
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let mut output = transcode_with_observer(
            Box::pin(rx),
            Protocol::Gemini,
            Protocol::Chat,
            Default::default(),
            Some(observer),
        );
        tx.unbounded_send(Ok(Bytes::from(format!("data: {incomplete}\n\n"))))
            .unwrap();
        assert!(output.next().await.is_some());
        drop(output);
        drop(tx);
        assert_eq!(saved.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn missing_gemini_call_ids_match_between_client_and_observer() {
        let saved = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = saved.clone();
        let observer: ReplyObserver =
            std::sync::Arc::new(move |reply| captured.lock().unwrap().push(reply.clone()));
        let input = json!({"responseId":"synthetic","candidates":[{"content":{"parts":[{"functionCall":{"name":"first","args":{"value":1}}},{"functionCall":{"name":"second","args":{"value":2}}}]},"finishReason":"STOP"}]});
        let output = transcode_with_observer(
            source(format!("data: {input}\n\n")),
            Protocol::Gemini,
            Protocol::Chat,
            Default::default(),
            Some(observer),
        );
        let client = collect(output, Protocol::Chat).await.unwrap();
        let ids = |r: &Reply| {
            r.parts
                .iter()
                .filter_map(|p| {
                    if let Part::ToolCall { id, .. } = p {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let client_ids = ids(&client);
        assert_eq!(client_ids.len(), 2);
        assert!(client_ids.iter().all(|s| !s.is_empty()));
        assert_ne!(client_ids[0], client_ids[1]);
        assert_eq!(client_ids, ids(&saved.lock().unwrap()[0]));
    }
    #[tokio::test]
    async fn native_responses_custom_input_deltas_preserve_exact_freeform_input() {
        let events = [
            json!({"type":"response.created","response":{"id":"synthetic","model":"fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"custom_tool_call","id":"native-custom","call_id":"call-native","name":"apply_patch","input":""}}),
            json!({"type":"response.custom_tool_call_input.delta","output_index":0,"item_id":"native-custom","delta":"synthetic \"quoted\""}),
            json!({"type":"response.custom_tool_call_input.delta","output_index":0,"item_id":"native-custom","delta":"\nsecond 你好"}),
            json!({"type":"response.custom_tool_call_input.done","output_index":0,"item_id":"native-custom","input":"synthetic \"quoted\"\nsecond 你好"}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"custom_tool_call","id":"native-custom","call_id":"call-native","name":"apply_patch","input":"synthetic \"quoted\"\nsecond 你好"}}),
            json!({"type":"response.completed","response":{"id":"synthetic","model":"fixture","status":"completed","output":[],"usage":{}}}),
        ];
        let wire = events
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        let result = collect(source(wire.clone()), Protocol::Responses)
            .await
            .unwrap();
        assert!(result.parts.iter().any(|p|matches!(p,Part::CustomToolCall{id,input,..}if id=="call-native"&&input=="synthetic \"quoted\"\nsecond 你好")));
        for target in [Protocol::Chat, Protocol::Gemini, Protocol::Anthropic] {
            let result = collect(
                transcode(source(wire.clone()), Protocol::Responses, target),
                target,
            )
            .await
            .unwrap();
            assert!(result.parts.iter().any(|p|matches!(p,Part::ToolCall{id,arguments,..}if id=="call-native"&&arguments["input"]=="synthetic \"quoted\"\nsecond 你好")));
        }
    }
    #[tokio::test]
    async fn native_chat_refusal_deltas_keep_text_and_filter_finish() {
        let input = [
            json!({"id":"synthetic","choices":[{"delta":{"refusal":"Synthetic "}}]}),
            json!({"choices":[{"delta":{"refusal":"refusal"},"finish_reason":"stop"}]}),
        ]
        .iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        for target in [Protocol::Anthropic, Protocol::Responses, Protocol::Gemini] {
            let actual = collect(
                transcode(source(input.clone()), Protocol::Chat, target),
                target,
            )
            .await
            .unwrap();
            assert_eq!(actual.stop, "filter");
            assert!(actual
                .parts
                .iter()
                .any(|p| matches!(p,Part::Text{text,..}if text=="Synthetic refusal")));
        }
    }
}
