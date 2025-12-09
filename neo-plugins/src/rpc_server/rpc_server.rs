use neo_core::{neo_system::NeoSystem, services::RpcService, wallets::Wallet};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use prometheus::{register_counter, Counter};
use serde_json::Value;
use tokio::{
    sync::{oneshot, Semaphore},
    task::JoinHandle,
    time::sleep,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::rcp_server_settings::RpcServerConfig;
use super::routes::{build_rpc_routes, BasicAuth};
use super::session::Session;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;

pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

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

    pub fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    pub fn callback(&self) -> Arc<RpcCallback> {
        Arc::clone(&self.callback)
    }
}

pub static RPC_REQ_TOTAL: Lazy<Counter> =
    Lazy::new(|| register_counter!("neo_rpc_requests_total", "Total RPC requests").unwrap());
pub static RPC_ERR_TOTAL: Lazy<Counter> =
    Lazy::new(|| register_counter!("neo_rpc_errors_total", "Total RPC errors").unwrap());

pub struct RpcServer {
    system: Arc<NeoSystem>,
    settings: RpcServerConfig,
    handler_lookup: Arc<RwLock<HashMap<String, Arc<RpcHandler>>>>,
    started: bool,
    wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    /// Session storage using Mutex instead of RwLock to enforce exclusive access.
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
    self_handle: Option<Weak<RwLock<RpcServer>>>,
}

impl RpcServer {
    pub fn new(system: Arc<NeoSystem>, settings: RpcServerConfig) -> Self {
        Self {
            system,
            settings,
            handler_lookup: Arc::new(RwLock::new(HashMap::new())),
            started: false,
            wallet: Arc::new(RwLock::new(None)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            server_task: None,
            shutdown_signal: None,
            session_purge_task: None,
            session_purge_shutdown: None,
            self_handle: None,
        }
    }

    pub fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.settings = settings;
    }

    pub fn system(&self) -> Arc<NeoSystem> {
        Arc::clone(&self.system)
    }

    pub fn start_rpc_server(&mut self, handle: Weak<RwLock<RpcServer>>) {
        if self.started {
            return;
        }

        self.self_handle = Some(handle.clone());

        if !self.settings.ssl_cert.is_empty()
            || !self.settings.ssl_cert_password.is_empty()
            || !self.settings.trusted_authorities.is_empty()
        {
            error!(
                "RPC TLS configuration (SslCert/SslCertPassword/TrustedAuthorities) is currently \
                 unsupported. Refusing to start the server to avoid running plaintext while TLS \
                 is expected. Remove TLS settings or place the server behind a TLS terminator."
            );
            return;
        }

        // Security warning for production deployments without TLS
        let has_auth = !self.settings.rpc_user.trim().is_empty();
        let is_localhost = self.settings.bind_address.is_loopback();
        if has_auth && !is_localhost {
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
        if has_auth && self.settings.enable_cors && self.settings.allow_origins.is_empty() {
            error!(
                "RPC CORS wildcard ('*') cannot be used with authentication enabled. Provide \
                 explicit allow_origins or disable CORS."
            );
            return;
        }

        let disabled_methods: Arc<HashSet<String>> = Arc::new(
            self.settings
                .disabled_methods
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
        );
        let auth = Arc::new(BasicAuth::from_settings(&self.settings));
        let semaphore = Arc::new(Semaphore::new(
            self.settings.max_concurrent_connections.max(1),
        ));
        let address = SocketAddr::new(self.settings.bind_address, self.settings.port);

        let routes = build_rpc_routes(
            handle.clone(),
            disabled_methods,
            auth.clone(),
            semaphore,
            self.settings.clone(),
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (bound_addr, server) =
            warp::serve(routes).bind_with_graceful_shutdown(address, async move {
                let _ = shutdown_rx.await;
            });

        info!("RPC server bound on {}", bound_addr);
        let task = tokio::spawn(async move {
            server.await;
        });

        self.shutdown_signal = Some(shutdown_tx);
        self.server_task = Some(task);
        self.started = true;

        // Background purge of expired sessions to avoid leaks when clients drop without cleanup.
        if self.session_enabled() {
            let interval_secs = self.settings.session_expiration_time.max(1) / 2;
            let interval = Duration::from_secs(interval_secs.max(5) as u64);
            let (purge_tx, mut purge_rx) = oneshot::channel();
            let purge_handle = handle.clone();
            let purge_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = sleep(interval) => {
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

    pub fn is_started(&self) -> bool {
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
    }

    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    pub fn session_enabled(&self) -> bool {
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

pub static PENDING_HANDLERS: Lazy<RwLock<HashMap<u32, Vec<RpcHandler>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn remove_server(network: u32) {
    if SERVERS.write().remove(&network).is_some() {
        info!("Removed RPC server for network {}", network);
    }
}

pub fn add_pending_handler(network: u32, handler: RpcHandler) {
    let mut guard = PENDING_HANDLERS.write();
    guard.entry(network).or_default().push(handler);
}

pub fn take_pending_handlers(network: u32) -> Vec<RpcHandler> {
    PENDING_HANDLERS
        .write()
        .remove(&network)
        .unwrap_or_default()
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
