//! Port stub for `TransactionManagerFactory.cs`.

use crate::{client::RpcClient, transaction_manager::TransactionManager};

#[derive(Debug, Default)]
pub struct TransactionManagerFactory;

impl TransactionManagerFactory {
    pub fn create<'a>(&self, client: &'a RpcClient) -> TransactionManager<'a> {
        TransactionManager::new(client)
    }
}
