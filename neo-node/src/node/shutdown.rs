//! Daemon shutdown signal handling.
//!
//! The node exits through a single graceful path whether shutdown comes from
//! the OS, a validation stop height, or an essential task failure.

use tokio_util::sync::CancellationToken;

pub(super) async fn wait_for_shutdown_signal(
    blockchain: neo_blockchain::BlockchainHandle,
    stop_at_height: Option<u32>,
    shutdown: CancellationToken,
) -> std::io::Result<String> {
    if let Some(target_height) = stop_at_height {
        tokio::select! {
            res = wait_for_os_shutdown_signal() => res.map(str::to_owned),
            res = wait_for_stop_height(blockchain.subscribe(), target_height) => res,
            _ = shutdown.cancelled() => Ok("essential_task".to_string()),
        }
    } else {
        tokio::select! {
            res = wait_for_os_shutdown_signal() => res.map(str::to_owned),
            _ = shutdown.cancelled() => Ok("essential_task".to_string()),
        }
    }
}

#[cfg(unix)]
async fn wait_for_os_shutdown_signal() -> std::io::Result<&'static str> {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => tokio::select! {
            res = tokio::signal::ctrl_c() => res.map(|()| "Ctrl-C"),
            _ = sigterm.recv() => Ok("SIGTERM"),
        },
        Err(_) => tokio::signal::ctrl_c().await.map(|()| "Ctrl-C"),
    }
}

#[cfg(not(unix))]
async fn wait_for_os_shutdown_signal() -> std::io::Result<&'static str> {
    tokio::signal::ctrl_c().await.map(|()| "Ctrl-C")
}

async fn wait_for_stop_height(
    mut events: tokio::sync::broadcast::Receiver<neo_blockchain::RuntimeEvent>,
    target_height: u32,
) -> std::io::Result<String> {
    use tokio::sync::broadcast::error::RecvError;

    loop {
        match events.recv().await {
            Ok(neo_blockchain::RuntimeEvent::Imported { height, .. })
                if height >= target_height =>
            {
                return Ok(format!("stop-at-height {target_height}"));
            }
            Ok(_) => {}
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "blockchain event stream closed before stop height",
                ));
            }
        }
    }
}
