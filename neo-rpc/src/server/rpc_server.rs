use futures::stream;
use hyper::server::accept::from_stream;
use hyper::service::{make_service_fn, service_fn, Service};
use neo_core::{neo_system::NeoSystem, services::RpcService, wallets::Wallet};
use once_cell::sync::Lazy;
use p12::PFX;
use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use prometheus::Counter;
use rustls::server::AllowAnyAuthenticatedClient;
use rustls::{Certificate, PrivateKey, RootCertStore, ServerConfig};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{oneshot, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::sleep,
};
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use super::rcp_server_settings::RpcServerConfig;
use super::routes::{build_rpc_routes, build_ws_route, BasicAuth};
use super::session::Session;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use sha1::{Digest, Sha1};
use socket2::{SockRef, TcpKeepalive};
use warp::filters::compression;
use warp::Filter;

pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

/// Type alias for wallet change callback to reduce complexity.
pub type WalletChangeCallback = Arc<dyn Fn(Option<Arc<dyn Wallet>>) + Send + Sync>;

pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: Arc<RpcCallback>,
}

impl RpcHandler {
    pub fn new(descriptor: RpcMethodDescriptor, callback: Arc<RpcCallback>) -> Self {
        Self {
            descriptor,
            callback,
        }
    }

    #[must_use]
    pub const fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn callback(&self) -> Arc<RpcCallback> {
        Arc::clone(&self.callback)
    }
}

pub static RPC_REQ_TOTAL: Lazy<Counter> = Lazy::new(|| {
    let counter =
        Counter::new("neo_rpc_requests_total", "Total RPC requests").unwrap_or_else(|_| {
            Counter::new("neo_rpc_requests_total_invalid", "Invalid")
                .expect("fallback counter creation should never fail")
        });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_requests_total: {}", err);
    }
    counter
});
pub static RPC_ERR_TOTAL: Lazy<Counter> = Lazy::new(|| {
    let counter = Counter::new("neo_rpc_errors_total", "Total RPC errors").unwrap_or_else(|_| {
        Counter::new("neo_rpc_errors_total_invalid", "Invalid")
            .expect("fallback counter creation should never fail")
    });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_errors_total: {}", err);
    }
    counter
});

pub struct RpcServer {
    system: Arc<NeoSystem>,
    settings: RpcServerConfig,
    handler_lookup: Arc<RwLock<HashMap<String, Arc<RpcHandler>>>>,
    started: bool,
    wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    wallet_change_callback: Option<WalletChangeCallback>,
    /// Session storage using Mutex instead of `RwLock` to enforce exclusive access.
    ///
    /// # Security Note
    /// Sessions contain `ApplicationEngine` which wraps `ExecutionEngine` with a raw pointer
    /// that is NOT thread-safe. Using `Mutex` instead of `RwLock` prevents accidental
    /// concurrent reads that would cause undefined behavior. See session.rs for details.
    sessions: Arc<Mutex<HashMap<Uuid, Session>>>,
    server_task: Option<JoinHandle<()>>,
    shutdown_signal: Option<oneshot::Sender<()>>,
    session_purge_task: Option<JoinHandle<()>>,
    session_purge_shutdown: Option<oneshot::Sender<()>>,
    self_handle: Option<Weak<RwLock<Self>>>,
    /// WebSocket event bridge for real-time subscriptions
    ws_bridge: Option<Arc<super::ws::WsEventBridge>>,
    /// WebSocket subscription manager
    ws_subscription_mgr: Option<Arc<super::ws::SubscriptionManager>>,
}

impl RpcServer {
    pub fn new(system: Arc<NeoSystem>, settings: RpcServerConfig) -> Self {
        Self {
            system,
            settings,
            handler_lookup: Arc::new(RwLock::new(HashMap::new())),
            started: false,
            wallet: Arc::new(RwLock::new(None)),
            wallet_change_callback: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            server_task: None,
            shutdown_signal: None,
            session_purge_task: None,
            session_purge_shutdown: None,
            self_handle: None,
            ws_bridge: None,
            ws_subscription_mgr: None,
        }
    }

