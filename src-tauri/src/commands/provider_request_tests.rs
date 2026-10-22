use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REQUEST_ID: &str = "12f57d58-7019-4e84-8ed7-f9ce4731e3cb";

fn request() -> ProviderRequest {
    ProviderRequest {
        request_id: REQUEST_ID.into(),
        name: " Example AI ".into(),
        website_url: " https://example.com?a=1 ".into(),
        description: " Coding\nplan\t说明 ".into(),
    }
}

fn payload(model: Option<&str>) -> ServerRequest<'_> {
    ServerRequest {
        request_id: REQUEST_ID,
        name: "Example AI",
        website_url: "https://example.com?a=1",
        description: "Coding\nplan\t说明",
        app_version: "0.2.3",
        device_model: model,
    }
}

async fn mock_server(
    status: u16,
    body: &str,
    extra_headers: &str,
) -> (reqwest::Url, tokio::task::JoinHandle<serde_json::Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let url = reqwest::Url::parse(&format!(
        "http://{}/api/v1/provider-requests",
        listener.local_addr().expect("address")
    ))
    .expect("url");
    let reply = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}", body.len());
    let task = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .expect("timeout")
            .expect("accept");
        let mut data = Vec::new();
        let (start, length) = loop {
            let mut buf = [0; 2048];
            let count = socket.read(&mut buf).await.expect("read");
            assert!(count > 0);
            data.extend_from_slice(&buf[..count]);
            if let Some(end) = data.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                assert!(headers.starts_with("post /api/v1/provider-requests http/1.1"));
                assert!(headers.contains("content-type: application/json"));
                assert!(!headers.contains("authorization:"));
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .map(|value| value.trim().parse().expect("length"))
                    })
                    .expect("content length");
                if data.len() >= end + 4 + length {
                    break (end + 4, length);
                }
            }
        };
        let _ = socket.write_all(reply.as_bytes()).await;
        serde_json::from_slice(&data[start..start + length]).expect("payload")
    });
    (url, task)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client")
}

#[test]
fn trims_fields_and_validates_uuid() {
    let mut input = request();
    input.request_id = REQUEST_ID.to_uppercase();
    let result = validate_request(input).expect("valid");
    assert_eq!(result.request_id, REQUEST_ID);
    assert_eq!(result.name, "Example AI");
    assert_eq!(result.website_url, "https://example.com?a=1");
    assert_eq!(result.description, "Coding\nplan\t说明");
    for id in [
        "bad",
        "00000000-0000-1000-8000-000000000000",
        "12f57d5870194e848ed7f9ce4731e3cb",
        "12f57d58-7019-4e84-0ed7-f9ce4731e3cb",
    ] {
        let mut input = request();
        input.request_id = id.into();
        assert_eq!(
            validate_request(input).expect_err("invalid").code,
            "invalid_request"
        );
    }
}

#[test]
fn rejects_invalid_fields_and_description_control_characters() {
    for name in ["".to_string(), "中".repeat(51), "a\nb".into()] {
        let mut input = request();
        input.name = name;
        assert!(validate_request(input).is_err());
    }
    for url in [
        "javascript:alert(1)",
        "https://user:secret@example.com",
        "",
        "/relative",
        "https:example.com",
        "https:///example.com",
        "https://@example.com",
        "https://example.com/path with space",
        "https://example.com/\\path",
        "https://example.com:99999",
        "https://example.com/\tpath",
    ] {
        let mut input = request();
        input.website_url = url.into();
        assert!(validate_request(input).is_err(), "{url}");
    }
    for description in ["private\0text".to_owned(), "a\rb".into(), "中".repeat(1001)] {
        let mut input = request();
        input.description = description;
        assert!(validate_request(input).is_err());
    }
    let mut input = request();
    input.name = "中".repeat(50);
    input.description = "😀".repeat(1000);
    assert!(validate_request(input).is_ok());
}

#[test]
fn endpoint_requires_https_and_an_origin() {
    assert_eq!(
        endpoint("https://www.varyai.net")
            .expect("endpoint")
            .as_str(),
        "https://www.varyai.net/api/v1/provider-requests"
    );
    for base in [
        "http://example.com",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com?secret=x",
        "https://example.com#secret",
        "bad",
    ] {
        assert_eq!(
            endpoint(base).expect_err("invalid").code,
            "temporarily_unavailable"
        );
    }
}

