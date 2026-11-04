//! Protocol-neutral conversation and reply representation. Wire adapters own
//! protocol spelling; routing, inspection, credentials and transport stay outside.
//! Behavior reference: yetone/magpie internal/gateway/ir.go (MIT); see NOTICE.
use super::Protocol;
use serde_json::Value;

pub(crate) mod build;
pub(crate) mod continuity;
pub(crate) mod parse;
pub(crate) mod reply;
pub(crate) mod stream;

#[cfg(test)]
mod integration_tests;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Part {
    Text {
        text: String,
        signature: Option<String>,
    },
    Image {
        media_type: String,
        data: Option<String>,
        url: Option<String>,
        detail: Option<String>,
    },
    File {
        media_type: String,
        data: Option<String>,
        url: Option<String>,
        id: Option<String>,
        name: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
        signature: Option<String>,
    },
    NamespacedToolCall {
        id: String,
        namespace: String,
        name: String,
        arguments: Value,
        signature: Option<String>,
    },
    CustomToolCall {
        id: String,
        name: String,
        input: String,
        namespace: Option<String>,
    },
    CustomToolResult {
        call_id: String,
        parts: Vec<Part>,
    },
    ToolResult {
        call_id: String,
        name: Option<String>,
        parts: Vec<Part>,
        is_error: bool,
    },
    Thinking {
        text: String,
        signature: Option<String>,
        source: Protocol,
        sealed: Option<Value>,
    },
    ToolReference {
        name: String,
    },
    Search {
        query: String,
        hits: Vec<Value>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tool {
    pub kind: String,
    pub native: Option<Value>,
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub strict: Option<bool>,
    pub deferred: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
    Namespaced {
        namespace: String,
        name: String,
    },
    Custom {
        name: String,
        namespace: Option<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Reasoning {
    pub effort: Option<String>,
    pub budget: Option<i64>,
    pub enabled: Option<bool>,
    pub summary: Option<String>,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Format {
    pub kind: String,
    pub name: Option<String>,
    pub schema: Option<Value>,
    pub strict: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeOptions {
    pub metadata: Option<Value>,
    pub client_metadata: Option<Value>,
    pub include: Option<Value>,
    pub text: Option<Value>,
    pub image_config: Option<Value>,
    pub safety_settings: Option<Value>,
    pub prompt_cache_key: Option<String>,
    pub service_tier: Option<String>,
    pub user: Option<String>,
    pub access_programs: Option<Value>,
    pub cache_retention: Option<String>,
    pub safeguards: Option<Value>,
    pub top_k: Option<Value>,
    pub modalities: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    pub model: String,
    pub system: Vec<Part>,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub tool_choice: Option<ToolChoice>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<Value>,
    pub top_p: Option<Value>,
    pub stop: Vec<String>,
    pub stream: bool,
    pub reasoning: Reasoning,
    pub parallel: Option<bool>,
    pub format: Option<Format>,
    pub native: NativeOptions,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reply {
    pub id: String,
    pub model: String,
    pub parts: Vec<Part>,
    pub stop: String,
    pub usage: Usage,
}

/// Incremental, ordered semantic events; adapters must not buffer a full reply
/// before emitting text. Index identifies interleaved content/tool blocks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Start {
        id: String,
        model: String,
    },
    Text {
        index: usize,
        text: String,
    },
    Thinking {
        index: usize,
        text: String,
    },
    Signature {
        index: usize,
        signature: String,
        source: Protocol,
    },
    ToolStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolArguments {
        index: usize,
        json: String,
    },
    Part {
        index: usize,
        part: Part,
    },
    PartEnd {
        index: usize,
    },
    Usage(Usage),
    Stop(String),
    End,
}
