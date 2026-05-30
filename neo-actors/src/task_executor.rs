//! Centralized background-task lifecycle owner with cooperative graceful shutdown.
//!
//! This is the native-Rust task-management idiom used by both reth
//! (`reth-tasks::TaskExecutor`) and Substrate (`sc-service::TaskManager` +
//! `SpawnHandle`): a single owner spawns tracked tasks, each task observes a
//! shared [`CancellationToken`] to exit promptly when shutdown is requested, and
//! the owner can await all outstanding tasks on stop. It replaces ad-hoc
//! `tokio::spawn` + hard `JoinHandle::abort()` (which kills a task mid-operation
//! with no chance to release resources or finish an in-flight unit of work).
//!
//! Neither reth nor Substrate uses an actor framework for this; they use exactly
//! this "driver future + cancellation token + tracked shutdown" pattern over
//! plain tokio. `TaskExecutor` is the seam neo-rs subsystems migrate onto as they
//! move off the actor runtime.

use std::future::Future;
use tokio::task::JoinHandle;
pub use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Spawns and tracks background tasks that share a cooperative shutdown signal.
///
/// Cheaply cloneable: every clone shares the same tracker and root cancellation
/// token, so any holder can request shutdown and the owner can await completion.
#[derive(Clone, Debug)]
pub struct TaskExecutor {
    tracker: TaskTracker,
    shutdown: CancellationToken,
}

impl TaskExecutor {
    /// Creates a new executor with no spawned tasks and an un-cancelled token.
    pub fn new() -> Self {
        Self {
            tracker: TaskTracker::new(),
            shutdown: CancellationToken::new(),
        }
    }

    /// Spawns a tracked background task.
    ///
    /// The closure receives a child [`CancellationToken`]; long-running tasks
    /// should `select!` on [`CancellationToken::cancelled`] so they exit promptly
    /// when [`shutdown`](Self::shutdown) or [`trigger_shutdown`](Self::trigger_shutdown)
    /// is called, rather than relying on a hard abort.
    pub fn spawn<F, Fut>(&self, task: F) -> JoinHandle<()>
    where
        F: FnOnce(CancellationToken) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let token = self.shutdown.child_token();
        self.tracker.spawn(task(token))
    }

    /// Returns a child cancellation token for code that wants to observe shutdown
    /// without spawning through this executor.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.shutdown.child_token()
    }

    /// Requests cooperative shutdown without waiting for tasks to finish.
    pub fn trigger_shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Returns `true` once shutdown has been requested.
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.is_cancelled()
    }

    /// Requests shutdown and awaits all currently-tracked tasks to finish.
    ///
    /// After this returns the executor is closed: no further tasks may be
    /// spawned (a post-close `spawn` returns a handle to a future that is never
    /// polled, matching `TaskTracker` semantics), so call it exactly once during
    /// node teardown.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        self.tracker.close();
        self.tracker.wait().await;
    }
}

impl Default for TaskExecutor {
    fn default() -> Self {
        Self::new()
    }
}
