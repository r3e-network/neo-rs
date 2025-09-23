//! Port stub for `StateAPI.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct StateApi<'a> {
    client: &'a RpcClient,
}

impl<'a> StateApi<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
