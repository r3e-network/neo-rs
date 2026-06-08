//! Signal handling for graceful shutdown.

use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
use tracing::{error, info};

pub(crate) async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            result = signal::ctrl_c() => {
                match result {
                    Ok(()) => info!(target: "neo", "received SIGINT, shutting down"),
                    Err(err) => error!(target: "neo", error = %err, "failed to wait for SIGINT")}
           }
            _ = sigterm.recv() => {
                info!(target: "neo", "received SIGTERM, shutting down");
           }
       }
   }
    #[cfg(not(unix))]
    {
        if let Err(err) = signal::ctrl_c().await {
            error!(target: "neo", error = %err, "failed to wait for shutdown signal");
       } else {
            info!(target: "neo", "shutdown signal received (Ctrl+C)");
       }
   }
}
