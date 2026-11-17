//! mTLS HTTPS transport for the reference signer service.
//!
//! The listener is TLS-only (plaintext connections fail the handshake) and
//! requires a client certificate issued by the configured CA, mirroring the
//! node client's requirement of credential-free HTTPS endpoints with dedicated
//! mTLS material.
//!
//! The transport is also bounded, because an unauthenticated peer can still
//! open sockets:
//!
//! * at most [`DEFAULT_MAX_CONCURRENT_CONNECTIONS`] connections are admitted at
//!   a time (a [`tokio::sync::Semaphore`] gates admission before the handshake),
//! * a TLS handshake must complete within [`DEFAULT_TLS_HANDSHAKE_TIMEOUT`],
//! * a request head must arrive within [`DEFAULT_REQUEST_READ_TIMEOUT`]
//!   (hyper's HTTP/1 header read timeout), and the body within the same
//!   deadline,
//! * a body larger than [`DEFAULT_MAX_REQUEST_BODY_BYTES`] is refused with
//!   `413` - by `Content-Length` before the first byte is read, and again by
//!   the streaming cap for chunked bodies.
//!
//! After the handshake the peer's certificate common name is extracted and
//! passed with every request; endpoints reject identities that are not scoped
//! to the requested key and purpose with `403`.

use crate::config::{ClientAuthorizationPolicy, SignerServerConfig, TlsPaths};
use crate::identity::ClientIdentity;
use crate::protocol::{SignOutcome, SignerService};
use anyhow::{Context, Result, bail};
use hyper::body::{Body, HttpBody};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, warn};

/// Maximum number of connections the listener serves concurrently.
pub const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// Deadline for the TLS handshake of one connection.
pub const DEFAULT_TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Deadline for receiving a request (head and body).
pub const DEFAULT_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum accepted request body size (1 MiB).
pub const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

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

/// Starts the signer server on an ephemeral or configured address, using the
/// client-authorization policy discovered from the environment or the
/// sealed-data directory.
///
/// Returns as soon as the listener is bound; per-connection work runs on
/// spawned tasks.
pub async fn spawn_signer_server(config: SignerServerConfig) -> Result<SignerServerHandle> {
    let authorization = ClientAuthorizationPolicy::load(&config)
        .context("failed to load the client-authorization policy")?;
    spawn_signer_server_with(config, authorization).await
}

/// Starts the signer server with an explicit client-identity policy.
pub async fn spawn_signer_server_with(
    config: SignerServerConfig,
    authorization: ClientAuthorizationPolicy,
) -> Result<SignerServerHandle> {
    let bind_addr = config.bind_addr;
    let tls = build_tls_server_config(&config.tls)?;
    let service = Arc::new(
        SignerService::with_client_authorization(config, authorization)
            .context("failed to build signer service")?,
    );
    let service_handle = Arc::clone(&service);

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind the signer listener on {bind_addr}"))?;
    let local_addr = listener.local_addr()?;

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let tls_acceptor = TlsAcceptor::from(Arc::new(tls));
    // Admission control: permits are acquired before accepting, so the accept
    // loop - and therefore the number of live connections and tasks - is
    // capped no matter how many peers connect.
    let connections = Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT_CONNECTIONS));

    let handle = tokio::spawn(async move {
        loop {
            let permit = tokio::select! {
                _ = shutdown_rx.changed() => break,
                permit = Arc::clone(&connections).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_) => break,
                },
            };
            let accepted = tokio::select! {
                _ = shutdown_rx.changed() => break,
                accepted = listener.accept() => accepted,
            };
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
                // The permit lives as long as the connection.
                let _permit = permit;
                let tls_stream = match tokio::time::timeout(
                    DEFAULT_TLS_HANDSHAKE_TIMEOUT,
                    acceptor.accept(socket),
                )
                .await
                {
                    Ok(Ok(stream)) => stream,
                    Ok(Err(error)) => {
                        // Non-TLS or certificate-less clients die here: the
                        // server refuses to speak anything but authenticated
                        // TLS.
                        warn!("rejected TLS handshake from {peer}: {error}");
                        return;
                    }
                    Err(_) => {
                        warn!(
                            "TLS handshake from {peer} exceeded {DEFAULT_TLS_HANDSHAKE_TIMEOUT:?}"
                        );
                        return;
                    }
                };
                let identity = peer_identity(&tls_stream);
                match &identity {
                    Some(identity) => {
                        debug!("accepted mTLS client '{}' from {peer}", identity.common_name());
                    }
                    None => warn!(
                        "accepted mTLS peer {peer} without a subject common name; \
                         every request from it will be refused with 403"
                    ),
                }
                let mut http = hyper::server::conn::Http::new();
                // Slow-loris bound: the whole request head must arrive inside
                // the read window.
                http.http1_header_read_timeout(DEFAULT_REQUEST_READ_TIMEOUT);
                let handler = service_fn(move |request| {
                    handle_request(service.clone(), identity.clone(), request)
                });
                if let Err(error) = http.serve_connection(tls_stream, handler).await {
                    warn!("signer connection failed from {peer}: {error}");
                }
            });
        }
    });

    Ok(SignerServerHandle {
        local_addr,
        service: service_handle,
        shutdown_tx,
        handle,
    })
}

