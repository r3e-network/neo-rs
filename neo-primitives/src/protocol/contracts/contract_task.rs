//! ContractTask - matches C# Neo.SmartContract.ContractTask exactly

use std::future::Future;
use std::future::{Ready, ready};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Represents an asynchronous contract task (matches C# ContractTask)
pub struct ContractTask<F = Ready<Result<(), String>>>
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    inner: Pin<Box<F>>,
}

impl<F> ContractTask<F>
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    /// Creates a new contract task
    pub fn new(future: F) -> Self {
        Self {
            inner: Box::pin(future),
        }
    }
}

impl ContractTask<Ready<Result<(), String>>> {
    /// Creates a completed task
    pub fn completed() -> Self {
        Self::new(ready(Ok(())))
    }

    /// Creates a failed task
    pub fn failed(error: String) -> Self {
        Self::new(ready(Err(error)))
    }
}

impl<F> Future for ContractTask<F>
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    type Output = Result<(), String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
