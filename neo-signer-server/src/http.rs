//! mTLS HTTPS transport for the reference signer service.
//!
//! The listener is TLS-only (plaintext connections fail the handshake) and
//! requires a client certificate issued by the configured CA, mirroring the
//! node client's requirement of credential-free HTTPS endpoints with dedicated
//! mTLS material.

use crate::config::{SignerServerConfig, TlsPaths};
use crate::protocol::{SignOutcome, SignerService};
use anyhow::{Context, Result, bail};
use hyper::body;
use hyper::header::CONTENT_TYPE;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::warn;

/// Handle to a running signer server.
pub struct SignerServerHandle {
    local_addr: SocketAddr,
    service: Arc<SignerService>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

impl SignerServerHandle {
    /// The address the HTTPS listener is bound to.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The service instance backing this server (used by tests to inspect
    /// signing counters).
    #[must_use]
    pub fn service(&self) -> Arc<SignerService> {
        self.service.clone()
    }

    /// Stops the listener and waits for the accept loop to finish.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.handle.await;
    }
}

/// Starts the signer server on an ephemeral or configured address.
///
/// Returns as soon as the listener is bound; per-connection work runs on
/// spawned tasks.
pub async fn spawn_signer_server(config: SignerServerConfig) -> Result<SignerServerHandle> {
    let bind_addr = config.bind_addr;
    let tls = build_tls_server_config(&config.tls)?;
    let service = Arc::new(SignerService::new(config).context("failed to build signer service")?);
    let service_handle = Arc::clone(&service);

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind the signer listener on {bind_addr}"))?;
    let local_addr = listener.local_addr()?;

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let tls_acceptor = TlsAcceptor::from(Arc::new(tls));

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                accepted = listener.accept() => {
                    let (socket, peer) = match accepted {
                        Ok(pair) => pair,
                        Err(error) => {
                            warn!("signer accept failed: {error}");
                            continue;
                        }
                    };
                    let acceptor = tls_acceptor.clone();
                    let service = service.clone();
                    tokio::spawn(async move {
                        match acceptor.accept(socket).await {
                            Ok(tls_stream) => {
                                let http = hyper::server::conn::Http::new();
                                let handler = service_fn(move |request| {
                                    handle_request(service.clone(), request)
                                });
                                if let Err(error) = http.serve_connection(tls_stream, handler).await {
                                    warn!("signer connection failed from {peer}: {error}");
                                }
                            }
                            Err(error) => {
                                // Non-TLS or certificate-less clients die here:
                                // the server refuses to speak anything but
                                // authenticated TLS.
                                warn!("rejected TLS handshake from {peer}: {error}");
                            }
                        }
                    });
                }
            }
        }
    });

    Ok(SignerServerHandle {
        local_addr,
        service: service_handle,
        shutdown_tx,
        handle,
    })
}

async fn handle_request(
    service: Arc<SignerService>,
    request: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    Ok(route(service, request).await)
}

