//! Bounded side-channel usage parsing; accounting never stalls forwarding.
use super::usage::{logger::UsageLogger, parser::TokenUsage};
use crate::database::Database;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
#[derive(Default)]
pub(crate) struct Collector {
    buffer: String,
    remainder: Vec<u8>,
    pub usage: TokenUsage,
}
impl Collector {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.buffer.len().saturating_add(bytes.len()) > 1024 * 1024 {
            self.buffer.clear();
            self.remainder.clear();
            return;
        }
        super::sse::append_utf8_safe(&mut self.buffer, &mut self.remainder, bytes);
        while let Some(block) = super::sse::take_sse_block(&mut self.buffer) {
            let data = block
                .lines()
                .filter_map(|l| super::sse::strip_sse_field(l, "data"))
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
        let anthropic =
            value["type"] == "message" || value["usage"].get("cache_read_input_tokens").is_some();
        let mut parsed = if anthropic {
            TokenUsage::from_claude_response(value)
        } else {
            TokenUsage::from_codex_response_auto(value)
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
    pub app_type: String,
    pub db: Arc<Database>,
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
        log::warn!("[Qoder] stage=usage_queue category=locked");
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
                            let id = job
                                .usage
                                .dedup_request_id(Some((&job.app_type, &job.provider_id)));
                            let (multiplier, source) = futures::executor::block_on(
                                logger.resolve_pricing_config(&job.provider_id, &job.app_type),
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
                                    job.app_type.clone(),
                                    model.clone(),
                                    job.request_model,
                                    pricing_model,
                                    job.usage,
                                    multiplier,
                                    job.latency_ms,
                                    None,
                                    job.status,
                                    job.session_id,
                                    Some(job.app_type),
                                    job.streaming,
                                )
                                .is_err()
                            {
                                log::warn!("[Qoder] stage=usage_write category=failed");
                            }
                        }
                    })
                    .await;
                    if result.is_err() {
                        log::warn!("[Qoder] stage=usage_worker category=failed");
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
        log::warn!("[Qoder] stage=usage_queue category=unavailable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qoder_usage_unwraps_official_sse_and_preserves_split_frames() {
        let mut c = Collector::default();
        let frame="data: {\"statusCodeValue\":200,\"body\":\"{\\\"model\\\":\\\"qwen-test\\\",\\\"usage\\\":{\\\"prompt_tokens\\\":12,\\\"completion_tokens\\\":7}}\"}\n\n";
        for bytes in frame.as_bytes().chunks(3) {
            c.push(bytes);
        }
        assert_eq!(c.usage.input_tokens, 12);
        assert_eq!(c.usage.output_tokens, 7);
        assert_eq!(c.usage.model.as_deref(), Some("qwen-test"));
        c.push(b"data: [DONE]\n\n");
        assert_eq!(c.usage.input_tokens, 12);
    }
    #[test]
    fn qoder_mixed_protocol_usage_keeps_fresh_input_semantics() {
        let mut openai = Collector::default();
        openai.push(br#"{"model":"synthetic","usage":{"prompt_tokens":100,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":30}}}"#);
        openai.finish();
        assert_eq!(openai.usage.input_tokens, 70);
        assert_eq!(openai.usage.cache_read_tokens, 30);
        let mut anthropic = Collector::default();
        anthropic.push(br#"{"type":"message","model":"synthetic","usage":{"input_tokens":70,"output_tokens":5,"cache_read_input_tokens":30}}"#);
        anthropic.finish();
        assert_eq!(anthropic.usage.input_tokens, 70);
        assert_eq!(anthropic.usage.cache_read_tokens, 30);
    }
}
