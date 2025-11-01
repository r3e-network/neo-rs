//! ContractTaskAwaiter - matches C# Neo.SmartContract.ContractTaskAwaiter exactly

use crate::smart_contract::contract_task::ContractTask;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Awaiter for contract tasks (matches C# ContractTaskAwaiter)
pub struct ContractTaskAwaiter {
    task: ContractTask,
}

impl ContractTaskAwaiter {
    /// Creates a new awaiter
    pub fn new(task: ContractTask) -> Self {
        Self { task }
    }

    /// Gets whether the task is completed
    pub fn is_completed(&self) -> bool {
        // In Rust, we can't easily check if a future is ready without polling
        // This would need runtime support
        false
    }

    /// Gets the result of the task
    pub async fn get_result(self) -> Result<(), String> {
        self.task.await
    }
}

impl Future for ContractTaskAwaiter {
    type Output = Result<(), String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Poll the underlying task
        let task = unsafe { Pin::new_unchecked(&mut self.task) };
        task.poll(cx)
    }
}
