//! ContractTask - matches C# Neo.SmartContract.ContractTask exactly

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Represents an asynchronous contract task (matches C# ContractTask)
pub struct ContractTask {
    inner: Pin<Box<dyn Future<Output = Result<(), String>> + Send>>,
}

impl ContractTask {
    /// Creates a new contract task
    pub fn new<F>(future: F) -> Self
    where
        F: Future<Output = Result<(), String>> + Send + 'static,
    {
        Self {
            inner: Box::pin(future),
        }
    }

    /// Creates a completed task
    pub fn completed() -> Self {
        Self::new(async { Ok(()) })
    }

    /// Creates a failed task
    pub fn failed(error: String) -> Self {
        Self::new(async move { Err(error) })
    }
}

impl Future for ContractTask {
    type Output = Result<(), String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
