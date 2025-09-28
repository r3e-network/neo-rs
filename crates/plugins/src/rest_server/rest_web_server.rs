// Copyright (C) 2015-2025 The Neo Project.
//
// rest_web_server.rs ports Neo.Plugins.RestServer.RestWebServer.cs to Rust.
// It leverages warp as the HTTP server while mirroring the structure and
// behaviour of the original ASP.NET Core pipeline for the implemented routes.

use crate::rest_server::controllers::v1::node_controller::NodeController;
use crate::rest_server::controllers::v1::utils_controller::UtilsController;
use crate::rest_server::models::error::ErrorModel;
use crate::rest_server::providers::black_list_controller_feature_provider::BlackListControllerFeatureProvider;
use crate::rest_server::rest_server_settings::RestServerSettings;
use hyper::StatusCode;
use serde::Serialize;
use serde_json::json;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinError};
use tracing::{info, warn};
use warp::filters::BoxedFilter;
use warp::reply::Response;
use warp::Filter;

static IS_RUNNING: AtomicBool = AtomicBool::new(false);

/// Rust port of the C# RestWebServer class.
pub struct RestWebServer {
    settings: RestServerSettings,
    shutdown: Option<Arc<Notify>>,
    server_task: Option<JoinHandle<()>>,
}

impl RestWebServer {
    /// Creates a new RestWebServer with the currently loaded settings.
    pub fn new() -> Self {
        Self {
            settings: RestServerSettings::current(),
            shutdown: None,
            server_task: None,
        }
    }

    /// Starts the web server (matches C# Start method semantics).
    pub fn start(&mut self) {
        if Self::is_running() {
            return;
        }

        let settings = RestServerSettings::current();
        self.settings = settings.clone();

        if settings.ssl_cert_file.is_some() {
            warn!("SSL certificates are not yet supported in the Rust REST server");
        }

        let address = socket_addr(settings.bind_address, settings.port);
        let routes = Self::build_routes(&settings);
        let shutdown = Arc::new(Notify::new());
        let shutdown_signal = shutdown.clone();

        info!("Starting REST server on {}", address);
        Self::set_running(true);

        let server_task = tokio::spawn(async move {
            warp::serve(routes)
                .bind_with_graceful_shutdown(address, async move {
                    shutdown_signal.notified().await;
                })
                .await;
        });

        self.shutdown = Some(shutdown);
        self.server_task = Some(server_task);
    }

    /// Stops the web server and waits for the background task to finish.
    pub fn stop(&mut self) {
        if !Self::is_running() {
            return;
        }

        if let Some(shutdown) = self.shutdown.take() {
            shutdown.notify_waiters();
        }

        if let Some(handle) = self.server_task.take() {
            tokio::spawn(async move {
                if let Err(error) = handle.await {
                    log_join_error(error);
                }
            });
        }

        Self::set_running(false);
        info!("REST server stopped");
    }

    /// Returns whether the server is currently running.
    pub fn is_running() -> bool {
        IS_RUNNING.load(Ordering::Relaxed)
    }

    fn set_running(running: bool) {
        IS_RUNNING.store(running, Ordering::Relaxed);
    }

    fn build_routes(_settings: &RestServerSettings) -> BoxedFilter<(Response,)> {
        let mut combined: Option<BoxedFilter<(Response,)>> = None;
        let provider = BlackListControllerFeatureProvider::new();

        if provider.is_controller_allowed("Node") {
            let node_routes = node_routes();
            combined = Some(match combined {
                Some(existing) => existing.or(node_routes).boxed(),
                None => node_routes.boxed(),
            });
        }

        if provider.is_controller_allowed("Utils") {
            let utils_routes = utils_routes();
            combined = Some(match combined {
                Some(existing) => existing.or(utils_routes).boxed(),
                None => utils_routes.boxed(),
            });
        }

        combined.unwrap_or_else(|| fallback_route().boxed())
    }
}

fn node_routes() -> impl Filter<Extract = (Response,), Error = Infallible> + Clone {
    let peers = warp::path!("api" / "v1" / "node" / "peers")
        .and(warp::path::end())
        .map(handle_get_peers);

    let plugins = warp::path!("api" / "v1" / "node" / "plugins")
        .and(warp::path::end())
        .map(handle_get_plugins);

    let settings = warp::path!("api" / "v1" / "node" / "settings")
        .and(warp::path::end())
        .map(handle_get_settings);

    peers.or(plugins).or(settings)
}

fn utils_routes() -> impl Filter<Extract = (Response,), Error = Infallible> + Clone {
    let script_hash_to_address = warp::path!("api" / "v1" / "utils" / String / "address")
        .and(warp::path::end())
        .map(handle_script_hash_to_address);

    let address_to_script_hash = warp::path!("api" / "v1" / "utils" / String / "scripthash")
        .and(warp::path::end())
        .map(handle_address_to_script_hash);

    let validate_address = warp::path!("api" / "v1" / "utils" / String / "validate")
        .and(warp::path::end())
        .map(handle_validate_address);

    script_hash_to_address
        .or(address_to_script_hash)
        .or(validate_address)
}

fn handle_get_peers() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_peers()),
        Err(error) => error_response(error),
    }
}

fn handle_get_plugins() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_plugins()),
        Err(error) => error_response(error),
    }
}

fn handle_get_settings() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_settings()),
        Err(error) => error_response(error),
    }
}

fn handle_script_hash_to_address(hash: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.script_hash_to_wallet_address(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_address_to_script_hash(address: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.wallet_address_to_script_hash(&address)),
        Err(error) => error_response(error),
    }
}

fn handle_validate_address(address: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.validate_address(&address)),
        Err(error) => error_response(error),
    }
}

fn response_from_result<T>(result: Result<T, ErrorModel>) -> Response
where
    T: Serialize,
{
    match result {
        Ok(value) => warp::reply::with_status(warp::reply::json(&value), StatusCode::OK).into_response(),
        Err(error) => error_response(error),
    }
}

fn error_response(error: ErrorModel) -> Response {
    warp::reply::with_status(warp::reply::json(&error), StatusCode::BAD_REQUEST).into_response()
}

fn fallback_route() -> impl Filter<Extract = (Response,), Error = Infallible> + Clone {
    warp::any().map(|| {
        let payload = json!({
            "code": 404,
            "name": "ControllerDisabled",
            "message": "All REST controllers are disabled in RestServerSettings.",
        });
        warp::reply::with_status(warp::reply::json(&payload), StatusCode::NOT_FOUND).into_response()
    })
}


fn socket_addr(address: IpAddr, port: u16) -> SocketAddr {
    SocketAddr::new(address, port)
}

fn log_join_error(error: JoinError) {
    if error.is_cancelled() {
        return;
    }
    warn!("REST server task finished with error: {}", error);
}
