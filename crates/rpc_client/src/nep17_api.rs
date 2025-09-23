//! Port stub for `Nep17API.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct Nep17Api<'a> {
    client: &'a RpcClient,
}

impl<'a> Nep17Api<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