/// Extracts the authenticated client identity from a completed TLS handshake.
fn peer_identity(tls_stream: &tokio_rustls::server::TlsStream<tokio::net::TcpStream>) -> Option<ClientIdentity> {
    let (_socket, connection) = tls_stream.get_ref();
    let certificate = connection.peer_certificates()?.first()?;
    ClientIdentity::from_certificate_der(&certificate.0)
}

async fn handle_request(
    service: Arc<SignerService>,
    identity: Option<ClientIdentity>,
    request: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    Ok(route(service, identity, request).await)
}

async fn route(
    service: Arc<SignerService>,
    identity: Option<ClientIdentity>,
    request: Request<Body>,
) -> Response<Body> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if method != Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "only POST is supported");
    }
    let body_bytes = match read_body(request).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    match path.as_str() {
        "/v1/signer/key-info" => {
            match serde_json::from_slice::<neo_wallets::signing::KeyRef>(&body_bytes) {
                Ok(key_ref) => {
                    if let Err(error) = service.authorize(identity.as_ref(), &key_ref, None) {
                        return forbidden(error);
                    }
                    match service.key_info(
                        identity.as_ref().expect("authorized requests carry an identity"),
                        &key_ref,
                    ) {
                        Ok(response) => json_response(StatusCode::OK, &response),
                        Err(error) => error_response(StatusCode::NOT_FOUND, error.to_string()),
                    }
                }
                Err(error) => error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid key-info request: {error}"),
                ),
            }
        }
        "/v1/signer/attest" => {
            match serde_json::from_slice::<neo_wallets::signing::AttestationChallenge>(&body_bytes)
            {
                Ok(challenge) => {
                    if let Err(error) = service.authorize(identity.as_ref(), &challenge.key_ref, None)
                    {
                        return forbidden(error);
                    }
                    match service
                        .attest(
                            identity.as_ref().expect("authorized requests carry an identity"),
                            &challenge,
                        )
                        .await
                    {
                        Ok(evidence) => json_response(StatusCode::OK, &evidence),
                        Err(error) => error_response(StatusCode::BAD_REQUEST, error.to_string()),
                    }
                }
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
            if let Err(error) = service.authorize(
                identity.as_ref(),
                &sign_request.key_ref,
                Some(&sign_request.purpose),
            ) {
                return forbidden(error);
            }
            match service.sign(
                identity.as_ref().expect("authorized requests carry an identity"),
                &sign_request,
            ) {
                SignOutcome::Succeeded(response) => json_response(StatusCode::OK, &response),
                SignOutcome::Rejected(rejection) => {
                    json_response(StatusCode::FORBIDDEN, &rejection)
                }
                SignOutcome::Unauthorized(reason) => {
                    error_response(StatusCode::FORBIDDEN, reason)
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
            if let Err(error) = service.authorize(identity.as_ref(), &query.key_ref, None) {
                return forbidden(error);
            }
            match service.query(
                identity.as_ref().expect("authorized requests carry an identity"),
                &query,
            ) {
                Ok(status) => json_response(StatusCode::OK, &status),
                Err(error) => error_response(StatusCode::BAD_REQUEST, error.to_string()),
            }
        }
        _ => error_response(StatusCode::NOT_FOUND, "unknown signer route"),
    }
}

/// Reads a request body with a hard size cap and a read deadline.
///
/// `Content-Length` is checked before the first byte of the body is consumed;
/// the streaming cap catches chunked bodies that declare no length.
async fn read_body(request: Request<Body>) -> Result<Vec<u8>, Response<Body>> {
    if let Some(length) = content_length(&request)
        && length > DEFAULT_MAX_REQUEST_BODY_BYTES as u64
    {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "request body of {length} bytes exceeds the {DEFAULT_MAX_REQUEST_BODY_BYTES} byte limit"
            ),
        ));
    }

    let mut body = request.into_body();
    let read = async {
        let mut collected: Vec<u8> = Vec::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.map_err(|error| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    format!("failed to read request body: {error}"),
                )
            })?;
            if collected.len() + chunk.len() > DEFAULT_MAX_REQUEST_BODY_BYTES {
                return Err(error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!(
                        "request body exceeds the {DEFAULT_MAX_REQUEST_BODY_BYTES} byte limit"
                    ),
                ));
            }
            collected.extend_from_slice(&chunk);
        }
        Ok(collected)
    };

    match tokio::time::timeout(DEFAULT_REQUEST_READ_TIMEOUT, read).await {
        Ok(result) => result,
        Err(_) => Err(error_response(
            StatusCode::REQUEST_TIMEOUT,
            format!(
                "request body was not received within {DEFAULT_REQUEST_READ_TIMEOUT:?}"
            ),
        )),
    }
}

/// Parsed `Content-Length`, when the client declared one.
fn content_length(request: &Request<Body>) -> Option<u64> {
    request
        .headers()
        .get(CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
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

/// `403 Forbidden` for a client identity that is not scoped to the requested
/// key or purpose.
fn forbidden(message: impl std::fmt::Display) -> Response<Body> {
    error_response(StatusCode::FORBIDDEN, message.to_string())
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