#[test]
fn device_model_is_optional_bounded_and_only_a_model() {
    assert_eq!(
        normalize_device_model(" MacBookPro18,3\n"),
        Some("MacBookPro18,3".into())
    );
    for value in ["".to_owned(), "a\0b".into(), "a\nb".into(), "a".repeat(129)] {
        assert_eq!(normalize_device_model(&value), None);
    }
    assert_eq!(
        normalize_device_model(&"中".repeat(128)),
        Some("中".repeat(128))
    );
    let json = serde_json::to_value(payload(None)).expect("serialize");
    assert!(json.get("device_model").is_none());
}

#[tokio::test]
async fn sends_server_schema_and_accepts_created_or_idempotent_confirmation() {
    for status in [201, 200] {
        let body = format!(r#"{{"request_id":"{REQUEST_ID}","status":"pending"}}"#);
        let (url, task) = mock_server(status, &body, "").await;
        send_provider_request(&client(), url, &payload(Some("MacBookPro18,3")))
            .await
            .expect("confirmed");
        let json = task.await.expect("task");
        assert_eq!(
            json,
            serde_json::json!({"request_id":REQUEST_ID,"name":"Example AI","website_url":"https://example.com?a=1","description":"Coding\nplan\t说明","app_version":"0.2.3","device_model":"MacBookPro18,3"})
        );
    }
}

#[tokio::test]
async fn rejects_unconfirmed_success_and_oversized_responses() {
    let bodies = [
        r#"{"code":0}"#.to_owned(),
        "invalid private body".into(),
        format!(r#"{{"request_id":"{REQUEST_ID}","status":"sent"}}"#),
        r#"{"request_id":"another-id","status":"pending"}"#.into(),
        "x".repeat(MAX_RESPONSE_BYTES + 1),
    ];
    for body in bodies {
        let (url, task) = mock_server(200, &body, "").await;
        assert_eq!(
            send_provider_request(&client(), url, &payload(None))
                .await
                .expect_err("unconfirmed")
                .code,
            "send_failed"
        );
        task.await.expect("task");
    }
}

#[tokio::test]
async fn maps_http_failures_without_echoing_body_or_following_redirects() {
    for (status, code) in [
        (400, "invalid_request"),
        (409, "request_id_conflict"),
        (413, "request_too_large"),
        (415, "invalid_request"),
        (429, "rate_limited"),
        (503, "temporarily_unavailable"),
        (404, "temporarily_unavailable"),
        (500, "temporarily_unavailable"),
        (302, "send_failed"),
        (202, "send_failed"),
    ] {
        let (url, task) = mock_server(
            status,
            "private-token-and-input",
            "Retry-After: 60\r\nLocation: http://127.0.0.1:1/private\r\n",
        )
        .await;
        let error = send_provider_request(&client(), url, &payload(None))
            .await
            .expect_err("failed");
        assert_eq!(error.code, code);
        assert_eq!(
            error.retry_after_seconds,
            if status == 429 { Some(60) } else { None }
        );
        assert!(!serde_json::to_string(&error)
            .expect("serialize")
            .contains("private"));
        task.await.expect("task");
    }
    for retry in ["invalid", "-1", "9999999999999999999999"] {
        let (url, task) = mock_server(429, "", &format!("Retry-After: {retry}\r\n")).await;
        let error = send_provider_request(&client(), url, &payload(None))
            .await
            .expect_err("limited");
        assert_eq!(error.code, "rate_limited");
        assert_eq!(error.retry_after_seconds, None);
        task.await.expect("task");
    }
}

#[tokio::test]
async fn transport_errors_are_sanitized() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = reqwest::Url::parse(&format!(
        "http://{}/private",
        listener.local_addr().expect("address")
    ))
    .expect("url");
    drop(listener);
    let error = send_provider_request(&client(), url, &payload(None))
        .await
        .expect_err("network");
    assert_eq!(
        serde_json::to_value(error).expect("error"),
        serde_json::json!({"code":"send_failed"})
    );
}
