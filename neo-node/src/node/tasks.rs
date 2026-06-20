use std::future::Future;

use super::observability::ObservabilityRuntime;
use tracing::warn;

pub(super) fn spawn_daemon_task<F>(
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
    task_name: &'static str,
    future: F,
) where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = match observability {
        Some(observability) => observability.spawn_monitored(task_name, future),
        None => tokio::spawn(future),
    };
    handles.push(handle);
}

pub(super) fn spawn_daemon_task_result<F>(
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
    task_name: &'static str,
    future: F,
) where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let handle = match observability {
        Some(observability) => observability.spawn_monitored_result(task_name, future),
        None => tokio::spawn(async move {
            match future.await {
                Ok(()) => warn!(
                    target: "neo",
                    task = task_name,
                    "background task exited unexpectedly"
                ),
                Err(err) => warn!(
                    target: "neo",
                    task = task_name,
                    error = %err,
                    "background task failed"
                ),
            }
        }),
    };
    handles.push(handle);
}