    #[must_use]
    pub const fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.settings = settings;
    }

    #[must_use]
    pub fn system(&self) -> Arc<NeoSystem> {
        Arc::clone(&self.system)
    }

    /// Enable WebSocket subscriptions
    ///
    /// Creates and returns an event bridge that can be used to push events
    /// to connected WebSocket clients. Call this before `start_rpc_server`.
    ///
    /// # Arguments
    /// * `capacity` - Buffer capacity for the broadcast channel
    ///
    /// # Returns
    /// The event bridge that should be used to publish events
    pub fn enable_websocket(&mut self, capacity: usize) -> Arc<super::ws::WsEventBridge> {
        let bridge = Arc::new(super::ws::WsEventBridge::new(capacity));
        let subscription_mgr = Arc::new(super::ws::SubscriptionManager::new());
        self.ws_bridge = Some(Arc::clone(&bridge));
        self.ws_subscription_mgr = Some(subscription_mgr);
        bridge
    }

    /// Get the WebSocket event bridge if enabled
    #[must_use]
    pub fn ws_bridge(&self) -> Option<Arc<super::ws::WsEventBridge>> {
        self.ws_bridge.clone()
    }

    /// Check if WebSocket is enabled
    #[must_use]
    pub const fn is_websocket_enabled(&self) -> bool {
        self.ws_bridge.is_some()
    }

    pub fn start_rpc_server(&mut self, handle: Weak<RwLock<Self>>) {
        if self.started {
            return;
        }

        self.self_handle = Some(handle.clone());

        let tls_config = match build_tls_config(&self.settings) {
            Ok(config) => config,
            Err(err) => {
                error!("RPC TLS configuration error: {}", err);
                return;
            }
        };
        let tls_enabled = tls_config.is_some();

        // Security warning for production deployments without TLS
        let has_auth = !self.settings.rpc_user.trim().is_empty();
        let is_localhost = self.settings.bind_address.is_loopback();
        if has_auth && !is_localhost && !tls_enabled {
            warn!(
                "SECURITY WARNING: RPC server is binding to non-localhost address ({}) with \
                authentication enabled but WITHOUT TLS encryption. Credentials will be \
                transmitted in plaintext. For production use, either:\n\
                  1. Place the RPC server behind a TLS-terminating reverse proxy (nginx, caddy)\n\
                  2. Bind only to localhost (127.0.0.1) and use SSH tunneling\n\
                  3. Use a VPN for network-level encryption",
                self.settings.bind_address
            );
        }
        let disabled_methods: Arc<HashSet<String>> = Arc::new(
            self.settings
                .disabled_methods
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
        );
        let auth = Arc::new(BasicAuth::from_settings(&self.settings));
        let connection_limiter = Arc::new(Semaphore::new(
            self.settings.max_concurrent_connections.max(1),
        ));
        let address = SocketAddr::new(self.settings.bind_address, self.settings.port);

        let rpc_routes = build_rpc_routes(
            handle.clone(),
            disabled_methods,
            auth,
            self.settings.clone(),
        )
        .with(compression::gzip())
        .map(warp::reply::Reply::into_response);

        // Combine RPC routes with WebSocket route if enabled
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let routes = if let (Some(bridge), Some(subscription_mgr)) =
            (&self.ws_bridge, &self.ws_subscription_mgr)
        {
            info!("WebSocket subscriptions enabled at /ws");
            let ws_route = build_ws_route(bridge.sender(), Arc::clone(subscription_mgr));
            rpc_routes.or(ws_route).unify().boxed()
        } else {
            rpc_routes.boxed()
        };
        let svc = warp::service(routes);
        let (task, bound_addr) = if let Some(tls_config) = tls_config {
            if self.settings.trusted_authorities.is_empty() {
                info!("RPC TLS enabled (client certificates optional)");
            } else {
                info!(
                    "RPC TLS enabled with {} trusted authorities",
                    self.settings.trusted_authorities.len()
                );
            }

            let std_listener = match std::net::TcpListener::bind(address) {
                Ok(listener) => listener,
                Err(err) => {
                    error!("error binding RPC TLS server to {}: {}", address, err);
                    return;
                }
            };
            let bound_addr = match std_listener.local_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    error!("error getting RPC TLS bound address: {}", err);
                    return;
                }
            };
            if let Err(err) = std_listener.set_nonblocking(true) {
                error!("error configuring RPC TLS listener: {}", err);
                return;
            }
            let listener = match TcpListener::from_std(std_listener) {
                Ok(listener) => listener,
                Err(err) => {
                    error!("error initializing RPC TLS listener: {}", err);
                    return;
                }
            };

            let tls_acceptor = TlsAcceptor::from(tls_config);
            let keepalive = self.settings.keep_alive_timeout_duration();
            let incoming = stream::unfold(listener, move |listener| {
                let tls_acceptor = tls_acceptor.clone();
                let connection_limiter = connection_limiter.clone();
                async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, remote_addr)) => {
                                apply_tcp_keepalive(&stream, keepalive);
                                let permit = if let Ok(permit) =
                                    connection_limiter.clone().try_acquire_owned()
                                {
                                    permit
                                } else {
                                    debug!(
                                        "RPC max concurrent connections reached; dropping {}",
                                        remote_addr
                                    );
                                    continue;
                                };
                                match tls_acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let conn = TlsConnection {
                                            stream: tls_stream,
                                            remote_addr,
                                            _permit: permit,
                                        };
                                        return Some((
                                            Ok::<TlsConnection, io::Error>(conn),
                                            listener,
                                        ));
                                    }
                                    Err(err) => {
                                        warn!(
                                            "RPC TLS handshake failed for {}: {}",
                                            remote_addr, err
                                        );
                                        continue;
                                    }
                                }
                            }
                            Err(err) => {
                                error!("RPC TLS accept error: {}", err);
                                sleep(Duration::from_millis(250)).await;
                                continue;
                            }
                        }
                    }
                }
            });
            let incoming = from_stream(incoming);
            let svc = svc;
            let make_svc = make_service_fn(move |conn: &TlsConnection| {
                let remote_addr = conn.remote_addr();
                let svc = svc.clone();

                async move {
                    Ok::<_, Infallible>(service_fn(move |mut req: hyper::Request<hyper::Body>| {
                        req.extensions_mut().insert(remote_addr);
                        let mut svc = svc.clone();
                        async move { svc.call(req).await }
                    }))
                }
            });

            let mut builder = hyper::Server::builder(incoming);
            if self.settings.request_headers_timeout > 0 {
                builder = builder
                    .http1_header_read_timeout(self.settings.request_headers_timeout_duration());
            }

            let server = builder.serve(make_svc);
            let server = server.with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
            let task = tokio::spawn(async move {
                if let Err(err) = server.await {
                    error!("RPC server error: {}", err);
                }
            });
            (task, bound_addr)
        } else {
            let svc = svc;
            let std_listener = match std::net::TcpListener::bind(address) {
                Ok(listener) => listener,
                Err(err) => {
                    error!("error binding RPC server to {}: {}", address, err);
                    return;
                }
            };
            let bound_addr = match std_listener.local_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    error!("error getting RPC bound address: {}", err);
                    return;
                }
            };
            if let Err(err) = std_listener.set_nonblocking(true) {
                error!("error configuring RPC listener: {}", err);
                return;
            }
            let listener = match TcpListener::from_std(std_listener) {
                Ok(listener) => listener,
                Err(err) => {
                    error!("error initializing RPC listener: {}", err);
                    return;
                }
            };

            let keepalive = self.settings.keep_alive_timeout_duration();
            let incoming = stream::unfold(listener, move |listener| {
                let connection_limiter = connection_limiter.clone();
                async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, remote_addr)) => {
                                apply_tcp_keepalive(&stream, keepalive);
                                let permit = if let Ok(permit) =
                                    connection_limiter.clone().try_acquire_owned()
                                {
                                    permit
                                } else {
                                    debug!(
                                        "RPC max concurrent connections reached; dropping {}",
                                        remote_addr
                                    );
                                    continue;
                                };
                                let conn = PlainConnection {
                                    stream,
                                    remote_addr,
                                    _permit: permit,
                                };
                                return Some((Ok::<PlainConnection, io::Error>(conn), listener));
                            }
                            Err(err) => {
                                error!("RPC accept error: {}", err);
                                sleep(Duration::from_millis(250)).await;
                                continue;
                            }
                        }
                    }
                }
            });
            let incoming = from_stream(incoming);
            let make_svc = make_service_fn(move |conn: &PlainConnection| {
                let remote_addr = conn.remote_addr();
                let svc = svc.clone();

                async move {
                    Ok::<_, Infallible>(service_fn(move |mut req: hyper::Request<hyper::Body>| {
                        req.extensions_mut().insert(remote_addr);
                        let mut svc = svc.clone();
                        async move { svc.call(req).await }
                    }))
                }
            });

            let mut builder = hyper::Server::builder(incoming);
            if self.settings.request_headers_timeout > 0 {
                builder = builder
                    .http1_header_read_timeout(self.settings.request_headers_timeout_duration());
            }

            let server = builder.serve(make_svc);
            let server = server.with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
            let task = tokio::spawn(async move {
                if let Err(err) = server.await {
                    error!("RPC server error: {}", err);
                }
            });
            (task, bound_addr)
        };

        info!("RPC server bound on {}", bound_addr);

        self.shutdown_signal = Some(shutdown_tx);
        self.server_task = Some(task);
        self.started = true;

        // Background purge of expired sessions to avoid leaks when clients drop without cleanup.
        if self.session_enabled() {
            let interval_secs = self.settings.session_expiration_time.max(1) / 2;
            let interval = Duration::from_secs(interval_secs.max(5));
            let (purge_tx, mut purge_rx) = oneshot::channel();
            let purge_handle = handle;
            let purge_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        () = sleep(interval) => {
                            if let Some(server_arc) = purge_handle.upgrade() {
                                if let Some(server) = server_arc.try_read() {
                                    server.purge_expired_sessions();
                                }
                            } else {
                                break;
                            }
                        }
                        _ = &mut purge_rx => break,
                    }
                }
            });
            self.session_purge_shutdown = Some(purge_tx);
            self.session_purge_task = Some(purge_task);
        }
        info!(
            "Starting RPC server on {}:{} (network {})",
            self.settings.bind_address, self.settings.port, self.settings.network
        );
    }

    pub fn stop_rpc_server(&mut self) {
        if !self.started {
            return;
        }

        if let Some(tx) = self.shutdown_signal.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.server_task.take() {
            tokio::spawn(async move {
                if let Err(err) = handle.await {
                    log_join_error(err);
                }
            });
        }

        if let Some(tx) = self.session_purge_shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.session_purge_task.take() {
            tokio::spawn(async move {
                let _ = handle.await;
            });
        }

        // Drop any lingering sessions to avoid carrying over state across restarts.
        self.sessions.lock().clear();
        self.set_wallet(None);

        info!("Stopping RPC server for network {}", self.settings.network);
        self.started = false;
    }

    pub fn register_method(&mut self, handler: RpcHandler) {
        let key = handler.descriptor().name.to_ascii_lowercase();
        self.handler_lookup.write().insert(key, Arc::new(handler));
    }

    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_method(handler);
        }
    }

    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handler_lookup.write().clear();
        self.set_wallet(None);
        self.sessions.lock().clear();
    }

    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.wallet.write() = wallet;
        if let Some(callback) = &self.wallet_change_callback {
            callback(self.wallet.read().clone());
        }
    }

    #[must_use]
    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    pub fn set_wallet_change_callback(&mut self, callback: Option<WalletChangeCallback>) {
        self.wallet_change_callback = callback;
    }

    const fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    #[must_use]
    pub const fn session_enabled(&self) -> bool {
        self.settings.session_enabled
    }

    pub fn purge_expired_sessions(&self) {
        if !self.session_enabled() {
            return;
        }
        let expiration = self.session_expiration();
        let mut guard = self.sessions.lock();
        guard.retain(|_, session| !session.is_expired(expiration));
    }

    pub fn store_session(&self, session: Session) -> Uuid {
        let id = Uuid::new_v4();
        self.sessions.lock().insert(id, session);
        id
    }

    pub fn with_session_mut<F, R>(&self, id: &Uuid, func: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self.sessions.lock();
        guard.get_mut(id).map(func)
    }

    #[must_use]
    pub fn terminate_session(&self, id: &Uuid) -> bool {
        self.sessions.lock().remove(id).is_some()
    }

    pub(crate) fn handlers_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Arc<RpcHandler>>> {
        self.handler_lookup.read()
    }
}

