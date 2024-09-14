use std::marker::PhantomData;
use crate::neo_contract::contract_task_awaiter::ContractTaskAwaiter;

pub struct ContractTask {
    awaiter: ContractTaskAwaiter,
}

impl ContractTask {
    pub fn new() -> Self {
        Self {
            awaiter: Self::create_awaiter(),
        }
    }

    fn create_awaiter() -> ContractTaskAwaiter {
        ContractTaskAwaiter::new()
    }

    pub fn get_awaiter(&self) -> &ContractTaskAwaiter {
        &self.awaiter
    }

    pub fn get_result(&self) -> Option<()> {
        None
    }
}

impl Default for ContractTask {
    fn default() -> Self {
        let mut task = Self::new();
        task.get_awaiter().set_result();
        task
    }
}

pub struct ContractTask<T> {
    awaiter: ContractTaskAwaiter<T>,
    _phantom: PhantomData<T>,
}

impl<T> ContractTask<T> {
    pub fn new() -> Self {
        Self {
            awaiter: Self::create_awaiter(),
            _phantom: PhantomData,
        }
    }

    fn create_awaiter() -> ContractTaskAwaiter<T> {
        ContractTaskAwaiter::new()
    }

    pub fn get_awaiter(&self) -> &ContractTaskAwaiter<T> {
        &self.awaiter
    }

    pub fn get_result(&self) -> T {
        self.get_awaiter().get_result()
    }
}

impl<T> Default for ContractTask<T> {
    fn default() -> Self {
        let mut task = Self::new();
        task.get_awaiter().set_result();
        task
    }
}