//! Background task supervision for node-local startup services.

use std::future::Future;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::trace;

/// Default grace period for node-local background tasks to exit.
pub(crate) const DEFAULT_BACKGROUND_TASK_SHUTDOWN: Duration = Duration::from_secs(5);

/// Tracks node-local background tasks and provides one cooperative shutdown signal.
#[derive(Clone, Debug)]
pub(crate) struct BackgroundTasks {
    cancellation: CancellationToken,
    tracker: TaskTracker}

impl BackgroundTasks {
    /// Creates an empty task set.
    pub(crate) fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            tracker: TaskTracker::new()}
   }

    /// Returns a clone of the shutdown token for a tracked task.
    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
   }

    /// Spawns and tracks a named background task.
    pub(crate) fn spawn<F>(&self, name: &'static str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tracker.spawn(async move {
            future.await;
            trace!(target: "neo", task = name, "background task stopped");
       });
   }

    /// Cancels all tracked tasks and waits for them to exit.
    pub(crate) async fn shutdown(&self, timeout: Duration) -> bool {
        self.cancellation.cancel();
        self.tracker.close();
        tokio::time::timeout(timeout, self.tracker.wait())
            .await
            .is_ok()
   }
}

impl Default for BackgroundTasks {
    fn default() -> Self {
        Self::new()
   }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn shutdown_cancels_and_waits_for_tasks() {
        let tasks = BackgroundTasks::new();
        let shutdown = tasks.cancellation_token();
        let (stopped_tx, stopped_rx) = oneshot::channel();

        tasks.spawn("test task", async move {
            shutdown.cancelled().await;
            let _ = stopped_tx.send(());
       });

        assert!(tasks.shutdown(Duration::from_secs(1)).await);
        stopped_rx.await.expect("tracked task should stop");
   }
}
