use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::app_config::AppType;
use crate::proxy::error::ProxyError;

fn blocked_message(reason: &str) -> String {
    format!(
        "[Vary AI 安全审计] 检测到敏感数据，请求已被拦截\n{reason}\n当前处理方式为「拦截」，如需继续，请在安全审计设置中调整对应规则。"
    )
}

pub fn build_blocked_response(app_type: &AppType, reason: &str) -> Result<Response, ProxyError> {
    let body_json = match app_type {
        AppType::Claude | AppType::ClaudeDesktop => {
            json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": blocked_message(reason)
                }
            })
        }
        AppType::Gemini => {
            json!({
                "error": {
                    "code": 400,
                    "message": blocked_message(reason),
                    "status": "INVALID_ARGUMENT"
                }
            })
        }
        AppType::Codex
        | AppType::GrokBuild
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::Pi
        | AppType::WorkBuddy
        | AppType::Qoder
        | AppType::Kimi
        | AppType::Qwen
        | AppType::ZCode
        | AppType::Trae
        | AppType::DoubaoWork => {
            json!({
                "error": {
                    "message": blocked_message(reason),
                    "type": "invalid_request_error",
                    "param": null,
                    "code": "sensitive_data_blocked"
                }
            })
        }
    };

    let body_bytes = serde_json::to_vec(&body_json).map_err(|e| {
        ProxyError::Internal(format!("Failed to serialize security block response: {e}"))
    })?;

    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(axum::body::Body::from(body_bytes))
        .map_err(|e| ProxyError::Internal(format!("Failed to build security block response: {e}")))
}

/// Fixed, non-sensitive admission failure; never expose persistence/parser details.
pub(super) fn build_unavailable_response(app_type: &AppType) -> Result<Response, ProxyError> {
    let body = match app_type {
        AppType::Claude | AppType::ClaudeDesktop => json!({
            "type": "error",
            "error": { "type": "api_error", "message": "dlp_unavailable" }
        }),
        AppType::Gemini => json!({
            "error": { "code": 503, "message": "dlp_unavailable", "status": "UNAVAILABLE" }
        }),
        _ => json!({
            "error": { "type": "server_error", "message": "dlp_unavailable", "code": "dlp_unavailable", "param": null }
        }),
    };
    let bytes =
        serde_json::to_vec(&body).map_err(|_| ProxyError::Internal("dlp_unavailable".into()))?;
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(bytes))
        .map_err(|_| ProxyError::Internal("dlp_unavailable".into()))
}
