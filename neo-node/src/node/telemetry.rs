//! Local telemetry endpoints for the node daemon.

use std::convert::Infallible;
use std::net::TcpListener;
use std::sync::Arc;

use anyhow::Context;
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};
use tracing::info;

use self::exporter::MetricsExporter;
use self::http::serve_metrics_request;
use super::config::TelemetryMetricsSection;

mod exporter;
mod http;
mod readiness;

#[cfg(test)]
mod tests;

pub(super) fn metrics_server_task(
    config: &TelemetryMetricsSection,
    node: Arc<neo_system::Node>,
) -> anyhow::Result<Option<impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static>>
{
    if !config.enabled {
        return Ok(None);
    }

    let requested_addr = config.bind_socket_addr()?;
    let listener = TcpListener::bind(requested_addr)
        .with_context(|| format!("binding metrics endpoint at {requested_addr}"))?;
    listener
        .set_nonblocking(true)
        .context("setting metrics listener nonblocking")?;
    let local_addr = listener
        .local_addr()
        .context("reading metrics listener address")?;
    let path = config.endpoint_path().to_string();
    let exporter = Arc::new(MetricsExporter::new(node)?);

    let make_service = make_service_fn(move |_| {
        let exporter = Arc::clone(&exporter);
        let path = path.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |request| {
                serve_metrics_request(request, path.clone(), Arc::clone(&exporter))
            }))
        }
    });
    let server = Server::from_tcp(listener)
        .context("creating metrics HTTP server")?
        .serve(make_service);

    info!(
        target: "neo::telemetry",
        bind_addr = %local_addr,
        path = %config.endpoint_path(),
        "Prometheus metrics endpoint started"
    );

    Ok(Some(async move {
        server.await.context("metrics HTTP server stopped")
    }))
}
