use tracing::{error, warn};

pub(crate) fn log_join_error(error: tokio::task::JoinError) {
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