async fn route(service: Arc<SignerService>, request: Request<Body>) -> Response<Body> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let body_bytes = match body::to_bytes(request.into_body()).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("failed to read request body: {error}"),
            );
        }
    };
    if method != Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "only POST is supported");
    }
    match path.as_str() {
        "/v1/signer/key-info" => {
            match serde_json::from_slice::<neo_wallets::signing::KeyRef>(&body_bytes) {
                Ok(key_ref) => match service.key_info(&key_ref) {
                    Ok(response) => json_response(StatusCode::OK, &response),
                    Err(error) => error_response(StatusCode::NOT_FOUND, error.to_string()),
                },
                Err(error) => error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid key-info request: {error}"),
                ),
            }
        }
        "/v1/signer/attest" => {
            match serde_json::from_slice::<neo_wallets::signing::AttestationChallenge>(&body_bytes)
            {
                Ok(challenge) => match service.attest(&challenge).await {
                    Ok(evidence) => json_response(StatusCode::OK, &evidence),
                    Err(error) => error_response(StatusCode::BAD_REQUEST, error.to_string()),
                },
                Err(error) => error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid attest request: {error}"),
                ),
            }
        }
        "/v1/signer/sign" => {
            let Ok(sign_request) =
                serde_json::from_slice::<neo_wallets::signing::RemoteSignRequest>(&body_bytes)
            else {
                return error_response(StatusCode::BAD_REQUEST, "invalid sign request");
            };
            match service.sign(&sign_request) {
                SignOutcome::Succeeded(response) => json_response(StatusCode::OK, &response),
                SignOutcome::Rejected(rejection) => {
                    json_response(StatusCode::FORBIDDEN, &rejection)
                }
                // Ambiguous dispatch: the node classifies 5xx after dispatch
                // as ResultUnknown and recovers through the query endpoint.
                SignOutcome::Ambiguous(reason) => {
                    error_response(StatusCode::SERVICE_UNAVAILABLE, reason)
                }
            }
        }
        "/v1/signer/query" => {
            let Ok(query) =
                serde_json::from_slice::<neo_wallets::signing::RemoteOperationQuery>(&body_bytes)
            else {
                return error_response(StatusCode::BAD_REQUEST, "invalid query request");
            };
            match service.query(&query) {
                Ok(status) => json_response(StatusCode::OK, &status),
                Err(error) => error_response(StatusCode::BAD_REQUEST, error.to_string()),
            }
        }
        _ => error_response(StatusCode::NOT_FOUND, "unknown signer route"),
    }
}

fn json_response<T: serde::Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    let bytes =
        serde_json::to_vec(value).unwrap_or_else(|_| b"{\"error\":\"encoding failed\"}".to_vec());
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    let body = serde_json::json!({ "error": message.into() });
    json_response(status, &body)
}

/// Builds the rustls server configuration requiring client-certificate
/// authentication against the configured CA. The TLS listener never speaks
/// plaintext, so non-TLS connections are refused at the handshake.
fn build_tls_server_config(paths: &TlsPaths) -> Result<rustls::ServerConfig> {
    let server_cert_pem = std::fs::read(&paths.server_certificate_path).with_context(|| {
        format!(
            "failed to read the server certificate {}",
            paths.server_certificate_path.display()
        )
    })?;
    let server_key_pem = std::fs::read(&paths.server_private_key_path).with_context(|| {
        format!(
            "failed to read the server private key {}",
            paths.server_private_key_path.display()
        )
    })?;
    let ca_pem = std::fs::read(&paths.ca_certificate_path).with_context(|| {
        format!(
            "failed to read the client CA certificate {}",
            paths.ca_certificate_path.display()
        )
    })?;

    let server_certs: Vec<_> = rustls_pemfile::certs(&mut server_cert_pem.as_slice())
        .map_err(|_| anyhow::anyhow!("server certificate file is not valid PEM"))?
        .into_iter()
        .map(rustls::Certificate)
        .collect();
    if server_certs.is_empty() {
        bail!("server certificate file contains no certificates");
    }

    // The reference server expects a PKCS#8 private key (the format emitted
    // by rcgen and standard TLS tooling).
    let server_key = rustls_pemfile::pkcs8_private_keys(&mut server_key_pem.as_slice())
        .map_err(|_| anyhow::anyhow!("server private key file is not valid PEM"))?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("server private key file contains no private key"))?;

    let ca_certs = rustls_pemfile::certs(&mut ca_pem.as_slice())
        .map_err(|_| anyhow::anyhow!("client CA certificate file is not valid PEM"))?;
    if ca_certs.is_empty() {
        bail!("client CA certificate file contains no certificates");
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(&ca_certs);

    rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(Arc::new(rustls::server::AllowAnyAuthenticatedClient::new(
            roots,
        )))
        .with_single_cert(server_certs, rustls::PrivateKey(server_key))
        .context("invalid server TLS identity material")
}
