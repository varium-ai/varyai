//! Consistent upstream TLS, independent of the process-wide Rustls provider.
use rustls::client::{
    ClientSessionMemoryCache, ClientSessionStore, Tls12ClientSessionValue, Tls13ClientSessionValue,
};
use rustls::pki_types::ServerName;
use std::sync::{Arc, OnceLock};

/// Cache session tickets without replacing the default first-flight key shares.
#[derive(Debug)]
struct UpstreamSessions(ClientSessionMemoryCache);

impl ClientSessionStore for UpstreamSessions {
    fn set_kx_hint(&self, _: ServerName<'static>, _: rustls::NamedGroup) {}

    fn kx_hint(&self, _: &ServerName<'_>) -> Option<rustls::NamedGroup> {
        // Some gateways close the shorter hello produced after a classical hint.
        None
    }

    fn set_tls12_session(&self, name: ServerName<'static>, value: Tls12ClientSessionValue) {
        self.0.set_tls12_session(name, value);
    }

    fn tls12_session(&self, name: &ServerName<'_>) -> Option<Tls12ClientSessionValue> {
        self.0.tls12_session(name)
    }

    fn remove_tls12_session(&self, name: &ServerName<'static>) {
        self.0.remove_tls12_session(name);
    }

    fn insert_tls13_ticket(&self, name: ServerName<'static>, value: Tls13ClientSessionValue) {
        self.0.insert_tls13_ticket(name, value);
    }

    fn take_tls13_ticket(&self, name: &ServerName<'static>) -> Option<Tls13ClientSessionValue> {
        self.0.take_tls13_ticket(name)
    }
}

pub(super) fn client_config() -> Result<rustls::ClientConfig, String> {
    static CONFIG: OnceLock<Result<rustls::ClientConfig, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let native = rustls_native_certs::load_native_certs();
            roots.add_parsable_certificates(native.certs);
            with_roots(roots)
        })
        .clone()
}

/// Upstream TLS config for pooled reqwest clients that support HTTP/2 via ALPN.
pub(super) fn client_config_h2() -> Result<rustls::ClientConfig, String> {
    static CONFIG: OnceLock<Result<rustls::ClientConfig, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut config = client_config()?;
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Ok(config)
        })
        .clone()
}

pub(super) fn with_roots(roots: rustls::RootCertStore) -> Result<rustls::ClientConfig, String> {
    // Keep hybrid TLS 1.3 available even when a local listener installs ring globally.
    rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map(|builder| {
        let mut config = builder.with_root_certificates(roots).with_no_client_auth();
        config.resumption = rustls::client::Resumption::store(Arc::new(UpstreamSessions(
            ClientSessionMemoryCache::new(256),
        )));
        config
    })
    .map_err(|_| "Upstream TLS configuration unavailable".to_string())
}
