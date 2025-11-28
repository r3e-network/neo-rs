//! Minimal health endpoint for neo-node.
use crate::{metrics, ProtocolSettings};
use neo_core::CoreResult;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server, StatusCode,
};
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::timeouts;
use neo_plugins::rpc_server::rpc_server::RpcServer;
use parking_lot::RwLock;
use serde::Serialize;
use std::fs;
use std::{net::SocketAddr, sync::Arc};

pub async fn serve_health(
    port: u16,
    max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
    system: Arc<NeoSystem>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let make_svc = make_service_fn(move |_conn| {
        let system = system.clone();
        let max_header_lag = max_header_lag;
        let storage_path = storage_path.clone();
        async move {
            let storage_path_inner = storage_path.clone();
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let system = system.clone();
                let storage_path_req = storage_path_inner.clone();
                async move {
                    handle_request(
                        req,
                        max_header_lag,
                        storage_path_req,
                        rpc_enabled,
                        system.clone(),
                    )
                    .await
                }
            }))
        }
    });

    Server::bind(&addr).serve(make_svc).await?;
    Ok(())
}

async fn handle_request(
    req: Request<Body>,
    max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
    system: Arc<NeoSystem>,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/healthz") | (&hyper::Method::GET, "/readyz") => {
            let settings: &ProtocolSettings = system.settings();
            let rpc_ready = check_rpc_ready(rpc_enabled, &system);
            let timeout_stats = timeouts::stats();
            let peer_count = system.peer_count().await.unwrap_or(0);
            let storage_ready = storage_path
                .as_deref()
                .map(|path| verify_storage_markers(path, settings.network))
                .unwrap_or(true);
            let readiness = system
                .readiness((max_header_lag != 0).then_some(max_header_lag))
                .with_services(rpc_ready, storage_ready);
            let ledger = system.ledger_context();
            let block_height = readiness.block_height;
            let header_height = readiness.header_height;
            let header_lag = readiness.header_lag;
            let mempool_size = ledger.mempool_transaction_hashes().len() as u32;
            let healthy = readiness.healthy;

            metrics::update_metrics(
                block_height,
                header_height,
                header_lag,
                mempool_size,
                timeout_stats,
                peer_count,
                storage_path.as_deref(),
            );

            let body = HealthStatus {
                status: if healthy && rpc_ready && storage_ready {
                    "ok"
                } else {
                    "degraded"
                },
                network_magic: settings.network,
                version: env!("CARGO_PKG_VERSION"),
                milliseconds_per_block: settings.milliseconds_per_block,
                block_height,
                header_height,
                header_lag,
                mempool_size,
                rpc_ready: readiness.rpc_ready,
                timeout_handshake: timeout_stats.handshake,
                timeout_read: timeout_stats.read,
                timeout_write: timeout_stats.write,
                peer_count,
                storage_ready: readiness.storage_ready,
            };
            let json =
                serde_json::to_string(&body).unwrap_or_else(|_| "{\"status\":\"ok\"}".into());
            let mut resp = Response::new(Body::from(json));
            if !healthy || !readiness.rpc_ready || !readiness.storage_ready {
                *resp.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
            }
            Ok(resp)
        }
        (&hyper::Method::GET, "/metrics") => {
            let body = metrics::gather();
            Ok(Response::new(Body::from(body)))
        }
        _ => {
            let mut not_found = Response::new(Body::from("not found"));
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}

#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
    network_magic: u32,
    version: &'static str,
    milliseconds_per_block: u32,
    block_height: u32,
    header_height: u32,
    header_lag: u32,
    mempool_size: u32,
    rpc_ready: bool,
    timeout_handshake: usize,
    timeout_read: usize,
    timeout_write: usize,
    peer_count: usize,
    storage_ready: bool,
}

fn verify_storage_markers(path: &str, expected_magic: u32) -> bool {
    let storage_path = std::path::Path::new(path);
    let magic_marker = storage_path.join("NETWORK_MAGIC");
    let version_marker = storage_path.join("VERSION");
    let magic_ok = fs::read_to_string(&magic_marker)
        .ok()
        .and_then(|contents| {
            let parsed = contents.trim_start_matches("0x").trim().to_string();
            u32::from_str_radix(&parsed, 16)
                .ok()
                .or_else(|| parsed.parse::<u32>().ok())
        })
        .map(|stored| stored == expected_magic)
        .unwrap_or(false);
    let version_ok = fs::read_to_string(&version_marker)
        .ok()
        .map(|contents| contents.trim() == crate::STORAGE_VERSION)
        .unwrap_or(false);
    magic_ok && version_ok
}

fn check_rpc_ready(enabled: bool, system: &Arc<NeoSystem>) -> bool {
    if !enabled {
        return true;
    }
    rpc_server_handle(system)
        .map(|srv| srv.map_or(false, |s| s.read().is_started()))
        .unwrap_or(false)
}

fn rpc_server_handle(system: &NeoSystem) -> CoreResult<Option<Arc<RwLock<RpcServer>>>> {
    let name = system.rpc_service_name();
    system.get_named_service::<RwLock<RpcServer>>(&name)
}
