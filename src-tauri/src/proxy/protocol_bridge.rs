//! Cross-protocol execution uses shared semantic requests, replies and events.
//! Application attribution and provider selection remain outside wire adaptation.
use super::{
    hyper_client::ProxyResponse,
    providers::{self, AuthInfo, AuthStrategy, ClaudeAdapter, ProviderAdapter},
    ProxyError,
};
use crate::{app_config::AppType, provider::Provider};
use bytes::Bytes;
use futures::Stream;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::{pin::Pin, time::Duration};

pub(super) mod grok;
pub(crate) mod ir;
type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Protocol {
    Anthropic,
    Chat,
    Responses,
    Gemini,
}
impl Protocol {
    pub(crate) fn from_format(format: &str) -> Option<Self> {
        match format {
            "anthropic" | "anthropic_messages" => Some(Self::Anthropic),
            "openai_chat" | "chat_completions" => Some(Self::Chat),
            "openai_responses" | "responses" => Some(Self::Responses),
            "gemini_native" => Some(Self::Gemini),
            _ => None,
        }
    }
    pub(crate) fn format(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Chat => "openai_chat",
            Self::Responses => "openai_responses",
            Self::Gemini => "gemini_native",
        }
    }
}

/// Public SDK options only. Query credentials and vendor-specific options are not passthrough.
pub(crate) fn supported_query(path: &str, query: Option<&str>) -> bool {
    query.is_none_or(|query| {
        query.is_empty()
            || (matches!(path, "/v1/messages" | "/v1/messages/count_tokens")
                && query == "beta=true")
            || (path.starts_with("/v1beta/models/")
                && path.ends_with(":streamGenerateContent")
                && query == "alt=sse")
    })
}

