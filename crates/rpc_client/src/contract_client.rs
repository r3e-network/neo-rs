//! Port stub for `ContractClient.cs`.

use crate::client::RpcClient;

#[derive(Debug, Clone)]
pub struct ContractClient<'a> {
    pub client: &'a RpcClient,
}

impl<'a> ContractClient<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    /// TODO: Mirror C# contract invocation helpers.
    pub fn client(&self) -> &RpcClient {
        self.client
    }
}
