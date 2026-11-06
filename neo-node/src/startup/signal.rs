//! Signal handling for graceful shutdown.

use neo_system::NeoSystem;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
use tracing::{error, info};

/// Optional deterministic stopping point for bounded live-sync audits.
/// Parsing is kept separate from environment access so the accepted values can
/// be tested without introducing process-wide environment-variable races.
pub(crate) fn parse_live_stop_height(value: Option<&str>) -> Result<Option<u32>, String> {
    value
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|error| format!("invalid NEO_LIVE_STOP_HEIGHT: {error}"))
}

pub(crate) async fn wait_for_live_stop_height(system: Arc<NeoSystem>, stop_height: u32) {
    loop {
        let current_height = system.current_block_index();
        if current_height >= stop_height {
            info!(
                target: "neo",
                current_height,
                stop_height,
                "reached NEO_LIVE_STOP_HEIGHT; shutting down through the normal flush path"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            result = signal::ctrl_c() => {
                match result {
                    Ok(()) => info!(target: "neo", "received SIGINT, shutting down"),
                    Err(err) => error!(target: "neo", error = %err, "failed to wait for SIGINT"),
                }
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

#[cfg(test)]
mod tests {
    use super::parse_live_stop_height;

    #[test]
    fn live_stop_height_is_optional_and_strictly_parsed() {
        assert_eq!(parse_live_stop_height(None), Ok(None));
        assert_eq!(
            parse_live_stop_height(Some("13142000")),
            Ok(Some(13_142_000))
        );
        assert_eq!(
            parse_live_stop_height(Some("4294967295")),
            Ok(Some(u32::MAX))
        );
        assert!(parse_live_stop_height(Some("")).is_err());
        assert!(parse_live_stop_height(Some("-1")).is_err());
        assert!(parse_live_stop_height(Some("4294967296")).is_err());
    }
}
