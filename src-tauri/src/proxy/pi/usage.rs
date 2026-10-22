//! Bounded side-channel usage parsing; accounting never stalls forwarding.
use super::super::usage::{logger::UsageLogger, parser::TokenUsage};
use crate::database::Database;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
pub(crate) struct Collector {
    protocol: crate::services::pi_routing::catalog::Protocol,
    pub observed: bool,
    buffer: String,
    remainder: Vec<u8>,
    pub usage: TokenUsage,
}
impl Collector {
    pub fn new(protocol: crate::services::pi_routing::catalog::Protocol) -> Self {
        Self {
            protocol,
            observed: false,
            buffer: String::new(),
            remainder: Vec::new(),
            usage: Default::default(),
        }
    }
    pub fn push(&mut self, bytes: &[u8]) {
        if self.buffer.len().saturating_add(bytes.len()) > 1024 * 1024 {
            self.buffer.clear();
            self.remainder.clear();
            return;
        }
        super::super::sse::append_utf8_safe(&mut self.buffer, &mut self.remainder, bytes);
        while let Some(block) = super::super::sse::take_sse_block(&mut self.buffer) {
            let data = block
                .lines()
                .filter_map(|l| super::super::sse::strip_sse_field(l, "data"))
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                self.observe(value);
            }
        }
    }
    pub fn finish(&mut self) {
        if let Ok(value) = serde_json::from_str::<Value>(&self.buffer) {
            self.observe(value);
        }
        self.buffer.clear();
    }
    fn observe(&mut self, mut value: Value) {
        if let Some(body) = value.get("body").and_then(Value::as_str) {
            if let Ok(decoded) = serde_json::from_str(body) {
                value = decoded;
            }
        }
        let value = value
            .get("response")
            .or_else(|| value.get("message"))
            .unwrap_or(&value);
        use crate::services::pi_routing::catalog::Protocol;
        let anthropic = self.protocol == Protocol::Anthropic;
        let mut parsed = match self.protocol {
            Protocol::Anthropic => TokenUsage::from_claude_response(value),
            Protocol::Gemini => TokenUsage::from_gemini_response(value),
            _ => TokenUsage::from_codex_response_auto(value),
        };
        if !anthropic {
            if let Some(usage) = &mut parsed {
                usage.input_tokens = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
            }
        }
        if parsed.is_none() && value["usage"]["output_tokens"].is_u64() {
            parsed = TokenUsage::from_claude_response(
                &json!({"usage":{"input_tokens":0,"output_tokens":value["usage"]["output_tokens"]}}),
            );
        }
        if let Some(parsed) = parsed {
            self.observed = true;
            self.usage.input_tokens = self.usage.input_tokens.max(parsed.input_tokens);
            self.usage.output_tokens = self.usage.output_tokens.max(parsed.output_tokens);
            self.usage.cache_read_tokens =
                self.usage.cache_read_tokens.max(parsed.cache_read_tokens);
            self.usage.cache_creation_tokens = self
                .usage
                .cache_creation_tokens
                .max(parsed.cache_creation_tokens);
            if parsed.model.is_some() {
                self.usage.model = parsed.model;
            }
            if parsed.message_id.is_some() {
                self.usage.message_id = parsed.message_id;
            }
        }
    }
}
pub(crate) struct Job {
    pub db: Arc<Database>,
    pub request_id: String,
    pub provider_id: String,
    pub request_model: String,
    pub session_id: Option<String>,
    pub usage: TokenUsage,
    pub latency_ms: u64,
    pub status: u16,
    pub streaming: bool,
}
static SENDER: OnceLock<Mutex<Option<tokio::sync::mpsc::Sender<Job>>>> = OnceLock::new();
pub(crate) fn emit(job: Job) {
    let Ok(mut slot) = SENDER.get_or_init(|| Mutex::new(None)).lock() else {
        log::warn!("[Pi] stage=usage_queue category=locked");
        return;
    };
    if slot.as_ref().is_none_or(|tx| tx.is_closed()) {
        *slot = Some({
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Job>(1000);
            tokio::spawn(async move {
                let mut batch = Vec::with_capacity(50);
                loop {
                    if rx.recv_many(&mut batch, 50).await == 0 {
                        break;
                    }
                    let jobs = std::mem::take(&mut batch);
                    let result = tokio::task::spawn_blocking(move || {
                        for job in jobs {
                            let logger = UsageLogger::new(&job.db);
                            let model = job
                                .usage
                                .model
                                .clone()
                                .unwrap_or_else(|| job.request_model.clone());
                            let id = job.request_id;
                            let (multiplier, source) = futures::executor::block_on(
                                logger.resolve_pricing_config(&job.provider_id, "pi"),
                            );
                            let pricing_model = if source == crate::database::PRICING_SOURCE_REQUEST
                            {
                                job.request_model.clone()
                            } else {
                                model.clone()
                            };
                            if logger
                                .log_with_calculation(
                                    id,
                                    job.provider_id,
                                    "pi".into(),
                                    model.clone(),
                                    job.request_model,
                                    pricing_model,
                                    job.usage,
                                    multiplier,
                                    job.latency_ms,
                                    None,
                                    job.status,
                                    job.session_id,
                                    Some("pi".into()),
                                    job.streaming,
                                )
                                .is_err()
                            {
                                log::warn!("[Pi] stage=usage_write category=failed");
                            }
                        }
                    })
                    .await;
                    if result.is_err() {
                        log::warn!("[Pi] stage=usage_worker category=failed");
                    }
                }
            });
            tx
        });
    }
    if slot
        .as_ref()
        .is_none_or(|sender| sender.try_send(job).is_err())
    {
        log::warn!("[Pi] stage=usage_queue category=unavailable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::pi_routing::catalog::Protocol;
    #[test]
    fn pi_usage_keeps_cache_and_fresh_input_separate_across_protocols() {
        for (protocol, frame) in [
            (
                Protocol::Chat,
                r#"{"model":"chat","usage":{"prompt_tokens":100,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":30}}}"#,
            ),
            (
                Protocol::Responses,
                r#"{"type":"response.completed","response":{"model":"responses","usage":{"input_tokens":100,"output_tokens":7,"input_tokens_details":{"cached_tokens":30}}}}"#,
            ),
            (
                Protocol::Gemini,
                r#"{"usageMetadata":{"promptTokenCount":100,"totalTokenCount":107,"cachedContentTokenCount":30}}"#,
            ),
            (
                Protocol::Anthropic,
                r#"{"type":"message","usage":{"input_tokens":70,"output_tokens":7,"cache_read_input_tokens":30}}"#,
            ),
        ] {
            let mut c = Collector::new(protocol);
            let sse = format!("data: {frame}\n\n");
            for bytes in sse.as_bytes().chunks(3) {
                c.push(bytes);
            }
            c.finish();
            assert!(c.observed);
            assert_eq!(c.usage.input_tokens, 70);
            assert_eq!(c.usage.output_tokens, 7);
            assert_eq!(c.usage.cache_read_tokens, 30);
        }
        let mut c = Collector::new(Protocol::Anthropic);
        c.push(b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":70,\"output_tokens\":0}}}\n\n");
        c.push(b"data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n");
        assert_eq!(c.usage.input_tokens, 70);
        assert_eq!(c.usage.output_tokens, 7);
        let mut missing = Collector::new(Protocol::Chat);
        missing.push(b"data: [DONE]\n\n");
        missing.finish();
        assert!(
            !missing.observed,
            "missing usage must not become a zero-usage record"
        );
    }
}
