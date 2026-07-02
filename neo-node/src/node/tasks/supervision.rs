//! Supervised daemon task spawning.
//!
//! This module keeps the Substrate-style essential/normal task policy out of
//! the node composition root. Essential task exits request node shutdown;
//! normal task exits are reported but do not stop the daemon.

use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};

use futures::FutureExt;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::node::observability::ObservabilityRuntime;

use super::metrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) enum TaskKind {
    Essential,
    Normal,
}

impl TaskKind {
    pub(super) fn as_label(self) -> &'static str {
        match self {
            Self::Essential => "essential",
            Self::Normal => "normal",
        }
    }

    fn cancels_node(self) -> bool {
        matches!(self, Self::Essential)
    }
}

pub(in crate::node) fn spawn_daemon_task<F>(
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
    shutdown: &CancellationToken,
    kind: TaskKind,
    task_name: &'static str,
    future: F,
) where
    F: Future<Output = ()> + Send + 'static,
{
    let shutdown = shutdown.clone();
    metrics::record_spawn(task_name, kind);
    let future = async move {
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(()) => {
                metrics::record_exit(task_name, kind);
                if kind.cancels_node() {
                    shutdown.cancel();
                }
            }
            Err(payload) => {
                metrics::record_panic(task_name, kind);
                if kind.cancels_node() {
                    shutdown.cancel();
                }
                resume_unwind(payload);
            }
        }
    };
    let handle = match observability {
        Some(observability) => observability.spawn_monitored(task_name, future),
        None => tokio::spawn(async move {
            future.await;
            warn!(
                target: "neo",
                task = task_name,
                "background task exited unexpectedly"
            );
        }),
    };
    handles.push(handle);
}

pub(in crate::node) fn spawn_daemon_task_result<F>(
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
    shutdown: &CancellationToken,
    kind: TaskKind,
    task_name: &'static str,
    future: F,
) where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let shutdown = shutdown.clone();
    metrics::record_spawn(task_name, kind);
    let future = async move {
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(result) => {
                match &result {
                    Ok(()) => metrics::record_exit(task_name, kind),
                    Err(_) => metrics::record_error(task_name, kind),
                }
                if kind.cancels_node() {
                    shutdown.cancel();
                }
                result
            }
            Err(payload) => {
                metrics::record_panic(task_name, kind);
                if kind.cancels_node() {
                    shutdown.cancel();
                }
                resume_unwind(payload);
            }
        }
    };
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
