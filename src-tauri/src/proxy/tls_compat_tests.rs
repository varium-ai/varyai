//! Exercise the TLS offers of the actual discovery and forwarding clients.
use std::{io::Cursor, time::Duration};

use tokio::io::AsyncReadExt;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn upstream_clients_offer_hybrid_key_exchange() -> TestResult {
    // Match the application's current startup provider, regardless of test order.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _isolation = super::forwarder::final_send_binding_test_support::install(|_, _| {});
    let mut offers = Vec::new();
    for transport in ["discovery", "raw", "hyper"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut header = [0; 5];
            socket.read_exact(&mut header).await?;
            let mut body = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
            socket.read_exact(&mut body).await?;
            let mut acceptor = rustls::server::Acceptor::default();
            let mut record = header.to_vec();
            record.extend(body);
            acceptor.read_tls(&mut Cursor::new(record))?;
            let accepted = acceptor
                .accept()
                .map_err(|_| "synthetic ClientHello rejected")?
                .ok_or("synthetic ClientHello incomplete")?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                accepted
                    .client_hello()
                    .named_groups()
                    .unwrap_or_default()
                    .to_vec(),
            )
        });
        let url = format!("https://{address}/v1/models");
        let request = async {
            if transport == "discovery" {
                let _ = super::http_client::get_without_redirects()?
                    .get(&url)
                    .timeout(Duration::from_secs(3))
                    .send()
                    .await;
            } else {
                let mut headers = http::HeaderMap::new();
                headers.insert(http::header::HOST, address.to_string().parse()?);
                let mut extensions = http::Extensions::new();
                if transport == "raw" {
                    extensions.insert(super::hyper_client::OriginalHeaderCases {
                        cases: vec![("host".into(), b"Host".to_vec())],
                    });
                }
                let _ = super::hyper_client::send_request(
                    url.parse()?,
                    "synthetic TLS endpoint",
                    http::Method::GET,
                    headers,
                    extensions,
                    Vec::new(),
                    Duration::from_secs(3),
                    None,
                )
                .await;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (sent, received) = tokio::join!(request, async {
            tokio::time::timeout(Duration::from_secs(5), server).await
        });
        sent?;
        let groups = received???;
        offers.push((
            transport,
            groups.contains(&rustls::NamedGroup::X25519MLKEM768),
        ));
    }
    for (transport, hybrid) in offers {
        assert!(hybrid, "{transport} must reach hybrid-only TLS upstreams");
    }
    Ok(())
}

#[tokio::test]
async fn upstream_tls_negotiates_hybrid_classical_and_tls12() -> TestResult {
    for (group, tls13) in [
        (rustls::NamedGroup::X25519MLKEM768, true),
        (rustls::NamedGroup::X25519, true),
        (rustls::NamedGroup::secp256r1, false),
    ] {
        assert_eq!(
            negotiate(group, tls13, true, "localhost").await?,
            Some(group)
        );
    }
    Ok(())
}

#[tokio::test]
async fn upstream_tls_rejects_untrusted_certificates_and_wrong_hostnames() -> TestResult {
    for (trusted, name) in [(false, "localhost"), (true, "other.invalid")] {
        assert!(
            negotiate(rustls::NamedGroup::X25519MLKEM768, true, trusted, name)
                .await?
                .is_none()
        );
    }
    Ok(())
}

#[test]
fn repeated_client_hellos_keep_hybrid_key_share() -> TestResult {
    use base64::Engine;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(rustls::pki_types::CertificateDer::from(
        base64::engine::general_purpose::STANDARD.decode(CERT)?,
    ))?;
    let config = std::sync::Arc::new(super::upstream_tls::with_roots(roots)?);
    assert_eq!(
        memory_handshake(config.clone(), rustls::NamedGroup::X25519)?,
        Some(rustls::HandshakeKind::Full)
    );
    assert_eq!(
        memory_handshake(config, rustls::NamedGroup::X25519MLKEM768)?,
        Some(rustls::HandshakeKind::Full),
        "a prior classical negotiation must not remove the initial hybrid key share"
    );
    Ok(())
}

fn memory_handshake(
    config: std::sync::Arc<rustls::ClientConfig>,
    group: rustls::NamedGroup,
) -> Result<Option<rustls::HandshakeKind>, Box<dyn std::error::Error + Send + Sync>> {
    memory_exchange(config, memory_server(group)?)
}

#[test]
fn upstream_tls_retains_session_resumption() -> TestResult {
    use base64::Engine;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(rustls::pki_types::CertificateDer::from(
        base64::engine::general_purpose::STANDARD.decode(CERT)?,
    ))?;
    let config = std::sync::Arc::new(super::upstream_tls::with_roots(roots)?);
    let server = memory_server(rustls::NamedGroup::X25519)?;
    assert_eq!(
        memory_exchange(config.clone(), server.clone())?,
        Some(rustls::HandshakeKind::Full)
    );
    assert_eq!(
        memory_exchange(config, server)?,
        Some(rustls::HandshakeKind::Resumed)
    );
    Ok(())
}

