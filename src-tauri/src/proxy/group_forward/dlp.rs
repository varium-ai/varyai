//! Reuse the established actual-send preparation and response evidence owners.
use super::*;
use crate::{
    app_config::AppType,
    proxy::{
        handler_context::RequestContext,
        security::http_restoration::{DlpForwardError, DlpHttpRequest},
    },
};
use futures::StreamExt;

#[derive(Clone)]
pub(crate) struct RoutedRestoration {
    pub request: Arc<DlpHttpRequest>,
    pub actor: &'static str,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn forward(
    state: &ProxyState,
    forwarder: RequestForwarder,
    routed: &RoutedRestoration,
    protocol: Protocol,
    provider: &Provider,
    method: &http::Method,
    endpoint: &str,
    body: &Value,
    headers: &HeaderMap,
    extensions: &http::Extensions,
    session: Option<&str>,
    history: Arc<CodexChatHistoryStore>,
) -> Result<ProxyResponse, ProxyError> {
    let app = match protocol {
        Protocol::Responses | Protocol::Chat => AppType::Codex,
        Protocol::Anthropic => AppType::Claude,
        Protocol::Gemini => AppType::Gemini,
    };
    if super::super::providers::provider_needs_responses_namespace_flatten(provider) {
        return Err(ProxyError::DlpForward(DlpForwardError::RestoreUnsupported));
    }
    let adapter = super::super::providers::get_adapter(&app)
        .ok_or(ProxyError::DlpForward(DlpForwardError::RestoreUnsupported))?;
    let gemini_endpoint;
    let endpoint = if protocol == Protocol::Gemini {
        let (path, query) = endpoint
            .split_once('?')
            .map_or((endpoint, None), |(p, q)| (p, Some(q)));
        let (prefix, _) = path
            .rsplit_once("/models/")
            .ok_or(ProxyError::DlpForward(DlpForwardError::RestoreUnsupported))?;
        let (_, operation) = path
            .rsplit_once(':')
            .ok_or(ProxyError::DlpForward(DlpForwardError::RestoreUnsupported))?;
        let model = body["model"]
            .as_str()
            .ok_or(ProxyError::DlpForward(DlpForwardError::RestoreUnsupported))?;
        gemini_endpoint = format!(
            "{prefix}/models/{}:{operation}{}",
            super::super::gemini_url::normalize_gemini_model_id(model),
            query.map(|q| format!("?{q}")).unwrap_or_default()
        );
        gemini_endpoint.as_str()
    } else {
        endpoint
    };
    let forwarder = forwarder.with_shared_dlp_request(routed.request.clone());
    let (response, format, outbound, schemas, attempt) = forwarder
        .forward(
            &app,
            method,
            provider,
            endpoint,
            body,
            headers,
            extensions,
            adapter.as_ref(),
        )
        .await?;
    let mut ctx =
        RequestContext::for_routed_target(provider.clone(), app, routed.actor, body, session);
    ctx.converted_output = format.is_some();
    ctx.outbound_model = outbound;
    ctx.tool_schemas = schemas;
    // The group runtime owns accounting and continuity. The reused finalizer
    // receives its isolated history store and cannot duplicate usage writes.
    let mut response_state = state.clone();
    let mut config = state.config.read().await.clone();
    config.enable_logging = false;
    response_state.config = Arc::new(tokio::sync::RwLock::new(config));
    response_state.codex_chat_history = history;
    let is_stream = body["stream"] == true || response.is_sse();
    let finalized = if protocol != Protocol::Responses {
        super::super::response_processor::process_native_response(
            response,
            &ctx,
            &response_state,
            match protocol {
                Protocol::Chat => &super::super::handler_config::OPENAI_PARSER_CONFIG,
                Protocol::Anthropic => &super::super::handler_config::CLAUDE_PARSER_CONFIG,
                Protocol::Gemini => &super::super::handler_config::GEMINI_PARSER_CONFIG,
                Protocol::Responses => &super::super::handler_config::CODEX_PARSER_CONFIG,
            },
            None,
            attempt,
        )
        .await?
    } else if format.as_deref() != Some("openai_responses")
        && (super::super::providers::should_convert_codex_responses_to_chat(provider, endpoint)
            || super::super::providers::should_convert_codex_responses_to_anthropic(
                provider, endpoint,
            ))
    {
        let tool_context =
            super::super::providers::transform_codex_chat::build_codex_tool_context_from_request(
                body,
            );
        let evidence = if is_stream {
            attempt
                .as_ref()
                .and_then(|attempt| attempt.upstream_sse_evidence_tracker())
        } else {
            None
        };
        let dlp = super::super::handlers::CodexDlpTransformContext {
            attempt: attempt.as_ref(),
            evidence: evidence.clone(),
        };
        let chat =
            super::super::providers::should_convert_codex_responses_to_chat(provider, endpoint);
        let response = if chat {
            super::super::handlers::handle_codex_chat_to_responses_transform(
                response,
                &ctx,
                &response_state,
                is_stream,
                None,
                tool_context,
                dlp,
            )
            .await
        } else {
            super::super::handlers::handle_codex_anthropic_to_responses_transform(
                response,
                &ctx,
                &response_state,
                is_stream,
                None,
                tool_context,
                dlp,
            )
            .await
        }
        .map_err(|error| {
            super::super::response_processor::isolate_codex_dlp_response_error(
                error,
                attempt.is_some(),
            )
        })?;
        match attempt {
            Some(attempt) => attempt.restore_client_http_response(response,
                if chat { super::super::security::response_restoration::ArgumentNormalizationMode::ChatCanonical }
                else { super::super::security::response_restoration::ArgumentNormalizationMode::Exact }, evidence).await,
            None => return Err(ProxyError::DlpForward(DlpForwardError::RestoreFailed)),
        }
    } else {
        super::super::response_processor::process_codex_response(
            response,
            &ctx,
            &response_state,
            &super::super::handler_config::CODEX_PARSER_CONFIG,
            None,
            attempt,
        )
        .await?
    };
    let (parts, body) = finalized.into_parts();
    Ok(ProxyResponse::streamed(
        parts.status,
        parts.headers,
        body.into_data_stream()
            .map(|chunk| chunk.map_err(|_| std::io::Error::other("routed_response_failed"))),
    ))
}
