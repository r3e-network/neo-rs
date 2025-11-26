//! Contract command - retrieves contract state

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, hash: &str) -> CommandResult {
    let contract = client.get_contract_state(hash).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    // Use Debug format since RpcContractState doesn't implement Serialize
    Ok(format!("{:#?}", contract))
}