fn memory_server(
    group: rustls::NamedGroup,
) -> Result<std::sync::Arc<rustls::ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    use base64::Engine;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let certificate = CertificateDer::from(base64::engine::general_purpose::STANDARD.decode(CERT)?);
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        base64::engine::general_purpose::STANDARD.decode(KEY)?,
    ));
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    provider
        .kx_groups
        .retain(|candidate| candidate.name() == group);
    let server_config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
    Ok(std::sync::Arc::new(server_config))
}

fn memory_exchange(
    config: std::sync::Arc<rustls::ClientConfig>,
    server: std::sync::Arc<rustls::ServerConfig>,
) -> Result<Option<rustls::HandshakeKind>, Box<dyn std::error::Error + Send + Sync>> {
    let mut server = rustls::ServerConnection::new(server)?;
    let mut client = rustls::ClientConnection::new(
        config,
        rustls::pki_types::ServerName::try_from("localhost")?,
    )?;
    for _ in 0..8 {
        let mut outgoing = Vec::new();
        client.write_tls(&mut outgoing)?;
        server.read_tls(&mut Cursor::new(outgoing))?;
        server.process_new_packets()?;
        let mut incoming = Vec::new();
        server.write_tls(&mut incoming)?;
        client.read_tls(&mut Cursor::new(incoming))?;
        client.process_new_packets()?;
        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(client.handshake_kind());
        }
    }
    Err("synthetic handshake did not finish".into())
}

async fn negotiate(
    group: rustls::NamedGroup,
    tls13: bool,
    trusted: bool,
    name: &str,
) -> Result<Option<rustls::NamedGroup>, Box<dyn std::error::Error + Send + Sync>> {
    use base64::Engine;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
    use std::sync::Arc;
    let certificate = CertificateDer::from(base64::engine::general_purpose::STANDARD.decode(CERT)?);
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        base64::engine::general_purpose::STANDARD.decode(KEY)?,
    ));
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    provider
        .kx_groups
        .retain(|candidate| candidate.name() == group);
    let version = if tls13 {
        &rustls::version::TLS13
    } else {
        &rustls::version::TLS12
    };
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[version])?
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        Ok::<_, std::io::Error>(
            tokio_rustls::TlsAcceptor::from(Arc::new(server_config))
                .accept(socket)
                .await
                .is_ok(),
        )
    });
    let mut roots = rustls::RootCertStore::empty();
    if trusted {
        roots.add(certificate)?;
    }
    let config = super::upstream_tls::with_roots(roots)?;
    let socket = tokio::net::TcpStream::connect(address).await?;
    let connected = tokio::time::timeout(
        Duration::from_secs(3),
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from(name.to_string())?, socket),
    )
    .await?;
    let selected = connected.as_ref().ok().and_then(|stream| {
        stream
            .get_ref()
            .1
            .negotiated_key_exchange_group()
            .map(|kx| kx.name())
    });
    drop(connected);
    let server_ok = tokio::time::timeout(Duration::from_secs(3), server).await???;
    assert_eq!(server_ok, selected.is_some());
    Ok(selected)
}

#[test]
fn client_config_h2_enables_alpn_while_http1_stays_empty() -> TestResult {
    let h1 = super::upstream_tls::client_config()?;
    assert!(h1.alpn_protocols.is_empty());

    let h2 = super::upstream_tls::client_config_h2()?;
    assert_eq!(
        h2.alpn_protocols,
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    );
    Ok(())
}

// Public, synthetic localhost identity for isolated tests; never installed in system trust.
const CERT: &str = "MIIBdzCCAR2gAwIBAgIUe9UYx9x2UUuzwvzrGwD2vbOpylYwCgYIKoZIzj0EAwIwFDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MTExNzA3NDk1N1oYDzIxMjYxMDI0MDc0OTU3WjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASU6dZxssN50dESb41f3TJ6rlmBPmSO4OZRnMsE30nX4O84eXBiJVOsguUS10WpiMjV/GWVdu4dX0cDmrLh9Jdoo0swSTAdBgNVHQ4EFgQUKhqqdQSg5321viwp7688PAZ6+bIwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMAwGA1UdEwEB/wQCMAAwCgYIKoZIzj0EAwIDSAAwRQIgTQamt+n+kHO5XiX3JJIaTDpSgq3gSMdKDLM7SzuzTTICIQCh2eOJYXTodxXDPL+CWi1Mjav1l136A7f7kPRe4ESc4A==";
const KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgiM6UaVdsMB6PMDFxQuBV8FIhSe+/sh4LJiB2wtmzmPWhRANCAASU6dZxssN50dESb41f3TJ6rlmBPmSO4OZRnMsE30nX4O84eXBiJVOsguUS10WpiMjV/GWVdu4dX0cDmrLh9Jdo";
