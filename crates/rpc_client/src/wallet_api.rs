//! Port stub for `WalletAPI.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct WalletApi<'a> {
    client: &'a RpcClient,
}

impl<'a> WalletApi<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
