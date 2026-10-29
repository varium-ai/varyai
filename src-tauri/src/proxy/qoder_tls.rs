//! Loopback-only TLS listener for the Qoder official SDK.
use super::qoder::OfficialState;
use axum::{routing::any, Router};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{path::Path, sync::Arc};
use tokio::sync::{oneshot, watch};
fn failure() -> String {
    "Cannot start the Qoder local TLS listener".into()
}
fn pem(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(path).map_err(|_| failure())?;
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let value = text
        .split_once(&begin)
        .and_then(|(_, s)| s.split_once(&end))
        .map(|(v, _)| v.chars().filter(|c| !c.is_whitespace()).collect::<String>())
        .ok_or_else(failure)?;
    STANDARD.decode(value).map_err(|_| failure())
}
pub(crate) struct Server {
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Server {
    pub async fn start(
        listener: tokio::net::TcpListener,
        directory: &Path,
        state: OfficialState,
    ) -> Result<Self, String> {
        use rustls::pki_types::{
            CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer,
        };
        let cert = CertificateDer::from(pem(&directory.join("localhost.pem"), "CERTIFICATE")?);
        let key = pem(&directory.join("localhost-key.pem"), "PRIVATE KEY")
            .map(|b| PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(b)))
            .or_else(|_| {
                pem(&directory.join("localhost-key.pem"), "RSA PRIVATE KEY")
                    .map(|b| PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(b)))
            })?;
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| failure())?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|_| failure())?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let router = Router::new()
            .fallback(any(super::qoder::handle_official))
            .with_state(state);
        let (tx, mut rx) = oneshot::channel();
        let (closed, _) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                 _=&mut rx=>{let _=closed.send(true);break;},
                 accepted=listener.accept()=>{let Ok((stream,_))=accepted else{break;};let acceptor=acceptor.clone();let app=router.clone();let mut closed=closed.subscribe();
                  connections.spawn(async move{let connection=async{
                   let Ok(Ok(tls))=tokio::time::timeout(std::time::Duration::from_secs(10),acceptor.accept(stream)).await else{return;};
                   let service=hyper::service::service_fn(move|request:hyper::Request<hyper::body::Incoming>|{let mut app=app.clone();async move{let(parts,body)=request.into_parts();let request=http::Request::from_parts(parts,axum::body::Body::new(body));<Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut app,request).await}});
                   let _=hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(tls),service).await;
                  };tokio::select!{_ = connection=>{},_ = closed.changed()=>{}}});
                 },
                 Some(_) = connections.join_next(), if !connections.is_empty()=>{}
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        Ok(Self {
            stop: Some(tx),
            task: Some(task),
        })
    }
    pub async fn stop(mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), task).await;
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}
