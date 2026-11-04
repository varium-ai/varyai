use super::*;

fn request(
    method: http::Method,
    path: &str,
    peer: Option<&str>,
    origins: &[&str],
    fetch_site: bool,
) -> Result<Request, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method(method).uri(path);
    for origin in origins {
        request = request.header("origin", *origin);
    }
    if fetch_site {
        request = request.header("sec-fetch-site", "cross-site");
    }
    let mut request = request.body(Body::empty())?;
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(peer.parse::<std::net::SocketAddr>()?);
    }
    Ok(request)
}

#[test]
fn native_trae_origin_requires_exact_route_method_and_loopback(
) -> Result<(), Box<dyn std::error::Error>> {
    for peer in ["127.0.0.1:12345", "[::1]:12345"] {
        for fetch_site in [false, true] {
            assert!(local_client(&request(
                http::Method::POST,
                "/connections/trae/v1/chat/completions",
                Some(peer),
                &["vscode-file://vscode-app"],
                fetch_site,
            )?));
        }
    }
    for peer in [None, Some("192.0.2.1:12345"), Some("[2001:db8::1]:12345")] {
        assert!(!local_client(&request(
            http::Method::POST,
            "/connections/trae/v1/chat/completions",
            peer,
            &["vscode-file://vscode-app"],
            true,
        )?));
    }
    for (method, path) in [
        (http::Method::GET, "/connections/trae/v1/chat/completions"),
        (http::Method::GET, "/connections/trae/v1/models"),
        (
            http::Method::OPTIONS,
            "/connections/trae/v1/chat/completions",
        ),
        (
            http::Method::POST,
            "/connections/claude/v1/chat/completions",
        ),
        (
            http::Method::POST,
            "/connections/trae-extra/v1/chat/completions",
        ),
        (http::Method::POST, "/connections/trae/v1/chat/completions/"),
        (http::Method::POST, "/connections/trae/v1/responses"),
        (http::Method::POST, "/connections/trae/v1/messages"),
        (http::Method::POST, "/intent/v1/chat/completions"),
    ] {
        assert!(
            !local_client(&request(
                method.clone(),
                path,
                Some("127.0.0.1:12345"),
                &["vscode-file://vscode-app"],
                true,
            )?),
            "{method} {path}"
        );
    }
    Ok(())
}

#[test]
fn native_trae_fetch_metadata_without_origin_requires_local_chat_route(
) -> Result<(), Box<dyn std::error::Error>> {
    for peer in ["127.0.0.1:12345", "[::1]:12345"] {
        assert!(local_client(&request(
            http::Method::POST,
            "/connections/trae/v1/chat/completions",
            Some(peer),
            &[],
            true,
        )?));
    }
    for peer in [None, Some("192.0.2.1:12345")] {
        assert!(!local_client(&request(
            http::Method::POST,
            "/connections/trae/v1/chat/completions",
            peer,
            &[],
            true,
        )?));
    }
    for (method, path) in [
        (
            http::Method::OPTIONS,
            "/connections/trae/v1/chat/completions",
        ),
        (http::Method::GET, "/connections/trae/v1/models"),
        (
            http::Method::POST,
            "/connections/claude/v1/chat/completions",
        ),
        (http::Method::POST, "/connections/trae/v1/chat/completions/"),
    ] {
        assert!(!local_client(&request(
            method,
            path,
            Some("127.0.0.1:12345"),
            &[],
            true,
        )?));
    }
    Ok(())
}

#[test]
fn native_trae_origin_rejects_ambiguous_and_browser_values(
) -> Result<(), Box<dyn std::error::Error>> {
    for origins in [
        vec!["null"],
        vec!["https://fixture.invalid"],
        vec!["http://127.0.0.1"],
        vec!["vscode-file://vscode-app/"],
        vec!["vscode-file://vscode-app.fixture.invalid"],
        vec!["vscode-file://vscode-app@fixture.invalid"],
        vec!["vscode-file://VSCODE-APP"],
        vec!["vscode-file://vscode-app, https://fixture.invalid"],
        vec!["vscode-file://vscode-app https://fixture.invalid"],
        vec!["vscode-file://vscode-app", "vscode-file://vscode-app"],
        vec!["vscode-file://vscode-app", "https://fixture.invalid"],
        vec!["https://fixture.invalid", "vscode-file://vscode-app"],
    ] {
        assert!(
            !local_client(&request(
                http::Method::POST,
                "/connections/trae/v1/chat/completions",
                Some("127.0.0.1:12345"),
                &origins,
                true,
            )?),
            "unexpected admission for synthetic origin case"
        );
    }
    let mut forwarded = request(
        http::Method::POST,
        "/connections/trae/v1/chat/completions",
        None,
        &["vscode-file://vscode-app"],
        true,
    )?;
    forwarded.headers_mut().insert(
        "x-forwarded-for",
        http::HeaderValue::from_static("127.0.0.1"),
    );
    assert!(!local_client(&forwarded));
    Ok(())
}

#[test]
fn header_free_local_client_admission_is_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    for (peer, allowed) in [
        (None, true),
        (Some("127.0.0.1:12345"), true),
        (Some("[::1]:12345"), true),
        (Some("192.0.2.1:12345"), false),
    ] {
        assert_eq!(
            local_client(&request(
                http::Method::POST,
                "/connections/claude/v1/messages",
                peer,
                &[],
                false,
            )?),
            allowed
        );
    }
    Ok(())
}

#[test]
fn ingress_diagnostics_never_include_request_values() -> Result<(), Box<dyn std::error::Error>> {
    let request = request(
        http::Method::POST,
        "/connections/trae/v1/chat/completions?synthetic-private-query",
        Some("127.0.0.1:12345"),
        &["https://synthetic-private-origin.invalid"],
        true,
    )?;
    let summary = ingress_diagnostic(&request);
    assert_eq!(
        summary,
        "peer=loopback method=post route=trae_chat origin=other fetch_site=true"
    );
    assert!(!summary.contains("synthetic-private"));
    Ok(())
}
