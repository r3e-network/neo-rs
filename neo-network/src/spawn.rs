//! Guards for `tokio::spawn` that catch panics and log them, preventing
//! silent task loss and leaked registry entries.

use std::any::Any;
use std::future::Future;

use futures::FutureExt;
use tokio::task::JoinHandle;
use tracing::error;

/// Spawn a future on the Tokio runtime, catching any panic and
/// logging it at `error` level. Returns the `JoinHandle` so the
/// caller can optionally await completion (e.g. during shutdown).
pub fn spawn_guarded<F>(name: &'static str, future: F) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        // Use `futures::FutureExt::catch_unwind` which is the async
        // equivalent of `std::panic::catch_unwind`. `AssertUnwindSafe`
        // is required because the compiler cannot prove the future is
        // unwind-safe across a task boundary.
        if let Err(panic) = std::panic::AssertUnwindSafe(future).catch_unwind().await {
            let msg = panic_to_string(&panic);
            error!(target: "neo_network", %msg, "spawned task \"{name}\" panicked");
        }
    })
}

/// Best-effort conversion of a panic payload into a human-readable
/// string suitable for logging.
fn panic_to_string(panic: &Box<dyn Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}
