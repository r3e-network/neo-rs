//! Port stub for `PolicyAPI.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct PolicyApi<'a> {
    client: &'a RpcClient,
}

impl<'a> PolicyApi<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
