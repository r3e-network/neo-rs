//! Port stub for `TransactionManager.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct TransactionManager<'a> {
    client: &'a RpcClient,
}

impl<'a> TransactionManager<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