impl RpcService for RpcServer {
    fn is_started(&self) -> bool {
        self.started
    }
}

struct TlsConnection {
    stream: tokio_rustls::server::TlsStream<TcpStream>,
    remote_addr: SocketAddr,
    _permit: OwnedSemaphorePermit,
}

impl TlsConnection {
    const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl AsyncRead for TlsConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

struct PlainConnection {
    stream: TcpStream,
    remote_addr: SocketAddr,
    _permit: OwnedSemaphorePermit,
}

impl PlainConnection {
    const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl AsyncRead for PlainConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for PlainConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

fn build_tls_config(settings: &RpcServerConfig) -> Result<Option<Arc<ServerConfig>>, String> {
    let cert_path = settings.ssl_cert.trim();
    if cert_path.is_empty() {
        if !settings.ssl_cert_password.is_empty() || !settings.trusted_authorities.is_empty() {
            warn!(
                "RPC TLS settings provided without SslCert; TLS remains disabled (network {}).",
                settings.network
            );
        }
        return Ok(None);
    }

    let cert_bytes = std::fs::read(cert_path)
        .map_err(|err| format!("failed to read TLS certificate {cert_path}: {err}"))?;
    let pfx =
        PFX::parse(&cert_bytes).map_err(|err| format!("invalid PKCS#12 {cert_path}: {err:?}"))?;
    if !pfx.verify_mac(settings.ssl_cert_password.as_str()) {
        return Err(format!("invalid TLS certificate password for {cert_path}"));
    }

    let certs_der = pfx
        .cert_x509_bags(settings.ssl_cert_password.as_str())
        .map_err(|err| format!("failed to read TLS certificate chain from {cert_path}: {err:?}"))?;
    if certs_der.is_empty() {
        return Err(format!("no TLS certificates found in {cert_path}"));
    }
    let certs = certs_der.into_iter().map(Certificate).collect::<Vec<_>>();

    let mut keys = pfx
        .key_bags(settings.ssl_cert_password.as_str())
        .map_err(|err| format!("failed to read TLS private key from {cert_path}: {err:?}"))?;
    let key_der = keys
        .pop()
        .ok_or_else(|| format!("no TLS private key found in {cert_path}"))?;
    let key = PrivateKey(key_der);

    let builder = ServerConfig::builder().with_safe_defaults();
    let builder = if settings.trusted_authorities.is_empty() {
        builder.with_no_client_auth()
    } else {
        let roots = load_trusted_authorities(&settings.trusted_authorities)?;
        builder.with_client_cert_verifier(Arc::new(AllowAnyAuthenticatedClient::new(roots)))
    };
    let config = builder
        .with_single_cert(certs, key)
        .map_err(|err| format!("failed to configure TLS for {cert_path}: {err}"))?;

    Ok(Some(Arc::new(config)))
}

fn load_trusted_authorities(thumbprints: &[String]) -> Result<RootCertStore, String> {
    let allowed: HashSet<String> = thumbprints
        .iter()
        .map(|value| normalize_thumbprint(value))
        .filter(|value| !value.is_empty())
        .collect();
    let native_certs = rustls_native_certs::load_native_certs()
        .map_err(|err| format!("failed to load native TLS roots: {err:?}"))?;

    let mut roots = RootCertStore::empty();
    let mut matched = 0usize;
    for cert in native_certs {
        let cert_der = cert.0;
        let thumbprint = thumbprint_hex(&cert_der);
        if allowed.contains(&thumbprint) {
            let rustls_cert = Certificate(cert_der);
            roots
                .add(&rustls_cert)
                .map_err(|err| format!("failed to add trusted authority {thumbprint}: {err}"))?;
            matched += 1;
        }
    }

    if matched == 0 {
        warn!("RPC TLS configured with TrustedAuthorities, but no matching roots were found.");
    }

    Ok(roots)
}

fn thumbprint_hex(cert_der: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(cert_der);
    let digest = hasher.finalize();
    hex::encode_upper(digest)
}

fn normalize_thumbprint(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .replace(':', "")
        .to_ascii_uppercase()
}

fn apply_tcp_keepalive(stream: &TcpStream, keepalive: Option<Duration>) {
    let Some(keepalive) = keepalive else {
        return;
    };
    let sock_ref = SockRef::from(stream);
    let config = TcpKeepalive::new().with_time(keepalive);
    if let Err(err) = sock_ref.set_tcp_keepalive(&config) {
        warn!("error setting TCP keepalive: {}", err);
    }
}

fn log_join_error(error: tokio::task::JoinError) {
    if error.is_cancelled() {
        warn!(target: "neo", "rpc server task cancelled before completion");
    } else {
        match error.try_into_panic() {
            Ok(payload) => {
                if let Some(message) = payload.downcast_ref::<&str>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else {
                    error!(target: "neo", "rpc server panicked");
                }
            }
            Err(join_err) => {
                error!(target: "neo", error = %join_err, "rpc server task failed");
            }
        }
    }
}

pub static SERVERS: Lazy<RwLock<HashMap<u32, Arc<RwLock<RpcServer>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn remove_server(network: u32) {
    if SERVERS.write().remove(&network).is_some() {
        info!("Removed RPC server for network {}", network);
    }
}

pub fn register_server(network: u32, server: Arc<RwLock<RpcServer>>) {
    let mut guard = SERVERS.write();
    if let Some(previous) = guard.insert(network, Arc::clone(&server)) {
        warn!(
            "Replacing existing RPC server instance for network {}",
            network
        );
        if let Some(mut previous_guard) = previous.try_write() {
            previous_guard.dispose();
        }
    }
}

pub fn get_server(network: u32) -> Option<Arc<RwLock<RpcServer>>> {
    SERVERS.read().get(&network).cloned()
}