/// Preserve Claude Code's SDK identity for Anthropic, never its credentials or arbitrary headers.
pub(crate) fn copy_caller_headers(
    protocol: Protocol,
    source: &http::HeaderMap,
    destination: &mut http::HeaderMap,
) {
    let claude_code = protocol == Protocol::Anthropic
        && source
            .get(http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("claude-cli/"));
    for name in source.keys() {
        let name_str = name.as_str();
        if matches!(
            name_str,
            "openai-beta" | "x-client-request-id" | "session-id" | "thread-id"
        ) || (protocol == Protocol::Anthropic
            && matches!(name_str, "anthropic-version" | "anthropic-beta"))
            || (claude_code
                && (matches!(name_str, "user-agent" | "x-app")
                    || name_str.starts_with("x-stainless-")
                    || name_str.starts_with("x-claude-code-")))
        {
            destination.remove(name);
            for value in source.get_all(name) {
                destination.append(name.clone(), value.clone());
            }
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Bridge {
    pub input: Protocol,
    pub output: Protocol,
}
pub(super) struct BridgeContext {
    pub tool_context: providers::transform_codex_chat::CodexToolContext,
    pub history: std::sync::Arc<providers::codex_chat_history::CodexChatHistoryStore>,
    pub shadow: std::sync::Arc<providers::gemini_shadow::GeminiShadowStore>,
    pub provider_id: String,
    pub session_id: Option<String>,
    pub observation: Option<super::tool_observation::channel::ResponseMetadata>,
    pub reply_observer: Option<ir::stream::ReplyObserver>,
}
impl Bridge {
    /// Plan only cross-protocol work; native requests must keep their own wire path.
    pub fn plan(input: Protocol, output: Protocol) -> Option<Self> {
        (input != output).then_some(Self { input, output })
    }

    pub fn endpoint(self, endpoint: &str) -> String {
        let query = endpoint
            .split_once('?')
            .map(|(_, query)| query)
            .unwrap_or("");
        let query = query
            .split('&')
            .filter(|pair| {
                !url::form_urlencoded::parse(pair.as_bytes()).any(|(name, _)| {
                    (self.input == Protocol::Gemini && (name == "alt" || name == "key"))
                        || (self.input == Protocol::Anthropic
                            && self.output != Protocol::Anthropic
                            && name == "beta")
                })
            })
            .filter(|pair| !pair.is_empty())
            .collect::<Vec<_>>()
            .join("&");
        if query.is_empty() {
            "/v1/messages".into()
        } else {
            format!("/v1/messages?{query}")
        }
    }
    pub fn route(app: &AppType, endpoint: &str, provider: &Provider) -> Option<Self> {
        let path = endpoint.split('?').next().unwrap_or(endpoint);
        let input = if path.ends_with("/chat/completions") {
            Protocol::Chat
        } else if path.ends_with("/responses") {
            Protocol::Responses
        } else if path.ends_with("/messages") {
            Protocol::Anthropic
        } else if *app == AppType::Gemini
            && (path.ends_with(":generateContent") || path.ends_with(":streamGenerateContent"))
        {
            Protocol::Gemini
        } else {
            return None;
        };
        let default = if *app == AppType::Gemini {
            Protocol::Gemini
        } else if matches!(app, AppType::Claude | AppType::ClaudeDesktop) {
            Protocol::from_format(providers::get_claude_api_format(provider))
                .unwrap_or(Protocol::Anthropic)
        } else if providers::should_convert_codex_responses_to_chat(provider, "/v1/responses") {
            Protocol::Chat
        } else if providers::should_convert_codex_responses_to_anthropic(provider, "/v1/responses")
        {
            Protocol::Anthropic
        } else {
            // Legacy clients without an upstream format keep their native wire path.
            input
        };
        let output = provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .or_else(|| provider.settings_config["api_format"].as_str())
            .and_then(Protocol::from_format)
            .unwrap_or(default);
        let output = if provider.is_codex_oauth() || provider.is_xai_oauth() {
            Protocol::Responses
        } else {
            output
        };
        Self::plan(input, output)
    }
    pub async fn response(
        self,
        response: ProxyResponse,
        streaming: bool,
        timeout: Duration,
        context: BridgeContext,
    ) -> Result<ProxyResponse, ProxyError> {
        if !response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let mut headers = response.headers().clone();
        super::response_processor::strip_entity_headers_for_rebuilt_body(&mut headers);
        super::response_processor::strip_hop_by_hop_response_headers(&mut headers);
        let reply = if response.is_sse() {
            let source: ByteStream = match context.observation.clone() {
                Some(mut metadata) => {
                    metadata.transport = super::tool_observation::channel::Transport::Sse;
                    Box::pin(super::tool_observation::mount::observe_stream(
                        response.bytes_stream(),
                        metadata,
                    ))
                }
                None => Box::pin(response.bytes_stream()),
            };
            if streaming {
                let observer = if context.reply_observer.is_some() {
                    context.reply_observer.clone()
                } else if self.output == Protocol::Gemini {
                    let shadow = context.shadow.clone();
                    let provider = context.provider_id.clone();
                    let session = context.session_id.clone();
                    Some(std::sync::Arc::new(move |reply: &ir::Reply| {
                        ir::continuity::record(reply, &shadow, &provider, session.as_deref());
                    }) as ir::stream::ReplyObserver)
                } else {
                    None
                };
                let stream = match observer {
                    Some(observer) => ir::stream::transcode_with_observer(
                        source,
                        self.output,
                        self.input,
                        context.tool_context,
                        Some(observer),
                    ),
                    None => ir::stream::transcode_with_context(
                        source,
                        self.output,
                        self.input,
                        context.tool_context,
                    ),
                };
                headers.insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/event-stream"),
                );
                return Ok(ProxyResponse::Streamed {
                    status,
                    headers,
                    stream: self.record_stream(stream, context.history),
                });
            }
            let collect = ir::stream::collect(source, self.output);
            if timeout.is_zero() {
                collect.await?
            } else {
                tokio::time::timeout(timeout, collect)
                    .await
                    .map_err(|_| ProxyError::Timeout("Upstream response timed out".into()))??
            }
        } else {
            let (_, _, bytes) =
                super::response_processor::read_decoded_body(response, "ProtocolBridge", timeout)
                    .await?;
            if let Some(metadata) = context.observation.clone() {
                super::tool_observation::mount::observe_non_streaming(&bytes, metadata);
            }
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid upstream JSON"))?;
            ir::reply::parse(self.output, &value)?
        };
        if self.output == Protocol::Gemini {
            ir::continuity::record(
                &reply,
                &context.shadow,
                &context.provider_id,
                context.session_id.as_deref(),
            );
        }
        if streaming {
            let bytes =
                ir::stream::from_reply_with_context(&reply, self.input, context.tool_context)?;
            if let Some(observer) = &context.reply_observer {
                observer(&reply);
            }
            let stream: ByteStream = Box::pin(futures::stream::once(async move { Ok(bytes) }));
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/event-stream"),
            );
            return Ok(ProxyResponse::Streamed {
                status,
                headers,
                stream: self.record_stream(stream, context.history),
            });
        }
        let encoded = ir::reply::render_with_context(self.input, &reply, &context.tool_context)?;
        if let Some(observer) = &context.reply_observer {
            observer(&reply);
        }
        if self.input == Protocol::Responses {
            context.history.record_response(&encoded).await;
        }
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        Ok(ProxyResponse::buffered(
            status,
            headers,
            Bytes::from(encoded.to_string()),
        ))
    }

    fn record_stream(
        self,
        source: ByteStream,
        history: std::sync::Arc<providers::codex_chat_history::CodexChatHistoryStore>,
    ) -> ByteStream {
        if self.input == Protocol::Responses {
            Box::pin(providers::codex_chat_history::record_responses_sse_stream(
                source, history,
            ))
        } else {
            source
        }
    }
    pub fn provider(self, provider: &Provider) -> Provider {
        let mut provider = provider.clone();
        provider
            .meta
            .get_or_insert_with(Default::default)
            .api_format = Some(self.output.format().into());
        provider
    }
}

pub(super) struct BridgeAdapter<'a> {
    pub original: &'a dyn ProviderAdapter,
    pub output: Protocol,
}
impl ProviderAdapter for BridgeAdapter<'_> {
    fn name(&self) -> &'static str {
        "Claude"
    }
    fn extract_base_url(&self, p: &Provider) -> Result<String, ProxyError> {
        self.original.extract_base_url(p)
    }
    fn extract_auth(&self, p: &Provider) -> Option<AuthInfo> {
        self.original.extract_auth(p).map(|mut auth| {
            auth.strategy = match self.output {
                Protocol::Anthropic => {
                    if matches!(
                        auth.strategy,
                        AuthStrategy::ClaudeAuth | AuthStrategy::Bearer
                    ) {
                        auth.strategy
                    } else {
                        AuthStrategy::Anthropic
                    }
                }
                Protocol::Gemini => {
                    if auth.strategy == AuthStrategy::GoogleOAuth {
                        AuthStrategy::GoogleOAuth
                    } else {
                        AuthStrategy::Google
                    }
                }
                _ => AuthStrategy::Bearer,
            };
            auth
        })
    }
    fn build_url(&self, base: &str, endpoint: &str) -> String {
        ClaudeAdapter::new().build_url(base, endpoint)
    }
    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        ClaudeAdapter::new().get_auth_headers(auth)
    }
}
fn invalid(message: &str) -> ProxyError {
    ProxyError::InvalidRequest(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_plan_depends_only_on_protocols_not_application_or_model() {
        for input in [
            Protocol::Anthropic,
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Gemini,
        ] {
            for output in [
                Protocol::Anthropic,
                Protocol::Chat,
                Protocol::Responses,
                Protocol::Gemini,
            ] {
                let bridge = Bridge::plan(input, output);
                assert_eq!(bridge.is_some(), input != output);
                if let Some(bridge) = bridge {
                    assert_eq!(bridge.input, input);
                    assert_eq!(bridge.output, output);
                }
                assert_eq!(Protocol::from_format(output.format()), Some(output));
            }
        }
    }
    #[test]
    fn native_agents_bridge_every_selected_upstream_protocol() {
        for app in [
            AppType::Claude,
            AppType::ClaudeDesktop,
            AppType::Codex,
            AppType::GrokBuild,
            AppType::Qoder,
            AppType::WorkBuddy,
            AppType::Kimi,
            AppType::Qwen,
            AppType::ZCode,
            AppType::DeepSeekHarness,
            AppType::Trae,
            AppType::Hermes,
        ] {
            for (endpoint, input) in [
                ("/v1/chat/completions", Protocol::Chat),
                ("/v1/responses", Protocol::Responses),
                ("/v1/messages", Protocol::Anthropic),
            ] {
                for output in [
                    Protocol::Chat,
                    Protocol::Responses,
                    Protocol::Anthropic,
                    Protocol::Gemini,
                ] {
                    let mut provider =
                        Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
                    provider
                        .meta
                        .get_or_insert_with(Default::default)
                        .api_format = Some(output.format().into());
                    let bridge = Bridge::route(&app, endpoint, &provider);
                    assert_eq!(
                        bridge.is_some(),
                        input != output,
                        "{} {endpoint} -> {}",
                        app.as_str(),
                        output.format()
                    );
                }
            }
        }
    }

    #[test]
    fn legacy_chat_clients_keep_native_routing_without_a_format_override() {
        let provider = Provider::with_id(
            "legacy".into(),
            "Legacy".into(),
            json!({"base_url":"https://relay.example"}),
            None,
        );
        for app in [
            AppType::Codex,
            AppType::OpenCode,
            AppType::OpenClaw,
            AppType::Hermes,
        ] {
            assert!(
                Bridge::route(&app, "/v1/chat/completions", &provider).is_none(),
                "a legacy native Chat client was rerouted without a protocol selection"
            );
        }
    }
    #[test]
    fn anthropic_bridge_honors_the_selected_credential_header() {
        for (field, expected) in [
            ("ANTHROPIC_API_KEY", AuthStrategy::Anthropic),
            ("ANTHROPIC_AUTH_TOKEN", AuthStrategy::Bearer),
        ] {
            let mut provider = Provider::with_id(
                "fixture".into(),
                "Fixture".into(),
                json!({"auth":{"OPENAI_API_KEY":"fixture-key"}}),
                None,
            );
            provider.meta = Some(crate::provider::ProviderMeta {
                api_format: Some("anthropic".into()),
                api_key_field: Some(field.into()),
                ..Default::default()
            });
            let original = providers::CodexAdapter::new();
            let adapter = BridgeAdapter {
                original: &original,
                output: Protocol::Anthropic,
            };
            assert_eq!(adapter.extract_auth(&provider).unwrap().strategy, expected);
        }
    }

    #[test]
    fn conversion_preserves_query_options_without_forwarding_gemini_credentials() {
        let chat = Bridge {
            input: Protocol::Chat,
            output: Protocol::Responses,
        };
        assert_eq!(
            chat.endpoint("/v1/chat/completions?api-version=Case%20Value&custom=1"),
            "/v1/messages?api-version=Case%20Value&custom=1"
        );
        let gemini = Bridge {
            input: Protocol::Gemini,
            output: Protocol::Anthropic,
        };
        assert_eq!(
            gemini.endpoint("/v1beta/models/a:streamGenerateContent?alt=sse&key=private&custom=1"),
            "/v1/messages?custom=1"
        );
    }
}
